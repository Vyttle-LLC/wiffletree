use serde_json::{Value, json};
use std::time::{Duration, Instant};
use workspace_core::*;
use workspace_host::{Host, now, service::Service};

const MINUTE: i64 = 60_000;

fn project(home: &tempfile::TempDir) -> (Host, Session) {
    let mut host = Host::open(home.path()).unwrap();
    host.create_project("Monitor").unwrap();
    let root = host.sessions().unwrap().remove(0);
    (host, root)
}

fn schedule(host: &mut Host, session: &str, args: Value) -> anyhow::Result<Value> {
    host.agent_tool(session, "schedule", args)
}

fn next_fire(host: &Host) -> Option<i64> {
    host.snapshot().unwrap().schedules.first()?.next_fire_at
}

/// The fire's turn ran to completion.
fn complete(host: &mut Host, message: &str) {
    for receipt in [
        Receipt::Delivered,
        Receipt::Acknowledged,
        Receipt::Completed,
    ] {
        host.advance_receipt(message, receipt).unwrap();
    }
}

fn fires(host: &Host, session: &str) -> Vec<Message> {
    host.messages(session, None, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.id.starts_with("timer:"))
        .collect()
}

#[test]
fn a_timer_keeps_its_cadence_across_restarts_and_folds_missed_slots_into_one_late_fire() {
    let home = tempfile::tempdir().unwrap();
    let (mut host, root) = project(&home);
    let created = now();
    let args =
        json!({"label":"Service check","prompt":"Check the service","every":"30m","until":"+3h"});
    let id = schedule(&mut host, &root.id, args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = next_fire(&host).unwrap();
    assert!((first - created - 30 * MINUTE).abs() < 5_000);

    // A host kill and restart keeps the schedule.
    drop(host);
    let mut host = Host::open(home.path()).unwrap();
    assert_eq!(next_fire(&host), Some(first));

    // On time: one fire from the session to itself, not late.
    assert_eq!(
        host.fire_due_schedules(first + 1_000).unwrap().next,
        Some(first + 30 * MINUTE)
    );
    let fired = fires(&host, &root.id);
    assert_eq!(fired.len(), 1);
    let on_time = &fired[0];
    assert_eq!(on_time.sender.as_deref(), Some(root.id.as_str()));
    assert!(on_time.body.starts_with("[timer] Service check\n"));
    assert!(on_time.body.contains(&id) && !on_time.body.contains("late"));
    assert_eq!(
        host.fire_status(&on_time.id, first + 1_000)
            .unwrap()
            .as_deref(),
        Some("Timer status: on time")
    );
    // Enqueued on time but delivered hours later, say from a stopped project: late after all.
    assert_eq!(
        host.fire_status(&on_time.id, first + 120 * MINUTE)
            .unwrap()
            .as_deref(),
        Some("Timer status: late")
    );
    assert!(on_time.body.contains("not a message from the human"));
    complete(&mut host, &on_time.id);

    // Down across three slots: one late fire, then the next slot on the original cadence.
    drop(host);
    let mut host = Host::open(home.path()).unwrap();
    let back = first + 95 * MINUTE;
    assert_eq!(
        host.fire_due_schedules(back).unwrap().next,
        Some(first + 120 * MINUTE)
    );
    assert_eq!(
        host.fire_due_schedules(back).unwrap().next,
        Some(first + 120 * MINUTE)
    );
    let fired = fires(&host, &root.id);
    assert_eq!(fired.len(), 2, "exactly one fire for the outage");
    let late = &fired[1];
    assert_eq!(late.id, format!("timer:{id}:{}", first + 30 * MINUTE));
    let status = host.fire_status(&late.id, back).unwrap().unwrap();
    assert!(status.contains("late; 2 later slot(s)"), "{status}");

    // A slot that passes while that fire still waits folds into it.
    host.fire_due_schedules(first + 121 * MINUTE).unwrap();
    let fired = fires(&host, &root.id);
    assert_eq!(fired.len(), 2);
    assert_eq!(fired[1].body, late.body, "a stored fire never changes");
    let status = host
        .fire_status(&fired[1].id, first + 121 * MINUTE)
        .unwrap()
        .unwrap();
    assert!(status.contains("late; 3 later slot(s)"), "{status}");
    complete(&mut host, &fired[1].id);

    // The last slot is `until`; then the timer is finished.
    assert_eq!(
        host.fire_due_schedules(first + 150 * MINUTE).unwrap().next,
        None
    );
    assert_eq!(fires(&host, &root.id).len(), 3);
    assert!(host.snapshot().unwrap().schedules.is_empty());
}

#[test]
fn only_coordinators_schedule_within_limits_and_stopped_timers_never_fire() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
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
            .current_dir(repo.path())
            .args(args)
            .status()
            .unwrap();
        assert!(git.success());
    }
    let (mut host, root) = project(&home);
    let repository = host
        .attach_repository(&root.project_id, repo.path().to_str().unwrap(), "HEAD")
        .unwrap();
    // Another project's coordinator, archived below.
    let other = host.create_project("Other").unwrap();
    let coordinator = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == other.id)
        .unwrap();
    let worker = host
        .create_session(
            &root.project_id,
            &root.id,
            Some(&repository.id),
            "Implementer",
            Role::Implementer,
            Provider::Claude,
        )
        .unwrap();
    let check = |every: &str| json!({"label":"check","prompt":"Check","every":every});

    let refused = schedule(&mut host, &worker.id, check("30m")).unwrap_err();
    assert!(
        refused.to_string().contains("Only coordinators"),
        "{refused}"
    );
    assert!(
        host.agent_tool(&worker.id, "list_schedules", json!({}))
            .is_err()
    );
    let too_often = schedule(&mut host, &root.id, check("4m")).unwrap_err();
    assert!(too_often.to_string().contains("at least 5m"), "{too_often}");
    for bad in [
        json!({"label":"x".repeat(81),"prompt":"Check","every":"30m"}),
        json!({"label":"check","prompt":"x".repeat(4097),"every":"30m"}),
        json!({"label":"check","prompt":"Check","at":"2020-01-01T00:00:00Z"}),
        json!({"label":"check","prompt":"Check","every":"30m","until":"+10m"}),
        json!({"label":"check","prompt":"Check"}),
        json!({"label":"check","prompt":"Check","every":"91d"}),
    ] {
        assert!(schedule(&mut host, &root.id, bad.clone()).is_err(), "{bad}");
    }

    let kept = schedule(&mut host, &root.id, check("5m")).unwrap();
    let stopped = schedule(&mut host, &root.id, check("1h")).unwrap();
    let listed = host
        .agent_tool(&root.id, "list_schedules", json!({}))
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(listed[0]["id"], kept["id"]);
    assert_eq!(listed[0]["cadence"], "every 5m");
    assert!(listed[0]["next_fire_at"].as_str().unwrap().ends_with('Z'));
    host.agent_tool(&root.id, "unschedule", json!({"id":stopped["id"]}))
        .unwrap();
    let listed = host
        .agent_tool(&root.id, "list_schedules", json!({}))
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // Archiving the owner stops its timers too.
    schedule(&mut host, &coordinator.id, check("30m")).unwrap();
    host.set_archived(&coordinator.id, true).unwrap();
    let archived = schedule(&mut host, &coordinator.id, check("30m")).unwrap_err();
    assert!(archived.to_string().contains("archived"), "{archived}");
    let worker_args = json!({"label":"x","prompt":"x","every":"5分"});
    let invalid = schedule(&mut host, &root.id, worker_args).unwrap_err();
    assert!(
        invalid.to_string().contains("Invalid duration"),
        "{invalid}"
    );
    host.fire_due_schedules(now() + 2 * 60 * MINUTE).unwrap();
    assert!(fires(&host, &coordinator.id).is_empty());
    let fired = fires(&host, &root.id);
    assert_eq!(fired.len(), 1, "only the timer still running fires");
    assert!(fired[0].id.contains(kept["id"].as_str().unwrap()));
}

#[test]
fn a_fire_that_cannot_be_enqueued_backs_off_and_fires_once_the_queue_drains() {
    let home = tempfile::tempdir().unwrap();
    let (mut host, root) = project(&home);
    let args = json!({"label":"check","prompt":"Check","every":"30m"});
    schedule(&mut host, &root.id, args).unwrap();
    let due = next_fire(&host).unwrap();
    for n in 0..1024 {
        host.send(&format!("busy-{n}"), None, &root.id, "Busy")
            .unwrap();
    }

    // The owner's queue is full: nothing fires, and the next look is a minute ahead, not now.
    let failed = due + 1_000;
    let retry = host.fire_due_schedules(failed).unwrap().next.unwrap();
    assert_eq!(retry, failed + MINUTE);
    assert!(fires(&host, &root.id).is_empty());
    for later in [failed + 1, failed + 30_000] {
        assert_eq!(host.fire_due_schedules(later).unwrap().next, Some(retry));
    }

    // Once there is room, the retry fires the slot once, late.
    host.advance_receipt("busy-0", Receipt::Delivered).unwrap();
    let next = host.fire_due_schedules(retry).unwrap().next.unwrap();
    assert_eq!(next, due + 30 * MINUTE);
    let fired = fires(&host, &root.id);
    assert_eq!(fired.len(), 1);
    assert_eq!(
        host.fire_status(&fired[0].id, retry).unwrap().as_deref(),
        Some("Timer status: late")
    );
}

#[test]
fn a_timer_firing_while_no_turn_starts_still_tells_clients_to_refresh() {
    let home = tempfile::tempdir().unwrap();
    let (mut host, root) = project(&home);
    let args = json!({"label":"check","prompt":"Check","every":"30m"});
    schedule(&mut host, &root.id, args).unwrap();
    drop(host);
    // The project is not live, so the fire starts no turn; only the timer changes state.
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute("UPDATE schedules SET next_fire_at=?1", [now() + 1_500])
        .unwrap();
    drop(db);
    let service = Service::start(
        home.path().to_path_buf(),
        env!("CARGO_BIN_EXE_workspace-host").into(),
    )
    .unwrap();
    let changes = service.changes();
    while changes.try_recv().is_ok() {}
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if changes.try_recv().is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no change signal for the timer fire"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let snapshot: Snapshot = serde_json::from_value(
        service
            .request(Command::Snapshot)
            .unwrap()
            .recv_blocking()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let next = snapshot.schedules[0].next_fire_at.unwrap();
    assert!(
        next > now() + 29 * MINUTE,
        "the Overview sees the next slot"
    );
}
