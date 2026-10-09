//! An SH-1171-sized task end to end through the real Service, with the fake provider standing
//! in for the CLIs: one repository, one ticket, three verifiers, one failure fixed in round 2.
//! Kept in its own test binary because it sets the provider environment for the process.
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::{path::Path, time::Duration};
use workspace_core::*;
use workspace_host::{Host, service::Service};

fn request(service: &Service, command: Command) -> Value {
    service
        .request(command)
        .unwrap()
        .recv_blocking()
        .unwrap()
        .unwrap()
}

fn snapshot(service: &Service) -> Snapshot {
    serde_json::from_value(request(service, Command::Snapshot)).unwrap()
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn verifier(role: Role, focus: &str, provider: Option<Provider>) -> VerifierConfig {
    VerifierConfig {
        role,
        focus: focus.into(),
        instruction: None,
        provider,
    }
}

/// Lets the tester role run on either provider.
fn testers_on_both_providers(host: &mut Host) {
    let mut selection = host.model_selection().unwrap();
    selection
        .role_providers
        .insert(Role::Tester, [Provider::Claude, Provider::Codex].into());
    host.set_model_selection(&selection).unwrap();
}

#[test]
fn a_single_repository_ticket_runs_without_relay_turns_and_verifies_concurrently() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let (project, main) = {
        let mut host = Host::open(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
            .unwrap();
        testers_on_both_providers(&mut host);
        host.set_verification(VerificationSettings {
            verifiers: vec![
                verifier(Role::Tester, "Claude", Some(Provider::Claude)),
                verifier(Role::Tester, "Codex", Some(Provider::Codex)),
                verifier(Role::Reviewer, "Style", None),
            ],
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
        let project = host.create_project("SH-1171").unwrap();
        let main = host.sessions().unwrap().remove(0);
        host.attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        (project, main)
    };
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider.py");
    // SAFETY: this test binary holds one test, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("WORKSPACE_CLAUDE_BIN", &fixture);
        std::env::set_var("WORKSPACE_CODEX_BIN", &fixture);
        std::env::set_var("WORKSPACE_TEST_USAGE_DIR", scratch.path());
    }
    let service = Service::start(
        home.path().to_path_buf(),
        env!("CARGO_BIN_EXE_workspace-host").into(),
    )
    .unwrap();

    request(
        &service,
        Command::Send {
            id: "start".into(),
            sender: None,
            recipient: main.id.clone(),
            body: "START_HANDOFF FAIL_CODEX_ROUND_1".into(),
            attachments: vec![],
        },
    );
    // While round 1 runs, the ticket's one worktree is the only one on its branch.
    let mut ticket = None;
    until("round 1 to start", || {
        ticket = snapshot(&service)
            .tickets
            .into_iter()
            .find(|t| t.state == "verifying");
        ticket.is_some()
    });
    let ticket = ticket.unwrap();
    let registered = git(&repo, &["worktree", "list", "--porcelain"]);
    let on_branch: Vec<&str> = registered
        .split("\n\n")
        .filter(|entry| entry.contains(&format!("branch refs/heads/{}", ticket.branch)))
        .collect();
    assert_eq!(on_branch.len(), 1, "{registered}");
    let path = on_branch[0]
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("worktree ");
    assert_eq!(
        std::fs::canonicalize(path).unwrap(),
        std::fs::canonicalize(&ticket.worktree).unwrap()
    );

    until("acceptance and a quiet host", || {
        let snapshot = snapshot(&service);
        snapshot
            .tickets
            .iter()
            .any(|t| t.state == "accepted" && !Path::new(&t.worktree).exists())
            && snapshot
                .sessions
                .iter()
                .all(|s| s.status != Status::Working)
    });
    let snapshot = snapshot(&service);
    assert_eq!(snapshot.tickets.len(), 1);
    let ticket = snapshot.tickets[0].clone();
    let verification = ticket.verification.clone().unwrap();
    assert_eq!(verification.outcome, VerificationOutcome::Passed);
    assert_eq!(verification.rounds.len(), 2);
    assert_eq!(verification.rounds[0].verifiers.len(), 3);
    let round_two: Vec<&str> = verification.rounds[1]
        .verifiers
        .iter()
        .map(|v| v.focus.as_str())
        .collect();
    assert_eq!(round_two, ["Codex"], "only the failed verifier re-ran");

    // No relay: the project's sessions are its coordinator and the ticket's agents.
    assert!(
        !snapshot
            .sessions
            .iter()
            .any(|s| s.role == Role::TaskOrchestrator)
    );
    let agents: Vec<&Session> = snapshot
        .sessions
        .iter()
        .filter(|s| s.project_id == project.id && s.id != main.id)
        .collect();
    assert_eq!(agents.len(), 4, "one implementer and three verifiers");
    for agent in &agents {
        assert_eq!(agent.parent_id.as_ref(), Some(&main.id));
        let runtime = snapshot
            .runtimes
            .iter()
            .find(|r| r.session_id == agent.id)
            .unwrap();
        assert_eq!(runtime.workdir.as_ref(), Some(&ticket.worktree));
    }
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    for agent in &agents {
        // Every input an agent received came from the coordinator.
        let senders: Vec<Option<String>> = db
            .prepare("SELECT sender FROM messages WHERE recipient=?1 AND (sender IS NULL OR sender<>recipient)")
            .unwrap()
            .query_map([&agent.id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!senders.is_empty());
        assert!(
            senders.iter().all(|s| s.as_ref() == Some(&main.id)),
            "{}: {senders:?}",
            agent.name
        );
        // Every report it sent went to the coordinator.
        let elsewhere: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE sender=?1 AND id LIKE 'report:%' AND recipient<>?2",
                [&agent.id, &main.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(elsewhere, 0);
    }
    let roles: Vec<String> = db
        .prepare("SELECT DISTINCT s.role FROM provider_runs r JOIN sessions s ON s.id=r.session_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for role in &roles {
        assert!(
            ["project_orchestrator", "implementer", "tester", "reviewer"].contains(&role.as_str()),
            "{role}"
        );
    }

    // The coordinator woke for the request, the ready report and the passed cycle; every one of
    // its turns took a waking message, so verdicts and the fix never started one.
    let coordinator_runs: Vec<String> = db
        .prepare("SELECT messages FROM provider_runs WHERE session_id=?1 ORDER BY started_at")
        .unwrap()
        .query_map([&main.id], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(coordinator_runs.len(), 3, "{coordinator_runs:?}");
    for run in &coordinator_runs {
        let woke: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM json_each(?1) j JOIN messages m ON m.id=j.value WHERE m.quiet=0)",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        assert!(woke, "{run}");
    }
    // The same implementer session took the failure and fixed it.
    let implementer = agents.iter().find(|s| s.role == Role::Implementer).unwrap();
    let failure: Option<String> = db
        .query_row(
            "SELECT id FROM messages WHERE recipient=?1 AND id LIKE 'verification:%'",
            [&implementer.id],
            |r| r.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(failure, Some(format!("verification:{}:1:1", ticket.id)));

    // Round 1's verifiers ran at the same time: the latest start precedes the earliest finish.
    let (overlapped, runs): (bool, i64) = db
        .query_row(
            "SELECT MAX(r.started_at) < MIN(r.finished_at), COUNT(*)
             FROM tickets t, json_each(t.data,'$.verification.rounds[0].verifiers') v
             JOIN provider_runs r ON r.session_id=json_extract(v.value,'$.session_id')
              AND EXISTS(SELECT 1 FROM json_each(r.messages) m WHERE m.value=json_extract(v.value,'$.message_id'))
             WHERE t.id=?1",
            [&ticket.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(runs, 3);
    assert!(overlapped, "round 1's verifier runs overlap");

    // One ticket, one branch: no other worktree or branch was made for it.
    let branches = git(&repo, &["branch", "--list", "wiffletree/*"]);
    assert_eq!(branches.lines().count(), 1, "{branches}");
    assert!(git(&repo, &["show", &format!("{}:fix.txt", ticket.branch)]).contains("fixed"));
}
