//! Noninteractive provider turns with the user's YOLO execution policy.
mod output;
use crate::stream::{ClaudeSteps, CodexSteps, StepUpdate};
use crate::*;
pub use output::{MAX_BYTES_AFTER_EXIT, READ_LIMIT};
use output::{Next, Output, StopReading};
use std::os::unix::process::CommandExt;
use std::{
    io::{Read, Write},
    process::{Child, Command as ProcessCommand, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    /// The provider process started; it leads its own process group.
    Spawned(u32),
    Session(String),
    Step(StepUpdate),
    /// The turn's final text, sent once before `Finished` when the turn succeeds.
    Reply(String),
    Usage {
        request_id: String,
        model: Option<String>,
        usage: Value,
        complete: bool,
    },
    Quota(QuotaReading),
    Finished {
        error: Option<String>,
        usage: Value,
        /// The turn stopped because its cancel flag was set, never because of what the
        /// provider said; see `Cancelled`.
        cancelled: bool,
    },
}
pub struct Turn {
    pub run_id: String,
    pub session: Session,
    pub profile: ModelProfile,
    pub provider_session: Option<String>,
    pub cwd: PathBuf,
    pub prompt: String,
    /// The delivered messages' stored files, in order; images also go to the model as images.
    pub attachments: Vec<Attachment>,
    pub socket: PathBuf,
    pub token: String,
    pub helper: PathBuf,
}
pub fn executable(provider: Provider) -> Result<PathBuf> {
    let name = match provider {
        Provider::Codex => "codex",
        Provider::Claude => "claude",
    };
    let variable = format!("WORKSPACE_{}_BIN", name.to_uppercase());
    if let Some(path) = std::env::var_os(variable) {
        ensure!(
            Path::new(&path).is_file(),
            "Provider executable does not exist"
        );
        return Ok(path.into());
    }
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".local/bin"));
    }
    paths.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    paths
        .into_iter()
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .with_context(|| format!("Install {name} CLI and sign in before starting live work"))
}
pub fn check() -> Value {
    json!({"claude":executable(Provider::Claude).map(|p|p.to_string_lossy().to_string()).map_err(|e|e.to_string()),"codex":executable(Provider::Codex).map(|p|p.to_string_lossy().to_string()).map_err(|e|e.to_string()),"authentication":"Uses existing CLI sign-in; first live turn verifies access"})
}
pub fn skill(role: Role) -> String {
    format!(
        "{}\n{}",
        include_str!("../skills/communication.md"),
        match role {
            Role::ProjectOrchestrator => include_str!("../skills/main-coordinator.md"),
            Role::TaskOrchestrator => include_str!("../skills/repository-coordinator.md"),
            Role::Implementer | Role::Maintenance => include_str!("../skills/implementer.md"),
            _ => include_str!("../skills/tester.md"),
        }
    )
}
/// Lists every attachment's absolute path, to follow the message body in the prompt.
pub fn attachment_list(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let lines = attachments
        .iter()
        .map(|a| format!("\n- {}", a.path.display()))
        .collect::<String>();
    format!("\n\nAttached files (read-only copies):{lines}")
}
fn images(turn: &Turn) -> impl Iterator<Item = (&Path, ImageFormat)> {
    turn.attachments
        .iter()
        .filter_map(|a| Some((a.path.as_path(), a.image?)))
}
/// Each message's files share its own directory, and their Codex paths are chosen within it.
fn codex_images(turn: &Turn) -> impl Iterator<Item = PathBuf> {
    turn.attachments
        .chunk_by(|a, b| a.path.parent() == b.path.parent())
        .flat_map(|message| {
            message
                .iter()
                .enumerate()
                .filter(|(_, a)| a.image.is_some())
                .map(|(index, _)| crate::attachments::codex_image_path(message, index))
        })
}
/// What the provider reads on stdin. Claude takes images only as content blocks of a
/// stream-json user message; otherwise the prompt is plain text.
fn input(turn: &Turn) -> Result<Vec<u8>> {
    if turn.session.provider == Provider::Codex || images(turn).next().is_none() {
        return Ok(turn.prompt.clone().into_bytes());
    }
    use base64::Engine as _;
    let mut content = vec![json!({"type":"text","text":turn.prompt})];
    for (path, format) in images(turn) {
        let data = base64::engine::general_purpose::STANDARD.encode(fs::read(path)?);
        content.push(json!({"type":"image","source":{"type":"base64","media_type":format.media_type(),"data":data}}));
    }
    let mut line =
        serde_json::to_vec(&json!({"type":"user","message":{"role":"user","content":content}}))?;
    line.push(b'\n');
    Ok(line)
}
pub fn command(turn: &Turn) -> Result<ProcessCommand> {
    let mut cmd = ProcessCommand::new(executable(turn.session.provider)?);
    configure(&mut cmd, turn);
    Ok(cmd)
}
fn configure(cmd: &mut ProcessCommand, turn: &Turn) {
    cmd.current_dir(&turn.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // Credentials go to the MCP subprocess environment, never in the prompt.
    cmd.env("WORKSPACE_SOCKET", &turn.socket)
        .env("WORKSPACE_TOKEN", &turn.token);
    let instructions = skill(turn.session.role);
    match turn.session.provider {
        Provider::Claude => {
            cmd.args(["-p","--output-format","stream-json","--verbose","--include-partial-messages","--strict-mcp-config","--no-chrome"])
                .args(["--model",&turn.profile.model,"--effort",&turn.profile.effort])
                .args(["--append-system-prompt",&instructions])
                .args(["--mcp-config",&json!({"mcpServers":{"agent_workspace":{"command":turn.helper,"args":["agent-mcp"]}}}).to_string()])
                .args(["--allowedTools","mcp__agent_workspace__*"])
                // Agents work only in the directory Wiffletree assigned them.
                .args(["--disallowedTools","EnterWorktree,ExitWorktree"]);
            cmd.args(["--dangerously-skip-permissions", "--tools", "default"]);
            if images(turn).next().is_some() {
                cmd.args(["--input-format", "stream-json"]);
            }
            if let Some(id) = &turn.provider_session {
                cmd.args(["--resume", id]);
            }
        }
        Provider::Codex => {
            cmd.args(["exec","--json","--skip-git-repo-check","--color","never"])
                .arg("--dangerously-bypass-approvals-and-sandbox")
                .args(["-m",&turn.profile.model,"-c",&format!("model_reasoning_effort={}",json!(turn.profile.effort))])
                .args(["-c",&format!("developer_instructions={}",json!(instructions))])
                .args(["-c",&format!("mcp_servers.agent_workspace={{required=true, default_tools_approval_mode=\"approve\", command={}, args=[\"agent-mcp\"], env_vars=[\"WORKSPACE_SOCKET\",\"WORKSPACE_TOKEN\"] }}",json!(turn.helper))]);
            // `--image` takes several values: the joined form keeps it from consuming `-`, and
            // `codex_images` avoids the commas it also splits on.
            for path in codex_images(turn) {
                let mut image = std::ffi::OsString::from("--image=");
                image.push(path);
                cmd.arg(image);
            }
            if let Some(id) = &turn.provider_session {
                cmd.args(["resume", id]);
            }
            cmd.arg("-");
        }
    }
}
/// How long a provider's process group may take to exit after SIGTERM before it is killed.
const TERMINATION_GRACE: Duration = Duration::from_secs(1);
/// How long a finished turn waits for the rest of the provider's stderr.
const STDERR_DRAIN: Duration = Duration::from_millis(500);
/// The provider and its tool subprocesses, one owned process group. The leader stays
/// unreaped until `stop` has finished signalling: its zombie keeps its pid, which is also the
/// group id, from being reused, so the signals can only reach this turn's processes.
struct Process {
    child: Child,
    status: Option<ExitStatus>,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
impl Process {
    /// Whether the leader has exited, without reaping it.
    fn exited(&self) -> Result<bool> {
        if self.status.is_some() {
            return Ok(true);
        }
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let options = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
        if unsafe { libc::waitid(libc::P_PID, self.child.id(), &mut info, options) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(info.si_pid != 0)
    }
    /// Stops the whole group, SIGTERM and then SIGKILL after the grace period, and only then
    /// reaps the leader. Later calls return its status without signalling again.
    fn stop(&mut self) -> Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let group = self.child.id() as i32;
        unsafe {
            libc::killpg(group, libc::SIGTERM);
        }
        let deadline = Instant::now() + TERMINATION_GRACE;
        // A group whose only member is the zombie leader answers EPERM, so this waits only
        // while live members remain.
        while unsafe { libc::killpg(group, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
        let status = self.child.wait()?;
        self.status = Some(status);
        Ok(status)
    }
}
/// The error a running turn stops with when its cancel flag is set. Callers recognise it by
/// type, so provider output with the same text cannot pass for it.
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Turn interrupted; inspect changes before retrying")
    }
}
impl std::error::Error for Cancelled {}
const TURN_LIMIT: Duration = Duration::from_secs(60 * 30);
/// Stops a turn that was cancelled or ran past the provider limit, in any phase that waits.
fn check_running(cancel: &AtomicBool, start: Instant) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    ensure!(
        start.elapsed() < TURN_LIMIT,
        "Provider turn exceeded 30-minute limit; inspect work before retrying"
    );
    Ok(())
}
pub fn run(turn: Turn, cancel: Arc<AtomicBool>, mut emit: impl FnMut(ProviderEvent)) -> Result<()> {
    let quota_account = if turn.session.provider == Provider::Claude {
        crate::runtime::claude_account(&turn.cwd).ok().flatten()
    } else {
        None
    };
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Turn interrupted before provider launch"
    );
    let input = input(&turn)?;
    let mut process = Process {
        child: command(&turn)?
            .spawn()
            .context("Could not start provider")?,
        status: None,
    };
    emit(ProviderEvent::Spawned(process.child.id()));
    let mut stdin = process.child.stdin.take().context("No provider stdin")?;
    // Images can make the input large; writing it alongside reading output cannot deadlock.
    // A provider that exits before reading it all is reported by its exit status.
    std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stderr = process.child.stderr.take().context("No provider stderr")?;
    // The tail is kept as it arrives, so a process still holding stderr when the turn ends
    // cannot withhold what was already written.
    let errors = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (errors_done, errors_closed) = std::sync::mpsc::sync_channel(1);
    let tail = errors.clone();
    std::thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(read @ 1..) = stderr.read(&mut chunk) {
            let mut tail = tail.lock().unwrap_or_else(|e| e.into_inner());
            tail.extend_from_slice(&chunk[..read]);
            if tail.len() > 16384 {
                let excess = tail.len() - 16384;
                tail.drain(..excess);
            }
        }
        let _ = errors_done.send(());
    });
    let stdout = process.child.stdout.take().context("No provider stdout")?;
    let output = Output::spawn(stdout)?;
    let _stop_reading = StopReading(output.clone());
    let start = Instant::now();
    let mut terminal = false;
    let mut failure = None;
    let mut usage = Value::Null;
    let mut requests = crate::telemetry::ClaudeRequests::default();
    let mut claude = ClaudeSteps::new(&turn.cwd);
    let mut codex = CodexSteps::new(&turn.cwd);
    // The exit is checked on every pass, so a leftover process writing to stdout cannot hide
    // it. Every event the reader accepted is handled; the reader decides when to stop.
    let mut exited = false;
    loop {
        check_running(&cancel, start)?;
        if !exited && process.exited()? {
            // First, so the reader never waits for room while the group is being stopped.
            output.provider_exited();
            process.stop()?;
            exited = true;
        }
        let event = match output.next(Duration::from_millis(50)) {
            Next::Event(event) => event,
            Next::Waiting => continue,
            Next::Finished => break,
        };
        let value = match event {
            Ok(value) => value,
            // Stray output after the terminal result cannot change the turn's outcome.
            Err(_) if terminal => continue,
            Err(error) => anyhow::bail!(error),
        };
        match turn.session.provider {
            Provider::Codex => match value["type"].as_str().unwrap_or_default() {
                "thread.started" => {
                    if let Some(id) = value["thread_id"].as_str() {
                        emit(ProviderEvent::Session(id.into()));
                    }
                }
                "item.started" | "item.updated" | "item.completed" => {
                    codex
                        .event(&value)
                        .into_iter()
                        .for_each(|step| emit(ProviderEvent::Step(step)));
                }
                "turn.completed" => {
                    terminal = true;
                    if let Some(reply) = codex.reply() {
                        emit(ProviderEvent::Reply(reply));
                    }
                    usage = value["usage"].clone();
                    emit(ProviderEvent::Usage {
                        request_id: format!("turn:{}", turn.run_id),
                        model: Some(turn.profile.model.clone()),
                        usage: usage.clone(),
                        complete: true,
                    });
                }
                "turn.failed" | "error" => {
                    failure = Some(value.to_string());
                }
                _ => {}
            },
            Provider::Claude => match value["type"].as_str().unwrap_or_default() {
                "stream_event" => {
                    if let Some(event) = requests.event(&value) {
                        emit(event);
                    }
                    claude
                        .event(&value)
                        .into_iter()
                        .for_each(|step| emit(ProviderEvent::Step(step)));
                }
                "rate_limit_event" => {
                    if let Some(mut reading) = crate::telemetry::claude_quota(&value, now()) {
                        reading.account = quota_account.clone();
                        emit(ProviderEvent::Quota(reading));
                    }
                }
                "system" if value["subtype"] == "init" => {
                    if let Some(id) = value["session_id"].as_str() {
                        emit(ProviderEvent::Session(id.into()));
                    }
                    ensure!(
                        value["mcp_servers"]
                            .as_array()
                            .is_some_and(|servers| servers
                                .iter()
                                .any(|s| s["name"] == "agent_workspace"
                                    && s["status"] == "connected")),
                        "Workspace communication server did not connect: {} {}",
                        value["mcp_servers"],
                        value["mcp_server_errors"]
                    );
                }
                "assistant" | "user" => {
                    claude
                        .event(&value)
                        .into_iter()
                        .for_each(|step| emit(ProviderEvent::Step(step)));
                }
                "result" => {
                    terminal = true;
                    usage = value["usage"].clone();
                    if value["is_error"] != true
                        && let Some(reply) = claude.reply(&value)
                    {
                        emit(ProviderEvent::Reply(reply));
                    }
                    if value["is_error"] == true {
                        failure = Some(
                            value["result"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string()),
                        );
                    }
                }
                _ => {}
            },
        }
    }
    // A provider can close stdout and keep running; returning early drops `process`, which
    // stops its whole process group.
    while !process.exited()? {
        check_running(&cancel, start)?;
        std::thread::sleep(Duration::from_millis(50));
    }
    // Stops tool subprocesses that outlived the provider.
    let status = process.stop()?;
    let _ = errors_closed.recv_timeout(STDERR_DRAIN);
    let stderr = without_terminal_codes(&String::from_utf8_lossy(
        &errors.lock().unwrap_or_else(|e| e.into_inner()),
    ));
    if (!status.success() || !terminal) && failure.is_none() {
        failure = Some(format!(
            "Provider exited without a successful terminal result ({status}). {}",
            stderr.chars().take(1500).collect::<String>()
        ));
    }
    emit(ProviderEvent::Finished {
        error: failure,
        usage,
        cancelled: false,
    });
    Ok(())
}

/// Drops terminal color and cursor sequences so provider errors read as plain text.
fn without_terminal_codes(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        match chars.next() {
            // Control sequence: parameters, then one final byte in @..~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // Operating system command, such as a hyperlink, ended by BEL or ESC \.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next() == Some('\\')) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leader_stays_unreaped_until_its_group_has_been_signalled() {
        let mut process = Process {
            child: ProcessCommand::new("/bin/sh")
                .args(["-c", "exit 3"])
                .process_group(0)
                .spawn()
                .unwrap(),
            status: None,
        };
        let group = process.child.id() as i32;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !process.exited().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        // Seeing the exit does not reap: the zombie still holds the group id.
        assert!(process.exited().unwrap());
        assert_eq!(unsafe { libc::killpg(group, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        );
        assert_eq!(process.stop().unwrap().code(), Some(3));
        // Reaped: the group id is free for reuse, and stopping again signals nothing.
        assert_eq!(unsafe { libc::killpg(group, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(process.stop().unwrap().code(), Some(3));
    }

    #[test]
    fn claude_cannot_open_its_own_worktrees() {
        let turn = Turn {
            run_id: new_id(),
            session: Session {
                id: new_id(),
                project_id: new_id(),
                parent_id: None,
                repository_id: None,
                name: "Implementer".into(),
                role: Role::Implementer,
                provider: Provider::Claude,
                status: Status::Ready,
                archived: false,
            },
            profile: ModelProfile {
                provider: Provider::Claude,
                model: "opus".into(),
                effort: "medium".into(),
            },
            provider_session: None,
            cwd: PathBuf::from("/tmp"),
            prompt: String::new(),
            attachments: vec![],
            socket: PathBuf::new(),
            token: String::new(),
            helper: PathBuf::new(),
        };
        let mut cmd = ProcessCommand::new("claude");
        configure(&mut cmd, &turn);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        let denied = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert_eq!(args[denied + 1], "EnterWorktree,ExitWorktree");
        assert!(args.iter().any(|a| a == "--allowedTools"));
    }

    fn turn(provider: Provider, resume: Option<&str>, attachments: Vec<Attachment>) -> Turn {
        Turn {
            run_id: "run".into(),
            session: Session {
                id: "s".into(),
                project_id: "p".into(),
                parent_id: None,
                repository_id: None,
                name: "Main".into(),
                role: Role::ProjectOrchestrator,
                provider,
                status: Status::Ready,
                archived: false,
            },
            profile: ModelProfile {
                provider,
                model: "m".into(),
                effort: "high".into(),
            },
            provider_session: resume.map(str::to_owned),
            cwd: "/tmp".into(),
            prompt: "Look".into(),
            attachments,
            socket: "/tmp/socket".into(),
            token: "t".into(),
            helper: "/tmp/helper".into(),
        }
    }

    /// A stored screenshot and a text file, as delivered with one message.
    fn attachments(dir: &Path) -> Vec<Attachment> {
        let shot = dir.join("shot.png");
        fs::write(&shot, b"\x89PNG\r\n\x1a\nimage").unwrap();
        vec![
            Attachment {
                path: shot,
                size: 13,
                image: Some(ImageFormat::Png),
            },
            Attachment {
                path: dir.join("notes.txt"),
                size: 5,
                image: None,
            },
        ]
    }

    fn arguments(turn: &Turn) -> Vec<String> {
        let mut cmd = ProcessCommand::new("provider");
        configure(&mut cmd, turn);
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_prompt_lists_every_attachment_path() {
        let dir = tempfile::tempdir().unwrap();
        let list = attachment_list(&attachments(dir.path()));
        assert_eq!(
            list,
            format!(
                "\n\nAttached files (read-only copies):\n- {}\n- {}",
                dir.path().join("shot.png").display(),
                dir.path().join("notes.txt").display()
            )
        );
        assert_eq!(attachment_list(&[]), "");
    }

    #[test]
    fn codex_attaches_images_before_resuming_from_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let turn = turn(Provider::Codex, Some("thread"), attachments(dir.path()));
        let args = arguments(&turn);
        let image = format!("--image={}", dir.path().join("shot.png").display());
        assert_eq!(
            &args[args.len() - 4..],
            [image, "resume".into(), "thread".into(), "-".into()]
        );
        assert!(!args.iter().any(|a| a.contains("notes.txt")));
        assert_eq!(input(&turn).unwrap(), b"Look");
    }

    #[test]
    fn claude_sends_images_as_stream_json_content() {
        let dir = tempfile::tempdir().unwrap();
        let turn = turn(Provider::Claude, Some("session"), attachments(dir.path()));
        let args = arguments(&turn);
        let format = args.iter().position(|a| a == "--input-format").unwrap();
        assert_eq!(args[format + 1], "stream-json");
        let denied = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert_eq!(args[denied + 1], "EnterWorktree,ExitWorktree");
        assert_eq!(&args[args.len() - 2..], ["--resume", "session"]);
        let line = input(&turn).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        let message: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(
            message,
            json!({"type":"user","message":{"role":"user","content":[
                {"type":"text","text":"Look"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgppbWFnZQ=="}}
            ]}})
        );
    }

    #[test]
    fn codex_gets_comma_free_image_paths_while_the_prompt_keeps_the_originals() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = attachments(dir.path());
        files[0].path = dir.path().join("odd,shot.png");
        let mut turn = turn(Provider::Codex, None, files);
        turn.prompt = attachment_list(&turn.attachments);
        let args = arguments(&turn);
        let image = format!("--image={}", dir.path().join(".codex-0.png").display());
        assert_eq!(&args[args.len() - 2..], [image, "-".into()]);
        assert!(
            !args
                .iter()
                .any(|a| a.contains(',') && a.starts_with("--image"))
        );
        assert!(turn.prompt.contains("odd,shot.png"));
    }

    #[test]
    fn a_batched_turn_keeps_each_messages_codex_image_paths() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut files = attachments(first.path());
        files[0].path = first.path().join("odd,shot.png");
        let mut later = attachments(second.path());
        later.swap(0, 1);
        later[1].path = second.path().join("odd,shot.png");
        files.extend(later);
        let args = arguments(&turn(Provider::Codex, None, files));
        let images = [
            first.path().join(".codex-0.png"),
            second.path().join(".codex-1.png"),
        ]
        .map(|path| format!("--image={}", path.display()));
        assert_eq!(&args[args.len() - 3..args.len() - 1], images);
    }

    #[test]
    fn turns_without_images_keep_plain_text_input() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = attachments(dir.path());
        files.remove(0);
        for provider in [Provider::Claude, Provider::Codex] {
            let turn = turn(provider, None, files.clone());
            let args = arguments(&turn);
            assert!(
                !args
                    .iter()
                    .any(|a| a == "--input-format" || a.starts_with("--image"))
            );
            assert_eq!(input(&turn).unwrap(), b"Look");
        }
    }

    #[test]
    fn provider_errors_lose_terminal_codes() {
        let traceback = "\x1b[35mFile\x1b[0m \x1b[1;31mKeyError\x1b[0m: \x1b]8;;https://x.invalid\x1b\\link\x1b]8;;\x07 done";
        assert_eq!(
            without_terminal_codes(traceback),
            "File KeyError: link done"
        );
    }
}
