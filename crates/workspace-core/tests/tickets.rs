use serde_json::json;
use workspace_core::*;

#[test]
fn a_ticket_stored_before_repositories_and_verification_still_decodes() {
    let ticket: Ticket = serde_json::from_value(json!({
        "id":"t","coordinator_id":"c","title":"Fix","brief":"Do",
        "worktree":"/w","branch":"wiffletree/fix","state":"assigned"
    }))
    .unwrap();
    assert_eq!(ticket.repository_id, "");
    assert!(ticket.verification.is_none());
    assert!(ticket.running_cycle().is_none());
    assert!(ticket.previous_cycles.is_empty() && ticket.ledger.is_empty());
    assert_eq!(ticket.waiver, None);
    assert_eq!(ticket.pull_request, None);
}

fn finding(severity: Severity, location: &str, trigger: &str, evidence: &str) -> Finding {
    Finding {
        severity,
        location: location.into(),
        summary: "Drops the call".into(),
        trigger: trigger.into(),
        evidence: evidence.into(),
    }
}

#[test]
fn only_a_located_blocking_finding_with_trigger_and_evidence_is_evidenced() {
    let evidenced = finding(
        Severity::Blocking,
        "src/calls.rs:42",
        "A busy line",
        "Test fails",
    );
    assert!(evidenced.is_evidenced_blocking());
    for rejected in [
        finding(
            Severity::NonBlocking,
            "src/calls.rs:42",
            "A busy line",
            "Test fails",
        ),
        finding(
            Severity::PreExisting,
            "src/calls.rs:42",
            "A busy line",
            "Test fails",
        ),
        finding(
            Severity::Blocking,
            "src/calls.rs",
            "A busy line",
            "Test fails",
        ),
        finding(
            Severity::Blocking,
            "src/calls.rs:",
            "A busy line",
            "Test fails",
        ),
        finding(
            Severity::Blocking,
            "src/calls.rs:4x",
            "A busy line",
            "Test fails",
        ),
        finding(Severity::Blocking, ":42", "A busy line", "Test fails"),
        finding(Severity::Blocking, "src/calls.rs:42", " ", "Test fails"),
        finding(Severity::Blocking, "src/calls.rs:42", "A busy line", ""),
    ] {
        assert!(!rejected.is_evidenced_blocking(), "{rejected:?}");
    }
}

#[test]
fn findings_and_ledger_entries_round_trip_in_snake_case() {
    let reported: ReportedFinding = serde_json::from_value(json!({
        "id":"F3","severity":"non_blocking","location":"a.rs:1","summary":"Naming"
    }))
    .unwrap();
    assert_eq!(reported.id.as_deref(), Some("F3"));
    assert_eq!(reported.finding.severity, Severity::NonBlocking);
    assert_eq!(reported.finding.evidence, "");
    assert!(
        serde_json::from_value::<ReportedFinding>(
            json!({"severity":"critical","location":"a.rs:1","summary":"Naming"})
        )
        .is_err()
    );
    let entry = LedgerEntry {
        id: "F1".into(),
        finding: finding(Severity::PreExisting, "a.rs:1", "t", "e"),
        focus: "Regressions".into(),
        cycle: 1,
        round: 2,
        status: EntryStatus::WontFix,
        reason: Some("Out of scope".into()),
    };
    let value = serde_json::to_value(&entry).unwrap();
    assert_eq!(value["severity"], "pre_existing");
    assert_eq!(value["status"], "wont_fix");
    assert_eq!(serde_json::from_value::<LedgerEntry>(value).unwrap(), entry);
    assert_eq!(
        serde_json::to_value(EntryStatus::FixNow).unwrap(),
        json!("fix_now")
    );
}

#[test]
fn ticket_commands_round_trip() {
    for command in [
        Command::CreateTicket {
            coordinator_id: "c".into(),
            repository_id: "r".into(),
            title: "Fix".into(),
            brief: "Do".into(),
        },
        Command::LeftoverWorktrees,
        Command::RemoveLeftoverWorktrees {
            ticket_ids: vec!["a".into(), "b".into()],
        },
        Command::SetVerification {
            verification: VerificationSettings::default(),
        },
        Command::SetCheckIns {
            child_turns: 25,
            human_turns: 100,
        },
    ] {
        let encoded = serde_json::to_value(&command).unwrap();
        let decoded: Command = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    }
    let removal = LeftoverRemoval {
        ticket_id: "a".into(),
        removed: false,
        reason: Some("dirty".into()),
    };
    let decoded: LeftoverRemoval =
        serde_json::from_value(serde_json::to_value(&removal).unwrap()).unwrap();
    assert_eq!(decoded, removal);
}

#[test]
fn settings_saved_before_verification_use_the_default_lenses_and_caps() {
    let settings: HostSettings = serde_json::from_value(json!({"workspaces_dir":"/w"})).unwrap();
    assert_eq!(settings.verification.max_rounds, 3);
    assert_eq!(settings.verification.max_cycles, 2);
    let verifiers: Vec<_> = settings
        .verification
        .verifiers
        .iter()
        .map(|v| (v.role.agent_label(Some(&v.focus)), v.provider))
        .collect();
    assert_eq!(
        verifiers,
        [
            ("Tester · Tests".to_owned(), None),
            ("Reviewer · Correctness".to_owned(), Some(Provider::Claude)),
            ("Reviewer · Regressions".to_owned(), Some(Provider::Codex)),
        ]
    );
    let instructions: Vec<_> = settings
        .verification
        .verifiers
        .iter()
        .map(|v| v.instruction.as_deref())
        .collect();
    assert_eq!(
        instructions,
        [
            None,
            Some("Correctness against the ticket's acceptance criteria."),
            Some("Regressions and test coverage."),
        ]
    );
    settings.verification.validate().unwrap();
    assert_eq!(settings.verification, VerificationSettings::default());
    assert_eq!(
        settings.verification.levels,
        [level(
            "Full",
            LevelKind::FullReview,
            &["Tests", "Correctness", "Regressions"],
            "for every review."
        )]
    );
}

fn level(name: &str, kind: LevelKind, verifiers: &[&str], use_when: &str) -> ReviewLevel {
    ReviewLevel {
        name: name.into(),
        kind,
        verifiers: verifiers.iter().map(|&focus| focus.into()).collect(),
        use_when: use_when.into(),
    }
}

/// Tests, Correctness, Regressions and Style with the levels Full and Light.
fn two_levels() -> VerificationSettings {
    let verifier = |role, focus: &str| VerifierConfig {
        role,
        focus: focus.into(),
        instruction: None,
        provider: None,
    };
    let mut settings = VerificationSettings::one_level(
        vec![
            verifier(Role::Tester, "Tests"),
            verifier(Role::Reviewer, "Correctness"),
            verifier(Role::Reviewer, "Regressions"),
            verifier(Role::Reviewer, "Style"),
        ],
        3,
        2,
    );
    settings.levels.push(level(
        "Light",
        LevelKind::Recheck,
        &["Correctness"],
        "after a rebase.",
    ));
    settings
}

#[test]
fn a_setting_saved_before_levels_keeps_everything_and_reads_as_one_full_level() {
    let verifiers = json!([
        {"role":"tester","focus":"Tests"},
        {"role":"reviewer","focus":"Claude","provider":"claude","instruction":"Correctness against the acceptance criteria."},
        {"role":"reviewer","focus":"Codex","provider":"codex","instruction":"Regressions and missing tests."},
        {"role":"reviewer","focus":"Style","instruction":"Run reviso:style with Codex"}
    ]);
    let settings: VerificationSettings =
        serde_json::from_value(json!({"verifiers":verifiers,"max_rounds":2,"max_cycles":3}))
            .unwrap();
    assert_eq!(
        serde_json::to_value(&settings.verifiers).unwrap(),
        verifiers
    );
    assert_eq!((settings.max_rounds, settings.max_cycles), (2, 3));
    // One level and no other: no Light is added on update.
    assert_eq!(
        settings.levels,
        [level(
            "Full",
            LevelKind::FullReview,
            &["Tests", "Claude", "Codex", "Style"],
            "for every review."
        )]
    );
    settings.validate().unwrap();
}

#[test]
fn levels_round_trip_and_a_saved_list_is_read_as_it_is() {
    let settings = two_levels();
    let encoded = serde_json::to_value(&settings).unwrap();
    assert_eq!(encoded["levels"][0]["kind"], "full_review");
    assert_eq!(
        encoded["levels"][1],
        json!({"name":"Light","kind":"recheck","verifiers":["Correctness"],"use_when":"after a rebase."})
    );
    assert_eq!(
        serde_json::from_value::<VerificationSettings>(encoded.clone()).unwrap(),
        settings
    );
    // Only a setting saved without levels gains "Full": a sent list that lost every level is
    // refused instead of being replaced.
    let mut emptied = encoded;
    emptied["levels"] = json!([]);
    let emptied: VerificationSettings = serde_json::from_value(emptied).unwrap();
    assert_eq!(
        emptied.validate().unwrap_err().to_string(),
        "Keep at least one level marked Full review."
    );
}

#[test]
fn level_rules_are_checked_on_save_in_the_cards_words() {
    two_levels().validate().unwrap();
    let refused = |change: fn(&mut VerificationSettings)| {
        let mut settings = two_levels();
        change(&mut settings);
        settings.validate().unwrap_err().to_string()
    };
    assert_eq!(
        refused(|s| s.levels[0].kind = LevelKind::Recheck),
        "Keep at least one level marked Full review."
    );
    assert_eq!(
        refused(|s| s.levels[1].verifiers.clear()),
        "Light would run no verifier. Tick another verifier for Light, or remove Light."
    );
    // Removing a verifier that a level still names.
    assert_eq!(
        refused(|s| {
            s.verifiers.pop();
        }),
        "Full runs \"Style\", which is not a configured verifier"
    );
    assert_eq!(
        refused(|s| s.levels[0].name = "light".into()),
        "Two levels are named \"Light\"."
    );
    assert_eq!(
        refused(|s| s.levels[1].verifiers.push("correctness".into())),
        "Light names the verifier \"correctness\" twice"
    );
    for name in ["  ", &"n".repeat(61)] {
        let mut settings = two_levels();
        settings.levels[1].name = name.into();
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "Give each level a name of 1–60 bytes"
        );
    }
    for note in ["", "after a rebase,\nor a small fix.", &"n".repeat(201)] {
        let mut settings = two_levels();
        settings.levels[1].use_when = note.into();
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "Say when to use Light, in one line of at most 200 characters",
            "{note:?}"
        );
    }
    // A verifier may be in no level.
    let mut settings = two_levels();
    settings.levels[0].verifiers.pop();
    settings.validate().unwrap();
    assert!(
        settings
            .levels
            .iter()
            .all(|level| !level.verifiers.contains(&"Style".into()))
    );
}

#[test]
fn a_level_is_found_by_name_and_becomes_its_verifiers_in_configured_order() {
    let mut settings = two_levels();
    settings.levels[1].verifiers = vec!["Style".into(), "tests".into()];
    let (level, verifiers) = settings.level(" light ").unwrap();
    assert_eq!(
        (level.name.as_str(), level.kind),
        ("Light", LevelKind::Recheck)
    );
    let focuses: Vec<&str> = verifiers.iter().map(|v| v.focus.as_str()).collect();
    assert_eq!(focuses, ["Tests", "Style"]);

    settings.levels.push(self::level(
        "Docs",
        LevelKind::FullReview,
        &["Style"],
        "for documentation.",
    ));
    assert_eq!(
        settings.level("Quick").unwrap_err().to_string(),
        "No review level is named \"Quick\". Name a review level: Full, Light or Docs."
    );
    // A hand-edited settings file can name a verifier that is not configured.
    settings.levels[2].verifiers = vec!["Prose".into()];
    assert_eq!(
        settings.level("Docs").unwrap_err().to_string(),
        "Docs runs \"Prose\", which is not a configured verifier. Choose its verifiers again in Models → Review."
    );
}

fn cycle(
    cycle: u32,
    level: Option<(&str, LevelKind)>,
    outcome: VerificationOutcome,
) -> Verification {
    Verification {
        cycle,
        level: level.map(|(name, kind)| CycleLevel {
            name: name.into(),
            kind,
        }),
        max_rounds: 3,
        outcome,
        rounds: vec![],
    }
}

#[test]
fn a_cycle_stored_without_a_level_decodes_and_counts_as_a_full_review() {
    let stored = json!({"cycle":1,"max_rounds":3,"outcome":"passed","rounds":[]});
    let verification: Verification = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(verification.level, None);
    assert!(verification.is_full_review());
    assert_eq!(serde_json::to_value(&verification).unwrap(), stored);

    let recheck = cycle(
        2,
        Some(("Light", LevelKind::Recheck)),
        VerificationOutcome::Passed,
    );
    assert!(!recheck.is_full_review());
    assert_eq!(
        serde_json::to_value(&recheck).unwrap()["level"],
        json!({"name":"Light","kind":"recheck"})
    );
    assert!(
        cycle(
            1,
            Some(("Full", LevelKind::FullReview)),
            VerificationOutcome::Passed
        )
        .is_full_review()
    );
}

#[test]
fn the_last_full_review_is_the_latest_passed_one_whatever_followed_it() {
    use VerificationOutcome::{Blocked, Passed};
    let mut ticket: Ticket = serde_json::from_value(json!({
        "id":"t","coordinator_id":"c","title":"Fix","brief":"Do",
        "worktree":"/w","branch":"wiffletree/fix","state":"passed"
    }))
    .unwrap();
    assert_eq!(ticket.last_full_review(), None);
    // A full review that ended blocked is not one a re-check can rest on.
    ticket.verification = Some(cycle(1, Some(("Full", LevelKind::FullReview)), Blocked));
    assert_eq!(ticket.last_full_review(), None);

    ticket.previous_cycles = vec![
        cycle(1, None, Passed),
        cycle(2, Some(("Full", LevelKind::FullReview)), Passed),
        cycle(3, Some(("Light", LevelKind::Recheck)), Passed),
    ];
    ticket.verification = Some(cycle(4, Some(("Light", LevelKind::Recheck)), Blocked));
    assert_eq!(ticket.last_full_review().map(|v| v.cycle), Some(2));
    // The latest cycle is itself the last full review once it passes.
    ticket.verification = Some(cycle(4, Some(("Thorough", LevelKind::FullReview)), Passed));
    assert_eq!(ticket.last_full_review().map(|v| v.cycle), Some(4));
}

#[test]
fn a_saved_setting_without_a_cycle_cap_keeps_its_verifiers_and_uses_two_cycles() {
    let settings: VerificationSettings = serde_json::from_value(json!({
        "verifiers":[
            {"role":"reviewer","focus":"Claude","provider":"claude"},
            {"role":"reviewer","focus":"Codex","provider":"codex"}
        ],
        "max_rounds":2
    }))
    .unwrap();
    assert_eq!((settings.max_rounds, settings.max_cycles), (2, 2));
    let focuses: Vec<_> = settings
        .verifiers
        .iter()
        .map(|v| v.focus.as_str())
        .collect();
    assert_eq!(focuses, ["Claude", "Codex"]);
}

#[test]
fn a_saved_verifier_size_is_ignored() {
    let verifier: VerifierConfig = serde_json::from_value(
        json!({"role":"reviewer","focus":"Codex","provider":"codex","size":"small"}),
    )
    .unwrap();
    assert_eq!(verifier.provider, Some(Provider::Codex));
}

#[test]
fn verification_settings_reject_duplicate_focus_and_out_of_range_caps() {
    let verifier = |role, focus: &str| VerifierConfig {
        role,
        focus: focus.into(),
        instruction: None,
        provider: None,
    };
    let mut settings = VerificationSettings::one_level(
        vec![
            verifier(Role::Tester, "Claude"),
            verifier(Role::Tester, "Codex"),
            verifier(Role::Reviewer, "Style"),
        ],
        5,
        5,
    );
    settings.validate().unwrap();
    settings.max_rounds = 0;
    assert!(settings.validate().is_err());
    settings.max_rounds = 6;
    assert!(settings.validate().is_err());
    settings.max_rounds = 2;
    for cycles in [0, 6] {
        settings.max_cycles = cycles;
        assert!(settings.validate().is_err(), "{cycles}");
    }
    settings.max_cycles = 2;
    settings.verifiers.push(verifier(Role::Reviewer, "style"));
    assert!(settings.validate().is_err());
    settings.verifiers.pop();
    settings.verifiers.push(verifier(Role::Implementer, "Fix"));
    assert!(settings.validate().is_err());
}

#[test]
fn latest_results_keep_each_verifiers_last_round() {
    let run = |session: &str, result| VerifierRun {
        session_id: session.into(),
        role: Role::Tester,
        focus: session.into(),
        message_id: format!("m-{session}"),
        result,
        report_id: None,
        reason: None,
    };
    let verification = Verification {
        cycle: 1,
        level: None,
        max_rounds: 2,
        outcome: VerificationOutcome::Running,
        rounds: vec![
            VerificationRound {
                round: 1,
                commit: "a".into(),
                verifiers: vec![
                    run("one", VerifierResult::Passed),
                    run("two", VerifierResult::Failed),
                ],
            },
            VerificationRound {
                round: 2,
                commit: "b".into(),
                verifiers: vec![run("two", VerifierResult::Passed)],
            },
        ],
    };
    let latest = verification.latest_results();
    assert_eq!(latest.len(), 2);
    assert!(latest.iter().all(|r| r.result == VerifierResult::Passed));
    assert_eq!(verification.verified_commit(), Some("b"));
}
