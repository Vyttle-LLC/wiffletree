//! A provider's whole process group stops when its turn ends, through the real Service, even
//! with a tool process that ignores SIGTERM and holds stderr open. Kept in its own test binary
//! because it sets the provider environment for the process.
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::{Host, service::Service};

/// Records its process group and leaves a SIGTERM-ignoring child holding stderr. HOLD_STDOUT
/// adds one holding stdout, CHATTY one writing to stdout every 10 ms. It then finishes the
/// turn (FINISH, with TRAILING adding a non-JSON line after the result), fails with a
/// diagnostic while a detached process keeps stderr open (DIAGNOSE), or closes stdout and
/// lingers. Other invocations, such as the host's `auth status` check, exit at once.
const PROVIDER: &str = r#"#!/bin/sh
case "$1" in -p|exec) ;; *) exit 0;; esac
prompt=$(cat)
echo $$ >> "$WORKSPACE_TEST_GROUPS"
(trap '' TERM; exec sleep 90) >/dev/null &
case "$prompt" in *HOLD_STDOUT*) (trap '' TERM; exec sleep 90) & ;; esac
case "$prompt" in *CHATTY*) (trap '' TERM; while :; do echo '{"type":"noise"}'; sleep 0.01; done) & ;; esac
echo '{"type":"system","subtype":"init","session_id":"s","mcp_servers":[{"name":"agent_workspace","status":"connected"}]}'
echo '{"type":"thread.started","thread_id":"s"}'
case "$prompt" in *DIAGNOSE*)
  perl -e 'use POSIX; setsid(); sleep 5' >/dev/null &
  sleep 0.3
  echo 'DIAGNOSTIC: fixture failure' >&2
  exit 1;;
esac
case "$prompt" in *FINISH*)
  echo '{"type":"result","is_error":false,"result":"DONE","usage":{}}'
  echo '{"type":"item.completed","item":{"type":"agent_message","text":"DONE"}}'
  echo '{"type":"turn.completed","usage":{}}'
  case "$prompt" in *TRAILING*) echo 'not json after the result';; esac
  exit 0;;
esac
exec 1>&-
sleep 90
"#;

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
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(&snapshot(service)) {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn session(snapshot: &Snapshot, id: &str) -> (Status, SessionRuntime) {
    let status = snapshot
        .sessions
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .status;
    let runtime = snapshot
        .runtimes
        .iter()
        .find(|r| r.session_id == id)
        .unwrap()
        .clone();
    (status, runtime)
}

/// The latest turn's process group, as the provider recorded it.
fn latest_group(groups: &Path) -> i32 {
    let groups = std::fs::read_to_string(groups).unwrap();
    groups.lines().last().unwrap().trim().parse().unwrap()
}

fn group_is_gone(group: i32) -> bool {
    unsafe { libc::killpg(group, 0) != 0 }
}

#[test]
fn every_way_a_turn_ends_stops_its_whole_process_group_promptly() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = scratch.path().join("provider");
    std::fs::write(&provider, PROVIDER).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let groups = scratch.path().join("groups");
    let main = {
        let mut host = Host::open(home.path()).unwrap();
        host.create_project("Process groups").unwrap();
        host.sessions().unwrap().remove(0)
    };
    // SAFETY: this test binary holds one test, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("WORKSPACE_CLAUDE_BIN", &provider);
        std::env::set_var("WORKSPACE_CODEX_BIN", &provider);
        std::env::set_var("WORKSPACE_TEST_GROUPS", &groups);
    }
    let service = Service::start(
        home.path().to_path_buf(),
        env!("CARGO_BIN_EXE_workspace-host").into(),
    )
    .unwrap();
    let send = |id: &str, body: &str| {
        request(
            &service,
            Command::Send {
                id: id.into(),
                sender: None,
                recipient: main.id.clone(),
                body: body.into(),
                attachments: vec![],
            },
        );
    };
    let working = |s: &Snapshot| session(s, &main.id).0 == Status::Working;
    // Starts a lingering turn and waits until its provider has closed stdout.
    let linger = |id: &str| {
        send(id, "LINGER");
        until(&service, "the lingering turn", working);
        std::thread::sleep(Duration::from_millis(300));
    };

    // A normal finish is not held up by the child holding stderr, and the child is stopped.
    send("finish", "FINISH");
    until(&service, "the finished turn", |s| {
        !working(s) && session(s, &main.id).1.last_finished_at.is_some()
    });
    let (status, runtime) = session(&snapshot(&service), &main.id);
    assert_eq!((status, runtime.last_error), (Status::Ready, None));
    assert!(group_is_gone(latest_group(&groups)));

    // Nor by one holding stdout: the provider's exit ends the turn without waiting for EOF.
    let previous = runtime.last_finished_at;
    let started = Instant::now();
    send("hold-stdout", "HOLD_STDOUT FINISH");
    until(&service, "the turn whose stdout is held", |s| {
        !working(s) && session(s, &main.id).1.last_finished_at > previous
    });
    assert!(started.elapsed() < Duration::from_secs(5));
    let (status, runtime) = session(&snapshot(&service), &main.id);
    assert_eq!((status, runtime.last_error), (Status::Ready, None));
    assert!(group_is_gone(latest_group(&groups)));

    // Nor by one that writes so often the provider's output never goes quiet; the final
    // reply still arrives.
    let previous = runtime.last_finished_at;
    let started = Instant::now();
    send("chatty", "CHATTY FINISH");
    until(&service, "the turn with a chatty child", |s| {
        !working(s) && session(s, &main.id).1.last_finished_at > previous
    });
    assert!(started.elapsed() < Duration::from_secs(5));
    let (status, runtime) = session(&snapshot(&service), &main.id);
    assert_eq!((status, runtime.last_error), (Status::Ready, None));
    assert!(group_is_gone(latest_group(&groups)));
    // A stray non-JSON line after the terminal result does not fail the turn.
    let previous = runtime.last_finished_at;
    send("trailing", "TRAILING FINISH");
    until(&service, "the turn with trailing output", |s| {
        !working(s) && session(s, &main.id).1.last_finished_at > previous
    });
    let (status, runtime) = session(&snapshot(&service), &main.id);
    assert_eq!((status, runtime.last_error), (Status::Ready, None));
    let replies = replies(&service, &main.id);
    assert_eq!(replies, 4, "every finished turn posts its reply");

    // Stop, which uses the same cancel flag as the turn budget.
    linger("stop");
    let stopped = Instant::now();
    request(
        &service,
        Command::SetLive {
            project_id: main.project_id.clone(),
            enabled: false,
        },
    );
    until(&service, "the stopped turn", |s| !working(s));
    assert!(stopped.elapsed() < Duration::from_secs(5));
    let (_, runtime) = session(&snapshot(&service), &main.id);
    assert_eq!(
        runtime.last_error.as_deref(),
        Some("Turn interrupted; inspect changes before retrying")
    );
    assert!(group_is_gone(latest_group(&groups)));
    request(
        &service,
        Command::ReconcileSession {
            session_id: main.id.clone(),
            retry: false,
        },
    );

    // Pause.
    linger("pause");
    let paused = Instant::now();
    request(
        &service,
        Command::SetStatus {
            session_id: main.id.clone(),
            status: Status::Paused,
        },
    );
    until(&service, "the paused turn", |s| {
        session(s, &main.id).1.last_error.is_some()
    });
    assert!(paused.elapsed() < Duration::from_secs(5));
    assert_eq!(session(&snapshot(&service), &main.id).0, Status::Paused);
    assert!(group_is_gone(latest_group(&groups)));
    request(
        &service,
        Command::ReconcileSession {
            session_id: main.id.clone(),
            retry: false,
        },
    );

    // A failure keeps the diagnostic it wrote, though a detached process still holds stderr.
    let failed = Instant::now();
    send("diagnose", "DIAGNOSE");
    until(&service, "the failed turn", |s| {
        session(s, &main.id).1.last_error.is_some()
    });
    assert!(failed.elapsed() < Duration::from_secs(5));
    let (_, runtime) = session(&snapshot(&service), &main.id);
    let error = runtime.last_error.unwrap();
    assert!(error.contains("DIAGNOSTIC: fixture failure"), "{error}");
    assert!(group_is_gone(latest_group(&groups)));
}

fn replies(service: &Service, session: &str) -> usize {
    let messages: Vec<Message> = serde_json::from_value(request(
        service,
        Command::Messages {
            session_id: session.into(),
            before: None,
            limit: 100,
        },
    ))
    .unwrap();
    messages
        .iter()
        .filter(|m| m.id.starts_with("output:") && m.body == "DONE")
        .count()
}
