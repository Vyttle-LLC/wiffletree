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
        let id = new_id();
        self.request(Command::Send {
            id: id.clone(),
            sender: None,
            recipient: self.main.id.clone(),
            body: body.into(),
            attachments: vec![],
        });
        id
    }

    fn receipt(&self, message: &str) -> Receipt {
        let messages = self.messages(&self.main.id);
        messages.iter().find(|m| m.id == message).unwrap().receipt
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
    let usage: UsageReport = serde_json::from_value(world.request(Command::Usage {
        days: 7,
        project_id: None,
        provider: None,
        timezone: "UTC".into(),
    }))
    .unwrap();
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
    host.set_verification(VerificationSettings {
        verifiers: vec![tester("First"), tester("Second")],
        max_rounds: 2,
        max_cycles: 2,
    })
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
