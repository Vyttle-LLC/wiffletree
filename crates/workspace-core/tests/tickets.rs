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
fn settings_saved_before_verification_use_one_tester_and_two_rounds() {
    let settings: HostSettings = serde_json::from_value(json!({"workspaces_dir":"/w"})).unwrap();
    assert_eq!(settings.verification.max_rounds, 2);
    assert_eq!(settings.verification.verifiers.len(), 1);
    assert_eq!(settings.verification.verifiers[0].role, Role::Tester);
    settings.verification.validate().unwrap();
}

#[test]
fn verification_settings_reject_duplicate_focus_and_out_of_range_caps() {
    let verifier = |role, focus: &str| VerifierConfig {
        role,
        focus: focus.into(),
        instruction: None,
        provider: None,
        size: None,
    };
    let mut settings = VerificationSettings {
        verifiers: vec![
            verifier(Role::Tester, "Claude"),
            verifier(Role::Tester, "Codex"),
            verifier(Role::Reviewer, "Style"),
        ],
        max_rounds: 5,
    };
    settings.validate().unwrap();
    settings.max_rounds = 0;
    assert!(settings.validate().is_err());
    settings.max_rounds = 6;
    assert!(settings.validate().is_err());
    settings.max_rounds = 2;
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
