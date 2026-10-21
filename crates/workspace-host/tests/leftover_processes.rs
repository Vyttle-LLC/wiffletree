//! Processes around a host stop, an archive and a stop, through the real Service with real
//! processes: what an agent starts keeps running between its turns and is stopped when the
//! agent is archived or stopped, at a quit and at the next start; startup stops a provider
//! process an earlier host left running, and a session starts no turn beside one it could not
//! confirm. Also how the host ends, by Stop agents and quit or an idle restart, and its power
//! assertion while turns run. Kept in its own test binary because it sets the provider
//! environment for the process.
mod common;
use common::{request, snapshot, until_snapshot as until};
use serde_json::Value;
use std::{
    os::unix::{
        fs::PermissionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::{Host, now, service::Service};

/// Records its process group, then does what the prompt names. `tool` runs a command as both
/// providers run a tool command: as the leader of a session and process group of its own. Other
/// invocations, such as the host's `auth status` check, exit at once.
const PROVIDER: &str = r#"#!/bin/sh
case "$1" in -p|exec) ;; *) exit 0;; esac
prompt=$(cat)
echo $$ >> "$WORKSPACE_TEST_GROUPS"
printf '%s\n=====\n' "$prompt" >> "$WORKSPACE_TEST_PROMPTS"
echo '{"type":"system","subtype":"init","session_id":"s","mcp_servers":[{"name":"agent_workspace","status":"connected"}]}'
ready="$WORKSPACE_TEST_PIDS.$$"
tool() {
  python3 -c 'import os, sys; os.setsid(); os.execvp(sys.argv[1], sys.argv[1:])' "$@" >/dev/null 2>&1 &
}
started() {
  while [ ! -e "$ready" ]; do sleep 0.01; done
  sleep 0.1
}
finish() {
  echo '{"type":"result","is_error":false,"result":"DONE","usage":{}}'
  exit 0
}
case "$prompt" in
*QUIET_THEN_TOOL*)
  # The provider closes its output, and only then starts a tool and lingers.
  exec 1>&-
  sleep 0.2
  tool sh -c 'echo $$ >> "$WORKSPACE_TEST_PIDS"; touch "$0"; exec sleep 300' "$ready"
  started;;
*TOOL_THEN_LINGER*)
  tool sh -c 'echo $$ >> "$WORKSPACE_TEST_PIDS"; touch "$0"; exec sleep 300' "$ready"
  started;;
*CHURN_CHILDREN*)
  tool python3 -c '
import os, signal, sys, time
ready = sys.argv[1]
with open(os.environ["WORKSPACE_TEST_PIDS"], "a") as saved:
    saved.write(str(os.getpid()) + "\n")
for batch in range(3):
    children = []
    for _ in range(12):
        child = os.fork()
        if child == 0:
            while True:
                signal.pause()
        children.append(child)
        with open(os.environ["WORKSPACE_TEST_PIDS"], "a") as saved:
            saved.write(str(child) + "\n")
    gate = ready + "." + str(batch)
    while not os.path.exists(gate + ".reap"):
        time.sleep(0.01)
    for child in children:
        os.kill(child, signal.SIGTERM)
        os.waitpid(child, 0)
    open(gate + ".reaped", "w").close()
    while not os.path.exists(gate + ".next"):
        time.sleep(0.01)
while True:
    signal.pause()
' "$ready";;
*LEAVE_ORPHANS*)
  # The tool shell leads the group and starts two children. It exits when the test has seen
  # them recorded, and the turn ends after it has.
  tool sh -c 'echo $$ >> "$WORKSPACE_TEST_PIDS"; for n in 1 2; do sleep 300 & echo $! >> "$WORKSPACE_TEST_PIDS"; done; touch "$0"; while [ ! -e "$0.exit" ]; do sleep 0.01; done' "$ready"
  shell=$!
  started
  wait "$shell"
  finish;;
*BRIEF_SHELL*)
  # A shell in a group of its own. When the test has seen it recorded, it starts a child and
  # exits at once, so no scan need ever see the child while the shell lives.
  set -m
  sh -c 'echo $$ >> "$WORKSPACE_TEST_PIDS"; while [ ! -e "$0.exit" ]; do sleep 0.01; done; sleep 300 >/dev/null 2>&1 & echo $! >> "$WORKSPACE_TEST_PIDS"' "$ready" >/dev/null 2>&1 &
  wait
  finish;;
*LEAVE*)
  tool sh -c 'echo $$ >> "$WORKSPACE_TEST_PIDS"; touch "$0"; exec sleep 300' "$ready"
  started
  finish;;
*FINISH*)
  finish;;
esac
# Lingers, with the id of what it waits on saved so the test can stop it by that id.
sleep 90 &
echo $! >> "$WORKSPACE_TEST_PIDS"
wait
"#;

/// The tests share the provider environment, so they run one at a time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// The provider's files, and the processes it and its tools started. Dropping it kills every
/// one of them by its saved process id, so a failed test leaves nothing running.
struct Provider {
    /// The provider processes started so far, one process group per line.
    groups: PathBuf,
    /// The processes the provider's tools started and, after them, the `sleep` a lingering
    /// provider waits on, one id per line.
    pids: PathBuf,
    /// Every prompt a provider was given.
    prompts: PathBuf,
}
impl Drop for Provider {
    fn drop(&mut self) {
        for pid in ids(&self.groups).into_iter().chain(ids(&self.pids)) {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// Installs the provider in `scratch`.
fn install_provider(scratch: &Path) -> Provider {
    let provider = scratch.join("provider");
    std::fs::write(&provider, PROVIDER).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let files = Provider {
        groups: scratch.join("groups"),
        pids: scratch.join("pids"),
        prompts: scratch.join("prompts"),
    };
    // SAFETY: the caller holds ENVIRONMENT, and no earlier test's host is still running.
    unsafe {
        std::env::set_var("WORKSPACE_CLAUDE_BIN", &provider);
        std::env::set_var("WORKSPACE_CODEX_BIN", &provider);
        std::env::set_var("WORKSPACE_TEST_GROUPS", &files.groups);
        std::env::set_var("WORKSPACE_TEST_PIDS", &files.pids);
        std::env::set_var("WORKSPACE_TEST_PROMPTS", &files.prompts);
    }
    files
}

fn start(home: &Path) -> Service {
    // SAFETY: callers hold ENVIRONMENT while this service runs.
    unsafe { common::ServiceFixture::new(home).start() }
}

fn status(snapshot: &Snapshot, id: &str) -> Status {
    let session = snapshot.sessions.iter().find(|s| s.id == id);
    session.unwrap().status
}

fn runtime(snapshot: &Snapshot, id: &str) -> SessionRuntime {
    let runtime = snapshot.runtimes.iter().find(|r| r.session_id == id);
    runtime.unwrap().clone()
}

/// The process ids a provider or its tools wrote to `file`, one per line.
fn ids(file: &Path) -> Vec<i32> {
    let ids = std::fs::read_to_string(file).unwrap_or_default();
    ids.lines().map(|id| id.trim().parse().unwrap()).collect()
}

/// The provider processes started so far, by process group.
fn groups(provider: &Provider) -> Vec<i32> {
    ids(&provider.groups)
}

fn group_runs(group: i32) -> bool {
    unsafe { libc::killpg(group, 0) == 0 }
}

/// A process in its own group, as a provider runs, standing in for one a stopped host left
/// running. Dropping it kills only this process, so a failed assertion leaves nothing running.
struct Leftover(std::process::Child);
impl Leftover {
    fn spawn() -> Self {
        let sleep = std::process::Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn();
        Self(sleep.unwrap())
    }
    fn id(&self) -> u32 {
        self.0.id()
    }
}
impl Drop for Leftover {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
            uploads: vec![],
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
fn startup_stops_a_confirmed_process_the_last_host_left_running() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let _provider = install_provider(scratch.path());
    // The last host died mid-turn, in a version that kept its provider's process group and
    // start time on the run: the run is unfinished, and its provider still runs.
    let mut leftover = Leftover::spawn();
    let main = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().to_str().unwrap())
            .unwrap();
        host.create_project("Confirmed").unwrap();
        let main = host.sessions().unwrap().remove(0);
        let work = host.send("work", None, &main.id, "FINISH").unwrap();
        host.advance_receipt(&work.id, Receipt::Delivered).unwrap();
        host.set_status(&main.id, Status::Working).unwrap();
        main
    };
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,detail,process_group,process_started) VALUES ('run-cut-off',?1,'[\"work\"]',?2,'{}',?3,?4)",
        rusqlite::params![
            main.id,
            now(),
            leftover.id(),
            common::process_started(leftover.id() as i32)
        ],
    )
    .unwrap();

    let service = start(home.path());

    // The start itself returned only once the group was stopped. Under load that is a moment
    // before the process can be collected, so its exit is waited for, but not its stopping.
    assert!(!group_runs(leftover.id() as i32));
    let stopped = leftover.0.wait().unwrap();
    assert_eq!(stopped.signal(), Some(libc::SIGTERM));
    let started = snapshot(&service);
    assert_eq!(status(&started, &main.id), Status::Disconnected);
    assert!(started.attention.is_empty(), "{:?}", started.attention);
    let (outcome, stopped_at): (String, Option<i64>) = db
        .query_row(
            "SELECT outcome,json_extract(detail,'$.process_stopped_at') FROM provider_runs WHERE id='run-cut-off'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(outcome, "interrupted");
    assert!(stopped_at.is_some());
}

#[test]
fn a_session_starts_no_turn_beside_a_process_the_last_host_left_running() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let recorded = install_provider(scratch.path());
    // The last host died mid-turn: its run is unfinished and its provider still runs. The run
    // kept no start time, so the process cannot be confirmed and is never signalled.
    let mut leftover = Leftover::spawn();
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
    assert_eq!(leftover.0.try_wait().unwrap(), None, "never signalled");

    // Once the process has exited, the host settles its note and starts the turn.
    drop(leftover);
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

#[test]
fn a_run_an_older_host_marked_interrupted_still_holds_its_session_beside_its_process() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let recorded = install_provider(scratch.path());
    // A host before this change restarted mid-turn: it marked the run interrupted without
    // looking for its process, which still runs, and a later turn has finished since. Nothing
    // is unfinished, no note exists, and new input is queued for the session.
    let mut leftover = Leftover::spawn();
    let main = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().to_str().unwrap())
            .unwrap();
        let project = host.create_project("Upgrade").unwrap();
        let main = host.sessions().unwrap().remove(0);
        host.set_live(&project.id, true).unwrap();
        host.send("next", None, &main.id, "FINISH").unwrap();
        main
    };
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail,process_group) VALUES ('run-cut-off',?1,'[]',?2,?2,'interrupted','{}',?3)",
        rusqlite::params![main.id, now() - 2, leftover.id()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail) VALUES ('run-after',?1,'[]',?2,?2,'completed','{}')",
        rusqlite::params![main.id, now() - 1],
    )
    .unwrap();
    drop(db);

    let service = start(home.path());
    until(&service, "the held turn", |s| {
        status(s, &main.id) == Status::Blocked
    });
    let held = snapshot(&service);
    let reason = runtime(&held, &main.id).last_error.unwrap();
    assert!(reason.contains(&leftover.id().to_string()), "{reason}");
    assert_eq!(held.attention.len(), 1, "{:?}", held.attention);
    assert_eq!(
        held.attention[0].operation_id,
        "leftover-process:run-cut-off"
    );
    assert!(groups(&recorded).is_empty(), "no provider was started");
    assert_eq!(leftover.0.try_wait().unwrap(), None, "never signalled");

    drop(leftover);
    until(&service, "the turn after the process exited", |s| {
        status(s, &main.id) == Status::Ready && runtime(s, &main.id).last_finished_at.is_some()
    });
    assert_eq!(groups(&recorded).len(), 1);
    assert!(snapshot(&service).attention.is_empty());
}

fn store(home: &Path) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(home.join("workspace.sqlite3")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db
}

/// A project that runs, and its coordinator: the agent these tests leave processes with.
fn project(home: &Path, scratch: &Path, name: &str) -> (Project, Session) {
    let mut host = Host::open_set_up(home).unwrap();
    host.set_workspaces_dir(scratch.to_str().unwrap()).unwrap();
    let project = host.create_project(name).unwrap();
    host.set_live(&project.id, true).unwrap();
    let sessions = host.sessions().unwrap().into_iter();
    let main = sessions.rev().find(|s| s.project_id == project.id);
    (project, main.unwrap())
}

fn send(service: &Service, id: &str, recipient: &Session, body: &str) {
    request(
        service,
        Command::Send {
            id: id.into(),
            sender: None,
            recipient: recipient.id.clone(),
            body: body.into(),
            attachments: vec![],
            uploads: vec![],
        },
    );
}

/// Waits until `count` of the session's turns have ended and the host has settled each: the
/// `turn_finished` entry is the last thing it writes for a turn, after the record of what the
/// turn left running and its notice. A run's `finished_at` is the first.
fn until_finished(home: &Path, session: &Session, count: i64) {
    let db = store(home);
    let deadline = Instant::now() + Duration::from_secs(20);
    let finished = || -> i64 {
        db.query_row(
            "SELECT COUNT(*) FROM activity WHERE session_id=?1 AND kind='turn_finished'",
            [&session.id],
            |r| r.get(0),
        )
        .unwrap()
    };
    while finished() < count {
        assert!(
            Instant::now() < deadline,
            "timed out: run {count} to finish"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until the provider's tools have written `count` process ids and the last of them
/// leads a group recorded as running for the agent, and returns it.
fn until_tool_recorded(home: &Path, session: &Session, provider: &Provider, count: usize) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let tool = ids(&provider.pids).get(count - 1).copied();
        let running = recorded(home, session);
        let mut running = running.iter().filter(|r| !r.provider && r.ended.is_none());
        if let Some(tool) = tool
            && running.any(|r| r.group == tool)
        {
            return tool;
        }
        assert!(Instant::now() < deadline, "timed out: tool process {count}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until a group one of the agent's tools started is recorded as running, and returns it.
fn until_tool_running(home: &Path, session: &Session) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let running = recorded(home, session);
        if let Some(tool) = running.iter().find(|r| !r.provider && r.ended.is_none()) {
            return tool.group;
        }
        assert!(Instant::now() < deadline, "timed out: a tool process");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One process group the record holds for an agent.
#[derive(Debug, PartialEq)]
struct Recorded {
    group: i32,
    provider: bool,
    /// Why it is no longer running, once it is not.
    ended: Option<String>,
    /// Its members' process ids.
    members: Vec<i32>,
}

/// What the record holds for the agent, oldest first.
fn recorded(home: &Path, session: &Session) -> Vec<Recorded> {
    let db = store(home);
    let mut groups = db
        .prepare("SELECT id,process_group,provider,ended,ended_at FROM process_groups WHERE session_id=?1 ORDER BY rowid")
        .unwrap();
    let groups = groups
        .query_map([&session.id], |r| {
            let ended: Option<String> = r.get(3)?;
            let ended_at: Option<i64> = r.get(4)?;
            assert_eq!(ended.is_some(), ended_at.is_some(), "a reason and a time");
            Ok((
                r.get::<_, String>(0)?,
                Recorded {
                    group: r.get(1)?,
                    provider: r.get(2)?,
                    ended,
                    members: vec![],
                },
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let mut members = db
        .prepare("SELECT pid FROM process_group_members WHERE group_id=?1 ORDER BY pid")
        .unwrap();
    groups
        .into_iter()
        .map(|(id, mut group)| {
            let pids = members.query_map([id], |r| r.get(0)).unwrap();
            group.members = pids.collect::<rusqlite::Result<_>>().unwrap();
            group
        })
        .collect()
}

/// A tool's group as the record should hold it.
fn tool_group(group: i32, ended: Option<&str>, members: &[i32]) -> Recorded {
    Recorded {
        group,
        provider: false,
        ended: ended.map(str::to_owned),
        members: members.to_vec(),
    }
}

/// Asserts that the record holds `tool`'s group as stopped for `reason`. A tool stopped a
/// moment after it started can have had a child of its own stopped with it, so its members
/// are not pinned beyond the tool itself.
fn assert_tool_stopped(record: &Recorded, tool: i32, reason: &str) {
    assert_eq!(
        (record.group, record.provider, record.ended.as_deref()),
        (tool, false, Some(reason)),
        "{record:?}"
    );
    assert!(record.members.contains(&tool), "{record:?}");
}

/// The bodies of the Wiffletree process notices in the agent's conversation, oldest first.
fn notices(home: &Path, session: &Session) -> Vec<String> {
    let db = store(home);
    let mut notices = db
        .prepare("SELECT body FROM messages WHERE recipient=?1 AND sender IS NULL AND id LIKE 'processes:%' ORDER BY sequence")
        .unwrap();
    let notices = notices.query_map([&session.id], |r| r.get(0)).unwrap();
    notices.collect::<rusqlite::Result<_>>().unwrap()
}

#[test]
fn what_a_turn_starts_keeps_running_is_announced_once_and_is_stopped_when_its_agent_is_archived() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Leave");
    let service = start(home.path());

    // The provider starts a child in a new session and process group, and finishes normally.
    send(&service, "leave", &main, "LEAVE");
    until_finished(home.path(), &main, 1);
    let child = ids(&provider.pids)[0];
    assert!(group_runs(child), "a normal turn end leaves it running");
    assert!(!group_runs(groups(&provider)[0]), "but not the provider");
    assert_eq!(
        recorded(home.path(), &main),
        [tool_group(child, None, &[child])]
    );
    let kept: i64 = store(home.path())
        .query_row(
            "SELECT started FROM process_group_members WHERE pid=?1",
            [child],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, common::process_started(child));
    let name = &main.name;
    let left = notices(home.path(), &main);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(
        left[0].starts_with(&format!(
            "{name} left 1 process running. It is the program `sleep`.\nStarted at "
        )),
        "{left:?}"
    );
    let rest = format!(" · process group {child} · it keeps running until {name} is archived.");
    assert!(left[0].ends_with(&rest), "{left:?}");
    assert!(
        snapshot(&service).attention.is_empty(),
        "nothing needs the human"
    );

    // The agent's own process does not hold its next turn, which is told about it.
    send(&service, "next", &main, "FINISH");
    until_finished(home.path(), &main, 2);
    let prompts = std::fs::read_to_string(&provider.prompts).unwrap();
    let told = prompts.split("=====").nth(1).unwrap();
    assert!(told.contains("Sender: Wiffletree"), "{told}");
    assert!(told.contains(&left[0]), "{told}");
    // Once per set of processes, not after every turn.
    send(&service, "again", &main, "FINISH");
    until_finished(home.path(), &main, 3);
    assert_eq!(notices(home.path(), &main), left);
    assert!(group_runs(child));

    request(
        &service,
        Command::SetArchived {
            session_id: main.id.clone(),
            archived: true,
        },
    );

    assert!(!group_runs(child), "stopped before the archive returned");
    assert_eq!(
        recorded(home.path(), &main),
        [tool_group(child, Some("archived"), &[child])]
    );
    let stopped = notices(home.path(), &main).pop().unwrap();
    assert_eq!(
        stopped,
        format!(
            "Stopped 1 process {name} had left running. It was the program `sleep`.\nProcess group {child} · stopped when {name} was archived."
        )
    );
    assert!(snapshot(&service).attention.is_empty());
}

#[test]
fn a_later_turns_processes_get_a_notice_of_their_own() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Twice");
    let service = start(home.path());
    send(&service, "first", &main, "LEAVE");
    until_finished(home.path(), &main, 1);
    send(&service, "second", &main, "LEAVE");
    until_finished(home.path(), &main, 2);
    let [first, second] = ids(&provider.pids)[..] else {
        panic!("one process a turn")
    };
    assert!(group_runs(first) && group_runs(second));

    // One notice a set: the second names only what the second turn left, not the first again.
    let left = notices(home.path(), &main);
    assert_eq!(left.len(), 2, "{left:?}");
    let named = |notice: &str, group: i32| notice.contains(&format!("process group {group} "));
    assert!(
        named(&left[0], first) && !named(&left[0], second),
        "{left:?}"
    );
    assert!(
        named(&left[1], second) && !named(&left[1], first),
        "{left:?}"
    );
    let said = format!("{} left 1 process running. It is", main.name);
    assert!(left[1].starts_with(&said), "{left:?}");

    service.shutdown();
    assert!(!group_runs(first) && !group_runs(second));
}

#[test]
fn an_agent_archived_in_its_turn_has_a_tool_started_a_moment_ago_stopped_when_the_turn_ends() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Archived at work");
    let service = start(home.path());
    send(&service, "tool", &main, "TOOL_THEN_LINGER");
    // As soon as the tool has said its id, whether or not the host has stored its sighting.
    let deadline = Instant::now() + Duration::from_secs(20);
    let tool = loop {
        if let Some(tool) = ids(&provider.pids).first() {
            break *tool;
        }
        assert!(Instant::now() < deadline, "timed out: the tool process");
        std::thread::sleep(Duration::from_micros(100));
    };

    request(
        &service,
        Command::SetArchived {
            session_id: main.id.clone(),
            archived: true,
        },
    );

    // The archive cancels the turn. Its thread hands on everything the watcher saw before it
    // reports the turn's end, so nothing is still on its way when the host stops what the
    // archived agent started.
    until_finished(home.path(), &main, 1);
    assert!(!group_runs(tool));
    let record = recorded(home.path(), &main);
    let record: Vec<_> = record.iter().filter(|r| !r.provider).collect();
    assert_eq!(record.len(), 1, "{record:?}");
    assert_eq!(
        (record[0].group, record[0].ended.as_deref()),
        (tool, Some("archived"))
    );
}

#[test]
fn archiving_stops_orphans_whose_group_leader_exited_before_the_turn_ended() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Orphans");
    let service = start(home.path());

    send(&service, "leave", &main, "LEAVE_ORPHANS");
    let deadline = Instant::now() + Duration::from_secs(20);
    let (shell, first, second) = loop {
        if let [shell, first, second] = ids(&provider.pids)[..]
            && recorded(home.path(), &main).iter().any(|r| {
                r.group == shell && r.members.contains(&first) && r.members.contains(&second)
            })
        {
            break (shell, first, second);
        }
        assert!(Instant::now() < deadline, "timed out: the tool's processes");
        std::thread::sleep(Duration::from_millis(20));
    };
    // The tool shell exits, and then the turn ends.
    let ready = format!("{}.{}", provider.pids.display(), groups(&provider)[0]);
    std::fs::write(format!("{ready}.exit"), "").unwrap();
    until_finished(home.path(), &main, 1);
    // The shell that led the group is gone; only its orphans remain in it.
    assert_eq!(unsafe { libc::kill(shell, 0) }, -1);
    assert!(group_runs(shell));
    let record = recorded(home.path(), &main);
    assert_eq!(record.len(), 1, "{record:?}");
    assert_eq!((record[0].group, &record[0].ended), (shell, &None));
    assert!(record[0].members.contains(&first) && record[0].members.contains(&second));
    let left = notices(home.path(), &main);
    assert!(
        left[0].contains("left 2 processes running. Both are the program `sleep`."),
        "{left:?}"
    );

    request(
        &service,
        Command::SetArchived {
            session_id: main.id.clone(),
            archived: true,
        },
    );

    assert!(!group_runs(shell));
    let record = recorded(home.path(), &main);
    assert_eq!(record[0].ended.as_deref(), Some("archived"));
    let stopped = notices(home.path(), &main).pop().unwrap();
    assert!(stopped.starts_with("Stopped 2 processes "), "{stopped}");
}

#[test]
fn a_child_started_by_a_tool_shell_on_its_way_out_is_recorded_in_the_shells_group() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Brief");
    let service = start(home.path());

    // Only the watcher's own scans record anything while a turn runs. How short a shell they
    // catch depends on the scan interval, which a unit test bounds, as it does the scan that
    // finds a child only in its exited shell's group. Here the shell is seen first, then starts
    // its child and exits at once, and the child is recorded in the shell's group.
    send(&service, "brief", &main, "BRIEF_SHELL");
    let deadline = Instant::now() + Duration::from_secs(20);
    let shell = loop {
        if let [shell] = ids(&provider.pids)[..]
            && recorded(home.path(), &main)
                .iter()
                .any(|r| r.group == shell)
        {
            break shell;
        }
        assert!(Instant::now() < deadline, "timed out: the tool shell");
        std::thread::sleep(Duration::from_millis(20));
    };
    let ready = format!("{}.{}", provider.pids.display(), groups(&provider)[0]);
    std::fs::write(format!("{ready}.exit"), "").unwrap();
    until_finished(home.path(), &main, 1);
    let [_, child] = ids(&provider.pids)[..] else {
        panic!("the brief shell and its child")
    };
    assert_eq!(unsafe { libc::kill(shell, 0) }, -1);
    assert!(group_runs(shell));
    let record = recorded(home.path(), &main);
    assert_eq!(record.len(), 1, "{record:?}");
    assert_eq!(record[0].group, shell);
    assert!(record[0].members.contains(&child), "{record:?}");

    service.shutdown();
    assert!(!group_runs(shell));
}

#[test]
fn stopping_an_agent_stops_what_it_started_before_the_stop_returns() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (project, main) = project(home.path(), scratch.path(), "Stop");
    let service = start(home.path());
    send(&service, "leave", &main, "LEAVE");
    until_finished(home.path(), &main, 1);
    let idle = ids(&provider.pids)[0];
    assert!(group_runs(idle));

    // The human pauses the agent between its turns.
    request(
        &service,
        Command::SetStatus {
            session_id: main.id.clone(),
            status: Status::Paused,
        },
    );
    assert!(!group_runs(idle), "stopped before the pause returned");
    assert_eq!(
        recorded(home.path(), &main),
        [tool_group(idle, Some("stopped"), &[idle])]
    );
    let name = &main.name;
    let stopped = notices(home.path(), &main).pop().unwrap();
    let why = format!("Process group {idle} · stopped when {name} was stopped.");
    assert!(stopped.ends_with(&why), "{stopped}");

    // The human stops the project while a tool process of the agent's running turn runs.
    request(
        &service,
        Command::SetStatus {
            session_id: main.id.clone(),
            status: Status::Ready,
        },
    );
    send(&service, "tool", &main, "TOOL_THEN_LINGER");
    let tool = until_tool_recorded(home.path(), &main, &provider, 2);
    assert!(group_runs(tool));
    request(
        &service,
        Command::SetLive {
            project_id: project.id.clone(),
            enabled: false,
        },
    );
    assert!(!group_runs(tool), "stopped before the stop returned");
    until_finished(home.path(), &main, 2);
    assert!(!group_runs(groups(&provider)[1]));
    let record = recorded(home.path(), &main);
    assert_eq!(record.len(), 2, "{record:?}");
    assert_eq!(record[0], tool_group(idle, Some("stopped"), &[idle]));
    assert_tool_stopped(&record[1], tool, "stopped");
}

#[test]
fn a_stop_reaches_a_tool_the_turn_started_a_moment_ago() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Just started");
    let service = start(home.path());
    send(&service, "tool", &main, "TOOL_THEN_LINGER");
    // As soon as the tool has said its id, before the turn's thread has passed on that the
    // watcher saw it: the pause looks for itself.
    let deadline = Instant::now() + Duration::from_secs(20);
    let tool = loop {
        if let Some(tool) = ids(&provider.pids).first() {
            break *tool;
        }
        assert!(Instant::now() < deadline, "timed out: the tool process");
        std::thread::sleep(Duration::from_micros(100));
    };

    request(
        &service,
        Command::SetStatus {
            session_id: main.id.clone(),
            status: Status::Paused,
        },
    );

    assert!(!group_runs(tool), "stopped before the pause returned");
    until_finished(home.path(), &main, 1);
    // Its record names what was stopped: the tool, and whatever it had itself just started.
    let record = recorded(home.path(), &main);
    let record: Vec<_> = record.iter().filter(|r| !r.provider).collect();
    assert_eq!(record.len(), 1, "{record:?}");
    assert_eq!(
        (record[0].group, record[0].ended.as_deref()),
        (tool, Some("stopped"))
    );
    assert!(record[0].members.contains(&tool), "{record:?}");
}

#[test]
fn a_pause_that_finds_the_turn_finishing_still_stops_what_it_left() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Finishing");
    let service = start(home.path());
    send(&service, "leave", &main, "LEAVE");
    // The provider has started its tool and exited on its own. The host is still settling the
    // turn: its thread waits a moment for what is left, then sends its last sightings and
    // the turn's end.
    let deadline = Instant::now() + Duration::from_secs(20);
    let tool = loop {
        let exited = groups(&provider).first().is_some_and(|g| !group_runs(*g));
        if let (true, Some(tool)) = (exited, ids(&provider.pids).first()) {
            break *tool;
        }
        assert!(Instant::now() < deadline, "timed out: the provider's exit");
        std::thread::sleep(Duration::from_micros(100));
    };
    assert!(group_runs(tool));

    request(
        &service,
        Command::SetStatus {
            session_id: main.id.clone(),
            status: Status::Paused,
        },
    );

    // Stopped with its agent, wherever the turn's end was on its way to the host.
    assert!(!group_runs(tool), "stopped before the pause returned");
    until_finished(home.path(), &main, 1);
    assert_eq!(
        recorded(home.path(), &main),
        [tool_group(tool, Some("stopped"), &[tool])]
    );
    let stopped = notices(home.path(), &main).pop().unwrap();
    let why = format!("stopped when {} was stopped.", main.name);
    assert!(stopped.ends_with(&why), "{stopped}");
    assert_eq!(status(&snapshot(&service), &main.id), Status::Paused);
}

#[test]
fn what_a_provider_starts_after_closing_its_output_is_recorded_while_it_runs() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Quiet");
    let service = start(home.path());
    send(&service, "quiet", &main, "QUIET_THEN_TOOL");

    // The provider said nothing more and still runs, so the turn has not ended; a host killed
    // now would find the tool in the store at its next start.
    let tool = until_tool_recorded(home.path(), &main, &provider, 1);
    assert!(group_runs(tool) && group_runs(groups(&provider)[0]));

    service.shutdown();
    assert!(!group_runs(tool));
}

#[test]
fn stopping_a_project_stops_what_its_idle_agents_started() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (project, main) = project(home.path(), scratch.path(), "Idle");
    let service = start(home.path());
    send(&service, "leave", &main, "LEAVE");
    until_finished(home.path(), &main, 1);
    let left = ids(&provider.pids)[0];
    assert!(group_runs(left));
    assert_eq!(status(&snapshot(&service), &main.id), Status::Ready);

    // No turn runs, so the Stop cancels nothing; an emergency stop still leaves nothing running.
    request(
        &service,
        Command::SetLive {
            project_id: project.id.clone(),
            enabled: false,
        },
    );

    assert!(!group_runs(left), "stopped before the stop returned");
    assert_eq!(
        recorded(home.path(), &main),
        [tool_group(left, Some("stopped"), &[left])]
    );
    let name = &main.name;
    let stopped = notices(home.path(), &main).pop().unwrap();
    assert_eq!(
        stopped,
        format!(
            "Stopped 1 process {name} had left running. It was the program `sleep`.\nProcess group {left} · stopped when {name} was stopped."
        )
    );
    // The agent itself is as it was: the project is stopped, not the session paused.
    assert_eq!(status(&snapshot(&service), &main.id), Status::Ready);
}

#[test]
fn a_quit_stops_what_agents_started_between_turns_and_in_a_running_turn() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, between) = project(home.path(), scratch.path(), "Between turns");
    let (_, working) = project(home.path(), scratch.path(), "Working");
    let service = start(home.path());
    send(&service, "leave", &between, "LEAVE");
    until_finished(home.path(), &between, 1);
    let left = ids(&provider.pids)[0];
    send(&service, "tool", &working, "TOOL_THEN_LINGER");
    let tool = until_tool_recorded(home.path(), &working, &provider, 2);
    assert!(group_runs(left) && group_runs(tool));

    let quit = Instant::now();
    service.shutdown();

    // No waiting here: the shutdown returned only once they were stopped, within its wait.
    assert!(!group_runs(left) && !group_runs(tool));
    assert!(
        !group_runs(groups(&provider)[1]),
        "the running turn's provider"
    );
    assert!(
        quit.elapsed() < Duration::from_secs(3),
        "{:?}",
        quit.elapsed()
    );
    assert_eq!(
        recorded(home.path(), &between),
        [tool_group(left, Some("quit"), &[left])]
    );
    let record = recorded(home.path(), &working);
    assert_eq!(record.len(), 1, "{record:?}");
    assert_tool_stopped(&record[0], tool, "quit");
    let stopped = notices(home.path(), &between).pop().unwrap();
    assert!(
        stopped.ends_with("stopped when Wiffletree quit."),
        "{stopped}"
    );
}

#[test]
fn a_restart_stops_what_a_killed_host_left_running_mid_turn_and_between_turns() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, between) = project(home.path(), scratch.path(), "Between turns");
    let (_, working) = project(home.path(), scratch.path(), "Working");
    // Both turns start together.
    {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.send("leave", None, &between.id, "LEAVE").unwrap();
        host.send("tool", None, &working.id, "TOOL_THEN_LINGER")
            .unwrap();
    }
    // A host of its own, so it can be killed as a crash would kill it. Its command input stays
    // open: closing it would be a quit.
    let mut headless = std::process::Command::new(env!("CARGO_BIN_EXE_workspace-host"))
        .arg("--data-dir")
        .arg(home.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    until_finished(home.path(), &between, 1);
    let left = until_tool_running(home.path(), &between);
    let tool = until_tool_running(home.path(), &working);
    // The unfinished turn's provider forks nothing more once it has saved the `sleep` it waits
    // on, after both tools. A process it forked while startup signals its group would join the
    // group after startup confirmed it, and by design that is left running with a note.
    let deadline = Instant::now() + Duration::from_secs(20);
    while ids(&provider.pids).len() < 3 {
        assert!(Instant::now() < deadline, "timed out: the provider's wait");
        std::thread::sleep(Duration::from_millis(20));
    }
    headless.kill().unwrap();
    headless.wait().unwrap();
    let providers = groups(&provider);
    let lingering: Vec<i32> = providers.into_iter().filter(|g| group_runs(*g)).collect();
    assert_eq!(
        lingering.len(),
        1,
        "the unfinished turn's provider outlived its host"
    );
    assert!(
        group_runs(left) && group_runs(tool),
        "and so did both processes"
    );

    let service = start(home.path());

    // Startup stopped them before it returned.
    assert!(!group_runs(left) && !group_runs(tool) && !group_runs(lingering[0]));
    assert_eq!(
        recorded(home.path(), &between),
        [tool_group(left, Some("startup"), &[left])]
    );
    let record = recorded(home.path(), &working);
    assert_eq!(record.len(), 2, "{record:?}");
    for group in &record {
        assert_eq!(group.ended.as_deref(), Some("startup"), "{record:?}");
    }
    assert!(record.iter().any(|r| r.provider && r.group == lingering[0]));
    assert!(record.iter().any(|r| !r.provider && r.group == tool));
    let after = snapshot(&service);
    assert_eq!(status(&after, &working.id), Status::Disconnected);
    assert!(after.attention.is_empty(), "{:?}", after.attention);
    let stopped = notices(home.path(), &working).pop().unwrap();
    assert!(
        stopped.ends_with("stopped when Wiffletree restarted."),
        "{stopped}"
    );
}

#[test]
fn a_recorded_group_whose_members_are_gone_is_never_signalled_and_is_noted_only_while_it_runs() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let _provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Recycled");
    let service = start(home.path());
    // Two groups the agent once started have emptied. The id of the first now belongs to an
    // unrelated process; nothing has the id of the second.
    let mut unrelated = Leftover::spawn();
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    gone.wait().unwrap();
    let db = store(home.path());
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail) VALUES ('earlier',?1,'[]',?2,?2,'completed','{}')",
        rusqlite::params![main.id, now()],
    )
    .unwrap();
    for (record, group) in [("recycled", unrelated.id()), ("emptied", gone.id())] {
        db.execute(
            "INSERT INTO process_groups(id,session_id,run_id,process_group,seen_at) VALUES (?1,?2,'earlier',?3,?4)",
            rusqlite::params![record, main.id, group, now()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO process_group_members(group_id,pid,started,command) VALUES (?1,?2,1,'yes')",
            rusqlite::params![record, gone.id()],
        )
        .unwrap();
    }

    request(
        &service,
        Command::SetArchived {
            session_id: main.id.clone(),
            archived: true,
        },
    );

    assert_eq!(unrelated.0.try_wait().unwrap(), None, "never signalled");
    // The group that still runs unconfirmed stays recorded and is noted; the other is forgotten.
    let recycled = unrelated.id() as i32;
    assert_eq!(
        recorded(home.path(), &main),
        [tool_group(recycled, None, &[gone.id() as i32])]
    );
    let notes = snapshot(&service).attention;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].operation_id, "leftover-process:recycled");
    let name = &main.name;
    assert_eq!(
        notes[0].prompt,
        format!(
            "{name}: process group {recycled} is still running after {name} was archived. Wiffletree could not confirm it is still that agent's process, so it did not stop it. Stop the process if it is the agent's, or dismiss this note if it is not."
        )
    );
    assert!(
        notices(home.path(), &main).is_empty(),
        "nothing was stopped"
    );

    // Dismissing the note settles the record; the process is left alone.
    request(
        &service,
        Command::DismissAttention {
            id: notes[0].id.clone(),
        },
    );
    assert_eq!(
        recorded(home.path(), &main)[0].ended.as_deref(),
        Some("dismissed")
    );
    assert_eq!(unrelated.0.try_wait().unwrap(), None);
}

/// What the Processes panel opened on `session` lists.
fn panel(service: &Service, session: &Session) -> Vec<AgentProcesses> {
    let listing = request(
        service,
        Command::Processes {
            session_id: session.id.clone(),
        },
    );
    serde_json::from_value(listing).unwrap()
}

fn stop_from_panel(service: &Service, session: &Session, groups: &[&str]) -> Vec<AgentProcesses> {
    let listing = request(
        service,
        Command::StopProcesses {
            session_id: session.id.clone(),
            groups: groups.iter().map(|g| g.to_string()).collect(),
        },
    );
    serde_json::from_value(listing).unwrap()
}

/// The rail's count for the agent, from the snapshot.
fn rail_count(service: &Service, session: &Session) -> u32 {
    let counts = snapshot(service).running_processes;
    let count = counts.iter().find(|c| c.session_id == session.id);
    count.map_or(0, |c| c.count)
}

#[test]
fn a_live_runs_record_and_rail_count_drop_reaped_children_and_the_group_still_stops() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Churn");
    let service = start(home.path());
    send(&service, "churn", &main, "CHURN_CHILDREN");
    let tool = until_tool_running(home.path(), &main);
    let ready = format!("{}.{}", provider.pids.display(), groups(&provider)[0]);

    for batch in 0..3 {
        until(&service, "all twelve children to be recorded", |s| {
            s.running_processes
                .iter()
                .any(|c| c.session_id == main.id && c.count == 13)
                && recorded(home.path(), &main)
                    .iter()
                    .any(|r| r.group == tool && r.members.len() == 13)
        });
        let gate = format!("{ready}.{batch}");
        std::fs::write(format!("{gate}.reap"), "").unwrap();
        common::until("the tool to reap its children", || {
            Path::new(&format!("{gate}.reaped")).exists()
        });
        // The run stays live, with just its long-lived tool. No between-turn replacement
        // or panel refresh can be responsible for shrinking the record or the badge.
        until(
            &service,
            "only the long-lived tool to remain recorded",
            |s| {
                s.running_processes
                    .iter()
                    .any(|c| c.session_id == main.id && c.count == 1)
                    && recorded(home.path(), &main)
                        .iter()
                        .any(|r| r.group == tool && r.members == [tool])
            },
        );
        assert_eq!(status(&snapshot(&service), &main.id), Status::Working);
        assert_eq!(run_result(home.path(), &main.id).0, None);
        if batch < 2 {
            std::fs::write(format!("{gate}.next"), "").unwrap();
        }
    }

    let listing = panel(&service, &main);
    let [group] = &listing[0].groups[..] else {
        panic!("one tool group: {listing:?}")
    };
    assert!(group.confirmed);
    assert_eq!(group.processes.len(), 1);
    assert_eq!(group.processes[0].pid, tool);
    assert!(
        stop_from_panel(&service, &main, &[&group.id])[0]
            .groups
            .is_empty()
    );
    assert!(!group_runs(tool));
    assert_eq!(rail_count(&service, &main), 0);
    assert_tool_stopped(
        recorded(home.path(), &main)
            .iter()
            .find(|r| r.group == tool)
            .unwrap(),
        tool,
        "stopped_by_human",
    );
    assert!(snapshot(&service).attention.is_empty());
}

#[test]
fn the_panel_lists_what_each_agent_left_running_and_stops_a_group_without_stopping_the_agent() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (project, main) = project(home.path(), scratch.path(), "Panel");
    let reviewer = Host::open_set_up(home.path())
        .unwrap()
        .create_session(
            &project.id,
            &main.id,
            None,
            "Reviewer",
            Role::Reviewer,
            workspace_core::Provider::Claude,
        )
        .unwrap();
    let service = start(home.path());
    send(&service, "first", &main, "LEAVE");
    until_finished(home.path(), &main, 1);
    send(&service, "second", &main, "LEAVE");
    until_finished(home.path(), &main, 2);
    let [first, second] = ids(&provider.pids)[..] else {
        panic!("one process a turn")
    };
    assert_eq!(rail_count(&service, &main), 2);

    // The coordinator's panel lists every agent, with nothing for the one that started none.
    let listing = panel(&service, &main);
    let agents: Vec<_> = listing.iter().map(|a| a.session_id.as_str()).collect();
    assert_eq!(agents, [main.id.as_str(), reviewer.id.as_str()]);
    assert!(listing[1].groups.is_empty());
    let groups = &listing[0].groups;
    assert_eq!(
        groups.iter().map(|g| g.group).collect::<Vec<_>>(),
        [i64::from(first), i64::from(second)]
    );
    let group = &groups[0];
    assert!(group.confirmed);
    assert_eq!(group.started, common::process_started(first));
    let [process] = &group.processes[..] else {
        panic!("one process: {group:?}")
    };
    assert_eq!(
        (process.pid, process.started, process.program.as_str()),
        (first, common::process_started(first), "sleep")
    );
    // The reviewer's own panel lists only the reviewer.
    assert_eq!(
        panel(&service, &reviewer),
        [AgentProcesses {
            session_id: reviewer.id.clone(),
            name: reviewer.name.clone(),
            groups: vec![],
        }]
    );

    // Stop on one row stops that group alone and tells the agent; the agent is not stopped.
    let listing = stop_from_panel(&service, &main, &[&group.id]);
    assert!(!group_runs(first), "stopped before the stop returned");
    assert!(group_runs(second));
    let left: Vec<_> = listing[0].groups.iter().map(|g| g.group).collect();
    assert_eq!(left, [i64::from(second)]);
    assert_eq!(
        recorded(home.path(), &main),
        [
            tool_group(first, Some("stopped_by_human"), &[first]),
            tool_group(second, None, &[second])
        ]
    );
    let name = &main.name;
    assert_eq!(
        notices(home.path(), &main).pop().unwrap(),
        format!(
            "Stopped 1 process {name} had left running. It was the program `sleep`.\nProcess group {first} · stopped when the human pressed Stop in the Processes panel."
        )
    );
    assert_eq!(rail_count(&service, &main), 1);
    let after = snapshot(&service);
    assert_eq!(status(&after, &main.id), Status::Ready);
    assert!(after.live_projects.contains(&project.id));
    assert!(after.attention.is_empty(), "{:?}", after.attention);

    // Stop all on the agent's panel stops the rest.
    let listing = stop_from_panel(&service, &main, &[]);
    assert!(!group_runs(second));
    assert!(listing[0].groups.is_empty());
    assert_tool_stopped(&recorded(home.path(), &main)[1], second, "stopped_by_human");
    assert_eq!(rail_count(&service, &main), 0);

    // The notices ride along with the agent's next turn.
    send(&service, "next", &main, "FINISH");
    until_finished(home.path(), &main, 3);
    let prompts = std::fs::read_to_string(&provider.prompts).unwrap();
    let told = prompts.split("=====").nth(2).unwrap();
    assert!(
        told.contains(&format!("Process group {first} · stopped when the human")),
        "{told}"
    );
    assert!(
        told.contains(&format!("Process group {second} · stopped when the human")),
        "{told}"
    );
}

#[test]
fn the_panel_never_lists_or_stops_a_group_it_cannot_confirm() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let _provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Unconfirmed");
    let service = start(home.path());
    // Two groups the agent once started have emptied. The id of the first now belongs to an
    // unrelated process; nothing has the id of the second.
    let mut unrelated = Leftover::spawn();
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    gone.wait().unwrap();
    let db = store(home.path());
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail) VALUES ('earlier',?1,'[]',?2,?2,'completed','{}')",
        rusqlite::params![main.id, now()],
    )
    .unwrap();
    for (record, group) in [("recycled", unrelated.id()), ("emptied", gone.id())] {
        db.execute(
            "INSERT INTO process_groups(id,session_id,run_id,process_group,seen_at) VALUES (?1,?2,'earlier',?3,?4)",
            rusqlite::params![record, main.id, group, now()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO process_group_members(group_id,pid,started,command) VALUES (?1,?2,7,'yes')",
            rusqlite::params![record, gone.id()],
        )
        .unwrap();
    }
    assert_eq!(rail_count(&service, &main), 2);

    // The emptied group is forgotten; the recycled one is listed as unconfirmed, with nothing
    // of the unrelated process in it.
    let listing = panel(&service, &main);
    let recycled = i64::from(unrelated.id());
    assert_eq!(
        listing[0].groups,
        [ProcessGroup {
            id: "recycled".into(),
            group: recycled,
            confirmed: false,
            started: 7,
            processes: vec![],
        }]
    );
    let record = recorded(home.path(), &main);
    assert_eq!(
        record,
        [tool_group(recycled as i32, None, &[gone.id() as i32])]
    );
    assert_eq!(rail_count(&service, &main), 1);

    // Stop is refused by the confirmation rule: nothing is signalled and nothing is told.
    let listing = stop_from_panel(&service, &main, &["recycled"]);
    assert_eq!(unrelated.0.try_wait().unwrap(), None, "never signalled");
    assert!(!listing[0].groups[0].confirmed);
    assert!(notices(home.path(), &main).is_empty());
}

#[test]
fn stop_agents_and_quit_stops_running_turns_as_a_quit_and_ends_the_host() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, working) = project(home.path(), scratch.path(), "Working");
    let service = start(home.path());
    send(&service, "tool", &working, "TOOL_THEN_LINGER");
    let tool = until_tool_recorded(home.path(), &working, &provider, 1);
    let group = groups(&provider)[0];

    // The reply comes once the shutdown has stopped the turn.
    assert_eq!(request(&service, Command::StopAgentsAndQuit), Value::Null);

    assert!(!group_runs(group) && !group_runs(tool));
    let (outcome, error) = run_result(home.path(), &working.id);
    assert_eq!(outcome.as_deref(), Some("failed"));
    assert!(
        error.unwrap().starts_with("Wiffletree quit during a turn"),
        "the turn is recorded as interrupted by the quit"
    );
    let record = recorded(home.path(), &working);
    assert_eq!(record.len(), 1, "{record:?}");
    assert_tool_stopped(&record[0], tool, "quit");
    common::until("the host to end", || service.ended().is_closed());
}

/// The power assertions this process holds under the host's name, as `pmset` lists them.
fn awake_assertions() -> usize {
    let output = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "assertions"])
        .output()
        .unwrap();
    let me = format!("pid {}(", std::process::id());
    let name = format!("named: \"{}\"", workspace_host::power::ASSERTION_NAME);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(&me) && line.contains(&name))
        .count()
}

/// Ends a lingering turn by stopping the `sleep` its provider waits on, the last id it saved.
fn end_lingering_turn(provider: &Provider) {
    let sleep = *ids(&provider.pids).last().unwrap();
    unsafe { libc::kill(sleep, libc::SIGKILL) };
}

#[test]
fn an_idle_restart_waits_for_the_running_turn_and_the_mac_stays_awake_meanwhile() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, main) = project(home.path(), scratch.path(), "Restart");
    let service = start(home.path());
    assert_eq!(awake_assertions(), 0, "an idle host holds no assertion");
    send(&service, "linger", &main, "LINGER");
    until(&service, "the turn to start", |s| {
        status(s, &main.id) == Status::Working && !ids(&provider.pids).is_empty()
    });
    assert_eq!(awake_assertions(), 1, "one assertion while a turn runs");

    let restart = Command::RestartHost {
        when: RestartWhen::Idle,
    };
    assert_eq!(request(&service, restart), Value::Null);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!service.ended().is_closed(), "the host waits for the turn");
    assert_eq!(status(&snapshot(&service), &main.id), Status::Working);

    end_lingering_turn(&provider);
    common::until("the host to end", || service.ended().is_closed());
    assert_eq!(awake_assertions(), 0, "released once nothing runs");
    let (outcome, error) = run_result(home.path(), &main.id);
    assert!(outcome.is_some(), "the turn ended on its own");
    assert!(
        !error.is_some_and(|e| e.starts_with("Wiffletree quit during a turn")),
        "the restart interrupted nothing"
    );
}

#[test]
fn the_power_assertion_is_released_when_the_last_turn_ends() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let provider = install_provider(scratch.path());
    let (_, first) = project(home.path(), scratch.path(), "First");
    let (_, second) = project(home.path(), scratch.path(), "Second");
    let service = start(home.path());
    send(&service, "first", &first, "LINGER");
    send(&service, "second", &second, "LINGER");
    until(&service, "both turns to start", |s| {
        status(s, &first.id) == Status::Working
            && status(s, &second.id) == Status::Working
            && ids(&provider.pids).len() == 2
    });
    assert_eq!(
        awake_assertions(),
        1,
        "one assertion however many turns run"
    );

    end_lingering_turn(&provider);
    until(&service, "one turn to end", |s| {
        [&first, &second]
            .iter()
            .filter(|session| status(s, &session.id) == Status::Working)
            .count()
            == 1
    });
    assert_eq!(awake_assertions(), 1, "held while a turn still runs");

    unsafe { libc::kill(ids(&provider.pids)[0], libc::SIGKILL) };
    until(&service, "both turns to end", |s| {
        status(s, &first.id) != Status::Working && status(s, &second.id) != Status::Working
    });
    assert_eq!(awake_assertions(), 0);
}
