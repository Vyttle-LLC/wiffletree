use std::collections::{BTreeMap, BTreeSet};
use workspace_core::*;

fn allowed(pairs: &[(&str, &str)]) -> Vec<AllowedModel> {
    pairs
        .iter()
        .map(|(model, effort)| AllowedModel {
            model: (*model).into(),
            effort: (*effort).into(),
        })
        .collect()
}
fn profile(provider: Provider, model: &str, effort: &str) -> ModelProfile {
    ModelProfile {
        provider,
        model: model.into(),
        effort: effort.into(),
    }
}
/// Both subscriptions; every role on Claude.
fn both() -> ModelSelection {
    ModelSelection {
        revision: 1,
        providers: BTreeMap::from([
            (
                Provider::Claude,
                ProviderAccess {
                    enabled: true,
                    models: allowed(&[("opus", "high"), ("sonnet", "high")]),
                },
            ),
            (
                Provider::Codex,
                ProviderAccess {
                    enabled: true,
                    models: allowed(&[("gpt-6.1-sol", "high")]),
                },
            ),
        ]),
        role_providers: PROVIDER_ROLES
            .into_iter()
            .map(|r| (r, BTreeSet::from([Provider::Claude])))
            .collect(),
        guide: "Small change → sonnet.".into(),
    }
}

#[test]
fn a_role_may_only_use_its_providers_and_allowed_models() {
    let selection = both();
    selection.validate().unwrap();
    selection
        .permits(
            Role::Implementer,
            &profile(Provider::Claude, "sonnet", "high"),
        )
        .unwrap();
    let wrong = selection
        .permits(
            Role::Implementer,
            &profile(Provider::Codex, "gpt-6.1-sol", "high"),
        )
        .unwrap_err();
    assert_eq!(
        wrong.to_string(),
        "Codex is not configured for Implementer on this machine. Implementer uses: Claude."
    );
    let unticked = selection
        .permits(Role::Implementer, &profile(Provider::Claude, "opus", "max"))
        .unwrap_err();
    assert_eq!(
        unticked.to_string(),
        "claude · opus · max is not allowed on this machine. Allowed Claude models: opus · high, sonnet · high."
    );
}

#[test]
fn reviewers_skip_the_role_check_but_not_the_allowlist() {
    let mut selection = both();
    selection
        .permits(
            Role::Reviewer,
            &profile(Provider::Codex, "gpt-6.1-sol", "high"),
        )
        .unwrap();
    assert!(matches!(
        selection.permits(
            Role::Reviewer,
            &profile(Provider::Codex, "gpt-6-luna", "medium")
        ),
        Err(Rejection::NotAllowed { .. })
    ));
    selection
        .providers
        .get_mut(&Provider::Codex)
        .unwrap()
        .enabled = false;
    let disabled = selection
        .permits(
            Role::Reviewer,
            &profile(Provider::Codex, "gpt-6.1-sol", "high"),
        )
        .unwrap_err();
    assert_eq!(
        disabled.to_string(),
        "codex · gpt-6.1-sol · high is not allowed on this machine. Codex is disabled."
    );
}

#[test]
fn prefill_takes_the_first_provider_then_its_first_model() {
    let mut selection = both();
    assert_eq!(
        selection.prefill(Role::Implementer),
        Some(profile(Provider::Claude, "opus", "high"))
    );
    assert_eq!(
        selection.prefill(Role::Reviewer),
        Some(profile(Provider::Claude, "opus", "high"))
    );
    selection
        .role_providers
        .insert(Role::Tester, BTreeSet::from([Provider::Codex]));
    assert_eq!(
        selection.prefill(Role::Tester),
        Some(profile(Provider::Codex, "gpt-6.1-sol", "high"))
    );
    // A role whose providers have no allowed model: the first enabled provider that has one.
    selection
        .providers
        .get_mut(&Provider::Codex)
        .unwrap()
        .enabled = false;
    assert_eq!(
        selection.prefill(Role::Tester),
        Some(profile(Provider::Claude, "opus", "high"))
    );
}

/// Breaks one rule of an otherwise valid selection.
type Breakage = fn(&mut ModelSelection);

#[test]
fn invalid_configurations_are_rejected() {
    let cases: Vec<(&str, Breakage)> = vec![
        ("no enabled provider", |s| {
            for access in s.providers.values_mut() {
                access.enabled = false;
            }
        }),
        ("enabled provider without models", |s| {
            s.providers
                .get_mut(&Provider::Codex)
                .unwrap()
                .models
                .clear()
        }),
        ("duplicate model", |s| {
            let models = &mut s.providers.get_mut(&Provider::Claude).unwrap().models;
            models.push(models[0].clone());
        }),
        ("blank model", |s| {
            s.providers.get_mut(&Provider::Claude).unwrap().models[0].model = " ".into()
        }),
        ("role without provider", |s| {
            s.role_providers.insert(Role::Tester, BTreeSet::new());
        }),
        ("role missing", |s| {
            s.role_providers.remove(&Role::ProjectOrchestrator);
        }),
        ("role on a disabled provider", |s| {
            s.role_providers
                .insert(Role::Tester, BTreeSet::from([Provider::Codex]));
            s.providers.get_mut(&Provider::Codex).unwrap().enabled = false;
        }),
        ("reviewer listed as a role", |s| {
            s.role_providers
                .insert(Role::Reviewer, BTreeSet::from([Provider::Claude]));
        }),
        ("legacy repository coordinator", |s| {
            s.role_providers
                .insert(Role::TaskOrchestrator, BTreeSet::from([Provider::Claude]));
        }),
        ("long guide", |s| s.guide = "x".repeat(MAX_GUIDE_BYTES + 1)),
    ];
    for (name, break_it) in cases {
        let mut selection = both();
        break_it(&mut selection);
        assert!(selection.validate().is_err(), "{name}");
    }
    // Disabling a provider no role uses is fine.
    let mut selection = both();
    selection
        .providers
        .get_mut(&Provider::Codex)
        .unwrap()
        .enabled = false;
    selection.validate().unwrap();
}

#[test]
fn a_verifier_takes_its_configured_provider_and_its_roles_checks() {
    let selection = both();
    let style = VerifierConfig {
        role: Role::Reviewer,
        focus: "Style".into(),
        instruction: Some("Run reviso:style".into()),
        provider: Some(Provider::Codex),
    };
    selection
        .permits_verifier(&style, &profile(Provider::Codex, "gpt-6.1-sol", "high"))
        .unwrap();
    let wrong = selection
        .permits_verifier(&style, &profile(Provider::Claude, "opus", "high"))
        .unwrap_err();
    assert_eq!(
        wrong.to_string(),
        "Claude is not configured for Reviewer · Style. Reviewer · Style uses: Codex."
    );
    // Without a configured provider, a tester verifier follows the tester role's providers.
    let tests = VerifierConfig {
        role: Role::Tester,
        focus: "Tests".into(),
        instruction: None,
        provider: None,
    };
    assert!(matches!(
        selection.permits_verifier(&tests, &profile(Provider::Codex, "gpt-6.1-sol", "high")),
        Err(Rejection::WrongProvider { .. })
    ));
    selection
        .permits_verifier(&tests, &profile(Provider::Claude, "sonnet", "high"))
        .unwrap();
}

#[test]
fn a_reason_is_one_short_line() {
    assert_eq!(
        Selection::check_reason("  Small change  ").unwrap(),
        "Small change"
    );
    assert!(Selection::check_reason(" ").is_err());
    for broken in ["one\ntwo", "one\rtwo", "one\ttwo", "one\u{1b}[2Jtwo"] {
        assert!(Selection::check_reason(broken).is_err(), "{broken:?}");
    }
    assert!(Selection::check_reason(&"x".repeat(MAX_REASON_CHARS + 1)).is_err());
    let model = Selection::check_reason(" ").unwrap_err().to_string();
    assert_eq!(model, "Give a one-line reason for this model");
    let neutral = one_line_reason(" ").unwrap_err().to_string();
    assert_eq!(neutral, "Give a one-line reason");
    assert!(one_line_reason("one\ntwo").is_err());
}
