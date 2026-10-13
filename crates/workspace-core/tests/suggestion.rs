use std::collections::BTreeSet;
use workspace_core::*;

fn status(installed: bool, signed_in: bool) -> ProviderStatus {
    ProviderStatus {
        installed,
        signed_in,
        plan: None,
    }
}
fn detected(claude: ProviderStatus, codex: ProviderStatus) -> Detection {
    [(Provider::Claude, claude), (Provider::Codex, codex)].into()
}
fn models(setup: &Setup, provider: Provider) -> Vec<(&str, &str)> {
    setup.selection.providers[&provider]
        .models
        .iter()
        .map(|m| (m.model.as_str(), m.effort.as_str()))
        .collect()
}
fn verifiers(setup: &Setup) -> Vec<(Role, &str, Option<Provider>)> {
    setup
        .verification
        .verifiers
        .iter()
        .map(|v| (v.role, v.focus.as_str(), v.provider))
        .collect()
}
fn levels(setup: &Setup) -> Vec<(&str, LevelKind, Vec<&str>, &str)> {
    setup
        .verification
        .levels
        .iter()
        .map(|level| {
            (
                level.name.as_str(),
                level.kind,
                level.verifiers.iter().map(String::as_str).collect(),
                level.use_when.as_str(),
            )
        })
        .collect()
}
fn roles(setup: &Setup) -> Vec<(Role, Vec<Provider>)> {
    setup
        .selection
        .role_providers
        .iter()
        .map(|(role, providers)| (*role, providers.iter().copied().collect()))
        .collect()
}
const CLAUDE_MODELS: [(&str, &str); 4] = [
    ("opus", "high"),
    ("opus", "medium"),
    ("sonnet", "high"),
    ("sonnet", "medium"),
];
const CODEX_MODELS: [(&str, &str); 2] = [("gpt-6.1-sol", "high"), ("gpt-6.1-sol", "medium")];

#[test]
fn both_signed_in_splits_the_roles_and_keeps_the_default_verifiers() {
    let setup = suggest(&detected(status(true, true), status(true, true)));
    setup.selection.validate().unwrap();
    assert!(!setup.selection.is_saved());
    assert_eq!(models(&setup, Provider::Claude), CLAUDE_MODELS);
    assert_eq!(models(&setup, Provider::Codex), CODEX_MODELS);
    assert_eq!(
        roles(&setup),
        [
            (Role::ProjectOrchestrator, vec![Provider::Claude]),
            (Role::Implementer, vec![Provider::Claude]),
            (Role::Tester, vec![Provider::Codex]),
        ]
    );
    let default = VerificationSettings::default();
    assert_eq!(setup.verification.verifiers, default.verifiers);
    assert_eq!(
        (setup.verification.max_rounds, setup.verification.max_cycles),
        (default.max_rounds, default.max_cycles)
    );
    setup.verification.validate().unwrap();
    assert_eq!(
        levels(&setup),
        [
            (
                "Full",
                LevelKind::FullReview,
                vec!["Tests", "Correctness", "Regressions"],
                "for a ticket's first review, and after any change to what the code does."
            ),
            (
                "Light",
                LevelKind::Recheck,
                vec!["Correctness"],
                "after a rebase, a resolved merge conflict or a small fix, on a ticket whose last review passed."
            ),
        ]
    );
    assert_eq!(
        setup.selection.guide,
        "Pick the model by the size and risk of the work. Claude: small, contained change → Sonnet; large or cross-cutting → Opus. Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high. Raise effort for migrations, data or security changes.\n"
    );
}

#[test]
fn one_signed_in_provider_takes_every_role_and_the_only_review() {
    for (provider, other, expected, review) in [
        (
            Provider::Claude,
            Provider::Codex,
            &CLAUDE_MODELS[..],
            "Correctness",
        ),
        (
            Provider::Codex,
            Provider::Claude,
            &CODEX_MODELS[..],
            "Regressions",
        ),
    ] {
        // Installed but signed out counts as unavailable, like a missing CLI.
        for unavailable in [status(false, false), status(true, false)] {
            let detection: Detection =
                [(provider, status(true, true)), (other, unavailable)].into();
            let setup = suggest(&detection);
            setup.selection.validate().unwrap();
            setup.verification.validate().unwrap();
            assert_eq!(models(&setup, provider), expected);
            assert_eq!(setup.selection.providers[&other], ProviderAccess::default());
            assert!(
                roles(&setup)
                    .iter()
                    .all(|(_, providers)| providers == &[provider]),
                "{:?}",
                roles(&setup)
            );
            assert_eq!(roles(&setup).len(), PROVIDER_ROLES.len());
            assert_eq!(
                verifiers(&setup),
                [
                    (Role::Tester, "Tests", None),
                    (Role::Reviewer, review, Some(provider)),
                ]
            );
            // Full runs the tester and the one reviewer; Light, a re-check, runs that reviewer.
            let levels: Vec<_> = levels(&setup)
                .into_iter()
                .map(|(name, kind, verifiers, _)| (name, kind, verifiers))
                .collect();
            assert_eq!(
                levels,
                [
                    ("Full", LevelKind::FullReview, vec!["Tests", review]),
                    ("Light", LevelKind::Recheck, vec![review]),
                ]
            );
            assert!(setup.selection.guide.contains(provider.label()));
            assert!(!setup.selection.guide.contains(other.label()));
        }
    }
}

#[test]
fn neither_signed_in_disables_both_and_cannot_be_saved() {
    for detection in [
        Detection::new(),
        detected(status(false, false), status(true, false)),
        // A status that claims sign-in without a CLI is not usable either.
        detected(status(false, true), status(false, false)),
    ] {
        let setup = suggest(&detection);
        assert!(PROVIDERS.iter().all(|p| !setup.selection.is_enabled(*p)));
        assert_eq!(
            setup
                .selection
                .providers
                .keys()
                .copied()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(PROVIDERS)
        );
        assert!(setup.selection.role_providers.is_empty());
        assert!(setup.selection.guide.is_empty());
        assert_eq!(
            setup.selection.validate().unwrap_err().to_string(),
            "Enable at least one provider"
        );
    }
}

#[test]
fn the_plan_does_not_change_the_suggestion() {
    let with_plan = |plan: &str| ProviderStatus {
        plan: Some(plan.into()),
        ..status(true, true)
    };
    assert_eq!(
        suggest(&detected(with_plan("max"), with_plan("plus"))),
        suggest(&detected(status(true, true), status(true, true)))
    );
}
