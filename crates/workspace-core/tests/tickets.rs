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
    let mut settings = VerificationSettings {
        verifiers: vec![
            verifier(Role::Tester, "Claude"),
            verifier(Role::Tester, "Codex"),
            verifier(Role::Reviewer, "Style"),
        ],
        max_rounds: 5,
        max_cycles: 5,
    };
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
