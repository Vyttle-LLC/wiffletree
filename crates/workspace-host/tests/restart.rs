//! A host restart through the real Service, with the fake provider standing in for the CLIs.
//! Kept in its own test binary because it sets the provider environment for the process.
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use workspace_core::*;
use workspace_host::{Host, now, service::Service};

const MINUTE: i64 = 60_000;

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

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn replied(service: &Service, session: &str) -> bool {
    messages(service, session)
        .iter()
        .any(|m| m.id.starts_with("output:") && m.body == "NOTED")
}

fn prompts(dir: &Path, session: &str) -> String {
    std::fs::read_to_string(dir.join(format!("{session}.txt"))).unwrap_or_default()
}

#[test]
fn a_live_project_resumes_after_a_restart_except_for_the_session_it_interrupted() {
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
    let every =
        |label: &str| json!({"label":label,"prompt":format!("JUST_REPLY {label}"),"every":"30m"});

    // Before the stop: project A is live, its worker is mid-turn and both coordinators have timers.
    // Project B was never started.
    let (live, idle, worker, other, work) = {
        let mut host = Host::open(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let live = host.create_project("Live").unwrap();
        let idle = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&live.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let ticket = host
            .create_ticket(&idle.id, &repository.id, "Backend", "Work")
            .unwrap();
        let worker = host
            .assign_ticket(
                &ticket.id,
                Role::Implementer,
                Provider::Claude,
                "Work",
                None,
            )
            .unwrap();
        // The assignment was handled before the stop.
        let assignment = format!("assignment:{}", worker.id);
        for receipt in [
            Receipt::Delivered,
            Receipt::Acknowledged,
            Receipt::Completed,
        ] {
            host.advance_receipt(&assignment, receipt).unwrap();
        }
        // Every real creation path gives an agent a model; an unpinned one would be held.
        host.configure_session(
            &worker.id,
            ModelProfile {
                provider: Provider::Claude,
                model: "opus".into(),
                effort: "high".into(),
            },
        )
        .unwrap();
        let stopped = host.create_project("Stopped").unwrap();
        let other = host
            .sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.project_id == stopped.id)
            .unwrap();
        host.set_live(&live.id, true).unwrap();
        for (session, label) in [(&idle, "main check"), (&other, "other check")] {
            host.agent_tool(&session.id, "schedule", every(label))
                .unwrap();
        }
        let work = host
            .send("work", Some(&idle.id), &worker.id, "JUST_REPLY work")
            .unwrap();
        host.advance_receipt(&work.id, Receipt::Delivered).unwrap();
        host.set_status(&worker.id, Status::Working).unwrap();
        (live, idle, worker, other, work)
    };
    // The host dies mid-turn and stays down past two timer slots.
    let run = "run-cut-off";
    {
        let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,?3,?4,'{}')",
            rusqlite::params![run, worker.id, json!([work.id]).to_string(), now() - 70 * MINUTE],
        )
        .unwrap();
        db.execute(
            "UPDATE schedules SET first_at=first_at-?1,next_fire_at=next_fire_at-?1",
            [65 * MINUTE],
        )
        .unwrap();
    }

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider.py");
    // SAFETY: this test binary holds one test, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("WORKSPACE_CLAUDE_BIN", &fixture);
        std::env::set_var("WORKSPACE_CODEX_BIN", &fixture);
        std::env::set_var("WORKSPACE_TEST_USAGE_DIR", scratch.path());
        std::env::set_var("WORKSPACE_TEST_PROMPT_DIR", scratch.path());
    }
    let service = Service::start(
        home.path().to_path_buf(),
        env!("CARGO_BIN_EXE_workspace-host").into(),
    )
    .unwrap();

    // The live project carries on: the idle coordinator's timer fires once, late, with no Go live.
    until("the idle coordinator's timer turn", || {
        replied(&service, &idle.id)
    });
    let snapshot: Snapshot = serde_json::from_value(request(&service, Command::Snapshot)).unwrap();
    assert!(snapshot.live_projects.contains(&live.id));
    assert!(
        !snapshot.live_projects.contains(&other.project_id),
        "B stays stopped"
    );
    let prompt = prompts(scratch.path(), &idle.id);
    for expected in [
        "The workspace host restarted",
        "Missed: timer",
        "Sender: your timer",
        "Timer status: late; 1 later slot(s)",
        "JUST_REPLY main check",
    ] {
        assert!(prompt.contains(expected), "{expected}\n{prompt}");
    }
    let fires = |session: &str| {
        messages(&service, session)
            .into_iter()
            .filter(|m| m.id.starts_with("timer:"))
            .collect::<Vec<_>>()
    };
    assert_eq!(fires(&idle.id).len(), 1, "one fire for both missed slots");

    // The interrupted session waits, its input held, until the human reconciles it.
    let session = |id: &str| {
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .status
    };
    assert_eq!(session(&worker.id), Status::Disconnected);
    let held = messages(&service, &worker.id);
    assert_eq!(
        held.iter().find(|m| m.id == work.id).unwrap().receipt,
        Receipt::Held
    );
    assert!(prompts(scratch.path(), &worker.id).is_empty());
    // B's timer fires into its queue, but nothing runs there.
    assert_eq!(fires(&other.id)[0].receipt, Receipt::Queued);
    assert!(prompts(scratch.path(), &other.id).is_empty());

    request(
        &service,
        Command::ReconcileSession {
            session_id: worker.id.clone(),
            retry: true,
        },
    );
    until("the reconciled worker's turn", || {
        replied(&service, &worker.id)
    });
    let prompt = prompts(scratch.path(), &worker.id);
    for expected in [
        format!("Retry of interrupted turn {run}"),
        "JUST_REPLY work".into(),
    ] {
        assert!(prompt.contains(&expected), "{expected}\n{prompt}");
    }
    assert!(prompts(scratch.path(), &other.id).is_empty());
}
