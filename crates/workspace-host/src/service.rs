//! Event-driven host actor: all SQLite access stays on one thread; provider I/O never blocks it.
use crate::provider::{ProviderEvent, Turn};
use crate::stream::StepUpdate;
use crate::*;
use async_channel::{Receiver, Sender};
use std::{
    collections::HashMap,
    io::{BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

type Reply = Sender<std::result::Result<Value, String>>;
/// Operation prefix for the notice raised when a project pauses at its turn budget.
const TURN_BUDGET: &str = "turn-budget";

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
    /// This turn's steps; the store has each one as of its last start or state change.
    steps: Vec<Step>,
    omitted_steps: usize,
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
    turns: HashMap<String, usize>,
    /// Set at shutdown, whose synthetic finishes must not remove worktrees under live processes.
    stopping: bool,
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
        std::thread::Builder::new().name("workspace-host".into()).spawn(move || {
            let setup=(||->Result<_>{
                let mut host=Host::open(home)?;
                let interrupted=host.db.prepare("SELECT session_id FROM provider_runs WHERE finished_at IS NULL")?.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                for id in interrupted {
                    let mut runtime=host.session_runtime(&id)?;
                    runtime.last_error=Some("Host stopped during a turn. Inspect the worktree before retrying held input.".into());
                    host.save_runtime(&runtime)?;
                    host.set_status(&id,Status::Disconnected)?;
                }
                // A host restart never silently resumes uncertain provider side effects.
                host.db.execute("UPDATE live_projects SET enabled=0",[])?;
                host.db.execute("UPDATE provider_runs SET finished_at=?1,outcome='interrupted',detail='Host restarted; reconcile before retry' WHERE finished_at IS NULL",[now()])?;
                host.settle_pending_worktrees();
                let directory=tempfile::Builder::new().prefix("aw-").tempdir_in("/tmp")?;
                let socket=directory.path().join("ipc");
                let listener=UnixListener::bind(&socket)?;
                fs::set_permissions(&socket,fs::Permissions::from_mode(0o600))?;
                Ok((host,directory,socket,listener))
            })();
            let (host,directory,socket,listener)=match setup {Ok(v)=>{let _=ready.send(Ok(()));v},Err(e)=>{let _=ready.send(Err(format!("{e:#}")));return}};
            let connections=Arc::new(AtomicUsize::new(0));
            let socket_sender=worker.clone();
            let stop_listener=Arc::new(AtomicBool::new(false));let stopping=stop_listener.clone();
            std::thread::spawn(move || {
                for connection in listener.incoming() {
                    if stopping.load(Ordering::Relaxed){break}
                    let Ok(mut stream)=connection else {break};
                    if connections.fetch_add(1,Ordering::Relaxed)>=32 {connections.fetch_sub(1,Ordering::Relaxed);continue}
                    let count=connections.clone();let sender=socket_sender.clone();
                    std::thread::spawn(move || {
                        let response=(||->Result<Value>{
                            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                            let request=mcp::read_json(&mut BufReader::new(stream.try_clone()?))?.context("Empty request")?;
                            let (reply,receive)=async_channel::bounded(1);
                            sender.send_blocking(Event::Tool {token:request["token"].as_str().context("Missing credential")?.into(),name:request["name"].as_str().context("Missing tool")?.into(),args:request["arguments"].clone(),reply})?;
                            receive.recv_blocking()?.map_err(anyhow::Error::msg)
                        })();
                        let value=match response {Ok(value)=>json!({"result":value}),Err(error)=>json!({"error":format!("{error:#}")})};
                        let _=stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
                        let _=writeln!(stream,"{value}");
                        count.fetch_sub(1,Ordering::Relaxed);
                    });
                }
            });
            let mut actor=Actor {host,sender:worker,changed,steps_changed,steps_dirty:false,revisions:HashMap::new(),active:HashMap::new(),permissions:HashMap::new(),socket:socket.clone(),helper,turns:HashMap::new(),stopping:false};
            while let Ok(event)=receiver.recv_blocking(){
                if matches!(event,Event::Shutdown){break}
                let dirty=actor.handle(event);
                if let Err(e)=actor.schedule(){eprintln!("Workspace scheduler: {e:#}");}
                actor.signal(dirty);
            }
            actor.stop();
            for (_,permission) in actor.permissions.drain(){let _=permission.reply.try_send(Err("Workspace stopped".into()));}
            stop_listener.store(true,Ordering::Relaxed);
            let _=std::os::unix::net::UnixStream::connect(&socket);
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
                    match &command {
                        Command::SetLive {
                            project_id,
                            enabled: false,
                        } => {
                            for (id, active) in &self.active {
                                if self.host.session(id)?.project_id == *project_id {
                                    active.cancel.store(true, Ordering::Relaxed);
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
                        }
                        Command::ConfigureSession { session_id, .. }
                        | Command::ReconcileSession { session_id, .. } => ensure!(
                            !self.active.contains_key(session_id),
                            "Wait for the active turn to stop"
                        ),
                        Command::SetLive {
                            project_id,
                            enabled: true,
                        } => {
                            self.turns.insert(project_id.clone(), 0);
                        }
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
                        self.turns.insert(project.clone(), 0);
                        // Stepping in is the answer to a turn-budget pause.
                        for pause in self.host.open_attention(&project)? {
                            if pause.operation_id.starts_with(TURN_BUDGET) {
                                self.host
                                    .resolve_attention(&pause.id, "Resumed by the human")?;
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
                if !self.active.get(&session).is_some_and(|a| a.run == run) {
                    return false;
                }
                let changes_state =
                    !matches!(event, ProviderEvent::Step(_) | ProviderEvent::Reply(_));
                if let Err(e) = self.provider_event(&session, &run, event) {
                    eprintln!("Workspace provider event: {e:#}");
                }
                changes_state
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
            ProviderEvent::Finished { error, usage } => {
                let mut active = self.active.remove(id).unwrap();
                // Settle the run first so a later failure cannot leave it open. If this update
                // fails, the run stays open and blocks worktree removal until restart: safe.
                self.host.db.execute(
                    "UPDATE provider_runs SET finished_at=?2,outcome=?3,detail=json_set(detail,'$.result',json(?4),'$.omitted_steps',?5) WHERE id=?1",
                    params![
                        run,
                        now(),
                        if error.is_some() {
                            "failed"
                        } else {
                            "completed"
                        },
                        json!({"error":error,"usage":usage}).to_string(),
                        active.omitted_steps as i64
                    ],
                )?;
                self.settle_steps(id, &mut active, error.is_some())?;
                if error.is_none() {
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
                // A deliberately archived parent has no one to wake.
                if let Some(parent) = &session.parent_id
                    && !self.host.session(parent)?.archived
                    && (error.is_some() || (session.role.is_worker() && !active.reported))
                {
                    let body = if let Some(error) = &error {
                        format!(
                            "{} stopped with an error: {error}. Inspect its runtime/worktree before resuming.",
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
                    error: Some("Host stopped during a turn; inspect work before retrying".into()),
                    usage: Value::Null,
                },
            );
            // Its process may outlive the host, like one a crash leaves behind.
            let _ = self.host.db.execute(
                "UPDATE provider_runs SET outcome='interrupted' WHERE id=?1",
                [&run],
            );
        }
    }
    /// Wakes clients; each bounded signal coalesces any number of changes until it is read.
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
    fn schedule(&mut self) -> Result<()> {
        if self.active.len() >= 8 {
            return Ok(());
        }
        let messages=self.host.db.prepare("SELECT m.* FROM messages m JOIN live_projects p ON p.project_id=m.project_id AND p.enabled=1 JOIN sessions s ON s.id=m.recipient WHERE m.receipt='queued' AND COALESCE(json_extract(s.data,'$.archived'),0)=0 AND (m.sender IS NULL OR m.sender<>m.recipient) AND NOT EXISTS (SELECT 1 FROM messages h WHERE h.recipient=m.recipient AND h.receipt='held') ORDER BY CASE WHEN s.role IN ('project_orchestrator','task_orchestrator') THEN 0 ELSE 1 END,m.sequence LIMIT 100")?.query_map([],Host::message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for message in messages {
            if self.active.len() >= 8 {
                break;
            }
            let session = self.host.session(&message.recipient)?;
            if self.active.contains_key(&session.id)
                || matches!(session.status, Status::Paused | Status::Disconnected)
            {
                continue;
            }
            let project = self.host.project(&session.project_id)?;
            let project_active = self
                .active
                .keys()
                .filter(|id| {
                    self.host
                        .session(id)
                        .is_ok_and(|s| s.project_id == project.id)
                })
                .count();
            // Reserve capacity for coordinators so a full worker pool cannot starve handoffs.
            if project_active >= project.turn_limit
                || (session.role.is_worker()
                    && (project_active >= project.turn_limit.saturating_sub(1).max(1)
                        || self.active.len() >= 6))
            {
                continue;
            }
            if self.turns.get(&project.id).copied().unwrap_or_default() >= 100 {
                self.host.set_live(&project.id, false)?;
                self.host.request_attention(&session.id,"local",&format!("{TURN_BUDGET}:{}", new_id()),"This project ran 100 turns since you last stepped in. Review the work, then message it or resume to continue.",&[])?;
                continue;
            }
            let runtime = self.host.session_runtime(&session.id)?;
            // A ticket's worktree has one writer at a time; reviewers only read, so they run together.
            if let Some(ticket) = &runtime.ticket_id
                && self.active.keys().any(|id| {
                    self.host
                        .session_runtime(id)
                        .is_ok_and(|r| r.ticket_id.as_ref() == Some(ticket))
                        && self.host.session(id).is_ok_and(|active| {
                            !(session.role == Role::Reviewer && active.role == Role::Reviewer)
                        })
                })
            {
                continue;
            }
            if let Err(error) = self.start_turn(session.clone(), message.clone()) {
                self.start_failed(&session, &message, &error)?;
            }
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
    fn start_turn(&mut self, session: Session, message: Message) -> Result<()> {
        let mut runtime = self.host.session_runtime(&session.id)?;
        let profile = if let Some(profile) = &runtime.profile {
            profile.clone()
        } else {
            self.host
                .policy(session.role)?
                .select(Complexity::Standard, None, Some(session.provider), None)?
                .profile
        };
        runtime.profile = Some(profile.clone());
        provider::executable(session.provider)?;
        if let Some(ticket) = &runtime.ticket_id {
            self.host.prepare_ticket_worktree(ticket)?;
        }
        let cwd = if let Some(path) = &runtime.workdir {
            PathBuf::from(path)
        } else if session.role == Role::TaskOrchestrator {
            PathBuf::from(
                self.host
                    .repository(
                        session
                            .repository_id
                            .as_deref()
                            .context("Missing repository")?,
                    )?
                    .path,
            )
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
        let prompt = format!(
            "Managed session {} ({})\nSender: {}\nMessage ID: {}\n\n{}{}\n\nUse workspace_context to obtain current team IDs and tickets. If you have a parent, report through workspace tools before ending the turn. The main coordinator responds to the human directly. Never wait or poll for a reply.{}",
            session.name,
            session.role.label(),
            message.sender.as_deref().unwrap_or("human"),
            message.id,
            message.body,
            provider::attachment_list(&message.attachments),
            open_questions_reminder(&self.host.open_questions(&session)?)
        );
        self.host.db.execute(
            "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,?3,?4,?5)",
            params![run, session.id, json!([message.id]).to_string(), now(),json!({"profile":profile,"provider_session_id":runtime.provider_session_id,"skill_version":1,"policy":self.host.policy(session.role)?}).to_string()],
        )?;
        self.host.advance_receipt(&message.id, Receipt::Delivered)?;
        self.host.set_status(&session.id, Status::Working)?;
        runtime.last_error = None;
        runtime.last_started_at = Some(now());
        runtime.directory = Some(cwd.to_string_lossy().into_owned());
        self.host.save_runtime(&runtime)?;
        Host::event(
            &self.host.db,
            &session.project_id,
            Some(&session.id),
            "turn_scheduled",
            &format!("{}; queue_ms={}", run, now() - message.created_at),
        )?;
        let turn = Turn {
            run_id: run.clone(),
            session: session.clone(),
            profile,
            provider_session: runtime.provider_session_id,
            cwd,
            prompt,
            attachments: message.attachments.clone(),
            socket: self.socket.clone(),
            token: token.clone(),
            helper: self.helper.clone(),
        };
        self.active.insert(
            session.id.clone(),
            Active {
                run: run.clone(),
                token,
                cancel: cancel.clone(),
                messages: vec![message.id],
                output: String::new(),
                reported: false,
                steps: vec![],
                omitted_steps: 0,
            },
        );
        let _ = self.changed.try_send(());
        *self.turns.entry(session.project_id).or_default() += 1;
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
        actor.active.insert(
            session.clone(),
            Active {
                run: run.clone(),
                token: String::new(),
                cancel: Arc::new(AtomicBool::new(false)),
                messages: vec![],
                output: String::new(),
                reported: false,
                steps: vec![],
                omitted_steps: 0,
            },
        );
        (home, actor, session, run, changes, step_changes)
    }

    /// An actor with no turns, its change and step-change receivers.
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
            turns: HashMap::new(),
            stopping: false,
        };
        (actor, changes, step_changes)
    }

    /// A ticket whose tester is in its turn, run `run`, under its repository coordinator.
    fn ticket_in_turn() -> (tempfile::TempDir, Actor, Ticket, Session, Session) {
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
        let coordinator = host
            .create_session(
                &project.id,
                &root.id,
                Some(&attached.id),
                "Web",
                Role::TaskOrchestrator,
                Provider::Codex,
            )
            .unwrap();
        let ticket = host
            .create_ticket(&coordinator.id, "Toolbar", "Do")
            .unwrap();
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

    /// The same ticket closed, so its worktree removal waits for the tester's turn.
    fn ticket_pending_removal() -> (tempfile::TempDir, Actor, Ticket, Session) {
        let (home, mut actor, ticket, tester, _) = ticket_in_turn();
        actor.host.close_ticket(&ticket.id).unwrap();
        assert!(Path::new(&ticket.worktree).exists());
        (home, actor, ticket, tester)
    }

    fn finish(actor: &mut Actor, session: &Session, reported: bool, error: Option<&str>) {
        actor.active.insert(
            session.id.clone(),
            Active {
                run: "run".into(),
                token: String::new(),
                cancel: Arc::new(AtomicBool::new(false)),
                messages: vec![],
                output: String::new(),
                reported,
                steps: vec![],
                omitted_steps: 0,
            },
        );
        actor
            .provider_event(
                &session.id,
                "run",
                ProviderEvent::Finished {
                    error: error.map(Into::into),
                    usage: Value::Null,
                },
            )
            .unwrap();
    }

    #[test]
    fn an_interrupted_turn_under_an_archived_team_performs_its_pending_removal() {
        for archive_root in [true, false] {
            let (_home, mut actor, ticket, tester, coordinator) = ticket_in_turn();
            let archived = if archive_root {
                coordinator.parent_id.clone().unwrap()
            } else {
                coordinator.id.clone()
            };
            actor.host.set_archived(&archived, true).unwrap();
            assert!(Path::new(&ticket.worktree).exists());

            finish(&mut actor, &tester, false, Some("Turn interrupted"));

            assert!(!Path::new(&ticket.worktree).exists());
            assert!(actor.host.pending_worktree_removals().unwrap().is_empty());
        }
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
        actor.active.insert(
            tester.id.clone(),
            Active {
                run: "run".into(),
                token: String::new(),
                cancel: Arc::new(AtomicBool::new(false)),
                messages: vec![],
                output: String::new(),
                reported: true,
                steps: vec![],
                omitted_steps: 0,
            },
        );

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
