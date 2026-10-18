//! The real Service runs every due turn at once, with the fake provider standing in for the
//! CLIs: no project or host-wide turn limit holds workers back. Kept in its own test binary
//! because it sets the provider environment for the process.
mod common;
use common::{git, request, until_changed as until};
use std::time::{Duration, Instant};
use workspace_core::*;
use workspace_host::Host;

/// Far longer than ten turns take to start and stop on a loaded machine. It decides nothing
/// about a passing run; it only makes a regression that never gets there fail instead of hang.
const OVERALL: Duration = Duration::from_secs(300);

fn runtimes<'a>(
    snapshot: &'a Snapshot,
    sessions: &'a [String],
) -> impl Iterator<Item = &'a SessionRuntime> {
    snapshot
        .runtimes
        .iter()
        .filter(|r| sessions.contains(&r.session_id))
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
        let mut host = Host::open_set_up(home.path()).unwrap();
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
    // SAFETY: this binary has one test and owns its provider environment.
    let service = unsafe {
        common::ServiceFixture::new(home.path())
            .fake_provider(scratch.path())
            .start()
    };
    let deadline = Instant::now() + OVERALL;

    // Every turn started before any of them finished, so all ten ran at once. A limit would
    // start some only after others finished, however long that took.
    let started = until(&service, deadline, |s| {
        runtimes(s, &workers)
            .filter(|r| r.last_started_at.is_some())
            .count()
            == workers.len()
    });
    let last_start = runtimes(&started, &workers)
        .filter_map(|r| r.last_started_at)
        .max()
        .unwrap();
    for runtime in runtimes(&started, &workers) {
        assert!(
            runtime.last_finished_at.is_none_or(|f| f > last_start),
            "{runtime:?} finished before the last turn started at {last_start}"
        );
    }

    for worker in &workers {
        request(
            &service,
            Command::SetStatus {
                session_id: worker.clone(),
                status: Status::Paused,
            },
        );
    }
    until(&service, deadline, |s| {
        runtimes(s, &workers).all(|r| r.last_finished_at.is_some())
    });
}
