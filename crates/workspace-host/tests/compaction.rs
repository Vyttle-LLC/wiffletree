//! Compacting a Claude agent's context, through the real Service with the fake provider
//! standing in for the CLI and answering `/compact` with recorded streams. Kept in its own
//! test binary because it sets the provider environment for the process.
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::{Host, service::Service};

/// The tests share the provider environment, so they run one at a time.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// One run of an agent, as stored.
#[derive(Debug)]
struct Run {
    id: String,
    compaction: bool,
    outcome: Option<String>,
    started_at: i64,
    finished_at: Option<i64>,
    group: Option<i32>,
}

/// A live project whose coordinator, a Claude agent, has run one turn.
struct World {
    home: tempfile::TempDir,
    scratch: tempfile::TempDir,
    service: Option<Service>,
    main: Session,
    _environment: MutexGuard<'static, ()>,
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

fn claude() -> ModelProfile {
    ModelProfile {
        provider: Provider::Claude,
        model: "opus".into(),
        effort: "medium".into(),
    }
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl World {
    /// A store with one project, set up by `prepare` before the host starts.
    fn prepared(prepare: impl FnOnce(&mut Host, &Project, &Session, &Path)) -> Self {
        let environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let prompts = scratch.path().join("prompts");
        std::fs::create_dir(&prompts).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider.py");
        // SAFETY: the caller holds ENVIRONMENT, and no earlier test's host is still running.
        unsafe {
            std::env::set_var("WORKSPACE_CLAUDE_BIN", &fixture);
            std::env::set_var("WORKSPACE_CODEX_BIN", &fixture);
            std::env::set_var("WORKSPACE_TEST_USAGE_DIR", scratch.path());
            std::env::set_var("WORKSPACE_TEST_PROMPT_DIR", &prompts);
        }
        let main = {
            let mut host = Host::open_set_up(home.path()).unwrap();
            host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
                .unwrap();
            let project = host.create_project("Compact").unwrap();
            let main = host.sessions().unwrap().remove(0);
            host.configure_session(&main.id, claude()).unwrap();
            host.set_live(&project.id, true).unwrap();
            prepare(&mut host, &project, &main, scratch.path());
            main
        };
        let mut world = Self {
            home,
            scratch,
            service: None,
            main,
            _environment: environment,
        };
        world.start();
        world
    }

    /// The coordinator has replied once, so it has a Claude session to compact.
    fn new() -> Self {
        let world = Self::prepared(|_, _, _, _| {});
        world.send("JUST_REPLY first");
        world.until_replies(1);
        world
    }

    fn start(&mut self) {
        let helper = env!("CARGO_BIN_EXE_workspace-host").into();
        self.service = Some(Service::start(self.home.path().to_path_buf(), helper).unwrap());
    }

    fn stop(&mut self) {
        self.service.take().unwrap().shutdown();
    }

    fn answer(&self, command: Command) -> Result<Value, String> {
        let service = self.service.as_ref().unwrap();
        service.request(command).unwrap().recv_blocking().unwrap()
    }

    fn request(&self, command: Command) -> Value {
        self.answer(command).unwrap()
    }

    fn snapshot(&self) -> Snapshot {
        serde_json::from_value(self.request(Command::Snapshot)).unwrap()
    }

    fn status(&self, session: &str) -> Status {
        let sessions = self.snapshot().sessions;
        sessions.iter().find(|s| s.id == session).unwrap().status
    }

    fn runtime(&self, session: &str) -> SessionRuntime {
        let runtimes = self.snapshot().runtimes;
        let runtime = runtimes.into_iter().find(|r| r.session_id == session);
        runtime.unwrap()
    }

    /// The coordinator's compaction request, if it has one.
    fn compaction(&self) -> Option<Compaction> {
        self.runtime(&self.main.id).compaction
    }

    fn outcome(&self) -> Option<CompactionOutcome> {
        self.runtime(&self.main.id).last_compaction
    }

    fn messages(&self, session: &str) -> Vec<Message> {
        let page = Command::Messages {
            session_id: session.into(),
            before: None,
            limit: 100,
        };
        serde_json::from_value(self.request(page)).unwrap()
    }

    fn replies(&self) -> usize {
        let messages = self.messages(&self.main.id);
        let replies = messages.iter().filter(|m| m.id.starts_with("output:"));
        replies.count()
    }

    fn until_replies(&self, count: usize) {
        until(&format!("reply {count}"), || self.replies() == count);
    }

    fn lines(&self) -> Vec<String> {
        let messages = self.messages(&self.main.id);
        let lines = messages
            .into_iter()
            .filter(|m| m.id.starts_with(COMPACTION_LINE_PREFIX));
        lines.map(|m| m.body).collect()
    }

    /// Sends the coordinator a message from the human and returns its id.
    fn send(&self, body: &str) -> String {
        self.send_to(&self.main.id, body)
    }

    fn send_to(&self, session: &str, body: &str) -> String {
        let id = new_id();
        self.request(Command::Send {
            id: id.clone(),
            sender: None,
            recipient: session.into(),
            body: body.into(),
            attachments: vec![],
        });
        id
    }

    fn receipt(&self, message: &str) -> Receipt {
        self.receipt_in(&self.main.id, message)
    }

    fn receipt_in(&self, session: &str, message: &str) -> Receipt {
        let messages = self.messages(session);
        messages.iter().find(|m| m.id == message).unwrap().receipt
    }

    /// The coordinator's one worker, once its first turn is over and it has a session.
    fn worker(&self) -> String {
        let mut worker = None;
        until("the worker's first turn", || {
            let sessions = self.snapshot().sessions;
            let found = sessions
                .into_iter()
                .find(|s| s.parent_id.as_deref() == Some(&self.main.id));
            worker = found.map(|s| s.id);
            worker.as_ref().is_some_and(|id| {
                let turns = self.turns(id);
                turns.first().is_some_and(|t| t.finished_at.is_some())
            })
        });
        worker.unwrap()
    }

    /// The worker, with a compaction of its own running and held until `gate-compact`.
    fn compacting_worker(&self) -> String {
        let worker = self.worker();
        self.compact(&worker, Some("WAIT_FOR_GATE")).unwrap();
        self.until_compacting(&worker);
        worker
    }

    fn usage(&self) -> UsageReport {
        serde_json::from_value(self.request(Command::Usage {
            days: 7,
            project_id: None,
            provider: None,
            timezone: "UTC".into(),
        }))
        .unwrap()
    }

    fn compact(&self, session: &str, note: Option<&str>) -> Result<Value, String> {
        self.answer(Command::CompactContext {
            session_id: session.into(),
            instructions: note.map(str::to_owned),
        })
    }

    /// Asks to compact the coordinator and waits for the outcome.
    fn compacted(&self, note: Option<&str>) -> CompactionOutcome {
        self.compact(&self.main.id, note).unwrap();
        until("the compaction to settle", || self.outcome().is_some());
        self.outcome().unwrap()
    }

    fn until_compacting(&self, session: &str) {
        until("the compaction to start", || {
            let request = self.runtime(session).compaction;
            request.is_some_and(|c| c.state == CompactionState::Running)
        });
    }

    fn set_status(&self, session: &str, status: Status) {
        self.request(Command::SetStatus {
            session_id: session.into(),
            status,
        });
    }

    fn resume(&self, session: &str) {
        self.request(Command::ReconcileSession {
            session_id: session.into(),
            retry: true,
        });
    }

    /// Lets a turn or a compaction that waits for `gate` go on.
    fn open(&self, gate: &str) {
        std::fs::write(self.scratch.path().join(gate), "").unwrap();
    }

    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.home.path().join("workspace.sqlite3")).unwrap()
    }

    fn runs(&self, session: &str) -> Vec<Run> {
        let db = self.db();
        let mut runs = db
            .prepare(
                "SELECT id,json_extract(detail,'$.kind') IS NOT NULL,outcome,started_at,finished_at,process_group FROM provider_runs WHERE session_id=?1 ORDER BY rowid",
            )
            .unwrap();
        let runs = runs.query_map([session], |r| {
            Ok(Run {
                id: r.get(0)?,
                compaction: r.get(1)?,
                outcome: r.get(2)?,
                started_at: r.get(3)?,
                finished_at: r.get(4)?,
                group: r.get(5)?,
            })
        });
        runs.unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    fn compactions(&self, session: &str) -> Vec<Run> {
        let runs = self.runs(session).into_iter();
        runs.filter(|r| r.compaction).collect()
    }

    fn turns(&self, session: &str) -> Vec<Run> {
        let runs = self.runs(session).into_iter();
        runs.filter(|r| !r.compaction).collect()
    }

    /// The stopped store, to change what no client command can.
    fn host(&self) -> Host {
        assert!(self.service.is_none(), "stop the service first");
        Host::open_set_up(self.home.path()).unwrap()
    }

    /// Stops the service, queues a compaction of the coordinator and a message behind it, and
    /// runs the store under the headless host until the compaction's provider process is
    /// recorded. Then the host is killed, as a crash would: its process outlives it. Returns
    /// that process's group.
    fn crash_while_compacting(&mut self) -> i32 {
        self.stop();
        {
            let mut host = self.host();
            host.request_compaction(&self.main.id, Some("WAIT_FOR_GATE"))
                .unwrap();
            host.send("waiting", None, &self.main.id, "JUST_REPLY waiting")
                .unwrap();
        }
        let mut headless = std::process::Command::new(env!("CARGO_BIN_EXE_workspace-host"))
            .arg("--data-dir")
            .arg(self.home.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut group = None;
        until("the compaction's provider process", || {
            group = self
                .compactions(&self.main.id)
                .first()
                .and_then(|c| c.group);
            group.is_some()
        });
        headless.kill().unwrap();
        headless.wait().unwrap();
        let group = group.unwrap();
        assert_eq!(unsafe { libc::killpg(group, 0) }, 0, "it outlived the host");
        group
    }

    /// The ledger's requests, their input tokens, and how many of them are compactions'.
    fn ledger(&self) -> (i64, i64, i64) {
        let totals =
            "SELECT COUNT(*),SUM(input),SUM(request_id LIKE 'compact:%') FROM usage_requests";
        let db = self.db();
        db.query_row(totals, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
    }

    /// What the fake Claude itself counts for the session's whole life: its input tokens,
    /// cache reads and writes included.
    fn claude_input_total(&self, session: &str) -> i64 {
        let claude_session = self.runtime(session).provider_session_id.unwrap();
        let totals = format!("{claude_session}.model-usage");
        let totals = std::fs::read_to_string(self.scratch.path().join(totals)).unwrap();
        let totals: Value = serde_json::from_str(&totals).unwrap();
        [
            "inputTokens",
            "cacheReadInputTokens",
            "cacheCreationInputTokens",
        ]
        .iter()
        .map(|count| totals["claude-opus-5-5"][count].as_i64().unwrap())
        .sum()
    }

    /// Lets the scheduler pass several times; every request is followed by a pass.
    fn several_passes(&self) {
        for _ in 0..10 {
            self.snapshot();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// What the provider was given on stdin for each compaction, and the session it resumed.
    fn compact_commands(&self) -> Vec<Value> {
        let log = self.scratch.path().join("prompts/compactions.txt");
        let log = std::fs::read_to_string(log).unwrap_or_default();
        log.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn prompts(&self) -> String {
        let log = format!("prompts/{}.txt", self.main.id);
        std::fs::read_to_string(self.scratch.path().join(log)).unwrap()
    }
}

#[test]
fn only_a_claude_agent_with_a_session_and_no_request_can_be_asked_to_compact() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let project = host.create_project("Compact").unwrap();
    let main = host.sessions().unwrap().remove(0);
    host.configure_session(&main.id, claude()).unwrap();
    let compact = |host: &mut Host, session: &Session, note: Option<&str>| {
        host.execute(Command::CompactContext {
            session_id: session.id.clone(),
            instructions: note.map(str::to_owned),
        })
        .map_err(|e| e.to_string())
    };
    let request = |host: &Host, session: &Session| host.session_runtime(&session.id).unwrap();

    let refused = compact(&mut host, &main, None).unwrap_err();
    assert_eq!(refused, "Available after this agent’s first turn.");

    let codex = host
        .create_session(
            &project.id,
            &main.id,
            None,
            "Codex",
            Role::Reviewer,
            Provider::Codex,
        )
        .unwrap();
    for session in [&main, &codex] {
        let mut runtime = host.session_runtime(&session.id).unwrap();
        runtime.provider_session_id = Some("provider-session".into());
        host.save_runtime(&runtime).unwrap();
    }
    let refused = compact(&mut host, &codex, None).unwrap_err();
    assert_eq!(
        refused,
        "Not available for Codex agents: the Codex CLI has no compact command."
    );
    assert_eq!(request(&host, &codex).compaction, None);

    let long = "x".repeat(1001);
    let refused = compact(&mut host, &main, Some(&long)).unwrap_err();
    assert!(refused.contains("1,000 characters"), "{refused}");
    assert_eq!(request(&host, &main).compaction, None);

    // A note is one trimmed line; a second request leaves the first as it is.
    compact(&mut host, &main, Some(" the failing tests\nand the plan ")).unwrap();
    let refused = compact(&mut host, &main, Some("something else")).unwrap_err();
    assert!(refused.contains("already has a compaction"), "{refused}");
    let stored = request(&host, &main).compaction.unwrap();
    assert_eq!(stored.state, CompactionState::Queued);
    assert_eq!(
        stored.instructions.as_deref(),
        Some("the failing tests and the plan")
    );

    host.execute(Command::CancelCompaction {
        session_id: main.id.clone(),
    })
    .unwrap();
    assert_eq!(request(&host, &main).compaction, None);
    host.set_archived(&main.id, true).unwrap();
    let refused = compact(&mut host, &main, None).unwrap_err();
    assert!(refused.contains("archived"), "{refused}");
}

#[test]
fn an_idle_agent_is_compacted_at_once_with_claudes_figures_in_its_line_and_the_ledger() {
    let mut world = World::new();
    let main = world.main.id.clone();
    let before = world.runtime(&main);

    let outcome = world.compacted(Some("The failing test names and the migration plan"));

    assert_eq!(
        (outcome.result, outcome.reason),
        (CompactionResult::Compacted, None)
    );
    assert_eq!(world.compaction(), None);
    assert_eq!(world.status(&main), Status::Ready);
    // The provider got the bare command on the agent's own session, not a wrapped message.
    assert_eq!(
        world.compact_commands(),
        [json!({
            "input":"/compact The failing test names and the migration plan",
            "resumed":before.provider_session_id,
        })]
    );
    // One line under its own id, not a reply; a compaction is not the agent's last turn.
    let run = world.compactions(&main).remove(0);
    assert_eq!(run.outcome.as_deref(), Some("compacted"));
    let messages = world.messages(&main);
    let line = messages.last().unwrap();
    assert_eq!(line.id, format!("{COMPACTION_LINE_PREFIX}{}", run.id));
    assert_eq!(
        line.body,
        "Context compacted · 812k → 5k\nKept: The failing test names and the migration plan"
    );
    assert_eq!(line.receipt, Receipt::Completed);
    assert_eq!(world.replies(), 1);
    let after = world.runtime(&main);
    assert_eq!(after.last_finished_at, before.last_finished_at);
    assert_eq!(after.provider_session_id, before.provider_session_id);
    // Its tokens come from modelUsage, with the cached tokens added to Claude's input.
    let row: (String, String, i64, i64, i64, i64, f64) = world
        .db()
        .query_row(
            "SELECT u.request_id,u.model,u.input,u.cache_read,u.cache_write,u.output,json_extract(r.detail,'$.result.total_cost_usd') FROM usage_requests u JOIN provider_runs r ON r.id=u.run_id WHERE u.run_id=?1",
            [&run.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            format!("compact:{}:claude-opus-5-5", run.id),
            "claude-opus-5-5".into(),
            813_402,
            811_900,
            0,
            2_960,
            0.48
        )
    );
    let usage = world.usage();
    assert_eq!(usage.totals.requests, 2, "the turn's request and this one");
    assert_eq!(usage.unreported_runs, 0);

    // Without a note the line has no second line, and both lines survive a restart in place.
    world.compacted(None);
    let lines = world.lines();
    assert_eq!(lines[1], "Context compacted · 812k → 5k");
    let ids = |world: &World| -> Vec<String> {
        let messages = world.messages(&main);
        messages.into_iter().map(|m| m.id).collect()
    };
    let shown = ids(&world);
    world.stop();
    world.start();
    assert_eq!(ids(&world), shown);
    assert_eq!(world.lines(), lines);
}

#[test]
fn the_outcome_is_what_claudes_status_record_said_whatever_the_exit_code() {
    let world = World::new();
    let main = world.main.id.clone();

    // Too little to summarize: a quiet outcome, no line and nothing spent.
    let nothing = world.compacted(Some("NOTHING"));
    assert_eq!(nothing.result, CompactionResult::NothingToCompact);
    assert_eq!(
        nothing.reason.as_deref(),
        Some("Not enough messages to compact.")
    );
    assert!(world.lines().is_empty());
    let spent: i64 = world
        .db()
        .query_row(
            "SELECT COUNT(*) FROM usage_requests WHERE request_id LIKE 'compact:%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(spent, 0);
    world.request(Command::DismissCompaction {
        session_id: main.clone(),
    });
    assert_eq!(world.outcome(), None);

    // Claude said it failed and still exited 0 without an error flag.
    let failed = world.compacted(Some("FAIL"));
    assert_eq!(failed.result, CompactionResult::Failed);
    assert!(
        failed
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("Conversation too long")),
        "{failed:?}"
    );
    assert_eq!(world.lines(), ["Compaction failed · context unchanged"]);

    // A new request clears the last outcome, and once it runs it cannot be taken back.
    world
        .compact(&main, Some("NO_STATUS WAIT_FOR_GATE"))
        .unwrap();
    world.until_compacting(&main);
    assert_eq!(world.outcome(), None);
    let refused = world.answer(Command::CancelCompaction {
        session_id: main.clone(),
    });
    assert!(refused.unwrap_err().contains("cannot be cancelled"));
    assert!(world.compact(&main, None).is_err(), "one request at a time");
    world.open("gate-compact");
    until("the silent stream to end", || world.outcome().is_some());

    // No status record at all is a failure, however cleanly the process ended.
    let silent = world.outcome().unwrap();
    assert_eq!(silent.result, CompactionResult::Failed);
    assert_eq!(world.lines().len(), 2);
    assert_eq!(world.status(&main), Status::Ready);
    assert_eq!(world.replies(), 1, "no outcome is a reply");
}

#[test]
fn a_compaction_requested_mid_turn_runs_when_the_turn_ends_before_the_waiting_message() {
    let world = World::new();
    let main = world.main.id.clone();
    world.send("JUST_REPLY WAIT_FOR_GATE");
    until("the turn to start", || {
        world.status(&main) == Status::Working
    });

    world
        .compact(&main, Some("WAIT_FOR_GATE the plan"))
        .unwrap();
    let waiting = world.send("JUST_REPLY afterwards");
    world.several_passes();
    assert_eq!(
        world.compaction().map(|c| c.state),
        Some(CompactionState::Queued)
    );
    assert!(world.compactions(&main).is_empty(), "the turn still runs");

    // The turn ends: the compaction runs first and the message waits for it.
    world.open("gate-turn");
    world.until_compacting(&main);
    world.several_passes();
    assert_eq!(world.receipt(&waiting), Receipt::Queued);
    assert_eq!(world.turns(&main).len(), 2);
    assert_eq!(world.status(&main), Status::Ready);

    world.open("gate-compact");
    world.until_replies(3);

    // The turn's reply, then the compaction's line, then the turn that took the message.
    let messages = world.messages(&main);
    let place = |prefix: &str, nth: usize| {
        let mut matching = messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.id.starts_with(prefix));
        matching.nth(nth).unwrap().0
    };
    assert!(place("output:", 1) < place(COMPACTION_LINE_PREFIX, 0));
    assert!(place(COMPACTION_LINE_PREFIX, 0) < place("output:", 2));
    assert_eq!(world.receipt(&waiting), Receipt::Completed);
    // The session never ran a turn and a compaction at once.
    let compaction = world.compactions(&main).remove(0);
    let turns = world.turns(&main);
    assert!(turns[1].finished_at.unwrap() <= compaction.started_at);
    assert!(compaction.finished_at.unwrap() <= turns[2].started_at);
}

#[test]
fn a_cancelled_request_runs_no_compaction_when_its_turn_ends() {
    let world = World::new();
    let main = world.main.id.clone();
    world.send("JUST_REPLY WAIT_FOR_GATE");
    until("the turn to start", || {
        world.status(&main) == Status::Working
    });
    world.compact(&main, Some("the plan")).unwrap();

    world.request(Command::CancelCompaction {
        session_id: main.clone(),
    });

    assert_eq!(world.compaction(), None);
    world.open("gate-turn");
    world.until_replies(2);
    world.send("JUST_REPLY again");
    world.until_replies(3);
    assert!(world.compactions(&main).is_empty());
    assert!(world.lines().is_empty());
}

#[test]
fn a_request_outlives_a_failed_turn_and_runs_before_its_retry() {
    let world = World::new();
    let main = world.main.id.clone();
    world.send("JUST_REPLY WAIT_FOR_GATE EXIT_WITH_ERROR");
    until("the turn to start", || {
        world.status(&main) == Status::Working
    });
    world.compact(&main, None).unwrap();

    world.open("gate-turn");
    until("the turn to fail", || {
        world.status(&main) == Status::Disconnected
    });

    // Interrupted with its input held: the request waits, shown and still cancellable.
    world.several_passes();
    assert_eq!(
        world.compaction().map(|c| c.state),
        Some(CompactionState::Queued)
    );
    assert!(world.compactions(&main).is_empty());
    // Held input alone keeps it waiting, even on an agent that is ready again.
    world.set_status(&main, Status::Ready);
    world.several_passes();
    assert!(world.compactions(&main).is_empty());

    world.resume(&main);
    // The first reply, the failed turn's error notice and the retry's reply.
    world.until_replies(3);

    let compaction = world.compactions(&main).remove(0);
    let turns = world.turns(&main);
    assert_eq!(turns.len(), 3);
    assert!(compaction.finished_at.unwrap() <= turns[2].started_at);
    assert_eq!(world.lines().len(), 1);

    // The failed turn reported no totals, though Claude counted its request and the ledger
    // has it from the stream. The compaction right after it therefore has no baseline and
    // records nothing, rather than count that turn again: the ledger holds the three turns'
    // requests once each, short of Claude's own total by that one compaction.
    const TURN: i64 = 100 + 20 + 5;
    const COMPACTION: i64 = 1_502 + 811_900;
    assert_eq!(world.ledger(), (3, 3 * TURN, 0));
    assert_eq!(world.claude_input_total(&main), 3 * TURN + COMPACTION);

    // The next one follows a turn that finished, and records only its own tokens.
    world.compacted(None);
    assert_eq!(world.ledger(), (4, 3 * TURN + COMPACTION, 1));
    assert_eq!(world.claude_input_total(&main), 3 * TURN + 2 * COMPACTION);
}

#[test]
fn a_request_waits_for_a_paused_agent_and_for_a_stopped_project() {
    let mut world = World::new();
    let main = world.main.id.clone();
    world.set_status(&main, Status::Paused);
    world.compact(&main, Some("the plan")).unwrap();
    world.several_passes();
    assert!(world.compactions(&main).is_empty());

    // A restart leaves a queued request exactly as it was, and runs nothing.
    world.stop();
    world.start();
    world.several_passes();
    let request = world.compaction().unwrap();
    assert_eq!(request.state, CompactionState::Queued);
    assert_eq!(request.instructions.as_deref(), Some("the plan"));
    assert!(world.compactions(&main).is_empty());

    world.resume(&main);
    until("the resumed agent's compaction", || {
        world.outcome().is_some()
    });
    assert_eq!(world.compactions(&main).len(), 1);

    let project = world.main.project_id.clone();
    world.request(Command::SetLive {
        project_id: project.clone(),
        enabled: false,
    });
    world.compact(&main, None).unwrap();
    world.several_passes();
    assert_eq!(world.compactions(&main).len(), 1, "the project is stopped");
    world.request(Command::SetLive {
        project_id: project,
        enabled: true,
    });
    until("the live project's compaction", || {
        world.compactions(&main).len() == 2 && world.outcome().is_some()
    });
}

#[test]
fn a_pause_while_compacting_lets_it_finish_and_leaves_the_agent_paused() {
    let world = World::new();
    let main = world.main.id.clone();
    world.compact(&main, Some("WAIT_FOR_GATE")).unwrap();
    world.until_compacting(&main);
    let waiting = world.send("JUST_REPLY later");

    world.set_status(&main, Status::Paused);
    world.several_passes();
    assert_eq!(
        world.compaction().map(|c| c.state),
        Some(CompactionState::Running),
        "a compaction that has started is not interrupted"
    );

    world.open("gate-compact");
    until("the compaction to settle", || world.outcome().is_some());
    world.several_passes();
    assert_eq!(world.outcome().unwrap().result, CompactionResult::Compacted);
    assert_eq!(world.status(&main), Status::Paused);
    assert_eq!(world.receipt(&waiting), Receipt::Queued);
    assert_eq!(world.turns(&main).len(), 1, "no turn follows");
}

#[test]
fn a_quit_stops_a_running_compaction_and_the_next_start_delivers_the_waiting_message() {
    let mut world = World::new();
    let main = world.main.id.clone();
    world.compact(&main, Some("WAIT_FOR_GATE")).unwrap();
    world.until_compacting(&main);
    let waiting = world.send("JUST_REPLY waiting");
    let mut group = None;
    until("the provider's process group", || {
        group = world.compactions(&main)[0].group;
        group.is_some()
    });
    let group = group.unwrap();
    assert_eq!(unsafe { libc::killpg(group, 0) }, 0);

    world.stop();

    // No waiting here: the shutdown itself returned only once the group was stopped.
    assert_eq!(unsafe { libc::killpg(group, 0) }, -1);
    let run = world.compactions(&main).remove(0);
    assert_eq!(run.outcome.as_deref(), Some("interrupted"));
    assert!(run.finished_at.is_some());

    world.start();
    world.until_replies(2);

    // Never run again, and nothing needed resuming: the agent was ready and simply went on.
    let runtime = world.runtime(&main);
    assert_eq!(runtime.compaction, None);
    assert_eq!(
        runtime.last_compaction.map(|o| o.result),
        Some(CompactionResult::Interrupted)
    );
    assert_eq!(runtime.last_error, None);
    assert_eq!(world.lines(), ["Compaction interrupted"]);
    assert_eq!(world.compactions(&main).len(), 1);
    assert_eq!(world.receipt(&waiting), Receipt::Completed);
    assert!(world.snapshot().attention.is_empty());
    let prompts = world.prompts();
    let last = prompts.rsplit("=====").nth(1).unwrap();
    assert!(last.contains("JUST_REPLY waiting"), "{last}");
    assert!(
        !last.contains("Interrupted turn") && !last.contains("restarted"),
        "{last}"
    );
}

#[test]
fn a_compaction_that_cannot_start_fails_with_its_reason_and_the_next_turn_runs() {
    let mut world = World::new();
    let main = world.main.id.clone();
    world.stop();
    {
        // As for an agent whose last turn ran before its directory was recorded.
        let mut host = world.host();
        let mut runtime = host.session_runtime(&main).unwrap();
        runtime.directory = None;
        host.save_runtime(&runtime).unwrap();
        host.request_compaction(&main, Some("the plan")).unwrap();
        host.send("waiting", None, &main, "JUST_REPLY waiting")
            .unwrap();
    }

    world.start();
    world.until_replies(2);

    let outcome = world.outcome().unwrap();
    assert_eq!(outcome.result, CompactionResult::Failed);
    let reason = outcome.reason.unwrap();
    assert!(reason.contains("recorded by its next turn"), "{reason}");
    assert_eq!(world.compaction(), None);
    assert_eq!(world.lines(), ["Compaction failed · context unchanged"]);
    assert!(world.compactions(&main).is_empty(), "no run was started");
    assert_eq!(world.receipt("waiting"), Receipt::Completed);
    assert_eq!(world.turns(&main).len(), 2);
}

#[test]
fn a_crash_while_compacting_stops_its_process_and_the_next_turn_takes_the_waiting_message() {
    let mut world = World::new();
    let main = world.main.id.clone();
    let group = world.crash_while_compacting();

    world.start();
    world.until_replies(2);

    // Startup confirmed the process by its id and start time and stopped it.
    until("the leftover process to be gone", || unsafe {
        libc::killpg(group, 0) == -1
    });
    let run = world.compactions(&main).remove(0);
    assert_eq!(run.outcome.as_deref(), Some("interrupted"));
    assert_eq!(world.compactions(&main).len(), 1, "never run again");
    let runtime = world.runtime(&main);
    assert_eq!(runtime.compaction, None);
    assert_eq!(
        runtime.last_compaction.map(|o| o.result),
        Some(CompactionResult::Interrupted)
    );
    assert_eq!(runtime.last_error, None);
    assert_eq!(world.lines(), ["Compaction interrupted"]);
    assert!(world.snapshot().attention.is_empty());
    // Nothing was held and nothing needed resuming: the next turn took the message.
    assert_eq!(world.receipt("waiting"), Receipt::Completed);
    let prompts = world.prompts();
    let last = prompts.rsplit("=====").nth(1).unwrap();
    assert!(last.contains("JUST_REPLY waiting"), "{last}");
    assert!(!last.contains("Interrupted turn"), "{last}");
}

#[test]
fn a_compactions_process_the_start_cannot_confirm_holds_the_agent_until_its_note_is_dismissed() {
    let mut world = World::new();
    let main = world.main.id.clone();
    let group = world.crash_while_compacting();
    // As when the process id now belongs to a process started at another time.
    world
        .db()
        .execute(
            "UPDATE provider_runs SET process_started=process_started+1 WHERE process_group=?1",
            [group],
        )
        .unwrap();

    world.start();
    world.several_passes();

    // No signal, one note, and neither a turn nor a compaction beside the process.
    assert_eq!(unsafe { libc::killpg(group, 0) }, 0);
    let notes = world.snapshot().attention;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].prompt.contains(&group.to_string()), "{notes:?}");
    assert_eq!(
        world.outcome().map(|o| o.result),
        Some(CompactionResult::Interrupted)
    );
    assert_eq!(world.status(&main), Status::Blocked);
    assert_eq!(world.receipt("waiting"), Receipt::Queued);
    assert_eq!(world.turns(&main).len(), 1);
    assert_eq!(world.compactions(&main).len(), 1);

    world.request(Command::DismissAttention {
        id: notes[0].id.clone(),
    });
    world.until_replies(2);

    assert_eq!(world.receipt("waiting"), Receipt::Completed);
    assert_eq!(world.compactions(&main).len(), 1, "never run again");
    unsafe { libc::killpg(group, libc::SIGKILL) };
}

/// The project's ticket has a Claude reviewer whose model the coordinator chose; its
/// assignment is one quick turn. The coordinator itself is paused for good.
fn ticket_with_a_reviewer(host: &mut Host, project: &Project, main: &Session, scratch: &Path) {
    assign_a_reviewer(host, project, main, scratch);
    host.set_status(&main.id, Status::Paused).unwrap();
}

fn assign_a_reviewer(host: &mut Host, project: &Project, main: &Session, scratch: &Path) {
    let repo = scratch.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let repository = host
        .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
        .unwrap();
    let ticket = host
        .create_ticket(&main.id, &repository.id, "Review", "Check")
        .unwrap();
    let model = host.model_selection().unwrap().allowed(Provider::Claude)[0].clone();
    let profile = json!({"provider":"claude","model":model.model,"effort":model.effort});
    let assignment = json!({"ticket_id":ticket.id,"role":"reviewer","instruction":"JUST_REPLY start","profile":profile,"reason":"Routine review"});
    host.agent_tool(&main.id, "assign_ticket", assignment)
        .unwrap();
}

#[test]
fn a_disabled_provider_holds_an_agent_with_a_request_and_enabling_it_compacts_then_runs_the_turn() {
    let mut world = World::prepared(ticket_with_a_reviewer);
    let main = world.main.id.clone();
    let sessions = world.snapshot().sessions;
    let reviewer = sessions.iter().find(|s| s.role == Role::Reviewer).unwrap();
    let reviewer = reviewer.id.clone();
    let replies = |world: &World| {
        let messages = world.messages(&reviewer);
        let replies = messages.iter().filter(|m| m.id.starts_with("output:"));
        replies.count()
    };
    until("the reviewer's first turn", || replies(&world) == 1);

    // Claude is disabled in Models while the reviewer has a request and a message waiting.
    world.stop();
    let enabled = {
        let mut host = world.host();
        let enabled = host.model_selection().unwrap();
        let mut disabled = enabled.clone();
        disabled
            .providers
            .get_mut(&Provider::Claude)
            .unwrap()
            .enabled = false;
        for providers in disabled.role_providers.values_mut() {
            *providers = [Provider::Codex].into();
        }
        host.set_model_selection(&disabled).unwrap();
        host.request_compaction(&reviewer, None).unwrap();
        host.send("next", Some(&main), &reviewer, "JUST_REPLY next")
            .unwrap();
        enabled
    };
    world.start();
    world.several_passes();

    // Held as without a request: blocked with the reason, its parent told once, nothing run.
    assert_eq!(world.status(&reviewer), Status::Blocked);
    let held = world.runtime(&reviewer);
    assert!(held.last_error.unwrap().starts_with("Claude is disabled"));
    assert_eq!(
        held.compaction.map(|c| c.state),
        Some(CompactionState::Queued)
    );
    assert!(world.compactions(&reviewer).is_empty());
    assert_eq!(world.turns(&reviewer).len(), 1);
    let told = world.messages(&main);
    let told = told.iter().filter(|m| m.id.contains(":turn-held:"));
    assert_eq!(told.count(), 1);

    // Enabled again, the compaction runs first and the turn then takes the message.
    world.stop();
    {
        let mut host = world.host();
        let mut selection = host.model_selection().unwrap();
        selection.providers = enabled.providers;
        selection.role_providers = enabled.role_providers;
        host.set_model_selection(&selection).unwrap();
    }
    world.start();
    until("the reviewer's next turn", || replies(&world) == 2);

    let compaction = world.compactions(&reviewer).remove(0);
    let turns = world.turns(&reviewer);
    assert_eq!(compaction.outcome.as_deref(), Some("compacted"));
    assert!(compaction.finished_at.unwrap() <= turns[1].started_at);
    let next = world.messages(&reviewer);
    let next = next.iter().find(|m| m.id == "next").unwrap();
    assert_eq!(next.receipt, Receipt::Completed);
}

/// The repository's ticket has a verification round of two Claude testers waiting. The second
/// is paused with a queued compaction; everyone else on the ticket is paused for good.
fn round_with_a_paused_verifier(
    host: &mut Host,
    project: &Project,
    main: &Session,
    scratch: &Path,
) -> Vec<VerifierRun> {
    let repo = scratch.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("result.txt"), "verified").unwrap();
    git(&repo, &["add", "result.txt"]);
    git(&repo, &["commit", "-q", "-m", "Initial"]);
    let tester = |focus: &str| VerifierConfig {
        role: Role::Tester,
        focus: focus.into(),
        instruction: None,
        provider: Some(Provider::Claude),
    };
    host.set_verification(VerificationSettings::one_level(
        vec![tester("First"), tester("Second")],
        2,
        2,
    ))
    .unwrap();
    let mut selection = host.model_selection().unwrap();
    selection
        .role_providers
        .insert(Role::Tester, [Provider::Claude, Provider::Codex].into());
    host.set_model_selection(&selection).unwrap();
    let repository = host
        .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
        .unwrap();
    let ticket = host
        .create_ticket(&main.id, &repository.id, "Round", "Check")
        .unwrap();
    let implementer = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Do", None)
        .unwrap();
    let ready = json!({"message_id":"ready","kind":"ready_for_testing","body":"Done"});
    host.agent_tool(&implementer.id, "report", ready).unwrap();
    let model = host.model_selection().unwrap().allowed(Provider::Claude)[0].clone();
    let choices: Vec<_> = ["First", "Second"]
        .map(|focus| VerifierChoice {
            focus: focus.into(),
            profile: ModelProfile {
                provider: Provider::Claude,
                model: model.model.clone(),
                effort: model.effort.clone(),
            },
            reason: "Routine check".into(),
        })
        .into();
    let ticket = host.verify_ticket(&ticket.id, &choices).unwrap();
    let verifiers = ticket.verification.unwrap().rounds[0].verifiers.clone();
    for session in [&main.id, &implementer.id, &verifiers[1].session_id] {
        host.set_status(session, Status::Paused).unwrap();
    }
    let second = &verifiers[1].session_id;
    let mut runtime = host.session_runtime(second).unwrap();
    runtime.provider_session_id = Some("verifier-session".into());
    runtime.directory = Some(scratch.to_string_lossy().into_owned());
    host.save_runtime(&runtime).unwrap();
    host.request_compaction(second, Some("WAIT_FOR_GATE"))
        .unwrap();
    verifiers
}

#[test]
fn a_verifiers_compaction_runs_before_its_round_and_the_round_is_admitted_after_it() {
    let mut verifiers = vec![];
    let world = World::prepared(|host, project, main, scratch| {
        verifiers = round_with_a_paused_verifier(host, project, main, scratch);
    });
    let (first, second) = (&verifiers[0].session_id, &verifiers[1].session_id);
    let round_waits = |world: &World| {
        for verifier in &verifiers {
            assert!(world.turns(&verifier.session_id).is_empty());
            let round = world.messages(&verifier.session_id);
            let message = round.iter().find(|m| m.id == verifier.message_id).unwrap();
            assert_eq!(message.receipt, Receipt::Queued);
        }
    };
    world.several_passes();
    round_waits(&world);
    assert!(world.compactions(second).is_empty(), "paused");

    // Resumed: the compaction runs, and while it does no verifier of the round starts a turn.
    world.resume(second);
    world.until_compacting(second);
    world.several_passes();
    round_waits(&world);

    world.open("gate-compact");
    until("the round's turns", || {
        let finished = |session: &str| {
            let turns = world.turns(session);
            turns.first().is_some_and(|t| t.finished_at.is_some())
        };
        finished(first) && finished(second)
    });

    // The round was admitted whole after the compaction, and its turn took the round message.
    let compaction = world.compactions(second).remove(0);
    let turn = world.turns(second).remove(0);
    assert!(compaction.finished_at.unwrap() <= turn.started_at);
    let round = world.messages(second);
    let message = round.iter().find(|m| m.id == verifiers[1].message_id);
    assert_eq!(message.unwrap().receipt, Receipt::Completed);
    assert_eq!(world.turns(first).len(), 1);
}

#[test]
fn a_compactions_stream_events_are_not_counted_as_requests() {
    let world = World::new();
    world.compacted(Some("STREAM_EVENTS"));

    let (requests, _, compactions) = world.ledger();
    assert_eq!(compactions, 1);
    assert_eq!(requests - compactions, 1, "only the turn's own request");
}

/// A world whose coordinator is paused and whose reviewer has finished its first turn.
fn world_with_a_worker() -> World {
    World::prepared(|host, project, main, scratch| {
        ticket_with_a_reviewer(host, project, main, scratch);
    })
}

/// Holds a message for the worker, which is compacting, and does `stop`. The compaction is not
/// interrupted and ends compacted; the stop then still holds the message back.
fn stop_during_a_compaction(world: &World, stop: impl FnOnce(&World, &str)) -> (String, String) {
    let worker = world.compacting_worker();
    let waiting = world.send_to(&worker, "JUST_REPLY later");

    stop(world, &worker);
    world.several_passes();
    assert_eq!(
        world.runtime(&worker).compaction.map(|c| c.state),
        Some(CompactionState::Running),
        "a compaction that has started is not interrupted"
    );

    world.open("gate-compact");
    until("the compaction to settle", || {
        world.runtime(&worker).last_compaction.is_some()
    });
    world.several_passes();
    let outcome = world.runtime(&worker).last_compaction.unwrap();
    assert_eq!(outcome.result, CompactionResult::Compacted);
    assert_eq!(world.turns(&worker).len(), 1, "no turn follows");
    assert_eq!(world.receipt_in(&worker, &waiting), Receipt::Queued);
    (worker, waiting)
}

#[test]
fn a_parents_stop_while_a_child_compacts_lets_it_finish_and_leaves_the_child_stopped() {
    let world = world_with_a_worker();
    let (worker, _) = stop_during_a_compaction(&world, |world, worker| {
        world.set_status(&world.main.id, Status::Ready);
        world.send("STOP_CHILDREN");
        until("the parent's stop", || {
            world.status(worker) == Status::Paused
        });
    });
    assert_eq!(world.status(&worker), Status::Paused);
}

#[test]
fn stopping_the_project_while_a_worker_compacts_lets_it_finish_and_holds_its_messages() {
    let world = world_with_a_worker();
    let project = world.main.project_id.clone();
    let (worker, waiting) = stop_during_a_compaction(&world, |world, _| {
        world.request(Command::SetLive {
            project_id: world.main.project_id.clone(),
            enabled: false,
        });
    });

    world.request(Command::SetLive {
        project_id: project,
        enabled: true,
    });
    until("the live project's turn", || {
        world.receipt_in(&worker, &waiting) == Receipt::Completed
    });
}

#[test]
fn archiving_a_worker_while_it_compacts_lets_it_finish_and_runs_no_turn_after() {
    let world = world_with_a_worker();
    let (worker, _) = stop_during_a_compaction(&world, |world, worker| {
        world.request(Command::SetArchived {
            session_id: worker.into(),
            archived: true,
        });
    });
    let sessions = world.snapshot().sessions;
    assert!(sessions.iter().find(|s| s.id == worker).unwrap().archived);
}

#[test]
fn a_workers_compaction_raises_its_own_and_its_coordinators_totals() {
    let world = world_with_a_worker();
    let worker = world.worker();
    let coordinator = |usage: &UsageReport| {
        let found = usage.coordinators.iter().find(|c| c.id == world.main.id);
        found.unwrap().clone()
    };
    let before = world.usage();

    world.compact(&worker, None).unwrap();
    until("the worker's compaction", || {
        world.runtime(&worker).last_compaction.is_some()
    });
    let after = world.usage();

    let session: String = world
        .db()
        .query_row(
            "SELECT r.session_id FROM usage_requests u JOIN provider_runs r ON r.id=u.run_id WHERE u.request_id LIKE 'compact:%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(session, worker, "the worker's own request");
    let (input, output) = (813_402, 2_960);
    let raised = |before: &TokenTotals, after: &TokenTotals| {
        (
            after.requests - before.requests,
            after.input - before.input,
            after.output - before.output,
        )
    };
    assert_eq!(raised(&before.totals, &after.totals), (1, input, output));
    let (before, after) = (coordinator(&before), coordinator(&after));
    assert_eq!(raised(&before.workers, &after.workers), (1, input, output));
    assert_eq!(
        raised(&before.own, &after.own),
        (0, 0, 0),
        "not the coordinator's own"
    );
}
