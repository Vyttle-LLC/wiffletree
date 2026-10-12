//! Provider processes around a host stop, through the real Service with real processes: a
//! quit stops running turns, and a session starts no turn beside a process an earlier host left
//! running. Kept in its own test binary because it sets the provider environment for the process.
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::{Host, now, service::Service};

/// Records its process group, then finishes the turn (FINISH) or lingers with a child. Other
/// invocations, such as the host's `auth status` check, exit at once.
const PROVIDER: &str = r#"#!/bin/sh
case "$1" in -p|exec) ;; *) exit 0;; esac
prompt=$(cat)
echo $$ >> "$WORKSPACE_TEST_GROUPS"
echo '{"type":"system","subtype":"init","session_id":"s","mcp_servers":[{"name":"agent_workspace","status":"connected"}]}'
case "$prompt" in *FINISH*)
  echo '{"type":"result","is_error":false,"result":"DONE","usage":{}}'
  exit 0;;
esac
sleep 90
"#;

/// The tests share the provider environment, so they run one at a time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// Installs the provider in `scratch` and returns the file it records process groups in.
fn install_provider(scratch: &Path) -> std::path::PathBuf {
    let provider = scratch.join("provider");
    std::fs::write(&provider, PROVIDER).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let groups = scratch.join("groups");
    // SAFETY: the caller holds ENVIRONMENT, and no earlier test's host is still running.
    unsafe {
        std::env::set_var("WORKSPACE_CLAUDE_BIN", &provider);
        std::env::set_var("WORKSPACE_CODEX_BIN", &provider);
        std::env::set_var("WORKSPACE_TEST_GROUPS", &groups);
    }
    groups
}

fn start(home: &Path) -> Service {
    Service::start(
        home.to_path_buf(),
        env!("CARGO_BIN_EXE_workspace-host").into(),
    )
    .unwrap()
}

fn request(service: &Service, command: Command) -> serde_json::Value {
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

fn until(service: &Service, what: &str, mut done: impl FnMut(&Snapshot) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(&snapshot(service)) {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn status(snapshot: &Snapshot, id: &str) -> Status {
    let session = snapshot.sessions.iter().find(|s| s.id == id);
    session.unwrap().status
}

fn runtime(snapshot: &Snapshot, id: &str) -> SessionRuntime {
    let runtime = snapshot.runtimes.iter().find(|r| r.session_id == id);
    runtime.unwrap().clone()
}

/// The provider processes started so far, by process group.
fn groups(file: &Path) -> Vec<i32> {
    let groups = std::fs::read_to_string(file).unwrap_or_default();
    groups.lines().map(|g| g.trim().parse().unwrap()).collect()
}

fn group_runs(group: i32) -> bool {
    unsafe { libc::killpg(group, 0) == 0 }
}

/// The run's outcome and error, read from the store.
fn run_result(home: &Path, session: &str) -> (Option<String>, Option<String>) {
    let db = rusqlite::Connection::open(home.join("workspace.sqlite3")).unwrap();
    db.query_row(
        "SELECT outcome,json_extract(detail,'$.result.error') FROM provider_runs WHERE session_id=?1 ORDER BY started_at DESC LIMIT 1",
        [session],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn a_quit_stops_running_turns_before_it_returns() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let recorded = install_provider(scratch.path());
    let main = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().to_str().unwrap())
            .unwrap();
        host.create_project("Quit").unwrap();
        host.sessions().unwrap().remove(0)
    };
    let service = start(home.path());
    request(
        &service,
        Command::Send {
            id: "linger".into(),
            sender: None,
            recipient: main.id.clone(),
            body: "LINGER".into(),
            attachments: vec![],
        },
    );
    until(&service, "the provider to start", |s| {
        status(s, &main.id) == Status::Working && !groups(&recorded).is_empty()
    });
    let group = groups(&recorded)[0];
    assert!(group_runs(group));

    service.shutdown();

    // No waiting here: the shutdown itself returned only once the group was stopped.
    assert!(!group_runs(group));
    let (outcome, error) = run_result(home.path(), &main.id);
    assert_eq!(outcome.as_deref(), Some("failed"));
    let error = error.unwrap();
    assert!(
        error.starts_with("Wiffletree quit during a turn"),
        "{error}"
    );
    // Nothing was left unfinished for the next start to stop.
    let service = start(home.path());
    let after = snapshot(&service);
    assert_eq!(status(&after, &main.id), Status::Disconnected);
    assert!(after.attention.is_empty(), "{:?}", after.attention);
    assert_eq!(run_result(home.path(), &main.id).1, Some(error));
}

#[test]
fn a_session_starts_no_turn_beside_a_process_the_last_host_left_running() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let recorded = install_provider(scratch.path());
    // The last host died mid-turn: its run is unfinished and its provider still runs. The run
    // kept no start time, so the process cannot be confirmed and is never signalled.
    let mut leftover = std::process::Command::new("sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    let main = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().to_str().unwrap())
            .unwrap();
        let project = host.create_project("Leftover").unwrap();
        let main = host.sessions().unwrap().remove(0);
        host.set_live(&project.id, true).unwrap();
        let work = host.send("work", None, &main.id, "FINISH").unwrap();
        host.advance_receipt(&work.id, Receipt::Delivered).unwrap();
        host.set_status(&main.id, Status::Working).unwrap();
        main
    };
    rusqlite::Connection::open(home.path().join("workspace.sqlite3"))
        .unwrap()
        .execute(
            "INSERT INTO provider_runs(id,session_id,messages,started_at,detail,process_group) VALUES ('run-cut-off',?1,'[\"work\"]',?2,'{}',?3)",
            rusqlite::params![main.id, now(), leftover.id()],
        )
        .unwrap();

    let service = start(home.path());
    let started = snapshot(&service);
    assert_eq!(status(&started, &main.id), Status::Disconnected);
    assert_eq!(started.attention.len(), 1, "{:?}", started.attention);
    let note = &started.attention[0];
    assert_eq!(note.operation_id, "leftover-process:run-cut-off");
    assert!(note.prompt.contains(&leftover.id().to_string()), "{note:?}");

    // Retrying the held input starts nothing while the process runs: the turn is held.
    request(
        &service,
        Command::ReconcileSession {
            session_id: main.id.clone(),
            retry: true,
        },
    );
    until(&service, "the held turn", |s| {
        status(s, &main.id) == Status::Blocked
    });
    let held = runtime(&snapshot(&service), &main.id);
    let reason = held.last_error.unwrap();
    assert!(reason.contains(&leftover.id().to_string()), "{reason}");
    assert!(held.held);
    assert!(groups(&recorded).is_empty(), "no provider was started");
    assert_eq!(leftover.try_wait().unwrap(), None, "never signalled");

    // Once the process has exited, the host settles its note and starts the turn.
    leftover.kill().unwrap();
    leftover.wait().unwrap();
    until(&service, "the turn after the process exited", |s| {
        status(s, &main.id) == Status::Ready && runtime(s, &main.id).last_finished_at.is_some()
    });
    assert_eq!(groups(&recorded).len(), 1);
    assert!(snapshot(&service).attention.is_empty());
    assert_eq!(
        run_result(home.path(), &main.id).0.as_deref(),
        Some("completed")
    );
}
