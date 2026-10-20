//! The real scheduler without turn limits, with the fake provider standing in for the CLIs: a
//! verification round waiting on its implementer holds no other worker back, and a coordinator
//! runs past 100 turns without the human, raising a check-in instead of stopping. Kept in its
//! own test binary because it sets the provider environment for the process.
mod common;
// Direct store writes below notify admission explicitly; `until` only reads snapshots.
use common::{git, request, snapshot, until_snapshot as until};
use serde_json::json;
use std::time::Duration;
use workspace_core::*;
use workspace_host::{Host, now};

fn status(snapshot: &Snapshot, session: &str) -> Status {
    snapshot
        .sessions
        .iter()
        .find(|s| s.id == session)
        .unwrap()
        .status
}

/// Asserts the long-running implementer is still in its turn, naming why it is not.
fn assert_working(snapshot: &Snapshot, session: &str) {
    let runtime = snapshot.runtimes.iter().find(|r| r.session_id == session);
    assert_eq!(
        status(snapshot, session),
        Status::Working,
        "last error: {:?}",
        runtime.and_then(|r| r.last_error.as_deref())
    );
}

fn runs(db: &rusqlite::Connection, session: &str) -> i64 {
    db.query_row(
        "SELECT COUNT(*) FROM provider_runs WHERE session_id=?1 AND finished_at IS NOT NULL",
        [session],
        |r| r.get(0),
    )
    .unwrap()
}

fn implementer(
    host: &mut Host,
    coordinator: &str,
    repository: &str,
    title: &str,
    instruction: &str,
) -> Session {
    let ticket = host
        .create_ticket(coordinator, repository, title, "Work")
        .unwrap();
    let session = host
        .assign_ticket(
            &ticket.id,
            Role::Implementer,
            Provider::Claude,
            instruction,
            None,
        )
        .unwrap();
    let profile = ModelProfile {
        provider: Provider::Claude,
        model: "opus".into(),
        effort: "medium".into(),
    };
    host.configure_session(&session.id, profile).unwrap();
    session
}

#[test]
fn a_waiting_round_holds_no_one_and_a_coordinator_runs_past_a_hundred_turns() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let (reviewing, writer, other, busy) = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let reviewing = host.create_project("Reviewing").unwrap();
        let main = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&reviewing.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        // Its implementer reports ready and then holds the worktree in a long turn, so the
        // round the coordinator starts waits for it.
        let writer = implementer(
            &mut host,
            &main.id,
            &repository.id,
            "Written",
            "JUST_REPLY HANG_UNTIL_CANCELLED",
        );
        host.agent_tool(
            &writer.id,
            "report",
            json!({"message_id":"ready","kind":"ready_for_testing","body":"Fixture planned; Fixture written"}),
        )
        .unwrap();
        let other = implementer(&mut host, &main.id, &repository.id, "Other", "JUST_REPLY");
        host.set_live(&reviewing.id, true).unwrap();
        let busy = host.create_project("Busy").unwrap();
        host.set_live(&busy.id, true).unwrap();
        (reviewing, writer, other, busy)
    };
    // SAFETY: this binary has one test and owns its provider environment.
    let service = unsafe {
        common::ServiceFixture::new(home.path())
            .fake_provider(scratch.path())
            .start()
    };
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();

    // The coordinator starts a round while the implementer holds the worktree.
    let mut ticket = None;
    until(&service, "a round waiting on the implementer", |s| {
        ticket = s
            .tickets
            .iter()
            .find(|t| t.state == TicketState::Verifying)
            .cloned();
        ticket.is_some() && runs(&db, &other.id) == 1
    });
    assert_working(&snapshot(&service), &writer.id);
    let round = ticket.unwrap().verification.unwrap().rounds.remove(0);

    // Another ticket's worker still starts while the round waits.
    request(
        &service,
        Command::Send {
            id: "again".into(),
            sender: None,
            recipient: other.id.clone(),
            body: "JUST_REPLY again".into(),
            attachments: vec![],
            uploads: vec![],
        },
    );
    until(&service, "the other worker's second turn", |_| {
        runs(&db, &other.id) == 2
    });
    assert_working(&snapshot(&service), &writer.id);
    for verifier in &round.verifiers {
        assert_eq!(runs(&db, &verifier.session_id), 0, "the round still waits");
    }
    request(
        &service,
        Command::SetLive {
            project_id: reviewing.id.clone(),
            enabled: false,
        },
    );

    // A coordinator woken 101 times without the human keeps running and checks in instead.
    let coordinator = snapshot(&service)
        .sessions
        .into_iter()
        .find(|s| s.project_id == busy.id)
        .unwrap();
    for turn in 1..=101 {
        db.execute(
            "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,NULL,?3,'JUST_REPLY tick','queued',?4)",
            rusqlite::params![format!("tick-{turn}"), busy.id, coordinator.id, now()],
        )
        .unwrap();
        // A human Send would reset the coordinator's turn count. Notify the actor of
        // the fixture's direct write without changing the project's live state.
        request(
            &service,
            Command::SetLive {
                project_id: busy.id.clone(),
                enabled: true,
            },
        );
        until(&service, &format!("coordinator turn {turn}"), |s| {
            runs(&db, &coordinator.id) == turn && status(s, &coordinator.id) != Status::Working
        });
    }
    let snapshot = snapshot(&service);
    assert!(snapshot.live_projects.contains(&busy.id), "never stopped");
    let check_ins: Vec<_> = snapshot
        .attention
        .iter()
        .filter(|a| a.session_id == coordinator.id && a.operation_id.starts_with("turn-count:"))
        .collect();
    assert_eq!(check_ins.len(), 1, "{:?}", snapshot.attention);
    assert!(check_ins[0].prompt.contains("has run 100 turns"));
}
