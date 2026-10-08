//! Coordinator-run monitoring through the real Service, with the fake provider standing in for
//! the CLIs. Kept in its own test binary because it sets the provider environment for the process.
use serde_json::Value;
use std::{path::Path, time::Duration};
use workspace_core::*;
use workspace_host::{Host, now, service::Service};

fn request(service: &Service, command: Command) -> Value {
    service
        .request(command)
        .unwrap()
        .recv_blocking()
        .unwrap()
        .unwrap()
}

fn messages(service: &Service, session: &str) -> Vec<Message> {
    let value = request(
        service,
        Command::Messages {
            session_id: session.into(),
            before: None,
            limit: 100,
        },
    );
    serde_json::from_value(value).unwrap()
}

fn snapshot(service: &Service) -> Snapshot {
    serde_json::from_value(request(service, Command::Snapshot)).unwrap()
}

/// Polls with a snapshot, which also lets the actor run its scheduler.
fn until(service: &Service, what: &str, mut done: impl FnMut(&Snapshot) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !done(&snapshot(service)) {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn idle(snapshot: &Snapshot) -> bool {
    snapshot
        .sessions
        .iter()
        .all(|s| s.status != Status::Working)
}

/// Moves a timer's clock forward to its next slot, as if that much time had passed.
fn advance_to_next_fire(db: &rusqlite::Connection, timer: &str) {
    db.execute(
        "UPDATE schedules SET first_at=first_at-(next_fire_at-?1),until=until-(next_fire_at-?1),next_fire_at=?1 WHERE id=?2",
        rusqlite::params![now() - 1, timer],
    )
    .unwrap();
}

fn outputs(service: &Service, session: &str, body: &str) -> usize {
    messages(service, session)
        .iter()
        .filter(|m| m.id.starts_with("output:") && m.body == body)
        .count()
}

#[test]
fn coordinators_run_scheduled_checks_themselves_and_post_each_result() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Initial",
        ],
    ] {
        let git = std::process::Command::new("git")
            .current_dir(&repo)
            .args(args)
            .status()
            .unwrap();
        assert!(git.success());
    }
    let monitored = {
        let mut host = Host::open(home.path()).unwrap();
        host.create_project("Monitor").unwrap()
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
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();

    // "Monitor service X every 30 minutes for 3 hours": the main coordinator schedules the
    // check and runs each one itself.
    let main = snapshot(&service)
        .sessions
        .into_iter()
        .find(|s| s.project_id == monitored.id)
        .unwrap();
    request(
        &service,
        Command::Send {
            id: "monitor".into(),
            sender: None,
            recipient: main.id.clone(),
            body: "SCHEDULE_MONITOR".into(),
            attachments: vec![],
        },
    );
    let mut timer = None;
    until(&service, "the monitor timer", |s| {
        timer = s
            .schedules
            .iter()
            .find(|t| t.session_id == main.id)
            .map(|t| t.id.clone());
        timer.is_some() && idle(s)
    });
    let timer = timer.unwrap();
    for check in 1..=6 {
        advance_to_next_fire(&db, &timer);
        until(&service, &format!("check {check}"), |s| {
            idle(s) && outputs(&service, &main.id, "SERVICE_HEALTHY") == check
        });
    }
    let snapshot = snapshot(&service);
    assert!(
        !snapshot.schedules.iter().any(|t| t.id == timer),
        "the timer ends after three hours"
    );
    let team_of_main = snapshot
        .sessions
        .iter()
        .filter(|s| s.project_id == monitored.id)
        .count();
    assert_eq!(team_of_main, 1, "no child sessions");
    let fires = messages(&service, &main.id)
        .into_iter()
        .filter(|m| m.id.starts_with("timer:"))
        .collect::<Vec<_>>();
    assert_eq!(fires.len(), 6);
    assert!(fires.iter().all(|m| m.receipt == Receipt::Completed));
}
