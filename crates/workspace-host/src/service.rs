//! Event-driven host actor: all SQLite access stays on one thread; provider I/O never blocks it.
use crate::provider::{ProviderEvent, Turn};
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
}
struct Active {
    run: String,
    token: String,
    cancel: Arc<AtomicBool>,
    messages: Vec<String>,
    output: String,
    reported: bool,
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
    active: HashMap<String, Active>,
    permissions: HashMap<String, Permission>,
    socket: PathBuf,
    helper: PathBuf,
    turns: HashMap<String, usize>,
}
impl Service {
    pub fn start(home: PathBuf, helper: PathBuf) -> Result<Self> {
        ensure!(
            helper.is_file(),
            "workspace-host helper is missing; rebuild the app bundle"
        );
        let (sender, receiver) = async_channel::bounded(256);
        let (changed, changes) = async_channel::bounded(1);
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
            let mut actor=Actor {host,sender:worker,changed,active:HashMap::new(),permissions:HashMap::new(),socket:socket.clone(),helper,turns:HashMap::new()};
            while let Ok(event)=receiver.recv_blocking(){
                if matches!(event,Event::Shutdown){break}
                let dirty=actor.handle(event);
                if let Err(e)=actor.schedule(){eprintln!("Workspace scheduler: {e:#}");}
                if dirty {let _=actor.changed.try_send(());}
            }
            let interrupted=actor.active.iter().map(|(id,a)|{a.cancel.store(true,Ordering::Relaxed);(id.clone(),a.run.clone())}).collect::<Vec<_>>();
            for (id,run) in interrupted {let _=actor.provider_event(&id,&run,ProviderEvent::Finished{error:Some("Host stopped during a turn; inspect work before retrying".into()),usage:Value::Null});}
            for (_,permission) in actor.permissions.drain(){let _=permission.reply.try_send(Err("Workspace stopped".into()));}
            stop_listener.store(true,Ordering::Relaxed);
            let _=std::os::unix::net::UnixStream::connect(&socket);
            drop(directory);
        })?;
        readiness.recv()?.map_err(anyhow::Error::msg)?;
        Ok(Self {
            handle: Arc::new(Handle { sender }),
            changes,
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
                        | Command::Logs { .. }
                        | Command::Activity { .. }
                        | Command::GitHistory { .. }
                        | Command::ProviderCheck
                        | Command::Quotas
                        | Command::Usage { .. }
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
                        Command::SetArchived {
                            session_id,
                            archived: true,
                        } => {
                            for session in self.host.session_tree(session_id)? {
                                if let Some(active) = self.active.get(&session.id) {
                                    active.cancel.store(true, Ordering::Relaxed);
                                }
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
                    let response = self.host.execute(command.clone())?;
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
                if let Err(e) = self.provider_event(&session, &run, event) {
                    eprintln!("Workspace provider event: {e:#}");
                }
                true
            }
            Event::Shutdown => false,
        }
    }
    fn provider_event(&mut self, id: &str, run: &str, event: ProviderEvent) -> Result<()> {
        let session = self.host.session(id)?;
        let mut runtime = self.host.session_runtime(id)?;
        match event {
            ProviderEvent::Session(provider_id) => {
                self.host.db.execute("UPDATE provider_runs SET detail=json_set(detail,'$.provider_thread_id',?2) WHERE id=?1",params![run,provider_id])?;
                runtime.provider_session_id = Some(provider_id);
                self.host.save_runtime(&runtime)?;
            }
            ProviderEvent::Text(text) => {
                let active = self.active.get_mut(id).unwrap();
                if active.output.len() < MAX_TEXT_BYTES {
                    if !active.output.is_empty() {
                        active.output.push_str("\n\n");
                    }
                    active.output.extend(
                        text.chars()
                            .take((MAX_TEXT_BYTES - active.output.len()) / 4),
                    );
                }
                self.host.append_output(&session, run, &active.output)?;
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
                let active = self.active.remove(id).unwrap();
                runtime.last_finished_at = Some(now());
                runtime.last_error = error.clone();
                self.host.save_runtime(&runtime)?;
                self.host.db.execute(
                    "UPDATE provider_runs SET finished_at=?2,outcome=?3,detail=json_set(detail,'$.result',json(?4)) WHERE id=?1",
                    params![
                        run,
                        now(),
                        if error.is_some() {
                            "failed"
                        } else {
                            "completed"
                        },
                        json!({"error":error,"usage":usage}).to_string()
                    ],
                )?;
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
                if let Some(parent) = &session.parent_id
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
                let mut runtime = self.host.session_runtime(&session.id)?;
                runtime.last_error = Some(format!("{error:#}"));
                self.host.save_runtime(&runtime)?;
                self.host.set_status(&session.id, Status::Disconnected)?;
                self.host.append_output(
                    &session,
                    &format!("start-error:{}", message.id),
                    &format!("Could not start: {error:#}"),
                )?;
            }
        }
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
            let path = self.host.home.join("projects").join(&session.project_id);
            fs::create_dir_all(&path)?;
            path
        };
        let run = new_id();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = format!("{}{}", new_id(), new_id());
        let prompt = format!(
            "Managed session {} ({})\nSender: {}\nMessage ID: {}\n\n{}\n\nUse workspace_context to obtain current team IDs and tickets. If you have a parent, report through workspace tools before ending the turn. The main coordinator responds to the human directly. Never wait or poll for a reply.{}",
            session.name,
            session.role.label(),
            message.sender.as_deref().unwrap_or("human"),
            message.id,
            message.body,
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
    use super::runtime_stopped;

    #[test]
    fn provider_errors_cannot_close_their_fence() {
        let notice = runtime_stopped("KeyError in __getitem__\n```\n**not markdown**");
        assert!(
            notice.contains("\n````text\nKeyError in __getitem__\n```\n**not markdown**\n````\n")
        );
    }
}
