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
/// Each row as "job: first → then | small first → small then", with "wait" for no then.
fn rows(setup: &Setup) -> Vec<String> {
    let line = |line: &Line| {
        let then = line
            .then
            .as_ref()
            .map_or("wait".into(), ToString::to_string);
        format!("{} → {then}", line.first)
    };
    (setup.selection.jobs.iter())
        .map(|row| {
            let small = row.small.as_ref().map(|s| format!(" | {}", line(s)));
            format!(
                "{}: {}{}",
                row.job.name(),
                line(&row.main),
                small.unwrap_or_default()
            )
        })
        .collect()
}
fn levels(setup: &Setup) -> Vec<(&str, LevelKind, Vec<&str>)> {
    (setup.verification.levels.iter())
        .map(|level| {
            let verifiers = level.verifiers.iter().map(String::as_str).collect();
            (level.name.as_str(), level.kind, verifiers)
        })
        .collect()
}

#[test]
fn both_signed_in_is_the_approved_table() {
    let setup = suggest(
        &detected(status(true, true), status(true, true)),
        &fallback_catalog(),
    );
    setup.selection.validate().unwrap();
    setup.verification.validate().unwrap();
    assert!(!setup.selection.is_saved());
    assert_eq!(setup.selection.switch_at, 95);
    assert_eq!(
        rows(&setup),
        [
            "Coordinator: claude · opus · high → codex · gpt-6.1-sol · high",
            "Implementer: claude · opus · medium → codex · gpt-6.1-sol · high | claude · sonnet · high → codex · gpt-6.1-sol · medium",
            "Tester: claude · sonnet · high → codex · gpt-6.1-sol · medium",
            "Tests: claude · sonnet · high → codex · gpt-6.1-sol · medium",
            // The provider lenses keep their second opinion: they wait rather than switch.
            "Correctness: claude · opus · high → wait",
            "Regressions: codex · gpt-6.1-sol · high → wait",
            "PR keeper: claude · sonnet · medium → codex · gpt-6.1-sol · medium",
        ]
    );
    assert_eq!(
        setup.verification.verifiers,
        VerificationSettings::default().verifiers
    );
    assert_eq!(
        levels(&setup),
        [
            (
                "Full",
                LevelKind::FullReview,
                vec!["Tests", "Correctness", "Regressions"]
            ),
            ("Light", LevelKind::Recheck, vec!["Correctness"]),
        ]
    );
    assert!(setup.selection.guide.starts_with("Pick the model"));
}

#[test]
fn one_signed_in_provider_runs_every_row_alone() {
    for (provider, other, review) in [
        (Provider::Claude, Provider::Codex, "Correctness"),
        (Provider::Codex, Provider::Claude, "Regressions"),
    ] {
        // Installed but signed out counts as unavailable, like a missing CLI.
        for unavailable in [status(false, false), status(true, false)] {
            let detection: Detection =
                [(provider, status(true, true)), (other, unavailable)].into();
            let setup = suggest(&detection, &fallback_catalog());
            setup.selection.validate().unwrap();
            setup.verification.validate().unwrap();
            for row in &setup.selection.jobs {
                for line in std::iter::once(&row.main).chain(&row.small) {
                    assert_eq!(line.first.provider, provider, "{:?}", row.job);
                    assert_eq!(line.then, None, "{:?}", row.job);
                }
            }
            let focuses: Vec<_> = (setup.verification.verifiers.iter())
                .map(|v| v.focus.as_str())
                .collect();
            assert_eq!(focuses, ["Tests", review]);
            assert_eq!(
                levels(&setup),
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
fn neither_signed_in_has_no_table_and_cannot_be_saved() {
    for detection in [
        Detection::new(),
        detected(status(false, false), status(true, false)),
        // A status that claims sign-in without a CLI is not usable either.
        detected(status(false, true), status(false, false)),
    ] {
        let setup = suggest(&detection, &fallback_catalog());
        assert!(setup.selection.jobs.is_empty());
        assert!(setup.selection.guide.is_empty());
        assert!(setup.selection.validate().is_err());
    }
}

#[test]
fn the_plan_does_not_change_the_suggestion() {
    let with_plan = |plan: &str| ProviderStatus {
        plan: Some(plan.into()),
        ..status(true, true)
    };
    assert_eq!(
        suggest(
            &detected(with_plan("max"), with_plan("plus")),
            &fallback_catalog()
        ),
        suggest(
            &detected(status(true, true), status(true, true)),
            &fallback_catalog()
        )
    );
}

#[test]
fn a_model_the_catalog_does_not_list_becomes_the_closest_one() {
    let option = |provider, model: &str, efforts: &[&str]| ModelOption {
        provider,
        model: model.into(),
        label: model.into(),
        efforts: efforts.iter().map(|e| (*e).into()).collect(),
    };
    let catalog = [
        option(
            Provider::Claude,
            "claude-opus-5-5",
            &["low", "medium", "max"],
        ),
        option(Provider::Claude, "claude-sonnet-5-5", &[]),
        option(Provider::Codex, "gpt-7-sol", &["minimal", "xhigh"]),
    ];
    let profile = |provider, model: &str, effort: &str| ModelProfile {
        provider,
        model: model.into(),
        effort: effort.into(),
    };
    // A model whose ID contains the seed's, at the nearest effort it offers.
    assert_eq!(
        closest(Provider::Claude, "opus", "high", &catalog),
        profile(Provider::Claude, "claude-opus-5-5", "medium")
    );
    // A model with no efforts listed offers its provider's usual ones.
    assert_eq!(
        closest(Provider::Claude, "sonnet", "high", &catalog),
        profile(Provider::Claude, "claude-sonnet-5-5", "high")
    );
    // Nothing alike: the provider's first model.
    assert_eq!(
        closest(Provider::Codex, "gpt-6.1-sol", "high", &catalog),
        profile(Provider::Codex, "gpt-7-sol", "xhigh")
    );
    // A catalog without the provider changes nothing.
    assert_eq!(
        closest(Provider::Codex, "gpt-6.1-sol", "high", &catalog[..2]),
        profile(Provider::Codex, "gpt-6.1-sol", "high")
    );
}
