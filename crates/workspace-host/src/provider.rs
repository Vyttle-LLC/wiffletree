//! Noninteractive provider turns with the user's YOLO execution policy.
use crate::*;
use std::os::unix::process::CommandExt;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    Session(String),
    Text(String),
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
    },
}
pub struct Turn {
    pub run_id: String,
    pub session: Session,
    pub profile: ModelProfile,
    pub provider_session: Option<String>,
    pub cwd: PathBuf,
    pub prompt: String,
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
pub fn command(turn: &Turn) -> Result<ProcessCommand> {
    let mut cmd = ProcessCommand::new(executable(turn.session.provider)?);
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
                .args(["--allowedTools","mcp__agent_workspace__*"]);
            cmd.args(["--dangerously-skip-permissions", "--tools", "default"]);
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
            if let Some(id) = &turn.provider_session {
                cmd.args(["resume", id]);
            }
            cmd.arg("-");
        }
    }
    Ok(cmd)
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        // The provider and its tool subprocesses are one owned process group.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
    let mut process = Process(
        command(&turn)?
            .spawn()
            .context("Could not start provider")?,
    );
    let mut stdin = process.0.stdin.take().context("No provider stdin")?;
    stdin.write_all(turn.prompt.as_bytes())?;
    drop(stdin);
    let stderr = process.0.stderr.take().context("No provider stderr")?;
    let errors = std::thread::spawn(move || {
        let mut tail = Vec::new();
        let mut reader = BufReader::new(stderr);
        loop {
            let mut line = Vec::new();
            match reader.by_ref().take(65537).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                _ => {
                    tail.extend(line);
                    if tail.len() > 16384 {
                        tail.drain(..tail.len() - 16384);
                    }
                }
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });
    let stdout = process.0.stdout.take().context("No provider stdout")?;
    let (send, receive) = std::sync::mpsc::sync_channel(128);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            match reader
                .by_ref()
                .take(2 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(0) => break,
                Ok(_) if line.len() <= 2 * 1024 * 1024 => {
                    if send
                        .send(serde_json::from_slice::<Value>(&line).map_err(|e| e.to_string()))
                        .is_err()
                    {
                        break;
                    }
                }
                _ => {
                    let _ = send.send(Err("Provider event exceeds bound".into()));
                    break;
                }
            }
        }
    });
    let start = Instant::now();
    let mut terminal = false;
    let mut failure = None;
    let mut usage = Value::Null;
    let mut requests = crate::telemetry::ClaudeRequests::default();
    loop {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Turn interrupted; inspect changes before retrying")
        }
        ensure!(
            start.elapsed() < Duration::from_secs(60 * 30),
            "Provider turn exceeded 30-minute limit; inspect work before retrying"
        );
        let value = match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(v) => v.map_err(anyhow::Error::msg)?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        match turn.session.provider {
            Provider::Codex => match value["type"].as_str().unwrap_or_default() {
                "thread.started" => {
                    if let Some(id) = value["thread_id"].as_str() {
                        emit(ProviderEvent::Session(id.into()));
                    }
                }
                "item.completed" => {
                    if value["item"]["type"] == "agent_message"
                        && let Some(text) = value["item"]["text"].as_str()
                    {
                        emit(ProviderEvent::Text(text.into()));
                    }
                }
                "turn.completed" => {
                    terminal = true;
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
                "assistant" => {
                    if let Some(content) = value["message"]["content"].as_array() {
                        for block in content {
                            if block["type"] == "text"
                                && let Some(text) = block["text"].as_str()
                            {
                                emit(ProviderEvent::Text(text.into()));
                            }
                        }
                    }
                }
                "result" => {
                    terminal = true;
                    usage = value["usage"].clone();
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
    let status = process.0.wait()?;
    let stderr = errors.join().unwrap_or_default();
    if (!status.success() || !terminal) && failure.is_none() {
        failure = Some(format!(
            "Provider exited without a successful terminal result ({status}). {}",
            stderr.chars().take(1500).collect::<String>()
        ));
    }
    emit(ProviderEvent::Finished {
        error: failure,
        usage,
    });
    Ok(())
}
