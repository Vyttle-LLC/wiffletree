//! The real Service runs every due turn at once, with the fake provider standing in for the
//! CLIs: no project or host-wide turn limit holds workers back. Kept in its own test binary
//! because it sets the provider environment for the process.
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};
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

fn working(service: &Service, sessions: &[String]) -> usize {
    let snapshot: Snapshot = serde_json::from_value(request(service, Command::Snapshot)).unwrap();
    snapshot
        .sessions
        .iter()
        .filter(|s| sessions.contains(&s.id) && s.status == Status::Working)
        .count()
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn git(path: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(path)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn ten_workers_in_two_projects_run_at_once() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let workers = {
        let mut host = Host::open(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let mut workers = vec![];
        for name in ["First", "Second"] {
            let project = host.create_project(name).unwrap();
            let main = host
                .sessions()
                .unwrap()
                .into_iter()
                .find(|s| s.project_id == project.id)
                .unwrap();
            let repository = host
                .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
                .unwrap();
            for n in 0..5 {
                let ticket = host
                    .create_ticket(&main.id, &repository.id, &format!("Slow {n}"), "Work")
                    .unwrap();
                // Each instruction is a turn that runs until something stops it.
                let worker = host
                    .assign_ticket(
                        &ticket.id,
                        Role::Implementer,
                        Provider::Claude,
                        "JUST_REPLY HANG_UNTIL_CANCELLED",
                        None,
                    )
                    .unwrap();
                let profile = ModelProfile {
                    provider: Provider::Claude,
                    model: "opus".into(),
                    effort: "medium".into(),
                };
                host.configure_session(&worker.id, profile).unwrap();
                workers.push(worker.id);
            }
            host.set_live(&project.id, true).unwrap();
        }
        workers
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

    until("every worker in a turn at once", || {
        working(&service, &workers) == workers.len()
    });

    for worker in &workers {
        request(
            &service,
            Command::SetStatus {
                session_id: worker.clone(),
                status: Status::Paused,
            },
        );
    }
    until("the turns to stop", || working(&service, &workers) == 0);
}
