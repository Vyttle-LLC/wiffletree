//! Event-driven host actor: all SQLite access stays on one thread; provider I/O never blocks it.
use crate::provider::{ProviderEvent, Turn};
use crate::stream::StepUpdate;
use crate::*;
use async_channel::{Receiver, Sender};
use std::{
    collections::{HashMap, HashSet},
    io::{BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

type Reply = Sender<std::result::Result<Value, String>>;
/// How long a turn runs before it checks in with its parent, and how often after that.
const CHECK_IN_MS: i64 = 30 * 60_000;
/// How long an idle parent waits after a child's first progress report, so a burst of
/// them arrives in one turn.
const PROGRESS_BATCH_MS: i64 = 20_000;
/// Bounds one turn's prompt; any further queued messages go to the next turn.
const MAX_TURN_MESSAGES: usize = 20;

/// Longest a wake-up sleeps before the actor looks again; macOS suspends sleeping threads
/// while the Mac sleeps, so a long sleep could miss a timer by hours.
const MAX_WAKE_SLEEP_MS: i64 = 60_000;
/// SQL condition for turn input, on columns of the messages `table` alias. A session's other
/// messages to itself are its own output; timer fires are the exception.
fn is_turn_input(table: &str) -> String {
    let t = table;
    format!("({t}.sender IS NULL OR {t}.sender<>{t}.recipient OR substr({t}.id,1,6)='timer:')")
}

/// SQL condition for a message that is a child's progress report: live.rs "report" writes
/// the id `report:{sender}:…`, so a human message cannot pass for one.
const IS_PROGRESS_REPORT: &str = "sender IS NOT NULL AND substr(id,1,length(sender)+8)='report:'||sender||':' AND substr(body,1,11)='[progress] '";

/// `input` pairs each message with a status line rendered under its id, such as a timer
/// fire's lateness; most messages have none.
fn turn_prompt(
    session: &Session,
    preamble: &str,
    input: &[(&Message, Option<String>)],
    reminder: &str,
) -> String {
    let messages = input
        .iter()
        .map(|(m, status)| {
            let status = status.as_ref().map_or(String::new(), |s| format!("\n{s}"));
            format!(
                "Sender: {}\nMessage ID: {}{status}\n\n{}{}",
                if m.id.starts_with(schedules::FIRE_PREFIX) {
                    "your timer"
                } else {
                    m.sender.as_deref().unwrap_or("human")
                },
                m.id,
                m.body,
                provider::attachment_list(&m.attachments)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    format!(
        "Managed session {} ({}){preamble}\n{messages}\n\nUse workspace_context to obtain current team IDs and tickets. If you have a parent, report through workspace tools before ending the turn. The coordinator responds to the human directly. Never wait or poll for a reply.{reminder}",
        session.name,
        session.role.label(),
    )
}

/// Lists the coordinator's unsettled inbox questions so each turn can close the ones it resolved.
fn open_questions_reminder(questions: &[Attention]) -> String {
    if questions.is_empty() {
        return String::new();
    }
    let list = questions
        .iter()
        .map(|q| format!("- {}: {}", q.operation_id, q.prompt))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n\nYour open inbox questions (request_id: question):\n{list}\nIf this message or the current state already settles one, call close_question for it before ending your turn."
    )
}
/// A clean exit stops turns this way; only an unexpected stop leaves runs unfinished for the next start.
const QUIT_DURING_TURN: &str =
    "Wiffletree quit during a turn (app quit or restart to update); inspect work before retrying";
const STOPPED_UNEXPECTEDLY: &str =
    "Host stopped unexpectedly during a turn; inspect the worktree before retrying held input";
/// Marks turns the last host left unfinished as interrupted. Projects keep their live state, so
/// queued work and timer fires run on their own after a restart; only the interrupted sessions
/// wait, Disconnected with their input held, until the human retries or skips it.
fn recover_unfinished_turns(host: &mut Host) -> Result<()> {
    let interrupted = host
        .db
        .prepare("SELECT session_id FROM provider_runs WHERE finished_at IS NULL")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in interrupted {
        let mut runtime = host.session_runtime(&id)?;
        runtime.last_error = Some(STOPPED_UNEXPECTEDLY.into());
        host.save_runtime(&runtime)?;
        host.set_status(&id, Status::Disconnected)?;
    }
    // A restart never silently repeats a turn that may have had side effects: those
    // sessions stay held for reconciliation, while the rest of a live project carries on.
    host.db.execute(
        "UPDATE provider_runs SET finished_at=?1,outcome='interrupted',detail=json_set(detail,'$.result',json_object('error',?2)) WHERE finished_at IS NULL",
        params![now(), STOPPED_UNEXPECTEDLY],
    )?;
    // No turn runs any more, so no check-in still says one does.
    let check_ins = host
        .db
        .prepare("SELECT id FROM attention WHERE substr(operation_id,1,length(?1))=?1 AND json_extract(data,'$.answer') IS NULL")?
        .query_map([CHECK_IN], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in check_ins {
        host.resolve_attention(&id, "Superseded by a host restart")?;
    }
    Ok(())
}
/// Names the turn that took this message and stopped before finishing, so a retry can find its partial work.
fn interrupted_turn_note(host: &Host, message_id: &str) -> Result<String> {
    let run = host
        .db
        .query_row(
            "SELECT id,started_at FROM provider_runs WHERE outcome IN ('failed','interrupted') AND EXISTS (SELECT 1 FROM json_each(messages) WHERE value=?1) ORDER BY started_at DESC LIMIT 1",
            [message_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    Ok(run.map_or_else(String::new, |(id, started_at)| {
        let started = schedules::rfc3339(started_at);
        format!(
            "\nRetry of interrupted turn {id} (started {started}). Check the worktree for its partial work before repeating it."
        )
    }))
}
/// What a session's first turn after a host start needs to pick up again: the turn the stop
/// cut off, its timers and the fires it missed. Empty when there is nothing to say.
fn resume_note(
    host: &Host,
    session: &str,
    started_at: i64,
    delivered_at: i64,
    notes: &[String],
) -> Result<String> {
    let mut lines = Vec::new();
    let interrupted = host.db.query_row(
        "SELECT id,started_at FROM provider_runs WHERE session_id=?1 AND started_at<?2 AND (outcome='interrupted' OR json_extract(detail,'$.result.error') IN (?3,?4)) AND started_at=(SELECT MAX(started_at) FROM provider_runs WHERE session_id=?1 AND started_at<?2)",
        params![session, started_at, QUIT_DURING_TURN, STOPPED_UNEXPECTEDLY],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
    ).optional()?;
    if let Some((run, started)) = interrupted
        && !notes.iter().any(|note| note.contains(&run))
    {
        lines.push(format!(
            "- Interrupted turn {run} (started {}); check for its partial work.",
            schedules::rfc3339(started)
        ));
    }
    for timer in host.active_schedules(Some(session))? {
        lines.push(format!(
            "- Timer {} \"{}\", {}, next fire {}.",
            timer.id,
            timer.label,
            schedules::cadence(&timer),
            timer
                .next_fire_at
                .map(schedules::rfc3339)
                .unwrap_or_default()
        ));
    }
    for (timer, label, due, missed) in host.late_fires(session, started_at, delivered_at)? {
        let more = if missed > 0 {
            format!(" and {missed} later slot(s)")
        } else {
            String::new()
        };
        lines.push(format!(
            "- Missed: timer {timer} \"{label}\" due {}{more}; it fires once, marked late.",
            schedules::rfc3339(due)
        ));
    }
    Ok(if lines.is_empty() {
        String::new()
    } else {
        format!(
            "\nThe workspace host restarted since your last turn.\n{}",
            lines.join("\n")
        )
    })
}
/// Tells a session that its parent stopped its last turn, and why, so it checks what that turn
/// finished. The stopped input is not retried: the parent chose to stop it and sends what next.
fn stopped_by_parent_note(host: &Host, session: &str) -> Result<String> {
    let run = host.db.query_row(
        "SELECT id,started_at,json_extract(detail,'$.stop_reason') FROM provider_runs WHERE session_id=?1 AND outcome='stopped_by_parent' AND started_at=(SELECT MAX(started_at) FROM provider_runs WHERE session_id=?1)",
        [session],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)),
    ).optional()?;
    Ok(run.map_or_else(String::new, |(id, started_at, reason)| {
        format!(
            "\nYour parent stopped your previous turn {id} (started {}). Reason: {reason}\nIts input was not retried. Check the worktree for its partial work before you continue.",
            schedules::rfc3339(started_at)
        )
    }))
}
fn stopped_by_parent_notice(reason: &str) -> String {
    format!(
        "Stopped by its parent: {reason}\nIts input will not be retried; the next message runs normally. Partial work may remain."
    )
}
/// Refuses arguments a tool does not take, so a misspelt one is not silently ignored.
fn only_arguments(tool: &str, args: &Value, allowed: &[&str]) -> Result<()> {
    if let Some(unknown) = args
        .as_object()
        .into_iter()
        .flat_map(|args| args.keys())
        .find(|key| !allowed.contains(&key.as_str()))
    {
        bail!("{tool} takes {}, not {unknown}", allowed.join(", "));
    }
    Ok(())
}
/// Provider output is not Markdown, so it is fenced to show verbatim.
fn runtime_stopped(error: &str) -> String {
    let longest = error
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest.max(2) + 1);
    format!(
        "Runtime stopped:\n{fence}text\n{error}\n{fence}\nInspect the worktree before retrying uncertain work."
    )
}
/// Runs a repository's check on its own thread and always reports back, even if it panics, so
/// the repository is never left marked as running.
fn spawn_overlap_check(
    sender: Sender<Event>,
    repository: String,
    check: impl FnOnce() -> (Option<i64>, Vec<BranchWarnings>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(check)).ok();
        let _ = sender.send_blocking(Event::Overlaps {
            repository,
            fetched_at: outcome.as_ref().and_then(|(at, _)| *at),
            warnings: outcome.map(|(_, warnings)| warnings),
        });
    });
}
enum Event {
    Command(Command, Reply),
    /// A root scan finished on its own thread; adding what it found is quick.
    Discovered {
        found: Vec<RepositoryCandidate>,
        warnings: Vec<String>,
        reply: Reply,
    },
    Tool {
        token: String,
        name: String,
        args: Value,
        reply: Reply,
    },
    Provider {
        session: String,
        run: String,
        event: ProviderEvent,
    },
    /// A repository's open-branch check finished on its own thread.
    Overlaps {
        repository: String,
        /// When the check fetched the base, if it did.
        fetched_at: Option<i64>,
        /// `None` when the check panicked; the cached warnings then stand.
        warnings: Option<Vec<BranchWarnings>>,
    },
    /// A wake-up armed for this time arrived; scheduling runs after every event.
    Wake(i64),
    Shutdown,
}
struct Handle {
    sender: Sender<Event>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.sender.try_send(Event::Shutdown);
    }
}
#[derive(Clone)]
pub struct Service {
    handle: Arc<Handle>,
    changes: Receiver<()>,
    step_changes: Receiver<()>,
}
struct Active {
    run: String,
    token: String,
    cancel: Arc<AtomicBool>,
    messages: Vec<String>,
    output: String,
    reported: bool,
    started_at: i64,
    /// When this turn next checks in; see `check_in_long_turns`.
    next_check_in: i64,
    /// When the provider last sent an event, for check-ins.
    last_event_at: i64,
    /// Why the session's parent stopped this turn, once it has.
    stopped_by_parent: Option<String>,
    /// The human stopped this turn; its parent has already been told.
    stopped_by_human: bool,
    /// This turn's steps; the store has each one as of its last start or state change.
    steps: Vec<Step>,
    omitted_steps: usize,
}
impl Active {
    fn new(
        run: String,
        token: String,
        cancel: Arc<AtomicBool>,
        messages: Vec<String>,
        started_at: i64,
    ) -> Self {
        Self {
            run,
            token,
            cancel,
            messages,
            output: String::new(),
            reported: false,
            started_at,
            next_check_in: started_at + CHECK_IN_MS,
            last_event_at: started_at,
            stopped_by_parent: None,
            stopped_by_human: false,
            steps: vec![],
            omitted_steps: 0,
        }
    }
}
struct Permission {
    session: String,
    input: Value,
    reply: Reply,
}
struct Actor {
    host: Host,
    sender: Sender<Event>,
    changed: Sender<()>,
    /// Work steps change many times a second, so they have their own signal; a pending
    /// step signal must never hold back a state change.
    steps_changed: Sender<()>,
    steps_dirty: bool,
    /// Each session's latest step revision, seeded from the store on first use.
    revisions: HashMap<String, u64>,
    active: HashMap<String, Active>,
    permissions: HashMap<String, Permission>,
    socket: PathBuf,
    helper: PathBuf,
    /// Each project coordinator's turns since the human last stepped in, by project. Kept in
    /// memory, so a host restart starts the count again.
    human_turns: HashMap<String, usize>,
    /// The earliest armed wake-up, for progress batches and timers alike.
    wake_at: Option<i64>,
    /// When this host started, and the sessions that have had a turn since.
    started_at: i64,
    resumed: HashSet<String>,
    /// Set at shutdown, whose synthetic finishes must not remove worktrees under live processes.
    stopping: bool,
    /// When each repository's latest open-branch check started; see `refresh_overlaps`.
    overlaps_checked: HashMap<String, i64>,
    /// Repositories whose check has not reported yet.
    overlaps_running: HashSet<String>,
    /// No repository's check can be due before this.
    overlaps_due_at: i64,
}
impl Service {
    pub fn start(home: PathBuf, helper: PathBuf) -> Result<Self> {
        ensure!(
            helper.is_file(),
            "workspace-host helper is missing; rebuild the app bundle"
        );
        let (sender, receiver) = async_channel::bounded(256);
        let (changed, changes) = async_channel::bounded(1);
        let (steps_changed, step_changes) = async_channel::bounded(1);
        let (ready, readiness) = std::sync::mpsc::sync_channel(1);
        let worker = sender.clone();
        std::thread::Builder::new()
            .name("workspace-host".into())
            .spawn(move || {
                let setup = (|| -> Result<_> {
                    let mut host = Host::open(home)?;
                    recover_unfinished_turns(&mut host)?;
                    host.settle_pending_worktrees();
                    let directory = tempfile::Builder::new().prefix("aw-").tempdir_in("/tmp")?;
                    let socket = directory.path().join("ipc");
                    let listener = UnixListener::bind(&socket)?;
                    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
                    Ok((host, directory, socket, listener))
                })();
                let (host, directory, socket, listener) = match setup {
                    Ok(v) => {
                        let _ = ready.send(Ok(()));
                        v
                    }
                    Err(e) => {
                        let _ = ready.send(Err(format!("{e:#}")));
                        return;
                    }
                };
                let connections = Arc::new(AtomicUsize::new(0));
                let socket_sender = worker.clone();
                let stop_listener = Arc::new(AtomicBool::new(false));
                let stopping = stop_listener.clone();
                std::thread::spawn(move || {
                    for connection in listener.incoming() {
                        if stopping.load(Ordering::Relaxed) {
                            break;
                        }
                        let Ok(mut stream) = connection else { break };
                        if connections.fetch_add(1, Ordering::Relaxed) >= 32 {
                            connections.fetch_sub(1, Ordering::Relaxed);
                            continue;
                        }
                        let count = connections.clone();
                        let sender = socket_sender.clone();
                        std::thread::spawn(move || {
                            let response = (|| -> Result<Value> {
                                stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                                let request =
                                    mcp::read_json(&mut BufReader::new(stream.try_clone()?))?
                                        .context("Empty request")?;
                                let (reply, receive) = async_channel::bounded(1);
                                sender.send_blocking(Event::Tool {
                                    token: request["token"]
                                        .as_str()
                                        .context("Missing credential")?
                                        .into(),
                                    name: request["name"].as_str().context("Missing tool")?.into(),
                                    args: request["arguments"].clone(),
                                    reply,
                                })?;
                                receive.recv_blocking()?.map_err(anyhow::Error::msg)
                            })();
                            let value = match response {
                                Ok(value) => json!({"result":value}),
                                Err(error) => json!({"error":format!("{error:#}")}),
                            };
                            let _ =
                                stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
                            let _ = writeln!(stream, "{value}");
                            count.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                });
                let mut actor = Actor {
                    host,
                    sender: worker,
                    changed,
                    steps_changed,
                    steps_dirty: false,
                    revisions: HashMap::new(),
                    active: HashMap::new(),
                    permissions: HashMap::new(),
                    socket: socket.clone(),
                    helper,
                    human_turns: HashMap::new(),
                    wake_at: None,
                    started_at: now(),
                    resumed: HashSet::new(),
                    stopping: false,
                    overlaps_checked: HashMap::new(),
                    overlaps_running: HashSet::new(),
                    overlaps_due_at: 0,
                };
                // Timers that came due while the host was stopped fire now.
                if let Err(e) = actor.schedule() {
                    eprintln!("Workspace scheduler: {e:#}");
                }
                actor.refresh_overlaps(now());
                while let Ok(event) = receiver.recv_blocking() {
                    if matches!(event, Event::Shutdown) {
                        break;
                    }
                    actor.process(event);
                }
                actor.stop();
                for (_, permission) in actor.permissions.drain() {
                    let _ = permission.reply.try_send(Err("Workspace stopped".into()));
                }
                stop_listener.store(true, Ordering::Relaxed);
                let _ = std::os::unix::net::UnixStream::connect(&socket);
                drop(directory);
            })?;
        readiness.recv()?.map_err(anyhow::Error::msg)?;
        Ok(Self {
            handle: Arc::new(Handle { sender }),
            changes,
            step_changes,
        })
    }
    pub fn request(
        &self,
        command: Command,
    ) -> std::result::Result<Receiver<std::result::Result<Value, String>>, String> {
        let (reply, receive) = async_channel::bounded(1);
        self.handle
            .sender
            .try_send(Event::Command(command, reply))
            .map_err(|_| "Host queue is full or disconnected".to_string())?;
        Ok(receive)
    }
    pub fn changes(&self) -> Receiver<()> {
        self.changes.clone()
    }
    /// Signals that some session's work steps changed; fetch them with `Command::Steps`.
    pub fn step_changes(&self) -> Receiver<()> {
        self.step_changes.clone()
    }
}
impl Actor {
    /// Repository scans run Git once per new repository, so they leave the host thread free
    /// for agents and send back only what the store needs.
    fn scan_in_background(&mut self, command: &Command, reply: &Reply) -> Result<bool> {
        match command {
            Command::DiscoverRepositories { folders } => {
                let known = self.host.known_paths()?;
                let (folders, reply) = (folders.clone(), reply.clone());
                std::thread::spawn(move || {
                    let result = repositories::discover(&folders, &known)
                        .and_then(|d| Ok(serde_json::to_value(d)?))
                        .map_err(|e| format!("{e:#}"));
                    let _ = reply.try_send(result);
                });
            }
            Command::RefreshRepositories => {
                let (roots, skip) = self.host.refresh_inputs()?;
                let (sender, reply) = (self.sender.clone(), reply.clone());
                std::thread::spawn(move || {
                    let (found, warnings) = repositories::scan_roots(&roots, &skip);
                    let _ = sender.send_blocking(Event::Discovered {
                        found,
                        warnings,
                        reply,
                    });
                });
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Handles one event, then starts what it made due.
    fn process(&mut self, event: Event) {
        let dirty = self.handle(event);
        if let Err(e) = self.schedule() {
            eprintln!("Workspace scheduler: {e:#}");
        }
        self.refresh_overlaps(now());
        self.signal(dirty);
    }

    /// Checks each repository with open tickets at most every five minutes, after any event,
    /// so reading `workspace_context` refreshes a stale cache. Fetching and comparing branches
    /// can take seconds, so Git runs on its own thread, one check per repository at a time, and
    /// the cached warnings stand until it reports.
    fn refresh_overlaps(&mut self, at: i64) {
        // Most events come well inside the interval; they must not cost a store read.
        if at < self.overlaps_due_at {
            return;
        }
        let checks = match self.host.overlap_checks() {
            Ok(checks) => checks,
            Err(e) => return eprintln!("Workspace overlap check: {e:#}"),
        };
        self.overlaps_due_at = at + overlaps::OVERLAP_REFRESH_MS;
        for (repository, check) in checks {
            if self.overlaps_running.contains(&repository) {
                continue;
            }
            if let Some(&last) = self.overlaps_checked.get(&repository)
                && at - last < overlaps::OVERLAP_REFRESH_MS
            {
                self.overlaps_due_at = self
                    .overlaps_due_at
                    .min(last + overlaps::OVERLAP_REFRESH_MS);
                continue;
            }
            self.overlaps_checked.insert(repository.clone(), at);
            self.overlaps_running.insert(repository.clone());
            let sender = self.sender.clone();
            spawn_overlap_check(sender, repository, move || {
                let fetched = overlaps::fetch_base(&check.repository, &check.base);
                let warnings = overlaps::check(&check, fetched.as_ref().err());
                (matches!(fetched, Ok(true)).then_some(at), warnings)
            });
        }
    }

    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Discovered {
                found,
                warnings,
                reply,
            } => {
                let result = self.host.add_discovered(found, warnings);
                let added = result.as_ref().is_ok_and(|r| !r.added.is_empty());
                let _ = reply.try_send(
                    result
                        .and_then(|r| Ok(serde_json::to_value(r)?))
                        .map_err(|e| format!("{e:#}")),
                );
                added
            }
            Event::Command(command, reply) => {
                match self.scan_in_background(&command, &reply) {
                    Ok(true) => return false,
                    Ok(false) => {}
                    Err(error) => {
                        let _ = reply.try_send(Err(format!("{error:#}")));
                        return false;
                    }
                }
                let read = matches!(
                    command,
                    Command::Snapshot
                        | Command::Messages { .. }
                        | Command::Steps { .. }
                        | Command::RunSummaries { .. }
                        | Command::Logs { .. }
                        | Command::Activity { .. }
                        | Command::GitHistory { .. }
                        | Command::ProviderCheck
                        | Command::Quotas
                        | Command::Usage { .. }
                        | Command::Settings
                );
                let result = (|| -> Result<Value> {
                    // Sessions this command stops on the human's behalf.
                    let mut human_stops = Vec::new();
                    match &command {
                        Command::SetLive {
                            project_id,
                            enabled: false,
                        } => {
                            for (id, active) in &self.active {
                                if self.host.session(id)?.project_id == *project_id {
                                    active.cancel.store(true, Ordering::Relaxed);
                                    human_stops.push(id.clone());
                                }
                            }
                        }
                        Command::SetStatus {
                            session_id,
                            status: Status::Paused,
                        } => {
                            if let Some(active) = self.active.get(session_id) {
                                active.cancel.store(true, Ordering::Relaxed);
                            }
                            human_stops.push(session_id.clone());
                        }
                        Command::ConfigureSession { session_id, .. }
                        | Command::ReconcileSession { session_id, .. } => ensure!(
                            !self.active.contains_key(session_id),
                            "Wait for the active turn to stop"
                        ),
                        _ => {}
                    }
                    let response = match &command {
                        Command::Steps {
                            session_id,
                            run_id,
                            after,
                        } => serde_json::to_value(self.steps(
                            session_id,
                            run_id.as_deref(),
                            *after,
                        )?)?,
                        _ => self.host.execute(command.clone())?,
                    };
                    // The stop itself has happened, and human_stopped records who made it before
                    // the notice; a notice that cannot be sent is only logged.
                    for id in human_stops {
                        if let Err(error) = self.human_stopped(&id) {
                            eprintln!("Recording the human's stop of {id}: {error:#}");
                        }
                    }
                    // Stop turns only once the archive succeeded; a refusal leaves them running.
                    if let Command::SetArchived {
                        session_id,
                        archived: true,
                    } = &command
                    {
                        for session in self.host.session_tree(session_id)? {
                            if let Some(active) = self.active.get(&session.id) {
                                active.cancel.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    // Speaking to a project, retrying its work or answering its coordinator
                    // starts it; there is no separate step to go live.
                    let started = match &command {
                        Command::Send {
                            sender: None,
                            recipient: session_id,
                            ..
                        }
                        | Command::ReconcileSession {
                            session_id,
                            retry: true,
                        } => Some(self.host.session(session_id)?.project_id),
                        Command::ResolveAttention { .. } => {
                            response["project_id"].as_str().map(str::to_owned)
                        }
                        _ => None,
                    };
                    if let Some(project) = started {
                        self.human_turns.insert(project.clone(), 0);
                        // Stepping in answers the coordinator's turn-count check-in.
                        for item in self.host.open_attention(&project)? {
                            if item.operation_id.starts_with(TURN_COUNT) {
                                self.host
                                    .resolve_attention(&item.id, "The human stepped in")?;
                            }
                        }
                        if !self.host.live_projects()?.contains(&project) {
                            self.host.set_live(&project, true)?;
                        }
                    }
                    if let Command::ResolveAttention { id, answer } = &command {
                        if let Some(permission) = self.permissions.remove(id) {
                            let decision = if answer == "approved" {
                                json!({"behavior":"allow","updatedInput":permission.input})
                            } else {
                                json!({"behavior":"deny","message":answer})
                            };
                            let _ = permission.reply.try_send(Ok(decision));
                        } else {
                            let attention: Attention = serde_json::from_value(response.clone())?;
                            self.host.send(
                                &format!("answer:{id}"),
                                None,
                                &attention.session_id,
                                &format!("Human answer to {}: {answer}", attention.prompt),
                            )?;
                        }
                    }
                    Ok(response)
                })()
                .map_err(|e| format!("{e:#}"));
                let _ = reply.try_send(result);
                !read
            }
            Event::Tool {
                token,
                name,
                args,
                reply,
            } => {
                let identity = self
                    .active
                    .iter()
                    .find(|(_, run)| run.token == token)
                    .map(|(id, _)| id.clone());
                let result = (|| -> Result<Option<Value>> {
                    let id = identity.context("Expired or invalid session credential")?;
                    match name.as_str() {
                        "stop_agents" => return self.stop_agents(&id, &args).map(Some),
                        "resume_agent" => return self.resume_agent(&id, &args).map(Some),
                        _ => {}
                    }
                    if name == "request_permission" {
                        let tool = args["tool_name"].as_str().context("Missing tool name")?;
                        let input = args["input"].clone();
                        let request = self.host.request_attention(
                            &id,
                            "local",
                            &format!("permission:{}", new_id()),
                            &format!(
                                "{} requests {tool}\n{}",
                                self.host.session(&id)?.name,
                                input.to_string().chars().take(3000).collect::<String>()
                            ),
                            &[],
                        )?;
                        self.permissions.insert(
                            request.id,
                            Permission {
                                session: id,
                                input,
                                reply: reply.clone(),
                            },
                        );
                        return Ok(None);
                    }
                    let terminal_report = name == "report" && args["kind"] != "progress";
                    let result = self.host.agent_tool(&id, &name, args)?;
                    if terminal_report && let Some(active) = self.active.get_mut(&id) {
                        active.reported = true;
                    }
                    Ok(Some(result))
                })();
                match result {
                    Ok(Some(value)) => {
                        let _ = reply.try_send(Ok(value));
                    }
                    Err(e) => {
                        let _ = reply.try_send(Err(format!("{e:#}")));
                    }
                    _ => {}
                }
                name != "workspace_context"
            }
            Event::Provider {
                session,
                run,
                event,
            } => {
                let Some(active) = self.active.get_mut(&session).filter(|a| a.run == run) else {
                    return false;
                };
                active.last_event_at = now();
                let changes_state =
                    !matches!(event, ProviderEvent::Step(_) | ProviderEvent::Reply(_));
                if let Err(e) = self.provider_event(&session, &run, event) {
                    eprintln!("Workspace provider event: {e:#}");
                }
                changes_state
            }
            Event::Overlaps {
                repository,
                fetched_at,
                warnings,
            } => {
                self.overlaps_running.remove(&repository);
                if let Some(&started) = self.overlaps_checked.get(&repository) {
                    self.overlaps_due_at = self
                        .overlaps_due_at
                        .min(started + overlaps::OVERLAP_REFRESH_MS);
                }
                warnings.is_some_and(|warnings| {
                    self.host
                        .set_branch_warnings(repository, fetched_at, warnings)
                })
            }
            Event::Wake(at) => {
                if self.wake_at == Some(at) {
                    self.wake_at = None;
                }
                false
            }
            Event::Shutdown => false,
        }
    }
    fn provider_event(&mut self, id: &str, run: &str, event: ProviderEvent) -> Result<()> {
        let session = self.host.session(id)?;
        let mut runtime = self.host.session_runtime(id)?;
        match event {
            ProviderEvent::Spawned(process_group) => {
                self.host.db.execute(
                    "UPDATE provider_runs SET process_group=?2 WHERE id=?1",
                    params![run, process_group],
                )?;
            }
            ProviderEvent::Session(provider_id) => {
                self.host.db.execute("UPDATE provider_runs SET detail=json_set(detail,'$.provider_thread_id',?2) WHERE id=?1",params![run,provider_id])?;
                runtime.provider_session_id = Some(provider_id);
                self.host.save_runtime(&runtime)?;
            }
            ProviderEvent::Step(update) => self.record_step(id, run, update)?,
            ProviderEvent::Reply(text) => {
                let active = self.active.get_mut(id).unwrap();
                active.output = text.chars().take(MAX_TEXT_BYTES / 4).collect();
            }
            ProviderEvent::Usage {
                request_id,
                model,
                usage,
                complete,
            } => {
                self.host.record_usage(
                    run,
                    &request_id,
                    model.as_deref(),
                    &usage,
                    complete,
                    now(),
                )?;
            }
            ProviderEvent::Quota(reading) => self.host.record_quota(reading)?,
            ProviderEvent::Finished {
                error,
                usage,
                cancelled,
            } => {
                let mut active = self.active.remove(id).unwrap();
                let failed = error.is_some();
                // A turn its parent stopped is finished rather than held: the parent decides
                // what runs next, and the session's next turn is told about the stop instead.
                // Only our own cancellation counts; a real failure keeps its error and held input.
                let stop_reason = active.stopped_by_parent.take().filter(|_| cancelled);
                let error = error.filter(|_| stop_reason.is_none());
                // Settle the run first so a later failure cannot leave it open. If this update
                // fails, the run stays open and blocks worktree removal until restart: safe.
                self.host.db.execute(
                    "UPDATE provider_runs SET finished_at=?2,outcome=?3,detail=json_set(detail,'$.result',json(?4),'$.omitted_steps',?5,'$.stop_reason',?6) WHERE id=?1",
                    params![
                        run,
                        now(),
                        match (&stop_reason, &error) {
                            (Some(_), _) => "stopped_by_parent",
                            (None, Some(_)) => "failed",
                            (None, None) => "completed",
                        },
                        json!({"error":error,"usage":usage}).to_string(),
                        active.omitted_steps as i64,
                        stop_reason
                    ],
                )?;
                self.settle_steps(id, &mut active, failed)?;
                if !failed {
                    self.host.append_output(&session, run, &active.output)?;
                }
                runtime.last_finished_at = Some(now());
                runtime.last_error = error.clone();
                self.host.save_runtime(&runtime)?;
                for message in &active.messages {
                    self.host.db.execute(
                        "UPDATE messages SET receipt=?2 WHERE id=?1",
                        params![message, if error.is_some() { "held" } else { "completed" }],
                    )?;
                }
                if let Some(reason) = &stop_reason {
                    self.host.append_output(
                        &session,
                        &format!("stopped:{run}"),
                        &stopped_by_parent_notice(reason),
                    )?;
                    Host::event(
                        &self.host.db,
                        &session.project_id,
                        Some(id),
                        "turn_stopped_by_parent",
                        &format!("{run}; {reason}"),
                    )?;
                }
                self.settle_check_ins(&session)?;
                if let Some(error) = &error {
                    self.host.append_output(
                        &session,
                        &format!("error:{run}"),
                        &runtime_stopped(error),
                    )?;
                    self.host.set_status(
                        id,
                        if session.status == Status::Paused {
                            Status::Paused
                        } else {
                            Status::Disconnected
                        },
                    )?;
                } else if session.status == Status::Working {
                    self.host.set_status(id, Status::Ready)?;
                }
                // Shutdown finishes turns without waiting for their processes; startup settles.
                if !self.stopping {
                    self.host.settle_pending_worktrees();
                }
                // A deliberately archived parent has no one to wake, and one that stopped
                // the turn, or was told the human did, already knows.
                if let Some(parent) = &session.parent_id
                    && !self.host.session(parent)?.archived
                    && stop_reason.is_none()
                    && !active.stopped_by_human
                    && (error.is_some() || (session.role.is_worker() && !active.reported))
                {
                    let body = if let Some(error) = &error {
                        format!(
                            "{} stopped with an error: {error}. Inspect its worktree, then resume it with resume_agent, choosing held: retry or skip.",
                            session.name
                        )
                    } else {
                        format!(
                            "{} finished a turn. This is not ticket acceptance.\n{}",
                            session.name, active.output
                        )
                    };
                    self.host.send(
                        &format!("turn-result:{run}"),
                        Some(id),
                        parent,
                        &body.chars().take(MAX_TEXT_BYTES / 4).collect::<String>(),
                    )?;
                }
                let expired = self
                    .permissions
                    .iter()
                    .filter(|(_, p)| p.session == id)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for key in expired {
                    if let Some(p) = self.permissions.remove(&key) {
                        let _ = p.reply.try_send(Err("Provider turn ended".into()));
                        let _ = self
                            .host
                            .resolve_attention(&key, "Provider turn ended; approval expired");
                    }
                }
                Host::event(
                    &self.host.db,
                    &session.project_id,
                    Some(id),
                    "turn_finished",
                    run,
                )?;
            }
        }
        Ok(())
    }
    /// Wakes clients; each bounded signal coalesces any number of changes until it is read.
    /// Cancels every active turn and records it finished without waiting for its process.
    fn stop(&mut self) {
        self.stopping = true;
        let interrupted = self
            .active
            .iter()
            .map(|(id, a)| {
                a.cancel.store(true, Ordering::Relaxed);
                (id.clone(), a.run.clone())
            })
            .collect::<Vec<_>>();
        for (id, run) in interrupted {
            let _ = self.provider_event(
                &id,
                &run,
                ProviderEvent::Finished {
                    error: Some(QUIT_DURING_TURN.into()),
                    usage: Value::Null,
                    cancelled: false,
                },
            );
        }
    }
    fn signal(&mut self, state_changed: bool) {
        if state_changed {
            let _ = self.changed.try_send(());
        }
        if std::mem::take(&mut self.steps_dirty) {
            let _ = self.steps_changed.try_send(());
        }
    }
    fn next_revision(&mut self, session_id: &str) -> Result<u64> {
        let latest = match self.revisions.get(session_id) {
            Some(&revision) => revision,
            None => self.host.step_revision(session_id)?,
        };
        self.revisions.insert(session_id.into(), latest + 1);
        Ok(latest + 1)
    }
    /// Merges an adapter's update into the turn's steps. Text deltas stay in memory; the store
    /// is written when a step starts or changes state.
    fn record_step(&mut self, session_id: &str, run: &str, update: StepUpdate) -> Result<()> {
        let revision = self.next_revision(session_id)?;
        let active = self.active.get_mut(session_id).context("No active turn")?;
        let existing = active.steps.iter().position(|s| s.id == update.id);
        let index = match existing {
            Some(index) => index,
            None if active.steps.len() >= MAX_RUN_STEPS => {
                active.omitted_steps += 1;
                return Ok(());
            }
            None => {
                active.steps.push(Step {
                    run_id: run.into(),
                    id: update.id.clone(),
                    parent_id: None,
                    kind: update.kind,
                    state: StepState::Running,
                    title: String::new(),
                    note: None,
                    detail: None,
                    omitted: 0,
                    seq: active.steps.len() as u32,
                    revision,
                    started_at: now(),
                    finished_at: None,
                });
                active.steps.len() - 1
            }
        };
        let step = &mut active.steps[index];
        let persist = existing.is_none() || step.state != update.state;
        step.parent_id = update.parent_id;
        step.kind = update.kind;
        step.state = update.state;
        step.title = update.title;
        step.note = update.note;
        step.detail = update.detail;
        step.omitted = 0;
        step.revision = revision;
        if step.state != StepState::Running {
            step.finished_at.get_or_insert_with(now);
        }
        steps::bound(step);
        if persist {
            self.host.save_step(session_id, step)?;
        }
        self.steps_dirty = true;
        Ok(())
    }
    /// A finished turn leaves nothing running: unfinished steps succeed with the turn, or are
    /// interrupted when it failed. Every step is stored as it ended.
    fn settle_steps(&mut self, session_id: &str, active: &mut Active, failed: bool) -> Result<()> {
        for index in 0..active.steps.len() {
            if active.steps[index].state == StepState::Running {
                let revision = self.next_revision(session_id)?;
                let step = &mut active.steps[index];
                step.state = if failed {
                    StepState::Interrupted
                } else {
                    StepState::Succeeded
                };
                step.finished_at = Some(now());
                step.revision = revision;
            }
            self.host.save_step(session_id, &active.steps[index])?;
        }
        self.steps_dirty = true;
        Ok(())
    }
    /// A running turn's steps come from memory, which is ahead of the store; others from the store.
    fn steps(&self, session_id: &str, run_id: Option<&str>, after: u64) -> Result<StepPage> {
        let active = self
            .active
            .get(session_id)
            .filter(|a| run_id.is_none_or(|run| run == a.run));
        let Some(active) = active else {
            return self.host.steps(session_id, run_id, after);
        };
        let mut page = self.host.run_page(session_id, Some(&active.run))?;
        page.steps = active
            .steps
            .iter()
            .filter(|s| s.revision > after)
            .cloned()
            .collect();
        page.revision = page.steps.iter().map(|s| s.revision).fold(after, u64::max);
        page.omitted_steps = active.omitted_steps;
        Ok(page)
    }
    /// Lets a coordinator stop its direct children, or all of them when none are named, and
    /// keeps them paused so queued messages and timer fires wait for resume_agent. A running
    /// turn stops through the same cancel flag a human stop uses; the provider thread then
    /// finishes it.
    fn stop_agents(&mut self, caller: &str, args: &Value) -> Result<Value> {
        // A misspelt session_ids must not silently widen the stop to every child.
        only_arguments("stop_agents", args, &["session_ids", "reason"])?;
        let reason = args["reason"].as_str().context("Missing reason")?.trim();
        ensure!(!reason.is_empty(), "Give a reason for stopping");
        let reason = reason.chars().take(1000).collect::<String>();
        // Only a missing session_ids means every child; null or a non-list is refused.
        let targets = match args.get("session_ids") {
            Some(ids) => serde_json::from_value::<Vec<String>>(ids.clone())
                .context("session_ids must be a list of session ids")?
                .iter()
                .map(|id| self.direct_child(caller, id, "stop"))
                .collect::<Result<Vec<_>>>()?,
            None => self
                .host
                .sessions()?
                .into_iter()
                .filter(|s| s.parent_id.as_deref() == Some(caller) && !s.archived)
                .collect(),
        };
        let mut results = Vec::new();
        for target in targets {
            let running = self
                .active
                .get_mut(&target.id)
                .filter(|a| !a.cancel.load(Ordering::Relaxed));
            let result = if let Some(active) = running {
                active.stopped_by_parent = Some(reason.clone());
                active.cancel.store(true, Ordering::Relaxed);
                let run = active.run.clone();
                Host::event(
                    &self.host.db,
                    &target.project_id,
                    Some(&target.id),
                    "turn_stop_requested",
                    &format!("{run}; {reason}"),
                )?;
                "stopped"
            } else if matches!(target.status, Status::Paused | Status::Disconnected)
                || self.active.contains_key(&target.id)
            {
                "already_paused"
            } else {
                Host::event(
                    &self.host.db,
                    &target.project_id,
                    Some(&target.id),
                    "paused_by_parent",
                    &reason,
                )?;
                "paused_idle"
            };
            if result != "already_paused" {
                self.host.set_status(&target.id, Status::Paused)?;
                self.host.record_stop(&target.id, Stopper::Parent)?;
            }
            results.push(json!({"session_id":target.id,"name":target.name,"result":result}));
        }
        Ok(json!({ "agents": results }))
    }
    /// Lets a coordinator release a direct child that is paused or interrupted, through the same
    /// reconciliation as the human's Retry and Skip. Held input must be retried or skipped
    /// explicitly; a retried turn's prompt says it was interrupted. Nothing changes unless all
    /// of it succeeds.
    fn resume_agent(&mut self, caller: &str, args: &Value) -> Result<Value> {
        only_arguments("resume_agent", args, &["session_id", "held", "message"])?;
        let target = self.direct_child(
            caller,
            args["session_id"].as_str().context("Missing session_id")?,
            "resume",
        )?;
        ensure!(
            !self.active.contains_key(&target.id),
            "{} is still finishing its turn; resume it once it has stopped",
            target.name
        );
        ensure!(
            matches!(target.status, Status::Paused | Status::Disconnected),
            "{} is {}, not stopped; send_message reaches it without resuming",
            target.name,
            target.status.label()
        );
        let held = self.host.held_input(&target.id)?;
        let retry = match args.get("held").filter(|h| !h.is_null()) {
            None => {
                ensure!(
                    held == 0,
                    "{} has {held} held input message(s); inspect its worktree, then pass held: retry or skip",
                    target.name
                );
                true
            }
            Some(held) => match held.as_str() {
                Some("retry") => true,
                Some("skip") => false,
                _ => bail!("held must be retry or skip"),
            },
        };
        let message = match args.get("message").filter(|m| !m.is_null()) {
            Some(message) => {
                let message = message.as_str().context("message must be text")?;
                text(message, MAX_TEXT_BYTES).context("Invalid message")?;
                Some(message)
            }
            None => None,
        };
        let stopped_by = self.host.session_runtime(&target.id)?.stopped_by;
        self.host.atomically(|host| {
            host.reconcile_session(&target.id, retry)?;
            if let Some(body) = message {
                host.send(
                    &format!("resume:{}", new_id()),
                    Some(caller),
                    &target.id,
                    body,
                )?;
            }
            Host::event(
                &host.db,
                &target.project_id,
                Some(&target.id),
                "resumed_by_parent",
                if held == 0 {
                    "no held input"
                } else if retry {
                    "held input retried"
                } else {
                    "held input skipped"
                },
            )
        })?;
        let queued = self.host.queued_input(&target.id)?;
        let mut result = json!({"session_id":target.id,"status":"ready","queued":queued,"turn_scheduled":queued > 0});
        if let Some(by) = stopped_by {
            result["stopped_by"] = json!(by);
            result["note"] = json!(format!("{} was stopped by {}.", target.name, by.label()));
        }
        Ok(result)
    }
    /// The caller's direct child, if it is not archived.
    fn direct_child(&self, caller: &str, id: &str, action: &str) -> Result<Session> {
        let target = self.host.session(id)?;
        ensure!(
            target.parent_id.as_deref() == Some(caller),
            "Only {}'s direct parent can {action} it",
            target.name
        );
        ensure!(
            !target.archived,
            "{} is archived; restore it before you {action} it",
            target.name
        );
        Ok(target)
    }
    /// Records a human Stop or Pause and tells the session's parent once, whether or not a turn
    /// was running. It overrides a parent's stop still winding down, so the turn's input is held
    /// and the turn's end sends no second notice.
    fn human_stopped(&mut self, id: &str) -> Result<()> {
        let running = match self.active.get_mut(id) {
            Some(active) => {
                active.stopped_by_parent = None;
                active.stopped_by_human = true;
                true
            }
            None => false,
        };
        // Who stopped the session never depends on the notice: send_message and resume_agent
        // name the human even if the notice cannot be sent. The record also keeps it to one
        // notice per stop.
        if !self.host.record_stop(id, Stopper::Human)? {
            return Ok(());
        }
        let session = self.host.session(id)?;
        Host::event(
            &self.host.db,
            &session.project_id,
            Some(id),
            "stopped_by_human",
            if running { "running turn" } else { "idle" },
        )?;
        let Some(parent) = &session.parent_id else {
            return Ok(());
        };
        if session.archived || self.host.session(parent)?.archived {
            return Ok(());
        }
        let held = if running {
            "; its interrupted turn's input is held"
        } else {
            ""
        };
        self.host.send_human_notice(
            &format!("human-stop:{id}:{}", new_id()),
            Some(id),
            parent,
            &format!(
                "{} was stopped by the human{held}. This is an emergency stop: resume it with resume_agent only when the human says so.",
                session.name
            ),
        )?;
        Ok(())
    }
    /// Every `CHECK_IN_MS` of a running turn, tells the session's parent how it is going, or the
    /// human when it has none. Turns are never stopped for time; the parent may call stop_agents.
    /// Keeps a wake-up armed for the next check-in.
    fn check_in_long_turns(&mut self, now: i64) {
        let due = self
            .active
            .iter()
            .filter(|(_, a)| now >= a.next_check_in && !a.cancel.load(Ordering::Relaxed))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in due {
            let active = self.active.get_mut(&id).expect("due turns are active");
            let checks = (now - active.started_at) / CHECK_IN_MS;
            active.next_check_in = active.started_at + (checks + 1) * CHECK_IN_MS;
            if let Err(e) = self.check_in(&id, now) {
                eprintln!("Workspace check-in: {e:#}");
            }
        }
        // A cancelled turn still winding down has no check-in to wake for.
        let next = self
            .active
            .values()
            .filter(|a| !a.cancel.load(Ordering::Relaxed))
            .map(|a| a.next_check_in)
            .min();
        if let Some(next) = next {
            self.wake(next);
        }
    }
    fn check_in(&mut self, id: &str, now: i64) -> Result<()> {
        let session = self.host.session(id)?;
        let active = &self.active[id];
        let run = active.run.clone();
        let minutes = (now - active.started_at) / 60_000;
        let ticket = self.ticket_label(id)?;
        let latest = active.steps.last().map_or_else(
            || "no steps yet".to_owned(),
            |step| {
                format!(
                    "latest step: {}",
                    step.title.chars().take(300).collect::<String>()
                )
            },
        );
        let status = format!(
            "{}{ticket} has been in one turn for {minutes} minutes, since {}. Last provider event {}; {latest}.",
            session.name,
            schedules::rfc3339(active.started_at),
            schedules::rfc3339(active.last_event_at),
        );
        match &session.parent_id {
            Some(parent) => {
                if self.host.session(parent)?.archived {
                    return Ok(());
                }
                self.host.send(
                    &format!("{CHECK_IN}{run}:{minutes}"),
                    Some(id),
                    parent,
                    &format!(
                        "[check-in] {status}\nTo let it continue, do nothing. To stop it, call stop_agents with session_ids [{id}] and a reason."
                    ),
                )?;
            }
            None => {
                self.settle_check_ins(&session)?;
                self.host.request_attention(
                    id,
                    "local",
                    &format!("{CHECK_IN}{run}:{minutes}"),
                    &format!("{status} It keeps running; use Pause or Stop if it is stuck."),
                    &[],
                )?;
            }
        }
        Ok(())
    }
    /// ` on ticket "…"` for a ticket agent, or nothing.
    fn ticket_label(&self, id: &str) -> Result<String> {
        Ok(match self.host.session_runtime(id)?.ticket_id {
            Some(ticket) => format!(" on ticket \"{}\"", self.host.ticket(&ticket)?.title),
            None => String::new(),
        })
    }
    /// Counts the turns a session starts without its overseer, and checks in at every multiple
    /// of the threshold in the settings: with the parent, counted in the store from the parent's
    /// last message, or for the project coordinator with the human. Nothing is paused.
    fn check_in_by_turns(&mut self, session: &Session, run: &str) -> Result<()> {
        let settings = self.host.settings();
        let Some(parent) = &session.parent_id else {
            let turns = self
                .human_turns
                .entry(session.project_id.clone())
                .or_default();
            *turns += 1;
            let turns = *turns;
            if !turns.is_multiple_of(settings.checkin_human_turns as usize) {
                return Ok(());
            }
            for item in self.host.open_attention(&session.project_id)? {
                if item.session_id == session.id && item.operation_id.starts_with(TURN_COUNT) {
                    self.host.resolve_attention(&item.id, "Superseded")?;
                }
            }
            self.host.request_attention(
                &session.id,
                "local",
                &format!("{TURN_COUNT}{run}"),
                &format!(
                    "{} has run {turns} turns since you last stepped in. It keeps running; message it, or use Pause or Stop if it is stuck.",
                    session.name
                ),
                &[],
            )?;
            return Ok(());
        };
        let (turns, since): (u32, Option<i64>) = self.host.db.query_row(
            "WITH last AS (SELECT MAX(created_at) AS at FROM messages WHERE recipient=?1 AND sender=?2) SELECT (SELECT COUNT(*) FROM provider_runs WHERE session_id=?1 AND started_at>=COALESCE(last.at,0)), last.at FROM last",
            params![session.id, parent],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if turns == 0
            || !turns.is_multiple_of(settings.checkin_child_turns)
            || self.host.session(parent)?.archived
        {
            return Ok(());
        }
        let since = since.map_or_else(
            || "without a message from you".to_owned(),
            |at| format!("since you last messaged it at {}", schedules::rfc3339(at)),
        );
        let reply = self
            .host
            .db
            .query_row(
                "SELECT body FROM messages WHERE sender=?1 AND recipient=?1 AND substr(id,1,7)='output:' ORDER BY sequence DESC LIMIT 1",
                [&session.id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map_or_else(
                || "No reply yet.".to_owned(),
                |body| format!("Last reply: {}", body.chars().take(300).collect::<String>()),
            );
        self.host.send(
            &format!("{CHECK_IN}{run}:turns"),
            Some(&session.id),
            parent,
            &format!(
                "[check-in] {}{} has run {turns} turns {since}. {reply}\nTo let it continue, do nothing. To stop it, call stop_agents with session_ids [{}] and a reason.",
                session.name,
                self.ticket_label(&session.id)?,
                session.id
            ),
        )?;
        Ok(())
    }
    /// Clears a session's check-in inbox items once a newer one or the turn's end replaces them.
    fn settle_check_ins(&mut self, session: &Session) -> Result<()> {
        for item in self.host.open_attention(&session.project_id)? {
            if item.session_id == session.id && item.operation_id.starts_with(CHECK_IN) {
                self.host.resolve_attention(&item.id, "Superseded")?;
            }
        }
        Ok(())
    }
    fn schedule(&mut self) -> Result<()> {
        self.check_in_long_turns(now());
        // Timers fire into the queue whether or not the project is live.
        match self.host.fire_due_schedules(now()) {
            Ok(pass) => {
                if let Some(next) = pass.next {
                    self.wake(next);
                }
                // Fires and next-fire times show in the client even when no turn starts.
                if pass.changed {
                    let _ = self.changed.try_send(());
                }
            }
            Err(e) => eprintln!("Workspace timers: {e:#}"),
        }
        let in_rounds = self.admit_rounds()?;
        // Quiet messages ride along with the next turn but never start one.
        let messages=self.host.db.prepare(&format!("SELECT m.* FROM messages m JOIN live_projects p ON p.project_id=m.project_id AND p.enabled=1 JOIN sessions s ON s.id=m.recipient WHERE m.receipt='queued' AND m.quiet=0 AND COALESCE(json_extract(s.data,'$.archived'),0)=0 AND {} AND NOT EXISTS (SELECT 1 FROM messages h WHERE h.recipient=m.recipient AND h.receipt='held') ORDER BY s.role<>'project_orchestrator',m.sequence LIMIT 100", is_turn_input("m")))?.query_map([],Host::message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut considered = HashSet::new();
        for message in messages {
            if !considered.insert(message.recipient.clone())
                || in_rounds.contains(&message.recipient)
            {
                continue;
            }
            let session = self.host.session(&message.recipient)?;
            if self.active.contains_key(&session.id)
                || matches!(session.status, Status::Paused | Status::Disconnected)
            {
                continue;
            }
            let Some(input) = self.due_input(&session.id)? else {
                continue;
            };
            let runtime = self.host.session_runtime(&session.id)?;
            if let Some(ticket) = &runtime.ticket_id
                && self.worktree_busy(ticket, &session)?
            {
                continue;
            }
            if let Some(reason) = self.host.hold_reason(&session)? {
                // One session's hold must never stop the others from being scheduled.
                if let Err(error) = self.hold_turn(&session, &input, &reason) {
                    eprintln!("Holding {}: {error:#}", session.id);
                }
                continue;
            }
            if let Err(error) = self.start_turn(session.clone(), input) {
                self.start_failed(&session, &message, &error)?;
            }
        }
        Ok(())
    }
    /// A ticket's worktree has one writer at a time. Readers run together: reviewers, and the
    /// testers verifying the ticket in its running cycle.
    fn worktree_busy(&self, ticket_id: &str, session: &Session) -> Result<bool> {
        let ticket = self.host.ticket(ticket_id)?;
        let reader = |session: &Session| {
            session.role == Role::Reviewer
                || ticket.running_cycle().is_some_and(|cycle| {
                    cycle
                        .rounds
                        .iter()
                        .flat_map(|r| &r.verifiers)
                        .any(|v| v.session_id == session.id)
                })
        };
        for id in self.active.keys() {
            if self
                .host
                .session_runtime(id)?
                .ticket_id
                .is_some_and(|t| t == ticket_id)
            {
                let active = self.host.session(id)?;
                if !(reader(session) && reader(&active)) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    /// Starts each verification round that can start. A round's verifiers start together or
    /// not at all, so they are never scheduled one by one; returns them all.
    fn admit_rounds(&mut self) -> Result<HashSet<String>> {
        let mut members_of_rounds = HashSet::new();
        for (ticket, members) in self.waiting_rounds()? {
            self.admit_round(&ticket, &members)?;
            members_of_rounds.extend(members.into_iter().map(|s| s.id));
        }
        Ok(members_of_rounds)
    }
    /// Each running cycle's current round whose verifiers still have their round message queued.
    fn waiting_rounds(&self) -> Result<Vec<(Ticket, Vec<Session>)>> {
        let mut rounds = vec![];
        for ticket in self.host.tickets()? {
            let Some(round) = ticket.running_cycle().and_then(Verification::current_round) else {
                continue;
            };
            let mut members = vec![];
            for run in &round.verifiers {
                if self
                    .host
                    .message(&run.message_id)
                    .is_ok_and(|m| m.receipt == Receipt::Queued)
                {
                    members.push(self.host.session(&run.session_id)?);
                }
            }
            if !members.is_empty() {
                rounds.push((ticket, members));
            }
        }
        Ok(rounds)
    }
    /// Starts every member of a round, or none of them.
    fn admit_round(&mut self, ticket: &Ticket, members: &[Session]) -> Result<()> {
        let held = |id: &str| -> Result<bool> {
            Ok(self.host.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE recipient=?1 AND receipt='held')",
                [id],
                |r| r.get(0),
            )?)
        };
        if !self.host.live_projects()?.contains(&members[0].project_id) {
            return Ok(());
        }
        for member in members {
            if member.archived
                || matches!(member.status, Status::Paused | Status::Disconnected)
                || held(&member.id)?
                || self.active.contains_key(&member.id)
                || self.worktree_busy(&ticket.id, member)?
            {
                return Ok(());
            }
        }
        let mut inputs = vec![];
        for member in members {
            match self.due_input(&member.id)? {
                Some(input) => inputs.push((member.clone(), input)),
                None => return Ok(()),
            }
        }
        for (member, input) in inputs {
            // A held verifier reports blocked and never runs on a substitute; the rest of the
            // round runs, so the round ends and its blocked outcome wakes the coordinator.
            if let Some(reason) = self.host.hold_reason(&member)? {
                if let Err(error) = self.hold_turn(&member, &input, &reason) {
                    eprintln!("Holding {}: {error:#}", member.id);
                }
                continue;
            }
            let first = input[0].clone();
            if let Err(error) = self.start_turn(member.clone(), input) {
                self.start_failed(&member, &first, &error)?;
            }
        }
        Ok(())
    }
    /// Keeps the input queued, blocks the session and tells its parent once per held input,
    /// through the agent's own `report` path so the parent wakes.
    fn hold_turn(&mut self, session: &Session, input: &[Message], reason: &str) -> Result<()> {
        let mut runtime = self.host.session_runtime(&session.id)?;
        let already = session.status == Status::Blocked
            && runtime.held
            && runtime.last_error.as_deref() == Some(reason);
        if !already {
            runtime.last_error = Some(reason.into());
            runtime.held = true;
            self.host.save_runtime(&runtime)?;
            self.host.set_status(&session.id, Status::Blocked)?;
            let _ = self.changed.try_send(());
            Host::event(
                &self.host.db,
                &session.project_id,
                Some(&session.id),
                "turn_held",
                reason,
            )?;
        }
        // Sent on every pass, so a report that failed earlier is retried. Keyed on the held
        // input's sequence: bounded, the same after a Retry, and a no-op once it exists.
        if session.parent_id.is_some()
            && let Some(first) = input.first()
        {
            self.host.agent_tool(
                &session.id,
                "report",
                json!({"message_id": format!("turn-held:{}", first.sequence), "kind": "blocked", "body": reason}),
            )?;
        }
        Ok(())
    }
    /// Records why a turn could not start and settles any run it opened.
    fn start_failed(
        &mut self,
        session: &Session,
        message: &Message,
        error: &anyhow::Error,
    ) -> Result<()> {
        // A turn that never launched must not look active to the worktree guard.
        self.host.db.execute(
            "UPDATE provider_runs SET finished_at=?2,outcome='failed',detail=json_set(detail,'$.result',json(?3)) WHERE session_id=?1 AND finished_at IS NULL",
            params![session.id, now(), json!({"error":format!("{error:#}")}).to_string()],
        )?;
        let mut runtime = self.host.session_runtime(&session.id)?;
        runtime.last_error = Some(format!("{error:#}"));
        self.host.save_runtime(&runtime)?;
        self.host.set_status(&session.id, Status::Disconnected)?;
        self.host.append_output(
            session,
            &format!("start-error:{}", message.id),
            &format!("Could not start: {error:#}"),
        )?;
        self.host.settle_pending_worktrees();
        Ok(())
    }
    /// The recipient's next turn input: every queued message, in order, up to
    /// MAX_TURN_MESSAGES. It is `None` while only progress reports wait inside
    /// their batch window; one wake-up is then armed for when the window closes.
    fn due_input(&mut self, recipient: &str) -> Result<Option<Vec<Message>>> {
        let input = self.host.db.prepare(&format!("SELECT * FROM messages WHERE recipient=?1 AND receipt='queued' AND {} ORDER BY sequence LIMIT ?2", is_turn_input("messages")))?.query_map(params![recipient, MAX_TURN_MESSAGES],Host::message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        // Urgency covers the whole queue, not just the bounded input.
        let urgent: bool = self.host.db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM messages WHERE recipient=?1 AND receipt='queued' AND quiet=0 AND {} AND NOT ({IS_PROGRESS_REPORT}))", is_turn_input("messages")), [recipient], |r| r.get(0))?;
        let due = input
            .first()
            .map_or(0, |m| m.created_at + PROGRESS_BATCH_MS);
        if urgent || now() >= due {
            return Ok(Some(input));
        }
        self.wake(due);
        Ok(None)
    }
    /// Arms one wake-up for `at` unless an earlier one is already armed.
    fn wake(&mut self, at: i64) {
        if self.wake_at.is_some_and(|armed| armed <= at) {
            return;
        }
        self.wake_at = Some(at);
        let sender = self.sender.clone();
        let wait = (at - now()).clamp(0, MAX_WAKE_SLEEP_MS) as u64;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let _ = sender.send_blocking(Event::Wake(at));
        });
    }
    /// Records the run and delivers its whole input together, so a failure leaves every
    /// message queued for the next attempt.
    fn record_turn_start(
        &mut self,
        run: &str,
        session: &Session,
        input: &[Message],
        detail: &Value,
        runtime: &SessionRuntime,
    ) -> Result<()> {
        let ids = input.iter().map(|m| &m.id).collect::<Vec<_>>();
        let queued_at = input.first().context("Nothing to deliver")?.created_at;
        self.host.atomically(|host| {
            host.db.execute(
                "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,?3,?4,?5)",
                params![run, session.id, json!(ids).to_string(), now(), detail.to_string()],
            )?;
            for id in ids {
                host.advance_receipt(id, Receipt::Delivered)?;
            }
            host.set_status(&session.id, Status::Working)?;
            host.save_runtime(runtime)?;
            Host::event(
                &host.db,
                &session.project_id,
                Some(&session.id),
                "turn_scheduled",
                &format!("{run}; queue_ms={}", now() - queued_at),
            )
        })
    }
    /// The turn's whole prompt. A session's first turn since the host started also says what
    /// the restart interrupted and which of its timers fired late.
    fn prompt(&self, session: &Session, input: &[Message]) -> Result<String> {
        let mut notes = Vec::new();
        for message in input {
            let note = interrupted_turn_note(&self.host, &message.id)?;
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
        notes.push(stopped_by_parent_note(&self.host, &session.id)?);
        let delivered_at = now();
        let resume = if self.resumed.contains(&session.id) {
            String::new()
        } else {
            resume_note(
                &self.host,
                &session.id,
                self.started_at,
                delivered_at,
                &notes,
            )?
        };
        let input = input
            .iter()
            .map(|m| Ok((m, self.host.fire_status(&m.id, delivered_at)?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(turn_prompt(
            session,
            &(resume + &notes.concat()),
            &input,
            &open_questions_reminder(&self.host.open_questions(session)?),
        ))
    }
    fn start_turn(&mut self, session: Session, input: Vec<Message>) -> Result<()> {
        let mut runtime = self.host.session_runtime(&session.id)?;
        let profile = self.host.turn_profile(&session)?;
        if runtime.profile.is_none() {
            runtime.selection = Some(Selection {
                chosen_by: Chooser::Default,
                reason: "Role default".into(),
                revision: self.host.model_selection()?.revision,
                at: now(),
            });
        }
        runtime.profile = Some(profile.clone());
        provider::executable(session.provider)?;
        if let Some(ticket) = &runtime.ticket_id {
            self.host.prepare_ticket_worktree(ticket)?;
        }
        let cwd = if let Some(path) = &runtime.workdir {
            PathBuf::from(path)
        } else {
            ensure!(
                !session.role.is_worker(),
                "Assign this worker to a ticket before running it"
            );
            let path = self.host.project_directory(&session.project_id)?;
            fs::create_dir_all(&path)?;
            path
        };
        let run = new_id();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = format!("{}{}", new_id(), new_id());
        let prompt = self.prompt(&session, &input)?;
        let started_at = now();
        let detail = json!({"profile":profile,"provider_session_id":runtime.provider_session_id,"skill_version":1,"selection":runtime.selection,"model_selection_revision":self.host.model_selection()?.revision});
        runtime.last_error = None;
        runtime.held = false;
        runtime.last_started_at = Some(started_at);
        runtime.directory = Some(cwd.to_string_lossy().into_owned());
        self.record_turn_start(&run, &session, &input, &detail, &runtime)?;
        self.resumed.insert(session.id.clone());
        // Nothing fallible may follow: the input is delivered and only Active can settle it.
        let turn = Turn {
            run_id: run.clone(),
            session: session.clone(),
            profile,
            provider_session: runtime.provider_session_id,
            cwd,
            prompt,
            attachments: input.iter().flat_map(|m| m.attachments.clone()).collect(),
            socket: self.socket.clone(),
            token: token.clone(),
            helper: self.helper.clone(),
        };
        let active = Active::new(
            run.clone(),
            token,
            cancel.clone(),
            input.into_iter().map(|m| m.id).collect(),
            started_at,
        );
        self.wake(active.next_check_in);
        self.active.insert(session.id.clone(), active);
        let _ = self.changed.try_send(());
        if let Err(e) = self.check_in_by_turns(&session, &run) {
            eprintln!("Workspace check-in: {e:#}");
        }
        let sender = self.sender.clone();
        let id = session.id;
        std::thread::spawn(move || {
            let result = provider::run(turn, cancel, |event| {
                let _ = sender.send_blocking(Event::Provider {
                    session: id.clone(),
                    run: run.clone(),
                    event,
                });
            });
            if let Err(error) = result {
                let _ = sender.send_blocking(Event::Provider {
                    session: id,
                    run,
                    event: ProviderEvent::Finished {
                        error: Some(format!("{error:#}")),
                        usage: Value::Null,
                        cancelled: error.is::<provider::Cancelled>(),
                    },
                });
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An actor with one active turn and no provider process behind it.
    fn actor_with_turn() -> (tempfile::TempDir, Actor, String, String) {
        let (home, actor, session, run, _, _) = actor_with_signals();
        (home, actor, session, run)
    }

    fn actor_with_signals() -> (
        tempfile::TempDir,
        Actor,
        String,
        String,
        Receiver<()>,
        Receiver<()>,
    ) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        host.create_project("Steps").unwrap();
        let session = host.sessions().unwrap().remove(0).id;
        let run = new_id();
        host.db
            .execute(
                "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,'[]',?3,'{}')",
                params![run, session, now()],
            )
            .unwrap();
        let (mut actor, changes, step_changes) = idle_actor(host);
        actor
            .active
            .insert(session.clone(), active_turn(&run, vec![]));
        (home, actor, session, run, changes, step_changes)
    }

    fn active_turn(run: &str, messages: Vec<String>) -> Active {
        Active::new(
            run.into(),
            String::new(),
            Arc::new(AtomicBool::new(false)),
            messages,
            now(),
        )
    }

    fn idle_actor(host: Host) -> (Actor, Receiver<()>, Receiver<()>) {
        let (sender, _) = async_channel::bounded(1);
        let (changed, changes) = async_channel::bounded(1);
        let (steps_changed, step_changes) = async_channel::bounded(1);
        let actor = Actor {
            host,
            sender,
            changed,
            steps_changed,
            steps_dirty: false,
            revisions: HashMap::new(),
            active: HashMap::new(),
            permissions: HashMap::new(),
            socket: PathBuf::new(),
            helper: PathBuf::new(),
            human_turns: HashMap::new(),
            wake_at: None,
            started_at: now(),
            resumed: HashSet::new(),
            stopping: false,
            overlaps_checked: HashMap::new(),
            overlaps_running: HashSet::new(),
            overlaps_due_at: 0,
        };
        (actor, changes, step_changes)
    }

    /// A ticket whose tester is in its turn, run `run`, under its project coordinator.
    fn ticket_in_turn() -> (tempfile::TempDir, Actor, Ticket, Session, Session) {
        let (home, mut host, ticket, coordinator) = ticket_fixture();
        let tester = host
            .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
            .unwrap();
        host.db
            .execute(
                "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES ('run',?1,'[]',1,'{}')",
                [&tester.id],
            )
            .unwrap();
        let (actor, _, _) = idle_actor(host);
        (home, actor, ticket, tester, coordinator)
    }

    /// A ticket "Toolbar" in a fresh repository, owned by its project's coordinator.
    fn ticket_fixture() -> (tempfile::TempDir, Host, Ticket, Session) {
        let home = tempfile::tempdir().unwrap();
        let repository = home.path().join("web");
        fs::create_dir_all(&repository).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec![
                "-c",
                "user.name=F",
                "-c",
                "user.email=f@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "F",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&repository)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let mut host = Host::open(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Start").unwrap();
        let root = host.sessions().unwrap().remove(0);
        let attached = host
            .attach_repository(&project.id, repository.to_str().unwrap(), "HEAD")
            .unwrap();
        let coordinator = root;
        let ticket = host
            .create_ticket(&coordinator.id, &attached.id, "Toolbar", "Do")
            .unwrap();
        (home, host, ticket, coordinator)
    }

    fn warning(ticket: &str, overlapping: Option<&str>) -> BranchWarnings {
        BranchWarnings {
            ticket_id: ticket.into(),
            base: "HEAD".into(),
            base_ahead: 0,
            overlaps: overlapping
                .map(|other| BranchOverlap {
                    ticket_id: other.into(),
                    title: "Other".into(),
                    project: "Start".into(),
                    files: vec!["a.rs".into()],
                    ..Default::default()
                })
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_repository_has_one_overlap_check_at_a_time_and_at_most_every_five_minutes() {
        let (_home, host, ticket, _) = ticket_fixture();
        let (mut actor, _, _) = idle_actor(host);
        let repository = ticket.repository_id.clone();
        let next = 1_000 + overlaps::OVERLAP_REFRESH_MS;
        actor.refresh_overlaps(1_000);
        assert_eq!(actor.overlaps_due_at, next);
        // Inside the interval nothing is read or started.
        actor.refresh_overlaps(next - 1);
        assert_eq!(actor.overlaps_checked[&repository], 1_000);
        // A check still running when the interval ends is not joined by a second one.
        actor.refresh_overlaps(next);
        assert_eq!(actor.overlaps_checked[&repository], 1_000);
        assert!(actor.handle(Event::Overlaps {
            repository: repository.clone(),
            fetched_at: None,
            warnings: Some(vec![warning(&ticket.id, None)]),
        }));
        assert!(actor.overlaps_due_at <= next);
        actor.refresh_overlaps(next);
        assert_eq!(actor.overlaps_checked[&repository], next);
        assert!(actor.overlaps_running.contains(&repository));
    }

    #[test]
    fn reading_workspace_context_refreshes_a_stale_cache_and_returns_it_meanwhile() {
        let (_home, host, ticket, coordinator) = ticket_fixture();
        let (mut actor, _, _) = idle_actor(host);
        let repository = ticket.repository_id.clone();
        let mut stale = warning(&ticket.id, None);
        stale.base_ahead = 2;
        actor.refresh_overlaps(now() - overlaps::OVERLAP_REFRESH_MS);
        actor.handle(Event::Overlaps {
            repository: repository.clone(),
            fetched_at: None,
            warnings: Some(vec![stale]),
        });
        let mut turn = active_turn("coordinating", vec![]);
        turn.token = "coordinator-token".into();
        actor.active.insert(coordinator.id.clone(), turn);
        let (reply, replies) = async_channel::bounded(1);
        let before = now();
        actor.process(Event::Tool {
            token: "coordinator-token".into(),
            name: "workspace_context".into(),
            args: json!({}),
            reply,
        });
        let context = replies.try_recv().unwrap().unwrap();
        assert_eq!(
            context["tickets"][0]["warnings"],
            json!(["HEAD is 2 commits ahead"])
        );
        assert!(actor.overlaps_checked[&repository] >= before);
        assert!(actor.overlaps_running.contains(&repository));
    }

    #[test]
    fn a_check_that_panics_still_frees_its_repository_for_the_next_refresh() {
        let (_home, host, ticket, _) = ticket_fixture();
        let (mut actor, _, _) = idle_actor(host);
        let (sender, events) = async_channel::bounded(1);
        let repository = ticket.repository_id.clone();
        actor.overlaps_checked.insert(repository.clone(), 1_000);
        actor.overlaps_running.insert(repository.clone());
        spawn_overlap_check(sender, repository.clone(), || panic!("injected"));
        assert!(!actor.handle(events.recv_blocking().unwrap()));
        let next = 1_000 + overlaps::OVERLAP_REFRESH_MS;
        actor.refresh_overlaps(next);
        assert_eq!(actor.overlaps_checked[&repository], next);
    }

    #[test]
    fn warnings_about_a_finished_ticket_are_dropped_even_from_a_late_check() {
        let (_home, mut host, first, coordinator) = ticket_fixture();
        let second = host
            .create_ticket(&coordinator.id, &first.repository_id, "Menu", "Do")
            .unwrap();
        host.set_branch_warnings(
            first.repository_id.clone(),
            None,
            vec![
                warning(&first.id, Some(&second.id)),
                warning(&second.id, Some(&first.id)),
            ],
        );
        assert_eq!(host.snapshot().unwrap().branch_warnings.len(), 2);
        host.close_ticket(&second.id).unwrap();
        // A check that started before the close reports after it.
        host.set_branch_warnings(
            first.repository_id.clone(),
            None,
            vec![
                warning(&first.id, Some(&second.id)),
                warning(&second.id, Some(&first.id)),
            ],
        );
        assert!(host.snapshot().unwrap().branch_warnings.is_empty());
        let context = host.agent_context(&coordinator.id).unwrap();
        let tickets = context["tickets"].as_array().unwrap();
        assert!(tickets.iter().all(|t| t.get("warnings").is_none()));
    }

    /// A live project whose ticket has a verification round of `count` testers queued and not
    /// yet started.
    fn waiting_round(count: usize) -> (tempfile::TempDir, Actor, Ticket, Vec<String>) {
        let (home, mut host, ticket, coordinator) = ticket_fixture();
        host.set_verification(VerificationSettings {
            verifiers: (0..count)
                .map(|i| VerifierConfig {
                    role: Role::Tester,
                    focus: format!("Tester {i}"),
                    instruction: None,
                    provider: None,
                })
                .collect(),
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Do", None)
            .unwrap();
        host.agent_tool(
            &implementer.id,
            "report",
            json!({"message_id":"ready","kind":"ready_for_testing","body":"Done"}),
        )
        .unwrap();
        let profile = host
            .model_selection()
            .unwrap()
            .prefill(Role::Tester)
            .unwrap();
        let choices: Vec<_> = host
            .settings()
            .verification
            .verifiers
            .into_iter()
            .map(|v| VerifierChoice {
                focus: v.focus,
                profile: profile.clone(),
                reason: "Routine check".into(),
            })
            .collect();
        let ticket = host.verify_ticket(&ticket.id, &choices).unwrap();
        host.set_live(&coordinator.project_id, true).unwrap();
        let verifiers = ticket.verification.as_ref().unwrap().rounds[0]
            .verifiers
            .iter()
            .map(|v| v.session_id.clone())
            .collect();
        let (actor, _, _) = idle_actor(host);
        (home, actor, ticket, verifiers)
    }

    fn queued(actor: &Actor, ticket: &Ticket) -> usize {
        ticket.verification.as_ref().unwrap().rounds[0]
            .verifiers
            .iter()
            .filter(|v| actor.host.message(&v.message_id).unwrap().receipt == Receipt::Queued)
            .count()
    }

    #[test]
    fn testers_verifying_a_ticket_share_its_worktree_but_writers_do_not() {
        let (_home, mut actor, ticket, verifiers) = waiting_round(2);
        let second = actor.host.session(&verifiers[1]).unwrap();
        actor
            .active
            .insert(verifiers[0].clone(), active_turn("first", vec![]));
        assert!(!actor.worktree_busy(&ticket.id, &second).unwrap());

        let implementer = actor
            .host
            .ticket_agents(&ticket.id)
            .unwrap()
            .into_iter()
            .find(|s| s.role == Role::Implementer)
            .unwrap();
        actor
            .active
            .insert(implementer.id.clone(), active_turn("write", vec![]));
        assert!(actor.worktree_busy(&ticket.id, &second).unwrap());
        actor.active.remove(&implementer.id);

        let ad_hoc = actor
            .host
            .assign_ticket(
                &ticket.id,
                Role::Tester,
                Provider::Codex,
                "Test",
                Some("Extra"),
            )
            .unwrap();
        assert!(
            actor.worktree_busy(&ticket.id, &ad_hoc).unwrap(),
            "a tester outside the cycle still writes"
        );
    }

    #[test]
    fn a_round_waits_whole_while_the_implementer_writes_in_its_worktree() {
        let (_home, mut actor, ticket, verifiers) = waiting_round(3);
        let implementer = actor
            .host
            .sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.role == Role::Implementer)
            .unwrap();
        actor
            .active
            .insert(implementer.id.clone(), active_turn("writing", vec![]));

        let members = actor.admit_rounds().unwrap();

        assert_eq!(members, verifiers.iter().cloned().collect::<HashSet<_>>());
        assert_eq!(actor.active.len(), 1, "no verifier started alone");
        assert_eq!(queued(&actor, &ticket), 3);
    }

    #[test]
    fn a_round_with_a_paused_verifier_waits_whole() {
        let (_home, mut actor, ticket, verifiers) = waiting_round(2);
        actor
            .host
            .set_status(&verifiers[1], Status::Paused)
            .unwrap();

        let members = actor.admit_rounds().unwrap();

        assert_eq!(members.len(), 2, "its verifiers still start only together");
        assert!(actor.active.is_empty());
        assert_eq!(queued(&actor, &ticket), 2);
    }

    #[test]
    fn a_disabled_provider_holds_coordinator_choices_and_reports_once() {
        let (_home, mut actor, first, tester, coordinator) = ticket_in_turn();
        let host = &mut actor.host;
        let ticket = host
            .create_ticket(&coordinator.id, &first.repository_id, "Review", "Do")
            .unwrap();
        let reviewer = host
            .assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        let sol = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "medium".into(),
        };
        host.pin(
            &reviewer.id,
            sol.clone(),
            Chooser::Coordinator {
                session_id: coordinator.id.clone(),
            },
            "Routine review",
        )
        .unwrap();
        // Only the reviewer may be scheduled; nothing here may launch a real provider.
        for id in [&tester.id, &coordinator.id] {
            host.set_status(id, Status::Paused).unwrap();
        }
        let mut selection = host.model_selection().unwrap();
        selection
            .providers
            .get_mut(&Provider::Codex)
            .unwrap()
            .enabled = false;
        selection
            .role_providers
            .insert(Role::Tester, [Provider::Claude].into());
        host.set_model_selection(&selection).unwrap();
        host.set_live(&reviewer.project_id, true).unwrap();
        let reports = |host: &Host| -> Vec<String> {
            host.db
                .prepare("SELECT id FROM messages WHERE recipient=?1 AND sender=?2")
                .unwrap()
                .query_map(params![coordinator.id, reviewer.id], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        // A long input ID, such as a forwarded child report, must not break the report's ID.
        host.db
            .execute(
                "UPDATE messages SET receipt='completed' WHERE id=?1",
                [format!("assignment:{}", reviewer.id)],
            )
            .unwrap();
        let assignment = format!("report:{}:assignment:{}", new_id(), new_id());
        let input = host
            .send(
                &assignment,
                Some(&coordinator.id),
                &reviewer.id,
                "Review it",
            )
            .unwrap();
        let report_id = format!("report:{}:turn-held:{}", reviewer.id, input.sequence);
        for retry in [false, true] {
            if retry {
                actor.host.reconcile_session(&reviewer.id, true).unwrap();
            }
            actor.schedule().unwrap();
            assert!(!actor.active.contains_key(&reviewer.id));
            let held = actor.host.session(&reviewer.id).unwrap();
            assert_eq!(held.status, Status::Blocked);
            let error = actor.host.session_runtime(&reviewer.id).unwrap().last_error;
            assert_eq!(
                error.as_deref(),
                Some(
                    "Codex is disabled on this machine. Enable it in Models, or choose another model for this agent."
                )
            );
            assert_eq!(
                actor.host.message(&assignment).unwrap().receipt,
                Receipt::Queued,
                "input kept"
            );
            assert_eq!(
                reports(&actor.host),
                [report_id.as_str()],
                "one report to the parent, even after a retry"
            );
            assert_eq!(
                actor
                    .host
                    .runtimes()
                    .unwrap()
                    .iter()
                    .find(|r| r.session_id == reviewer.id)
                    .unwrap()
                    .profile,
                Some(sol.clone()),
                "no model switch"
            );
        }
        let report = actor.host.message(&report_id).unwrap();
        assert!(report.body.starts_with("[blocked] "));
        assert!(report.body.ends_with("Codex is disabled on this machine. Enable it in Models, or choose another model for this agent."));
        // Skip, through the same command the banner sends, drops the held input.
        let skipped = actor.host.respond(Request {
            version: PROTOCOL_VERSION,
            id: new_id(),
            command: Command::ReconcileSession {
                session_id: reviewer.id.clone(),
                retry: false,
            },
        });
        assert_eq!(skipped.error, None);
        assert_eq!(
            actor.host.message(&assignment).unwrap().receipt,
            Receipt::Cancelled
        );
        actor.schedule().unwrap();
        assert_eq!(reports(&actor.host).len(), 1, "nothing left to hold");
        // The human's own choice is never held.
        actor.host.configure_session(&reviewer.id, sol).unwrap();
        assert_eq!(actor.host.hold_reason(&reviewer).unwrap(), None);
    }

    #[test]
    fn an_assignment_exists_only_with_its_model_and_an_unpinned_agent_never_runs() {
        let (_home, mut actor, first, tester, coordinator) = ticket_in_turn();
        let host = &mut actor.host;
        for id in [&tester.id, &coordinator.id] {
            host.set_status(id, Status::Paused).unwrap();
        }
        let ticket = host
            .create_ticket(&coordinator.id, &first.repository_id, "Atomic", "Do")
            .unwrap();
        let opus = json!({"provider":"claude","model":"opus","effort":"high"});
        let assign = json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Do",
            "profile":opus,"reason":"Large change"});
        // Fail the write that pins the model, after the session and its instruction were written.
        host.db
            .execute_batch(
                "CREATE TEMP TRIGGER fail_pin_insert BEFORE INSERT ON runtimes
                     WHEN json_extract(NEW.data,'$.selection') IS NOT NULL
                     BEGIN SELECT RAISE(ABORT,'injected'); END;
                 CREATE TEMP TRIGGER fail_pin_update BEFORE UPDATE ON runtimes
                     WHEN json_extract(NEW.data,'$.selection') IS NOT NULL
                     BEGIN SELECT RAISE(ABORT,'injected'); END;",
            )
            .unwrap();
        let sessions = host.sessions().unwrap().len();
        let count =
            |host: &Host, sql: &str| -> i64 { host.db.query_row(sql, [], |r| r.get(0)).unwrap() };
        let messages = count(host, "SELECT COUNT(*) FROM messages");
        assert!(
            host.agent_tool(&coordinator.id, "assign_ticket", assign.clone())
                .is_err()
        );
        assert_eq!(
            host.sessions().unwrap().len(),
            sessions,
            "no agent without its model"
        );
        assert_eq!(
            count(host, "SELECT COUNT(*) FROM messages"),
            messages,
            "no instruction"
        );
        assert_eq!(host.ticket(&ticket.id).unwrap().state, ticket.state);
        assert_eq!(
            count(
                host,
                "SELECT COUNT(*) FROM activity WHERE kind='model_selected'"
            ),
            0
        );
        // With the fault gone, retrying creates the agent with its model.
        host.db
            .execute_batch("DROP TRIGGER fail_pin_insert; DROP TRIGGER fail_pin_update;")
            .unwrap();
        let created: Session = serde_json::from_value(
            host.agent_tool(&coordinator.id, "assign_ticket", assign)
                .unwrap(),
        )
        .unwrap();
        let runtime = host.session_runtime(&created.id).unwrap();
        assert!(runtime.profile.is_some() && runtime.selection.is_some());
        // Belt and braces: an agent below the coordinator without a pinned model is held
        // visibly, never given a substitute.
        let unpinned = host
            .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
            .unwrap();
        assert!(host.turn_profile(&unpinned).is_err());
        host.set_live(&unpinned.project_id, true).unwrap();
        for id in host
            .sessions()
            .unwrap()
            .iter()
            .filter(|s| s.id != unpinned.id)
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
        {
            host.set_status(&id, Status::Paused).unwrap();
        }
        actor.schedule().unwrap();
        assert!(!actor.active.contains_key(&unpinned.id));
        assert_eq!(
            actor.host.session(&unpinned.id).unwrap().status,
            Status::Blocked
        );
        assert_eq!(
            actor
                .host
                .session_runtime(&unpinned.id)
                .unwrap()
                .last_error
                .as_deref(),
            Some(
                "This agent has no pinned model. Choose one from its model menu; Wiffletree never substitutes a model."
            )
        );
        assert_eq!(
            actor.host.session_runtime(&unpinned.id).unwrap().profile,
            None
        );
    }

    /// A coordinator-chosen Codex reviewer with queued input, Codex disabled, and every other
    /// session paused so nothing here can launch a real provider.
    fn codex_reviewer_on_a_disabled_provider() -> (tempfile::TempDir, Actor, Session, Session) {
        let (home, mut actor, first, tester, coordinator) = ticket_in_turn();
        let host = &mut actor.host;
        let ticket = host
            .create_ticket(&coordinator.id, &first.repository_id, "Review", "Do")
            .unwrap();
        let reviewer = host
            .assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        let sol = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "medium".into(),
        };
        host.pin(
            &reviewer.id,
            sol,
            Chooser::Coordinator {
                session_id: coordinator.id.clone(),
            },
            "Routine review",
        )
        .unwrap();
        for id in [&tester.id, &coordinator.id] {
            host.set_status(id, Status::Paused).unwrap();
        }
        set_codex(host, false);
        host.set_live(&reviewer.project_id, true).unwrap();
        (home, actor, reviewer, coordinator)
    }
    fn set_codex(host: &mut Host, enabled: bool) {
        let mut selection = host.model_selection().unwrap();
        selection
            .providers
            .get_mut(&Provider::Codex)
            .unwrap()
            .enabled = enabled;
        if !enabled {
            selection
                .role_providers
                .insert(Role::Tester, [Provider::Claude].into());
        }
        host.set_model_selection(&selection).unwrap();
    }
    fn turn_held_reports(host: &Host, from: &Session) -> i64 {
        host.db
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE sender=?1 AND id LIKE 'report:%:turn-held:%'",
                [&from.id],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_hold_report_that_failed_is_sent_on_a_later_pass() {
        let (_home, mut actor, reviewer, _) = codex_reviewer_on_a_disabled_provider();
        // The parent's queue refuses the report, say because it is full.
        actor
            .host
            .db
            .execute_batch(
                "CREATE TEMP TRIGGER refuse_report BEFORE INSERT ON messages
                     WHEN NEW.id LIKE 'report:%:turn-held:%'
                     BEGIN SELECT RAISE(ABORT,'queue full'); END;",
            )
            .unwrap();
        actor.schedule().unwrap();
        assert_eq!(
            actor.host.session(&reviewer.id).unwrap().status,
            Status::Blocked
        );
        assert_eq!(turn_held_reports(&actor.host, &reviewer), 0);
        actor
            .host
            .db
            .execute_batch("DROP TRIGGER refuse_report;")
            .unwrap();
        for _ in 0..2 {
            actor.schedule().unwrap();
            assert_eq!(turn_held_reports(&actor.host, &reviewer), 1);
        }
    }

    #[test]
    fn skip_drops_held_input_even_after_the_provider_is_back() {
        let (_home, mut actor, reviewer, _) = codex_reviewer_on_a_disabled_provider();
        actor.schedule().unwrap();
        assert!(actor.host.session_runtime(&reviewer.id).unwrap().held);
        // Stop the project, re-enable Codex, then Skip: the input must not run on resume.
        actor.host.set_live(&reviewer.project_id, false).unwrap();
        set_codex(&mut actor.host, true);
        assert_eq!(actor.host.hold_reason(&reviewer).unwrap(), None);
        let skipped = actor.host.respond(Request {
            version: PROTOCOL_VERSION,
            id: new_id(),
            command: Command::ReconcileSession {
                session_id: reviewer.id.clone(),
                retry: false,
            },
        });
        assert_eq!(skipped.error, None);
        assert_eq!(
            actor
                .host
                .message(&format!("assignment:{}", reviewer.id))
                .unwrap()
                .receipt,
            Receipt::Cancelled
        );
        assert!(!actor.host.session_runtime(&reviewer.id).unwrap().held);
        let queued: i64 = actor
            .host
            .db
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE recipient=?1 AND receipt='queued'",
                [&reviewer.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(queued, 0, "nothing left to run on resume");
    }

    /// The same ticket closed, so its worktree removal waits for the tester's turn.
    fn ticket_pending_removal() -> (tempfile::TempDir, Actor, Ticket, Session) {
        let (home, mut actor, ticket, tester, _) = ticket_in_turn();
        actor.host.close_ticket(&ticket.id).unwrap();
        assert!(Path::new(&ticket.worktree).exists());
        (home, actor, ticket, tester)
    }

    fn finish(actor: &mut Actor, session: &Session, reported: bool, error: Option<&str>) {
        let mut active = active_turn("run", vec![]);
        active.reported = reported;
        actor.active.insert(session.id.clone(), active);
        actor
            .provider_event(
                &session.id,
                "run",
                ProviderEvent::Finished {
                    error: error.map(Into::into),
                    usage: Value::Null,
                    cancelled: false,
                },
            )
            .unwrap();
    }

    #[test]
    fn an_interrupted_turn_under_an_archived_project_performs_its_pending_removal() {
        let (_home, mut actor, ticket, tester, coordinator) = ticket_in_turn();
        actor.host.set_archived(&coordinator.id, true).unwrap();
        assert!(Path::new(&ticket.worktree).exists());

        finish(&mut actor, &tester, false, Some("Turn interrupted"));

        assert!(!Path::new(&ticket.worktree).exists());
        assert!(actor.host.pending_worktree_removals().unwrap().is_empty());
    }

    #[test]
    fn a_turn_that_failed_to_start_performs_its_ticket_pending_removal() {
        let (_home, mut actor, ticket, tester) = ticket_pending_removal();
        let assignment = actor.host.messages(&tester.id, None, 10).unwrap().remove(0);

        actor
            .start_failed(&tester, &assignment, &anyhow::anyhow!("injected"))
            .unwrap();

        assert!(!Path::new(&ticket.worktree).exists());
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Disconnected
        );
    }

    #[test]
    fn shutdown_leaves_a_pending_removal_for_startup() {
        let (_home, mut actor, ticket, tester) = ticket_pending_removal();
        let mut active = active_turn("run", vec![]);
        active.reported = true;
        actor.active.insert(tester.id.clone(), active);

        actor.stop();

        assert!(Path::new(&ticket.worktree).exists());
        assert_eq!(
            actor.host.pending_worktree_removals().unwrap(),
            std::slice::from_ref(&ticket.id)
        );
    }

    #[test]
    fn a_finished_turn_performs_its_ticket_pending_removal() {
        let (_home, mut actor, ticket, tester) = ticket_pending_removal();

        finish(&mut actor, &tester, true, None);

        assert!(!Path::new(&ticket.worktree).exists());
        assert!(actor.host.pending_worktree_removals().unwrap().is_empty());
    }

    /// An idle coordinator with one child that reports to it.
    fn actor_with_child() -> (tempfile::TempDir, Actor, String, String) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let project = host.create_project("Reports").unwrap();
        let parent = host.sessions().unwrap().remove(0).id;
        let repo = home.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
        };
        git(&["init"]);
        git(&[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Initial",
        ]);
        let repo = host
            .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let child = host
            .create_session(
                &project.id,
                &parent,
                Some(&repo.id),
                "Child",
                Role::Implementer,
                Provider::Codex,
            )
            .unwrap()
            .id;
        (home, idle_actor(host).0, parent, child)
    }

    fn report(actor: &mut Actor, child: &str, kind: &str, body: &str) {
        actor
            .host
            .agent_tool(
                child,
                "report",
                json!({"message_id":body,"kind":kind,"body":body}),
            )
            .unwrap();
    }

    fn bodies(input: &[Message]) -> Vec<&str> {
        input
            .iter()
            .map(|m| m.body.lines().last().unwrap())
            .collect()
    }

    #[test]
    fn a_burst_of_progress_waits_for_one_wake_then_arrives_in_one_turn() {
        let (_home, mut actor, parent, child) = actor_with_child();
        for body in ["one", "two", "three"] {
            report(&mut actor, &child, "progress", body);
            assert!(actor.due_input(&parent).unwrap().is_none());
        }
        assert!(actor.wake_at.is_some());
        actor
            .host
            .db
            .execute(
                "UPDATE messages SET created_at=created_at-?1",
                [PROGRESS_BATCH_MS],
            )
            .unwrap();
        let input = actor.due_input(&parent).unwrap().unwrap();
        assert_eq!(bodies(&input), ["one", "two", "three"]);
        let session = actor.host.session(&parent).unwrap();
        let paired = input.iter().map(|m| (m, None)).collect::<Vec<_>>();
        let prompt = turn_prompt(&session, "", &paired, "");
        assert!(input.iter().all(|m| prompt.contains(&m.body)), "{prompt}");
    }

    #[test]
    fn a_timer_fire_wakes_at_once_and_is_the_only_self_message_taken_as_input() {
        let (_home, mut actor, parent, child) = actor_with_child();
        report(&mut actor, &child, "progress", "started");
        actor
            .host
            .db
            .execute(
                "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) SELECT 'output:run',project_id,?1,?1,'MY OWN REPLY','queued',?2 FROM sessions WHERE id=?1",
                params![parent, now()],
            )
            .unwrap();
        assert!(
            actor.due_input(&parent).unwrap().is_none(),
            "progress alone waits for its batch"
        );
        let session = actor.host.session(&parent).unwrap();
        let args = json!({"label":"check","prompt":"Check","every":"30m"});
        actor
            .host
            .schedule_tool(&session, "schedule", &args, now())
            .unwrap();
        actor.host.fire_due_schedules(now() + 31 * 60_000).unwrap();
        let input = actor.due_input(&parent).unwrap().unwrap();
        assert_eq!(input.len(), 2, "{:?}", bodies(&input));
        assert_eq!(bodies(&input)[0], "started");
        assert!(input[1].id.starts_with("timer:"));
    }

    #[test]
    fn a_timer_that_cannot_fire_arms_its_retry_not_a_wake_in_the_past() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        host.create_project("Full").unwrap();
        let session = host.sessions().unwrap().remove(0);
        let args = json!({"label":"check","prompt":"Check","every":"30m"});
        host.schedule_tool(&session, "schedule", &args, now())
            .unwrap();
        host.db
            .execute("UPDATE schedules SET next_fire_at=?1", [now() - 1_000])
            .unwrap();
        for n in 0..1024 {
            host.send(&format!("busy-{n}"), None, &session.id, "Busy")
                .unwrap();
        }
        let (mut actor, _, _) = idle_actor(host);
        actor.schedule().unwrap();
        let armed = actor.wake_at.unwrap();
        assert!(armed > now() + 50_000, "armed {} ms ahead", armed - now());
    }

    #[test]
    fn a_fire_queued_before_a_stop_that_turns_late_at_start_is_listed_as_missed() {
        let home = tempfile::tempdir().unwrap();
        let session = {
            let mut host = Host::open(home.path()).unwrap();
            host.create_project("Monitor").unwrap();
            let session = host.sessions().unwrap().remove(0);
            let args = json!({"label":"check","prompt":"Check","every":"30m"});
            host.schedule_tool(&session, "schedule", &args, now())
                .unwrap();
            // The first slot fires on time an hour ago and is still queued when the host stops.
            host.db
                .execute(
                    "UPDATE schedules SET first_at=first_at-?1,next_fire_at=next_fire_at-?1",
                    [90 * 60_000],
                )
                .unwrap();
            host.fire_due_schedules(now() - 59 * 60_000).unwrap();
            host.db
                .execute("UPDATE messages SET created_at=created_at-?1", [3_600_000])
                .unwrap();
            session
        };
        let host = Host::open(home.path()).unwrap();
        let (mut actor, _, _) = idle_actor(host);
        actor.schedule().unwrap();
        let input = actor.due_input(&session.id).unwrap().unwrap();
        assert_eq!(
            input.len(),
            1,
            "the missed slots fold into the waiting fire"
        );
        let prompt = actor.prompt(&session, &input).unwrap();
        assert!(prompt.contains("Missed: timer"), "{prompt}");
        assert!(
            prompt.contains("Timer status: late; 2 later slot(s)"),
            "{prompt}"
        );
    }

    #[test]
    fn a_one_shot_fire_queued_on_time_before_a_stop_is_late_and_missed_after_it() {
        let home = tempfile::tempdir().unwrap();
        let session = {
            let mut host = Host::open(home.path()).unwrap();
            host.create_project("Once").unwrap();
            let session = host.sessions().unwrap().remove(0);
            let args = json!({"label":"once","prompt":"Check once","at":"+10m"});
            host.schedule_tool(&session, "schedule", &args, now())
                .unwrap();
            // It fires on time an hour ago, then the host stops with it still queued.
            host.db
                .execute(
                    "UPDATE schedules SET first_at=first_at-?1,next_fire_at=next_fire_at-?1",
                    [70 * 60_000],
                )
                .unwrap();
            assert_eq!(
                host.fire_due_schedules(now() - 60 * 60_000).unwrap().next,
                None
            );
            host.db
                .execute("UPDATE messages SET created_at=created_at-?1", [3_600_000])
                .unwrap();
            session
        };
        let host = Host::open(home.path()).unwrap();
        let (mut actor, _, _) = idle_actor(host);
        actor.schedule().unwrap();
        let input = actor.due_input(&session.id).unwrap().unwrap();
        let prompt = actor.prompt(&session, &input).unwrap();
        for expected in [
            "The workspace host restarted",
            "Missed: timer",
            "\"once\"",
            "Timer status: late",
        ] {
            assert!(prompt.contains(expected), "{expected}\n{prompt}");
        }
    }

    #[test]
    fn a_report_that_needs_an_answer_wakes_at_once_with_the_whole_queue() {
        let (_home, mut actor, parent, child) = actor_with_child();
        report(&mut actor, &child, "progress", "started");
        report(&mut actor, &child, "blocked", "which schema?");
        report(&mut actor, &child, "failed", "tests fail");
        report(&mut actor, &child, "progress", "meanwhile");
        let input = actor.due_input(&parent).unwrap().unwrap();
        assert_eq!(
            bodies(&input),
            ["started", "which schema?", "tests fail", "meanwhile"]
        );
        assert!(actor.wake_at.is_none());
    }

    #[test]
    fn a_failed_turn_start_leaves_the_whole_batch_queued_for_retry() {
        let (_home, mut actor, parent, child) = actor_with_child();
        report(&mut actor, &child, "blocked", "one");
        report(&mut actor, &child, "progress", "two");
        let session = actor.host.session(&parent).unwrap();
        let runtime = actor.host.session_runtime(&parent).unwrap();
        let batch = actor.due_input(&parent).unwrap().unwrap();
        let receipts = |actor: &Actor| {
            batch
                .iter()
                .map(|m| actor.host.message(&m.id).unwrap().receipt)
                .collect::<Vec<_>>()
        };
        let faults = [
            format!(
                "BEFORE UPDATE OF receipt ON messages WHEN NEW.id='{}'",
                batch[1].id
            ),
            "BEFORE INSERT ON activity WHEN NEW.kind='turn_scheduled'".into(),
        ];
        for fault in faults {
            actor
                .host
                .db
                .execute(
                    &format!(
                        "CREATE TRIGGER fault {fault} BEGIN SELECT RAISE(ABORT,'injected'); END"
                    ),
                    [],
                )
                .unwrap();
            assert!(
                actor
                    .record_turn_start(&new_id(), &session, &batch, &json!({}), &runtime)
                    .is_err()
            );
            assert_eq!(
                receipts(&actor),
                [Receipt::Queued, Receipt::Queued],
                "{fault}"
            );
            assert_eq!(actor.host.session(&parent).unwrap().status, session.status);
            let runs: i64 = actor
                .host
                .db
                .query_row("SELECT COUNT(*) FROM provider_runs", [], |r| r.get(0))
                .unwrap();
            assert_eq!(runs, 0);
            actor.host.db.execute("DROP TRIGGER fault", []).unwrap();
        }
        let retry = actor.due_input(&parent).unwrap().unwrap();
        assert_eq!(retry, batch);
        actor
            .record_turn_start(&new_id(), &session, &retry, &json!({}), &runtime)
            .unwrap();
        assert_eq!(receipts(&actor), [Receipt::Delivered, Receipt::Delivered]);
    }

    #[test]
    fn a_human_message_shaped_like_progress_still_wakes_at_once() {
        let (_home, mut actor, parent, _child) = actor_with_child();
        actor
            .host
            .send("report:x", None, &parent, "[progress] imitation")
            .unwrap();
        assert_eq!(actor.due_input(&parent).unwrap().unwrap().len(), 1);
        assert!(actor.wake_at.is_none());
    }

    #[test]
    fn an_urgent_report_beyond_max_turn_messages_still_wakes_at_once() {
        let (_home, mut actor, parent, child) = actor_with_child();
        for n in 0..MAX_TURN_MESSAGES {
            report(&mut actor, &child, "progress", &format!("step {n}"));
        }
        report(&mut actor, &child, "blocked", "which schema?");
        let session = actor.host.session(&parent).unwrap();
        let runtime = actor.host.session_runtime(&parent).unwrap();
        let first = actor.due_input(&parent).unwrap().unwrap();
        assert_eq!(first.len(), MAX_TURN_MESSAGES);
        assert!(bodies(&first).iter().all(|b| b.starts_with("step ")));
        actor
            .record_turn_start(&new_id(), &session, &first, &json!({}), &runtime)
            .unwrap();
        let second = actor.due_input(&parent).unwrap().unwrap();
        assert_eq!(bodies(&second), ["which schema?"]);
        assert!(actor.wake_at.is_none());
    }

    /// A tester in a turn that started `started_at`, with one step under way.
    fn tester_in_long_turn(started_at: i64) -> (tempfile::TempDir, Actor, Session, Session) {
        let (home, mut actor, _, tester, coordinator) = ticket_in_turn();
        let mut active = Active::new(
            "run".into(),
            String::new(),
            Arc::new(AtomicBool::new(false)),
            vec![],
            started_at,
        );
        active.steps.push(Step {
            run_id: "run".into(),
            id: "test".into(),
            parent_id: None,
            kind: StepKind::Command,
            state: StepState::Running,
            title: "cargo test".into(),
            note: None,
            detail: None,
            omitted: 0,
            seq: 0,
            revision: 1,
            started_at,
            finished_at: None,
        });
        actor.active.insert(tester.id.clone(), active);
        (home, actor, tester, coordinator)
    }

    fn check_ins(actor: &Actor, parent: &Session) -> Vec<Message> {
        actor
            .host
            .messages(&parent.id, None, 100)
            .unwrap()
            .into_iter()
            .filter(|m| m.id.starts_with(CHECK_IN))
            .collect()
    }

    #[test]
    fn a_long_turn_checks_in_with_its_parent_every_half_hour_and_keeps_running() {
        let started = now();
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(started);
        let minutes = |m: i64| started + m * 60_000;

        actor.check_in_long_turns(minutes(29));
        assert!(check_ins(&actor, &coordinator).is_empty());
        assert_eq!(actor.wake_at, Some(minutes(30)));

        actor.check_in_long_turns(minutes(30));
        let first = check_ins(&actor, &coordinator);
        assert_eq!(first.len(), 1);
        for expected in [
            &format!("[check-in] {} on ticket \"Toolbar\"", tester.name),
            "for 30 minutes",
            "latest step: cargo test",
            &format!("stop_agents with session_ids [{}]", tester.id),
        ] {
            assert!(
                first[0].body.contains(expected),
                "{expected}\n{}",
                first[0].body
            );
        }
        assert_eq!(first[0].sender.as_deref(), Some(tester.id.as_str()));
        assert_eq!(first[0].receipt, Receipt::Queued);

        actor.check_in_long_turns(minutes(45));
        assert_eq!(check_ins(&actor, &coordinator).len(), 1);
        actor.check_in_long_turns(minutes(61));
        let second = check_ins(&actor, &coordinator);
        assert_eq!(second.len(), 2);
        assert!(
            second[1].body.contains("for 61 minutes"),
            "{}",
            second[1].body
        );
        assert!(!actor.active[&tester.id].cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn a_check_in_during_a_verification_round_wakes_the_coordinator_and_leaves_the_cycle_alone() {
        let (_home, mut actor, ticket, verifiers) = waiting_round(1);
        let started = now();
        actor.active.insert(
            verifiers[0].clone(),
            Active::new(
                "verifying".into(),
                String::new(),
                Arc::new(AtomicBool::new(false)),
                vec![],
                started,
            ),
        );
        let waking = |actor: &Actor| -> i64 {
            actor
                .host
                .db
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE recipient=?1 AND receipt='queued' AND quiet=0",
                    [&ticket.coordinator_id],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let before = waking(&actor);

        actor.check_in_long_turns(started + CHECK_IN_MS);

        let coordinator = actor.host.session(&ticket.coordinator_id).unwrap();
        let check_in = check_ins(&actor, &coordinator);
        assert_eq!(check_in.len(), 1, "the project coordinator receives it");
        assert_eq!(check_in[0].sender.as_deref(), Some(verifiers[0].as_str()));
        assert_eq!(waking(&actor), before + 1, "it wakes the coordinator");
        let after = actor.host.ticket(&ticket.id).unwrap();
        assert_eq!(after.state, ticket.state);
        assert_eq!(after.verification, ticket.verification);
    }

    /// Starts `count` turns of `session` one millisecond apart from `from`, as the scheduler
    /// records them.
    fn run_turns(actor: &mut Actor, session: &str, from: i64, count: i64) {
        let session = actor.host.session(session).unwrap();
        for n in 0..count {
            let run = new_id();
            actor
                .host
                .db
                .execute(
                    "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,'[]',?3,'{}')",
                    params![run, session.id, from + n],
                )
                .unwrap();
            actor.check_in_by_turns(&session, &run).unwrap();
        }
    }

    fn turn_check_ins(actor: &Actor, parent: &str) -> Vec<Message> {
        actor
            .host
            .messages(parent, None, 100)
            .unwrap()
            .into_iter()
            .filter(|m| m.id.starts_with(CHECK_IN) && m.id.ends_with(":turns"))
            .collect()
    }

    #[test]
    fn a_child_checks_in_with_its_parent_at_every_multiple_of_its_turns_without_it() {
        let (_home, mut actor, parent, child) = actor_with_child();
        let base = now() + 10_000;

        run_turns(&mut actor, &child, base, 24);
        assert!(turn_check_ins(&actor, &parent).is_empty());
        run_turns(&mut actor, &child, base + 24, 1);
        let first = turn_check_ins(&actor, &parent);
        assert_eq!(first.len(), 1);
        for expected in [
            "[check-in] Child has run 25 turns without a message from you.",
            "No reply yet.",
            &format!("stop_agents with session_ids [{child}]"),
        ] {
            assert!(
                first[0].body.contains(expected),
                "{expected}\n{}",
                first[0].body
            );
        }
        assert_eq!(first[0].sender.as_deref(), Some(child.as_str()));
        assert_eq!(first[0].receipt, Receipt::Queued);

        run_turns(&mut actor, &child, base + 25, 1);
        assert_eq!(turn_check_ins(&actor, &parent).len(), 1, "none at 26");
        run_turns(&mut actor, &child, base + 26, 24);
        assert_eq!(turn_check_ins(&actor, &parent).len(), 2, "another at 50");
        assert!(
            actor
                .host
                .open_attention(&actor.host.session(&child).unwrap().project_id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_parent_message_restarts_the_count_and_a_new_threshold_applies_at_once() {
        let (_home, mut actor, parent, child) = actor_with_child();
        let base = now() + 10_000;
        run_turns(&mut actor, &child, base, 24);
        actor
            .host
            .send("carry-on", Some(&parent), &child, "Carry on")
            .unwrap();
        actor
            .host
            .db
            .execute(
                "UPDATE messages SET created_at=?1 WHERE id='carry-on'",
                [base + 100],
            )
            .unwrap();
        let session = actor.host.session(&child).unwrap();
        actor
            .host
            .append_output(&session, "earlier", "Wrote the parser")
            .unwrap();

        run_turns(&mut actor, &child, base + 200, 24);
        assert!(
            turn_check_ins(&actor, &parent).is_empty(),
            "the count restarted"
        );

        actor.host.set_check_ins(10, 100).unwrap();
        run_turns(&mut actor, &child, base + 300, 6);
        let check_ins = turn_check_ins(&actor, &parent);
        assert_eq!(check_ins.len(), 1, "at 30, the next multiple of 10");
        for expected in [
            "has run 30 turns since you last messaged it at ",
            "Last reply: Wrote the parser",
        ] {
            assert!(
                check_ins[0].body.contains(expected),
                "{expected}\n{}",
                check_ins[0].body
            );
        }
    }

    #[test]
    fn the_coordinator_checks_in_with_the_human_until_the_human_steps_in() {
        let (_home, mut actor, root, child) = actor_with_child();
        let project = actor.host.session(&root).unwrap().project_id;
        actor.host.set_live(&project, true).unwrap();
        let items = |actor: &Actor| {
            actor
                .host
                .open_attention(&project)
                .unwrap()
                .into_iter()
                .filter(|a| a.operation_id.starts_with(TURN_COUNT))
                .collect::<Vec<_>>()
        };
        let base = now() + 10_000;

        run_turns(&mut actor, &child, base, 100);
        assert!(items(&actor).is_empty(), "a child's turns do not count");
        run_turns(&mut actor, &root, base, 99);
        assert!(items(&actor).is_empty());
        run_turns(&mut actor, &root, base + 99, 1);
        let first = items(&actor);
        assert_eq!(first.len(), 1);
        assert!(
            first[0]
                .prompt
                .contains("has run 100 turns since you last stepped in"),
            "{}",
            first[0].prompt
        );
        let root_session = actor.host.session(&root).unwrap();
        actor.settle_check_ins(&root_session).unwrap();
        assert_eq!(items(&actor).len(), 1, "it outlives the turn");
        run_turns(&mut actor, &root, base + 100, 100);
        let second = items(&actor);
        assert_eq!(second.len(), 1, "a newer one supersedes it");
        assert_ne!(second[0].id, first[0].id);
        assert!(actor.host.live_projects().unwrap().contains(&project));

        command(
            &mut actor,
            Command::Send {
                id: "human".into(),
                sender: None,
                recipient: root.clone(),
                body: "Keep going".into(),
                attachments: vec![],
            },
        )
        .unwrap();
        assert!(items(&actor).is_empty(), "stepping in clears it");
        run_turns(&mut actor, &root, base + 200, 99);
        assert!(items(&actor).is_empty(), "and restarts the count");
    }

    #[test]
    fn the_coordinator_can_neither_see_nor_close_its_turn_count_check_in() {
        let (_home, mut actor, root, _) = actor_with_child();
        run_turns(&mut actor, &root, now() + 10_000, 100);
        let session = actor.host.session(&root).unwrap();
        let item = actor
            .host
            .open_attention(&session.project_id)
            .unwrap()
            .into_iter()
            .find(|a| a.operation_id.starts_with(TURN_COUNT))
            .unwrap();

        assert!(actor.host.open_questions(&session).unwrap().is_empty());
        let refused = actor
            .host
            .agent_tool(
                &root,
                "close_question",
                json!({"request_id":item.operation_id,"resolution":"Settled"}),
            )
            .unwrap_err();

        assert!(
            refused
                .to_string()
                .contains("Check-ins close when the human steps in"),
            "{refused}"
        );
        let open = actor.host.open_attention(&session.project_id).unwrap();
        assert!(open.iter().any(|a| a.id == item.id), "it stays open");
    }

    #[test]
    fn a_turn_started_in_the_same_millisecond_as_the_parent_message_counts() {
        let (_home, mut actor, parent, child) = actor_with_child();
        let at = now() + 10_000;
        actor.host.send("go", Some(&parent), &child, "Go").unwrap();
        actor
            .host
            .db
            .execute("UPDATE messages SET created_at=?1 WHERE id='go'", [at])
            .unwrap();

        run_turns(&mut actor, &child, at, 25);

        assert_eq!(turn_check_ins(&actor, &parent).len(), 1);
    }

    #[test]
    fn a_turn_count_check_in_during_a_round_wakes_the_coordinator_and_leaves_the_cycle_alone() {
        let (_home, mut actor, ticket, verifiers) = waiting_round(1);
        let waking = |actor: &Actor| -> i64 {
            actor
                .host
                .db
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE recipient=?1 AND receipt='queued' AND quiet=0",
                    [&ticket.coordinator_id],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let before = waking(&actor);

        run_turns(&mut actor, &verifiers[0], now() + 10_000, 25);

        let check_ins = turn_check_ins(&actor, &ticket.coordinator_id);
        assert_eq!(check_ins.len(), 1);
        assert!(
            check_ins[0].body.contains(" on ticket \"Toolbar\""),
            "{}",
            check_ins[0].body
        );
        assert_eq!(waking(&actor), before + 1, "it wakes the coordinator");
        let after = actor.host.ticket(&ticket.id).unwrap();
        assert_eq!(after.state, ticket.state);
        assert_eq!(after.verification, ticket.verification);
    }

    #[test]
    fn a_long_turn_without_a_parent_asks_the_human_once_at_a_time() {
        let (_home, mut actor, session, _) = actor_with_turn();
        let root = actor.host.session(&session).unwrap();
        let started = actor.active[&session].started_at;
        let open = |actor: &Actor| {
            actor
                .host
                .open_attention(&root.project_id)
                .unwrap()
                .into_iter()
                .filter(|a| a.operation_id.starts_with(CHECK_IN))
                .collect::<Vec<_>>()
        };

        actor.check_in_long_turns(started + CHECK_IN_MS);
        let first = open(&actor);
        assert_eq!(first.len(), 1);
        assert!(
            first[0].prompt.contains("for 30 minutes"),
            "{}",
            first[0].prompt
        );
        actor.check_in_long_turns(started + 2 * CHECK_IN_MS);
        let second = open(&actor);
        assert_eq!(second.len(), 1, "the newer check-in replaces the older");
        assert!(second[0].prompt.contains("for 60 minutes"));

        let run = actor.active[&session].run.clone();
        actor
            .provider_event(
                &session,
                &run,
                ProviderEvent::Finished {
                    error: None,
                    usage: Value::Null,
                    cancelled: false,
                },
            )
            .unwrap();
        assert!(
            open(&actor).is_empty(),
            "a finished turn clears its check-in"
        );
    }

    #[test]
    fn a_cancelled_long_turn_winding_down_arms_no_wake_in_the_past() {
        let (_home, mut actor, session, run) = actor_with_turn();
        let mut active = active_turn(&run, vec![]);
        active.started_at = now() - CHECK_IN_MS - 60_000;
        active.next_check_in = active.started_at + CHECK_IN_MS;
        active.cancel.store(true, Ordering::Relaxed);
        actor.active.insert(session, active);

        actor.check_in_long_turns(now());
        assert_eq!(actor.wake_at, None);
    }

    #[test]
    fn a_host_restart_clears_check_ins_for_turns_it_cut_off() {
        let (_home, mut actor, session, _) = actor_with_turn();
        let started = actor.active[&session].started_at;
        actor.check_in_long_turns(started + CHECK_IN_MS);
        let project = actor.host.session(&session).unwrap().project_id;
        assert_eq!(actor.host.open_attention(&project).unwrap().len(), 1);

        recover_unfinished_turns(&mut actor.host).unwrap();
        assert!(actor.host.open_attention(&project).unwrap().is_empty());
    }

    #[test]
    fn check_in_cleanup_leaves_human_questions_alone() {
        let (_home, mut actor, session, run) = actor_with_turn();
        let project = actor.host.session(&session).unwrap().project_id;
        let ask = |request_id: &str| json!({"request_id":request_id,"question":"Deploy?"});
        actor
            .host
            .agent_tool(&session, "ask_user", ask("check-in-deploy"))
            .unwrap();
        let refused = actor
            .host
            .agent_tool(&session, "ask_user", ask("check-in:deploy"))
            .unwrap_err();
        assert!(refused.to_string().contains("reserved"), "{refused}");
        let command = Command::RequestAttention {
            session_id: session.clone(),
            host: "local".into(),
            operation_id: "check-in:deploy".into(),
            prompt: "Deploy?".into(),
        };
        assert!(actor.host.execute(command).is_err());
        let open = |actor: &Actor| {
            actor
                .host
                .open_attention(&project)
                .unwrap()
                .into_iter()
                .map(|a| a.operation_id)
                .collect::<Vec<_>>()
        };

        let started = actor.active[&session].started_at;
        actor.check_in_long_turns(started + CHECK_IN_MS);
        assert_eq!(open(&actor).len(), 2);
        actor
            .provider_event(
                &session,
                &run,
                ProviderEvent::Finished {
                    error: None,
                    usage: Value::Null,
                    cancelled: false,
                },
            )
            .unwrap();
        assert_eq!(open(&actor), ["check-in-deploy"]);
        recover_unfinished_turns(&mut actor.host).unwrap();
        assert_eq!(open(&actor), ["check-in-deploy"]);
    }

    #[test]
    fn stop_agents_keeps_every_child_paused_while_input_queues() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let idle = actor
            .host
            .create_session(
                &coordinator.project_id,
                &coordinator.id,
                None,
                "Idle",
                Role::Reviewer,
                Provider::Codex,
            )
            .unwrap();
        let args = json!({"reason":"Wrong approach"});
        let stopped = actor.stop_agents(&coordinator.id, &args).unwrap();
        let result = |stopped: &Value, id: &str| {
            stopped["agents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["session_id"] == id)
                .unwrap()["result"]
                .clone()
        };
        assert_eq!(result(&stopped, &tester.id), "stopped");
        assert_eq!(result(&stopped, &idle.id), "paused_idle");
        assert!(actor.active[&tester.id].cancel.load(Ordering::Relaxed));
        actor
            .provider_event(
                &tester.id,
                "run",
                ProviderEvent::Finished {
                    error: Some(provider::Cancelled.to_string()),
                    usage: Value::Null,
                    cancelled: true,
                },
            )
            .unwrap();
        for child in [&tester, &idle] {
            assert_eq!(
                actor.host.session(&child.id).unwrap().status,
                Status::Paused
            );
        }

        // Neither a message nor a timer-fired coordinator turn starts a stopped child's turn.
        actor.host.set_live(&coordinator.project_id, true).unwrap();
        for child in [&tester, &idle] {
            actor
                .host
                .send(&new_id(), Some(&coordinator.id), &child.id, "Work")
                .unwrap();
        }
        actor.schedule().unwrap();
        assert!(actor.active.is_empty());

        let again = actor.stop_agents(&coordinator.id, &args).unwrap();
        assert_eq!(result(&again, &tester.id), "already_paused");
        assert_eq!(result(&again, &idle.id), "already_paused");
    }

    /// Runs a client command through the actor, as the desktop sends it.
    fn command(actor: &mut Actor, command: Command) -> std::result::Result<Value, String> {
        let (reply, replies) = async_channel::bounded(1);
        actor.handle(Event::Command(command, reply));
        replies.try_recv().unwrap()
    }

    fn pause(actor: &mut Actor, session: &Session) {
        let paused = Command::SetStatus {
            session_id: session.id.clone(),
            status: Status::Paused,
        };
        command(actor, paused).unwrap();
    }

    fn cancelled_turn_ends(actor: &mut Actor, session: &Session) {
        actor
            .provider_event(
                &session.id,
                "run",
                ProviderEvent::Finished {
                    error: Some(provider::Cancelled.to_string()),
                    usage: Value::Null,
                    cancelled: true,
                },
            )
            .unwrap();
    }

    /// What the parent was told about its children's turns ending or being stopped.
    fn stop_notices(actor: &Actor, parent: &Session) -> Vec<String> {
        actor
            .host
            .messages(&parent.id, None, 100)
            .unwrap()
            .into_iter()
            .filter(|m| m.id.starts_with("human-stop:") || m.id.starts_with("turn-result:"))
            .map(|m| m.body)
            .collect()
    }

    #[test]
    fn resume_agent_reconciles_held_input_like_retry_and_skip() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let resume = |actor: &mut Actor, args: Value| {
            actor
                .resume_agent(&coordinator.id, &args)
                .map_err(|e| e.to_string())
        };
        let refused = resume(&mut actor, json!({"session_id":tester.id})).unwrap_err();
        assert!(refused.contains("still finishing its turn"), "{refused}");

        // The human stops its turn; its input is held and the parent is told once.
        actor
            .host
            .send("work", Some(&coordinator.id), &tester.id, "Work")
            .unwrap();
        actor.active.get_mut(&tester.id).unwrap().messages = vec!["work".into()];
        pause(&mut actor, &tester);
        cancelled_turn_ends(&mut actor, &tester);
        let notices = stop_notices(&actor, &coordinator);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].starts_with(&format!("{} was stopped by the human", tester.name)),
            "{notices:?}"
        );

        let refused = resume(&mut actor, json!({"session_id":tester.id})).unwrap_err();
        assert!(refused.contains("1 held input message"), "{refused}");
        // A resume that fails changes nothing: the child stays stopped with its input held.
        for invalid in [
            json!({"session_id":tester.id,"held":"skip","message":""}),
            json!({"session_id":tester.id,"held":"later"}),
        ] {
            resume(&mut actor, invalid).unwrap_err();
            assert_eq!(
                actor.host.session(&tester.id).unwrap().status,
                Status::Paused
            );
            assert_eq!(actor.host.message("work").unwrap().receipt, Receipt::Held);
        }

        let resumed = resume(&mut actor, json!({"session_id":tester.id,"held":"skip"})).unwrap();
        assert_eq!(resumed["stopped_by"], "human");
        assert!(
            resumed["note"]
                .as_str()
                .unwrap()
                .contains("stopped by the human"),
            "{resumed}"
        );
        assert_eq!(
            actor.host.message("work").unwrap().receipt,
            Receipt::Cancelled
        );
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Ready
        );
        let runtime = actor.host.session_runtime(&tester.id).unwrap();
        assert_eq!((runtime.last_error, runtime.stopped_by), (None, None));

        let refused = resume(&mut actor, json!({"session_id":tester.id})).unwrap_err();
        assert!(refused.contains("not stopped"), "{refused}");
        let sibling = actor
            .host
            .create_session(
                &coordinator.project_id,
                &coordinator.id,
                None,
                "Sibling",
                Role::Implementer,
                Provider::Codex,
            )
            .unwrap();
        let refused = actor
            .resume_agent(&sibling.id, &json!({"session_id":tester.id}))
            .unwrap_err()
            .to_string();
        assert_eq!(
            refused,
            format!("Only {}'s direct parent can resume it", tester.name)
        );
    }

    #[test]
    fn a_human_pause_of_an_idle_child_tells_its_parent_and_the_held_hint_names_the_human() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        actor.active.remove(&tester.id);
        pause(&mut actor, &tester);
        pause(&mut actor, &tester);
        let notices = stop_notices(&actor, &coordinator);
        assert_eq!(notices.len(), 1, "told once: {notices:?}");
        assert!(
            notices[0].starts_with(&format!("{} was stopped by the human.", tester.name)),
            "{notices:?}"
        );

        let args = json!({"recipient":tester.id,"message_id":"next","body":"Next"});
        let sent = actor
            .host
            .agent_tool(&coordinator.id, "send_message", args)
            .unwrap();
        assert_eq!(sent["receipt"], "queued_while_held");
        assert!(
            sent["hint"]
                .as_str()
                .unwrap()
                .contains("stopped by the human"),
            "{sent}"
        );

        // A parent's stop never relabels the human's.
        actor
            .stop_agents(&coordinator.id, &json!({"reason":"Tidy up"}))
            .unwrap();
        assert_eq!(
            actor.host.session_runtime(&tester.id).unwrap().stopped_by,
            Some(Stopper::Human)
        );
    }

    #[test]
    fn a_human_stop_reaches_a_parent_whose_queue_is_full() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let idle = actor
            .host
            .create_session(
                &coordinator.project_id,
                &coordinator.id,
                None,
                "Idle",
                Role::Reviewer,
                Provider::Codex,
            )
            .unwrap();
        for n in 0..1024 {
            actor
                .host
                .send(&format!("busy-{n}"), None, &coordinator.id, "Busy")
                .unwrap();
        }

        pause(&mut actor, &idle);
        pause(&mut actor, &tester);
        cancelled_turn_ends(&mut actor, &tester);
        pause(&mut actor, &idle);
        let notices = stop_notices(&actor, &coordinator);
        assert_eq!(notices.len(), 2, "one per child: {notices:?}");
        for child in [&idle, &tester] {
            let named = format!("{} was stopped by the human", child.name);
            assert!(notices.iter().any(|n| n.starts_with(&named)), "{notices:?}");
        }

        // Agents still cannot flood the full queue.
        let args = json!({"recipient":coordinator.id,"message_id":"more","body":"More"});
        let refused = actor
            .host
            .agent_tool(&idle.id, "send_message", args)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("Recipient queue is full"), "{refused}");
    }

    #[test]
    fn a_human_stop_whose_notice_fails_still_names_the_human() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        actor.active.remove(&tester.id);
        actor
            .host
            .db
            .execute_batch(
                "CREATE TEMP TRIGGER refuse_notice BEFORE INSERT ON messages
                     WHEN NEW.id LIKE 'human-stop:%'
                     BEGIN SELECT RAISE(ABORT,'disk full'); END;",
            )
            .unwrap();
        pause(&mut actor, &tester);
        assert!(stop_notices(&actor, &coordinator).is_empty());
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Paused
        );
        assert_eq!(
            actor.host.session_runtime(&tester.id).unwrap().stopped_by,
            Some(Stopper::Human)
        );

        let args = json!({"recipient":tester.id,"message_id":"next","body":"Next"});
        let sent = actor
            .host
            .agent_tool(&coordinator.id, "send_message", args)
            .unwrap();
        assert!(
            sent["hint"]
                .as_str()
                .unwrap()
                .contains("stopped by the human"),
            "{sent}"
        );
        let resumed = actor
            .resume_agent(&coordinator.id, &json!({"session_id":tester.id}))
            .unwrap();
        assert!(
            resumed["note"]
                .as_str()
                .unwrap()
                .contains("stopped by the human"),
            "{resumed}"
        );
    }

    #[test]
    fn a_human_pause_overrides_a_parent_stop_still_winding_down() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        actor
            .host
            .send("work", Some(&coordinator.id), &tester.id, "Work")
            .unwrap();
        actor.active.get_mut(&tester.id).unwrap().messages = vec!["work".into()];
        let args = json!({"session_ids":[tester.id],"reason":"Wrong approach"});
        actor.stop_agents(&coordinator.id, &args).unwrap();
        pause(&mut actor, &tester);
        cancelled_turn_ends(&mut actor, &tester);

        assert_eq!(actor.host.message("work").unwrap().receipt, Receipt::Held);
        let notices = stop_notices(&actor, &coordinator);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("stopped by the human"), "{notices:?}");
        assert_eq!(
            actor.host.session_runtime(&tester.id).unwrap().stopped_by,
            Some(Stopper::Human)
        );
    }

    #[test]
    fn a_human_project_stop_tells_the_parent_once_per_running_child() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let stop = Command::SetLive {
            project_id: coordinator.project_id.clone(),
            enabled: false,
        };
        command(&mut actor, stop).unwrap();
        cancelled_turn_ends(&mut actor, &tester);
        let notices = stop_notices(&actor, &coordinator);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].contains("its interrupted turn's input is held"),
            "{notices:?}"
        );
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Disconnected
        );
    }

    #[test]
    fn a_report_from_a_turn_being_stopped_leaves_the_child_paused() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let args = json!({"session_ids":[tester.id],"reason":"Wrong approach"});
        actor.stop_agents(&coordinator.id, &args).unwrap();
        let report = json!({"message_id":"late","kind":"completed","body":"Done anyway"});
        actor.host.agent_tool(&tester.id, "report", report).unwrap();
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Paused
        );
        cancelled_turn_ends(&mut actor, &tester);
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Paused
        );

        actor.host.set_live(&coordinator.project_id, true).unwrap();
        actor
            .host
            .send("queued", Some(&coordinator.id), &tester.id, "Work")
            .unwrap();
        // The report woke the coordinator; keep its turn out of this test.
        actor
            .host
            .set_status(&coordinator.id, Status::Paused)
            .unwrap();
        actor.schedule().unwrap();
        assert!(
            actor.active.is_empty(),
            "no turn starts without resume_agent"
        );
    }

    #[test]
    fn stop_and_resume_refuse_archived_children_and_unknown_arguments() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        actor.active.remove(&tester.id);
        let archived = actor
            .host
            .create_session(
                &coordinator.project_id,
                &coordinator.id,
                None,
                "Archived",
                Role::Reviewer,
                Provider::Codex,
            )
            .unwrap();
        actor.host.set_status(&archived.id, Status::Paused).unwrap();
        actor.host.set_archived(&archived.id, true).unwrap();

        let stop = json!({"session_ids":[tester.id, archived.id],"reason":"Stop"});
        let refused = actor
            .stop_agents(&coordinator.id, &stop)
            .unwrap_err()
            .to_string();
        assert_eq!(
            refused,
            "Archived is archived; restore it before you stop it"
        );
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Ready,
            "a refused stop changes nothing"
        );
        let refused = actor
            .resume_agent(&coordinator.id, &json!({"session_id":archived.id}))
            .unwrap_err()
            .to_string();
        assert_eq!(
            refused,
            "Archived is archived; restore it before you resume it"
        );

        // A singular session_id must not widen into stopping every child.
        let singular = json!({"session_id":tester.id,"reason":"Stop"});
        let refused = actor
            .stop_agents(&coordinator.id, &singular)
            .unwrap_err()
            .to_string();
        assert_eq!(
            refused,
            "stop_agents takes session_ids, reason, not session_id"
        );
        let null = json!({"session_ids":null,"reason":"Stop"});
        actor.stop_agents(&coordinator.id, &null).unwrap_err();
        for child in [&tester, &archived] {
            assert_eq!(
                actor.host.session(&child.id).unwrap().status,
                if child.id == tester.id {
                    Status::Ready
                } else {
                    Status::Paused
                },
                "a refused stop changes no child"
            );
        }
    }

    #[test]
    fn only_the_direct_parent_can_stop_a_turn() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let sibling = actor
            .host
            .create_session(
                &coordinator.project_id,
                &coordinator.id,
                None,
                "Sibling",
                Role::Implementer,
                Provider::Codex,
            )
            .unwrap();
        // A ticket agent's parent is the project coordinator itself.
        assert_eq!(coordinator.role, Role::ProjectOrchestrator);
        assert_eq!(tester.parent_id.as_deref(), Some(coordinator.id.as_str()));
        let args = json!({"session_ids":[tester.id],"reason":"Wrong approach"});
        for caller in [&sibling.id, &tester.id] {
            let error = actor.stop_agents(caller, &args).unwrap_err().to_string();
            assert_eq!(
                error,
                format!("Only {}'s direct parent can stop it", tester.name)
            );
        }
        assert!(!actor.active[&tester.id].cancel.load(Ordering::Relaxed));

        // The parent calls the tool from its own turn.
        let mut turn = active_turn("coordinating", vec![]);
        turn.token = "parent-token".into();
        actor.active.insert(coordinator.id.clone(), turn);
        let (reply, replies) = async_channel::bounded(1);
        actor.handle(Event::Tool {
            token: "parent-token".into(),
            name: "stop_agents".into(),
            args,
            reply,
        });
        let reply = replies.try_recv().unwrap().unwrap();
        assert_eq!(reply["agents"][0]["result"], "stopped");
        assert!(actor.active[&tester.id].cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn a_turn_its_parent_stopped_completes_its_input_stays_paused_and_tells_the_next_turn_why() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        let input = actor
            .host
            .send("work", Some(&coordinator.id), &tester.id, "Work")
            .unwrap();
        actor.active.get_mut(&tester.id).unwrap().messages = vec![input.id.clone()];
        let args = json!({"session_ids":[tester.id],"reason":"Use the staging data instead"});
        actor.stop_agents(&coordinator.id, &args).unwrap();
        actor
            .provider_event(
                &tester.id,
                "run",
                ProviderEvent::Finished {
                    error: Some(provider::Cancelled.to_string()),
                    usage: Value::Null,
                    cancelled: true,
                },
            )
            .unwrap();

        let (outcome, reason): (String, String) = actor
            .host
            .db
            .query_row(
                "SELECT outcome,json_extract(detail,'$.stop_reason') FROM provider_runs WHERE id='run'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (outcome.as_str(), reason.as_str()),
            ("stopped_by_parent", "Use the staging data instead")
        );
        assert_eq!(
            actor.host.message("work").unwrap().receipt,
            Receipt::Completed
        );
        assert_eq!(
            actor.host.session(&tester.id).unwrap().status,
            Status::Paused,
            "it stays stopped until its parent resumes it"
        );
        assert_eq!(
            actor.host.session_runtime(&tester.id).unwrap().last_error,
            None
        );
        let parent_inbox = actor.host.messages(&coordinator.id, None, 100).unwrap();
        assert!(
            !parent_inbox
                .iter()
                .any(|m| m.id.starts_with("turn-result:")),
            "the parent that stopped it is not told again"
        );

        actor
            .host
            .send("next", Some(&coordinator.id), &tester.id, "Next")
            .unwrap();
        let next = vec![actor.host.message("next").unwrap()];
        let prompt = actor.prompt(&tester, &next).unwrap();
        for expected in [
            "Your parent stopped your previous turn run",
            "Reason: Use the staging data instead",
            "not retried",
        ] {
            assert!(prompt.contains(expected), "{expected}\n{prompt}");
        }
    }

    #[test]
    fn a_real_failure_racing_a_parent_stop_keeps_its_error_and_held_input() {
        let (_home, mut actor, tester, coordinator) = tester_in_long_turn(now());
        actor
            .host
            .send("work", Some(&coordinator.id), &tester.id, "Work")
            .unwrap();
        actor.active.get_mut(&tester.id).unwrap().messages = vec!["work".into()];
        let args = json!({"session_ids":[tester.id],"reason":"Stop"});
        actor.stop_agents(&coordinator.id, &args).unwrap();
        // The provider failed on its own before it saw the cancel flag.
        let crash = "Provider exited without a successful terminal result (exit status: 1)";
        actor
            .provider_event(
                &tester.id,
                "run",
                ProviderEvent::Finished {
                    error: Some(crash.into()),
                    usage: Value::Null,
                    cancelled: false,
                },
            )
            .unwrap();
        assert_eq!(actor.host.message("work").unwrap().receipt, Receipt::Held);
        assert_eq!(
            actor
                .host
                .session_runtime(&tester.id)
                .unwrap()
                .last_error
                .as_deref(),
            Some(crash)
        );
        let outcome: String = actor
            .host
            .db
            .query_row(
                "SELECT outcome FROM provider_runs WHERE id='run'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outcome, "failed");
    }

    fn update(id: &str, kind: StepKind, state: StepState, title: &str) -> StepUpdate {
        StepUpdate {
            id: id.into(),
            parent_id: None,
            kind,
            state,
            title: title.into(),
            note: None,
            detail: Some(title.into()),
        }
    }

    #[test]
    fn streamed_text_is_written_when_it_starts_and_ends() {
        let (_home, mut actor, session, run) = actor_with_turn();
        let writes = actor.host.db.total_changes();
        let mut text = String::new();
        for _ in 0..1_000 {
            text.push_str("w ");
            let delta = update("text", StepKind::Narration, StepState::Running, &text);
            actor
                .provider_event(&session, &run, ProviderEvent::Step(delta))
                .unwrap();
        }
        let done = update("text", StepKind::Narration, StepState::Succeeded, &text);
        actor
            .provider_event(&session, &run, ProviderEvent::Step(done))
            .unwrap();
        assert_eq!(actor.host.db.total_changes() - writes, 2);
        // A reader sees every delta live, and the store has the finished text.
        let live = actor.steps(&session, None, 0).unwrap();
        assert_eq!((live.revision, live.steps.len()), (1_001, 1));
        let stored = actor.host.steps(&session, Some(&run), 0).unwrap();
        assert_eq!(stored.steps[0].detail.as_deref(), Some(text.as_str()));
        assert!(
            actor
                .steps(&session, None, live.revision)
                .unwrap()
                .steps
                .is_empty()
        );
    }

    #[test]
    fn a_turn_left_unfinished_is_reported_as_an_unexpected_stop() {
        let (_home, mut actor, session, run) = actor_with_turn();
        recover_unfinished_turns(&mut actor.host).unwrap();
        let runtime = actor.host.session_runtime(&session).unwrap();
        assert_eq!(runtime.last_error.as_deref(), Some(STOPPED_UNEXPECTEDLY));
        assert_eq!(
            actor.host.session(&session).unwrap().status,
            Status::Disconnected
        );
        let (outcome, error): (String, String) = actor
            .host
            .db
            .query_row(
                "SELECT outcome,json_extract(detail,'$.result.error') FROM provider_runs WHERE id=?1",
                [&run],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (outcome.as_str(), error.as_str()),
            ("interrupted", STOPPED_UNEXPECTEDLY)
        );
    }

    #[test]
    fn a_retried_message_names_the_turn_that_was_cut_off() {
        let (_home, mut actor, session, run) = actor_with_turn();
        actor
            .host
            .db
            .execute(
                "UPDATE provider_runs SET messages='[\"m1\"]',started_at=1791400582000 WHERE id=?1",
                [&run],
            )
            .unwrap();
        let stopped = ProviderEvent::Finished {
            error: Some(QUIT_DURING_TURN.into()),
            usage: Value::Null,
            cancelled: false,
        };
        actor.provider_event(&session, &run, stopped).unwrap();
        let note = interrupted_turn_note(&actor.host, "m1").unwrap();
        assert!(note.contains(&run), "{note}");
        assert!(note.contains("2026-10-07T19:16:22Z"), "{note}");
        assert!(interrupted_turn_note(&actor.host, "m2").unwrap().is_empty());
    }

    #[test]
    fn the_first_turn_after_a_restart_names_the_cut_off_turn_its_timers_and_missed_fires() {
        let home = tempfile::tempdir().unwrap();
        let (session, run, timer) = {
            let mut host = Host::open(home.path()).unwrap();
            host.create_project("Monitor").unwrap();
            let session = host.sessions().unwrap().remove(0);
            let args =
                json!({"label":"check","prompt":"Check the service","every":"30m","until":"+3h"});
            let timer = host
                .schedule_tool(&session, "schedule", &args, now())
                .unwrap();
            let run = new_id();
            host.db
                .execute(
                    "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,'[]',?3,'{}')",
                    params![run, session.id, now()],
                )
                .unwrap();
            // The host dies mid-turn and stays down past the first two slots.
            host.db
                .execute(
                    "UPDATE schedules SET first_at=first_at-?1,next_fire_at=next_fire_at-?1,until=until-?1",
                    [65 * 60_000],
                )
                .unwrap();
            (session, run, timer["id"].as_str().unwrap().to_owned())
        };
        let mut host = Host::open(home.path()).unwrap();
        recover_unfinished_turns(&mut host).unwrap();
        let (mut actor, _, _) = idle_actor(host);
        actor.schedule().unwrap();
        let input = actor.due_input(&session.id).unwrap().unwrap();
        assert_eq!(input.len(), 1, "one fire for both missed slots");
        assert!(
            !input[0].body.contains("late"),
            "the stored body never changes"
        );
        let prompt = actor.prompt(&session, &input).unwrap();
        for expected in [
            format!("Interrupted turn {run}"),
            format!("Timer {timer} \"check\", every 30m until"),
            format!("Missed: timer {timer} \"check\""),
            "Sender: your timer".into(),
            "Timer status: late; 1 later slot(s)".into(),
            "Check the service".into(),
        ] {
            assert!(prompt.contains(&expected), "{expected}\n{prompt}");
        }
        assert!(!prompt.contains("Sender: human"), "{prompt}");
        actor.resumed.insert(session.id.clone());
        assert!(
            !actor
                .prompt(&session, &input)
                .unwrap()
                .contains("restarted")
        );
    }

    #[test]
    fn a_failed_turn_interrupts_its_running_steps_and_keeps_narration_out_of_chat() {
        let (_home, mut actor, session, run) = actor_with_turn();
        for step in [
            update(
                "say",
                StepKind::Narration,
                StepState::Succeeded,
                "Running the tests:",
            ),
            update("test", StepKind::Command, StepState::Running, "cargo test"),
        ] {
            actor
                .provider_event(&session, &run, ProviderEvent::Step(step))
                .unwrap();
        }
        let failure = ProviderEvent::Finished {
            error: Some("Turn interrupted".into()),
            usage: Value::Null,
            cancelled: false,
        };
        actor.provider_event(&session, &run, failure).unwrap();
        let page = actor.host.steps(&session, Some(&run), 0).unwrap();
        let states: Vec<_> = page.steps.iter().map(|s| s.state).collect();
        assert_eq!(states, [StepState::Succeeded, StepState::Interrupted]);
        assert!(!page.running);
        let transcript = actor.host.messages(&session, None, 100).unwrap();
        assert!(
            transcript
                .iter()
                .all(|m| !m.body.contains("Running the tests"))
        );
    }

    #[test]
    fn a_burst_of_steps_leaves_one_step_signal_and_no_state_signal() {
        let (_home, mut actor, session, run, changes, step_changes) = actor_with_signals();
        for n in 0..500 {
            let event = Event::Provider {
                session: session.clone(),
                run: run.clone(),
                event: ProviderEvent::Step(update(
                    &n.to_string(),
                    StepKind::Read,
                    StepState::Succeeded,
                    "a",
                )),
            };
            let state_changed = actor.handle(event);
            actor.signal(state_changed);
        }
        assert_eq!((step_changes.len(), changes.len()), (1, 0));
        step_changes.try_recv().unwrap();
        // The last change after a client reads is never lost.
        let last = update("last", StepKind::Command, StepState::Running, "ls");
        actor
            .provider_event(&session, &run, ProviderEvent::Step(last))
            .unwrap();
        actor.signal(false);
        assert_eq!(step_changes.len(), 1);
    }

    #[test]
    fn steps_beyond_the_run_limit_are_counted_not_kept() {
        let (_home, mut actor, session, run) = actor_with_turn();
        for n in 0..MAX_RUN_STEPS + 3 {
            let step = update(
                &n.to_string(),
                StepKind::Read,
                StepState::Succeeded,
                "notes.txt",
            );
            actor
                .provider_event(&session, &run, ProviderEvent::Step(step))
                .unwrap();
        }
        let page = actor.steps(&session, None, 0).unwrap();
        assert_eq!((page.steps.len(), page.omitted_steps), (MAX_RUN_STEPS, 3));
    }

    #[test]
    fn provider_errors_cannot_close_their_fence() {
        let notice = runtime_stopped("KeyError in __getitem__\n```\n**not markdown**");
        assert!(
            notice.contains("\n````text\nKeyError in __getitem__\n```\n**not markdown**\n````\n")
        );
    }
}
