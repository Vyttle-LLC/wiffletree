//! Noninteractive provider turns with the user's YOLO execution policy.
mod codex_compaction;
mod constrained;
mod output;
use crate::compaction::CompactionRecord;
use crate::processes::{self, Sighting, Watch};
use crate::runtime::LowPriority as _;
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
use workspace_core::OwnDescriptors as _;

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    /// The provider process started; it leads its own process group. Its start time tells it
    /// from a later process given the same id.
    Spawned {
        group: u32,
        started: Option<i64>,
    },
    /// What the watcher saw of the provider's tree: the processes the agent's tools started,
    /// each in a group of its own, and the groups that have exited since.
    Processes(Vec<Sighting>),
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
    /// A Claude result record's `modelUsage`: the session's counts so far, by model.
    ModelUsage(Value),
    /// What the provider said about the compaction this run is; a turn's run sends none.
    Compaction(CompactionRecord),
    Finished {
        error: Option<String>,
        usage: Value,
        /// The turn stopped because its cancel flag was set, never because of what the
        /// provider said; see `Cancelled`.
        cancelled: bool,
    },
}
/// What a run asks of the provider.
pub enum TurnKind {
    /// A turn: `prompt` and the delivered messages' attachments.
    Prompt,
    /// Claude's compact command with what to keep, sent bare. Not a turn: `prompt` is unused.
    Compaction(Option<String>),
    /// A Codex thread compacted through `codex app-server`, which takes no note. `before` is
    /// the context total in the thread's record, when it gave one. Not a turn either.
    CodexCompaction { before: Option<u64> },
    /// A PR keeper sorting turn: Claude with every tool off but the one that returns its
    /// answer, no MCP server, workspace access, permission bypass, earlier conversation or
    /// stored session. Its only reply is that answer.
    Sort,
}
pub struct Turn {
    pub run_id: String,
    pub kind: TurnKind,
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
    /// The role's instructions as the store held them when the turn was built.
    pub instructions: String,
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
/// The fixed core's parts, each with who gets it: the contract, then each role's core.
pub const CORE: [(&str, &str); 5] = [
    (
        "every agent",
        include_str!("../skills/core/communication.md"),
    ),
    (
        "coordinators",
        include_str!("../skills/core/coordinator.md"),
    ),
    (
        "implementers",
        include_str!("../skills/core/implementer.md"),
    ),
    (
        "testers and reviewers",
        include_str!("../skills/core/verifier.md"),
    ),
    ("the PR keeper", include_str!("../skills/core/keeper.md")),
];
/// The core's parts with their names and versions, for clients.
pub fn core_instructions() -> Vec<workspace_core::skills::CoreInstructions> {
    CORE.iter()
        .map(|(audience, text)| workspace_core::skills::CoreInstructions::new(audience, text))
        .collect()
}
/// The core's parts with their texts, for the Skills page.
pub fn core_texts() -> Vec<workspace_core::skills::CoreText> {
    CORE.iter()
        .map(|(audience, text)| workspace_core::skills::CoreText {
            about: workspace_core::skills::CoreInstructions::new(audience, text),
            text: (*text).into(),
        })
        .collect()
}
/// The part of a role's instructions the app depends on, which no skill text changes.
fn core(role: Role) -> &'static str {
    let part = match role {
        Role::ProjectOrchestrator | Role::TaskOrchestrator => 1,
        Role::Implementer | Role::Maintenance => 2,
        Role::Tester | Role::Reviewer => 3,
        Role::Keeper => 4,
    };
    CORE[part].1
}
/// Part of the fixed core: it says, in Wiffletree's words, that the human's rules win.
pub(crate) const RULES_SENTENCE: &str = "These are the human's own rules for this skill. Where they disagree with the skill above, follow them.";
/// A role's bundled instructions: the fixed core and its skill's stock text.
pub fn skill(role: Role) -> String {
    instructions(role, skills::stock(Skill::for_role(role)), "")
}
/// What a role's turns are sent, with its skill as the human left it: a rewrite in place of the
/// stock text, or the stock text and the human's rules.
pub fn role_instructions(role: Role, skill: &SkillState) -> String {
    match &skill.rewrite {
        Some(rewrite) => instructions(role, &rewrite.text, ""),
        None => instructions(role, &skill.stock, &skill.rules),
    }
}
/// The heading the human's own rules follow, as "## Your own rules for Implementing".
pub fn rules_heading(skill: Skill) -> String {
    format!("## Your own rules for {}", skill.name())
}
/// The communication contract, the role's core, the skill's text, stock or the human's
/// rewrite, and the human's own rules for that skill, if any, under a heading of Wiffletree's.
pub fn instructions(role: Role, skill_text: &str, rules: &str) -> String {
    let mut text = format!("{}\n{}\n{skill_text}", CORE[0].1, core(role));
    if !rules.is_empty() {
        let heading = rules_heading(Skill::for_role(role));
        text.push_str(&format!("\n{heading}\n{RULES_SENTENCE}\n{rules}"));
    }
    text
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
/// stream-json user message; otherwise the prompt is plain text. A Claude compaction is the
/// bare command, which Claude runs only when nothing wraps it; a Codex one is the app-server's
/// requests.
fn input(turn: &Turn) -> Result<Vec<u8>> {
    if let TurnKind::Compaction(instructions) = &turn.kind {
        let command = match instructions {
            Some(instructions) => format!("/compact {instructions}"),
            None => "/compact".into(),
        };
        return Ok(command.into_bytes());
    }
    if matches!(turn.kind, TurnKind::CodexCompaction { .. }) {
        return codex_compaction::input(turn);
    }
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
    ensure!(
        !matches!(turn.kind, TurnKind::Sort) || turn.session.provider == Provider::Claude,
        "Only Claude can sort comments: Codex cannot be started with every tool off"
    );
    let mut cmd = ProcessCommand::new(executable(turn.session.provider)?);
    configure(&mut cmd, turn);
    Ok(cmd)
}
fn configure(cmd: &mut ProcessCommand, turn: &Turn) {
    cmd.current_dir(&turn.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .at_low_priority()
        .own_descriptors_only();
    if matches!(turn.kind, TurnKind::Sort) {
        constrained::configure(cmd, &turn.profile, keeper::SORTING, keeper::ANSWER_SCHEMA);
        return;
    }
    // Credentials go to the MCP subprocess environment, never in the prompt.
    cmd.env("WORKSPACE_SOCKET", &turn.socket)
        .env("WORKSPACE_TOKEN", &turn.token);
    let instructions = &turn.instructions;
    match turn.session.provider {
        Provider::Claude => {
            cmd.args(["-p","--output-format","stream-json","--verbose","--include-partial-messages","--strict-mcp-config","--no-chrome"])
                .args(["--model",&turn.profile.model,"--effort",&turn.profile.effort])
                .args(["--append-system-prompt",instructions])
                // Otherwise a resumed session is sent its first turn's instructions.
                .args(["--system-prompt-snapshot","off"])
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
            let compaction = matches!(turn.kind, TurnKind::CodexCompaction { .. });
            if compaction {
                // The app-server takes a turn's overrides, so the summary request shares the
                // turns' prefix; what `exec`'s own flags say goes in `thread/resume`.
                cmd.arg("app-server");
            } else {
                cmd.args([
                    "exec",
                    "--json",
                    "--skip-git-repo-check",
                    "--color",
                    "never",
                ])
                .arg("--dangerously-bypass-approvals-and-sandbox")
                .args(["-m", &turn.profile.model]);
            }
            cmd.args(["-c",&format!("model_reasoning_effort={}",json!(turn.profile.effort))])
                .args(["-c",&format!("developer_instructions={}",json!(instructions))])
                .args(["-c",&format!("mcp_servers.agent_workspace={{required=true, default_tools_approval_mode=\"approve\", command={}, args=[\"agent-mcp\"], env_vars=[\"WORKSPACE_SOCKET\",\"WORKSPACE_TOKEN\"] }}",json!(turn.helper))]);
            if compaction {
                return;
            }
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
/// How long a skill merge's model run may take.
pub const MERGE_LIMIT: Duration = Duration::from_secs(5 * 60);
/// A skill merge's answer, as the run returned it, and the tokens the run used.
pub struct MergeAnswer {
    pub answer: Value,
    pub tokens: u64,
}
/// Runs `claude` once to merge a skill's overlapping passages: built like a sorting run, but of
/// no session, in an empty temporary folder, and stopped after `limit`.
pub fn merge_run(
    claude: &Path,
    profile: &ModelProfile,
    input: Vec<u8>,
    limit: Duration,
) -> Result<MergeAnswer> {
    let folder = tempfile::tempdir()?;
    let mut cmd = ProcessCommand::new(claude);
    cmd.current_dir(folder.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .at_low_priority()
        .own_descriptors_only();
    constrained::configure(
        &mut cmd,
        profile,
        skills::model::MERGING,
        skills::model::ANSWER_SCHEMA,
    );
    let mut child = cmd.spawn().context("Claude could not be started")?;
    let mut stdin = child.stdin.take().context("No Claude stdin")?;
    let output = Output::spawn(child.stdout.take().context("No Claude stdout")?)?;
    let _stop_reading = StopReading(output.clone());
    let errors = constrained::Errors::spawn(child.stderr.take().context("No Claude stderr")?);
    let mut process = Process {
        child,
        status: None,
    };
    std::thread::spawn(move || stdin.write_all(&input));
    let deadline = Instant::now() + limit;
    let mut run = constrained::Run::new("merge");
    let mut answer = None;
    loop {
        if process.exited()? {
            output.provider_exited();
        }
        ensure!(
            Instant::now() < deadline,
            "The model took too long to answer, so the run was stopped."
        );
        match output.next(Duration::from_millis(20)) {
            Next::Event(Ok(value)) => {
                if answer.is_none()
                    && let Some(result) = run.event(&value)?
                {
                    ensure!(
                        result.answer.is_some(),
                        "The merge run failed: {}.",
                        value["subtype"]
                    );
                    answer = Some(MergeAnswer {
                        answer: result.answer.unwrap(),
                        tokens: result.tokens,
                    });
                }
            }
            Next::Event(Err(error))
                if answer.is_none() && error == "Provider event exceeds bound" =>
            {
                bail!("{error}")
            }
            Next::Event(Err(_)) => {}
            Next::Waiting => {}
            Next::Finished => break,
        }
    }
    while !process.exited()? {
        ensure!(
            Instant::now() < deadline,
            "The model took too long to answer, so the run was stopped."
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = process.stop()?;
    if let Some(answer) = answer {
        return Ok(answer);
    }
    let stderr = errors.finish();
    let said = stderr.trim().chars().take(1500).collect::<String>();
    let ended = format!("Claude ended without an answer ({status}). {said}");
    bail!("{}", ended.trim_end())
}

/// How long a finished turn waits for the rest of the provider's stderr.
const STDERR_DRAIN: Duration = Duration::from_millis(500);
/// The provider and the helpers that stay in its process group, which this turn owns. What the
/// agent's tools start is not in it: Claude Code and Codex both run each tool command as the
/// leader of its own session and process group, and `processes::Watch` follows those. The
/// leader stays unreaped until `stop` has finished signalling: its zombie keeps its pid, which
/// is also the group id, from being reused, so the signals can only reach this turn's group.
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
    /// Stops the provider's group, after the watcher's last look at it, and only then reaps
    /// the leader. Later calls return its status without signalling again.
    fn stop(&mut self) -> Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        processes::stop_provider_group(self.child.id() as i32, processes::look);
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
/// Stops a cancelled turn, in any phase that waits.
fn check_running(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    Ok(())
}
/// Runs the provider to the end of its turn, sending `emit` every event and, when the turn ran
/// to an end of its own, `Finished` last. Whichever way it ends, the host hears of every
/// process the watcher saw before it hears of the end.
pub fn run(turn: Turn, cancel: Arc<AtomicBool>, mut emit: impl FnMut(ProviderEvent)) -> Result<()> {
    let mut watch = None;
    let finished = run_turn(turn, cancel, &mut emit, &mut watch);
    if let Some(watch) = watch {
        // A turn that ended on its own may leave processes running, so those still exiting
        // with the provider get a moment first. A turn cut short has everything stopped.
        let seen = watch.finish(finished.is_ok());
        if !seen.is_empty() {
            emit(ProviderEvent::Processes(seen));
        }
    }
    emit(finished?);
    Ok(())
}
/// Sends the host what the watcher has seen of the run's tree since it was last asked.
fn report_seen(watching: &Watch, emit: &mut impl FnMut(ProviderEvent)) {
    let seen = watching.seen();
    if !seen.is_empty() {
        emit(ProviderEvent::Processes(seen));
    }
}
/// The turn itself. Returns its `Finished` event; `watch` is set once the provider has started.
fn run_turn(
    turn: Turn,
    cancel: Arc<AtomicBool>,
    emit: &mut impl FnMut(ProviderEvent),
    watch: &mut Option<Watch>,
) -> Result<ProviderEvent> {
    let quota_account = if turn.session.provider == Provider::Claude {
        crate::runtime::claude_account(&turn.cwd).ok().flatten()
    } else {
        None
    };
    // A stop that lands before launch is a cancellation like any other.
    check_running(&cancel)?;
    let input = input(&turn)?;
    let mut process = Process {
        child: command(&turn)?
            .spawn()
            .context("Could not start provider")?,
        status: None,
    };
    emit(ProviderEvent::Spawned {
        group: process.child.id(),
        started: processes::process_start(process.child.id() as i32),
    });
    let watching = watch.insert(Watch::start(process.child.id() as i32, &turn.run_id));
    let mut codex_compaction = match turn.kind {
        TurnKind::CodexCompaction { before } => Some(codex_compaction::Messages::new(before)),
        _ => None,
    };
    // When a Codex compaction is out of time, counted from its process's start. It holds for
    // as long as the server runs, also once the server has closed its output.
    let deadline = codex_compaction
        .as_ref()
        .map(|_| Instant::now() + codex_compaction::limit());
    let out_of_time = || deadline.is_some_and(|deadline| Instant::now() >= deadline);
    let mut stdin = process.child.stdin.take().context("No provider stdin")?;
    // Images can make the input large; writing it alongside reading output cannot deadlock.
    // A provider that exits before reading it all is reported by its exit status. The writer
    // hands the input back when it is done, and dropping its handle closes it.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
        stdin
    });
    // Codex's app-server exits when its input closes, so a compaction holds it open until its
    // outcome is read. Any other run closes it as soon as it is written.
    let mut open_input = codex_compaction.is_some().then_some(writer);
    // When a Codex server that has not exited by itself is stopped: a margin after its outcome
    // is read and its input closed, and at once when its time is up.
    let mut stop_at: Option<Instant> = None;
    let due = |at: Option<Instant>| at.is_some_and(|at| Instant::now() >= at);
    let errors =
        constrained::Errors::spawn(process.child.stderr.take().context("No provider stderr")?);
    let stdout = process.child.stdout.take().context("No provider stdout")?;
    let output = Output::spawn(stdout)?;
    let _stop_reading = StopReading(output.clone());
    let compaction = matches!(turn.kind, TurnKind::Compaction(_));
    let sort = matches!(turn.kind, TurnKind::Sort);
    // A sorting run's answer counts only after its start reported nothing to act with.
    let mut constrained = constrained::Run::new("sorting");
    let mut terminal = false;
    let mut failure = None;
    let mut usage = Value::Null;
    let mut requests = crate::telemetry::ClaudeRequests::default();
    let mut claude = ClaudeSteps::new(&turn.cwd);
    let mut codex = CodexSteps::new(&turn.cwd);
    // A Codex compaction's outcome has been read, or its time is up: nothing its server says
    // or exits with afterwards counts.
    let mut ended = false;
    // The exit is checked on every pass, so a leftover process writing to stdout cannot hide
    // it. Every event the reader accepted is handled; the reader decides when to stop.
    let mut exited = false;
    loop {
        check_running(&cancel)?;
        report_seen(watching, emit);
        if !ended && out_of_time() {
            failure = Some(codex_compaction::out_of_time());
            ended = true;
            stop_at = Some(Instant::now());
        }
        if !exited && (due(stop_at) || process.exited()?) {
            drop(open_input.take());
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
            Err(_) if terminal || ended => continue,
            Err(error) => anyhow::bail!(error),
        };
        if let Some(messages) = &mut codex_compaction {
            if !ended && let Some(end) = messages.read(&value) {
                for record in end.records {
                    emit(ProviderEvent::Compaction(record));
                }
                failure = end.reported;
                ended = true;
                drop(open_input.take());
                stop_at = Some(Instant::now() + codex_compaction::EXIT_MARGIN);
            }
            continue;
        }
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
                // A compaction's requests are counted from its result record instead, and a
                // sorting run's too: its text could repeat a comment's.
                "stream_event" if !compaction && !sort => {
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
                // Anything a sorting run could act with ends it, and its answer with it.
                "system" if sort && value["subtype"] == "init" => {
                    constrained.event(&value)?;
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
                "system" if compaction && value["subtype"] == "status" => {
                    if let Some(result) = value["compact_result"].as_str() {
                        let status = if result == "success" {
                            Ok(())
                        } else {
                            Err(value["compact_error"].as_str().unwrap_or(result).to_owned())
                        };
                        emit(ProviderEvent::Compaction(CompactionRecord::Status(status)));
                    }
                }
                "system" if compaction && value["subtype"] == "compact_boundary" => {
                    let metadata = &value["compact_metadata"];
                    if let (Some(before), Some(after)) = (
                        metadata["pre_tokens"].as_u64(),
                        metadata["post_tokens"].as_u64(),
                    ) {
                        emit(ProviderEvent::Compaction(CompactionRecord::Boundary {
                            before,
                            after,
                        }));
                    }
                }
                // A compaction replays the summary as user records; they are not work steps. A
                // sorting run's steps are not kept: their text could repeat a comment's.
                "assistant" | "user" if !compaction && !sort => {
                    claude
                        .event(&value)
                        .into_iter()
                        .for_each(|step| emit(ProviderEvent::Step(step)));
                }
                "result" => {
                    terminal = true;
                    emit(ProviderEvent::ModelUsage(value["modelUsage"].clone()));
                    if compaction {
                        // The record's `usage` is zero for a compaction. These are Claude's
                        // totals for the session's whole life, kept as it reported them.
                        usage = json!({"modelUsage":value["modelUsage"],"total_cost_usd":value["total_cost_usd"]});
                    } else if sort {
                        if let Some(result) = constrained.event(&value)? {
                            usage = result.usage;
                            emit(ProviderEvent::Usage {
                                request_id: format!("turn:{}", turn.run_id),
                                model: Some(turn.profile.model.clone()),
                                usage: usage.clone(),
                                complete: true,
                            });
                            if let Some(answer) = result.answer {
                                emit(ProviderEvent::Reply(answer.to_string()));
                            }
                        }
                    } else {
                        usage = value["usage"].clone();
                        if value["is_error"] != true
                            && let Some(reply) = claude.reply(&value)
                        {
                            emit(ProviderEvent::Reply(reply));
                        }
                    }
                    if value["is_error"] == true && sort {
                        failure = Some(format!("The sorting run failed: {}", value["subtype"]));
                    } else if value["is_error"] == true {
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
    // stops its whole process group. A Codex server that does so can report no outcome any
    // more, and is stopped below when its time is up or its margin has passed.
    while !process.exited()? {
        check_running(&cancel)?;
        // What it starts meanwhile is recorded as it is seen, not when the turn ends.
        report_seen(watching, emit);
        if !ended && out_of_time() {
            failure = Some(codex_compaction::out_of_time());
            ended = true;
            break;
        }
        if due(stop_at) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Stops the helpers in the provider's group that outlived it.
    let status = process.stop()?;
    let stderr = errors.finish();
    if !ended && (!status.success() || !terminal) && failure.is_none() {
        failure = Some(format!(
            "Provider exited without a successful terminal result ({status}). {}",
            stderr.chars().take(1500).collect::<String>()
        ));
    }
    Ok(ProviderEvent::Finished {
        error: failure,
        usage,
        cancelled: false,
    })
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
        // Reaped: the group id is free for reuse, and stopping again signals nothing. Under
        // load the kernel takes a moment after the reap to let go of the group.
        let gone = || {
            let probe = unsafe { libc::killpg(group, 0) };
            (probe, std::io::Error::last_os_error().raw_os_error())
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while gone() != (-1, Some(libc::ESRCH)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(gone(), (-1, Some(libc::ESRCH)));
        assert_eq!(process.stop().unwrap().code(), Some(3));
    }

    #[test]
    fn claude_cannot_open_its_own_worktrees() {
        let turn = Turn {
            run_id: new_id(),
            kind: TurnKind::Prompt,
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
            instructions: String::new(),
        };
        let mut cmd = ProcessCommand::new("claude");
        configure(&mut cmd, &turn);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        let denied = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert_eq!(args[denied + 1], "EnterWorktree,ExitWorktree");
        assert!(args.iter().any(|a| a == "--allowedTools"));
    }

    #[test]
    fn a_resumed_turn_is_sent_the_instructions_it_was_built_with() {
        let mut claude = turn(Provider::Claude, Some("earlier"), vec![]);
        claude.instructions = "Changed instructions".into();
        let mut cmd = ProcessCommand::new("claude");
        configure(&mut cmd, &claude);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        let after = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        assert_eq!(after("--append-system-prompt"), "Changed instructions");
        assert_eq!(after("--system-prompt-snapshot"), "off");
        let mut codex = turn(Provider::Codex, Some("earlier"), vec![]);
        codex.instructions = "Changed instructions".into();
        let mut cmd = ProcessCommand::new("codex");
        configure(&mut cmd, &codex);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        assert!(args.contains(&r#"developer_instructions="Changed instructions""#.into()));
    }

    #[test]
    fn the_sorting_run_gets_the_keeper_core_and_no_skill() {
        assert!(keeper::SORTING.starts_with(core(Role::Keeper)));
        for line in skills::stock(Skill::KeepingAPr)
            .lines()
            .filter(|l| !l.is_empty())
        {
            assert!(!keeper::SORTING.contains(line), "{line}");
        }
    }

    #[test]
    fn a_merge_run_that_hangs_starts_with_tools_or_ends_without_an_answer_returns_nothing() {
        let init = |tools: &str| {
            format!(
                "print(json.dumps({{'type': 'system', 'subtype': 'init', 'tools': {tools}, 'mcp_servers': []}}), flush=True)"
            )
        };
        let result = "print(json.dumps({'type': 'result', 'subtype': 'success', 'is_error': False, 'structured_output': {'passages': []}, 'usage': {}}), flush=True)";
        let cases = [
            (
                format!("{}\ntime.sleep(30)", init("['StructuredOutput']")),
                "The model took too long to answer, so the run was stopped.",
            ),
            (
                format!("{}\n{result}", init("['StructuredOutput', 'Bash']")),
                r#"The merge run started with tools ["StructuredOutput","Bash"] and MCP servers []."#,
            ),
            (
                "print('Not signed in', file=sys.stderr); sys.exit(1)".into(),
                "Claude ended without an answer (exit status: 1). Not signed in",
            ),
            (
                "print('x' * (3 * 1024 * 1024), flush=True)".into(),
                "Provider event exceeds bound",
            ),
        ];
        let profile = ModelProfile {
            provider: Provider::Claude,
            model: "haiku".into(),
            effort: "low".into(),
        };
        for (body, error) in cases {
            let scratch = tempfile::tempdir().unwrap();
            let claude = scratch.path().join("claude");
            let script = format!("#!/usr/bin/env python3\nimport json, sys, time\n{body}\n");
            fs::write(&claude, script).unwrap();
            fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            let limit = Duration::from_secs(2);
            let started = Instant::now();
            let failed = merge_run(&claude, &profile, b"{}".to_vec(), limit)
                .err()
                .unwrap();
            assert_eq!(failed.to_string(), error);
            assert!(started.elapsed() < Duration::from_secs(10));
        }
    }

    #[test]
    fn a_sorting_run_has_no_tools_no_workspace_and_no_earlier_conversation() {
        let sort = Turn {
            kind: TurnKind::Sort,
            ..turn(Provider::Claude, Some("earlier"), vec![])
        };
        let mut cmd = ProcessCommand::new("claude");
        configure(&mut cmd, &sort);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into()).collect();
        let after = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap();
            args[at + 1].clone()
        };
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--json-schema"), keeper::ANSWER_SCHEMA);
        assert_eq!(after("--settings"), r#"{"disableAllHooks":true}"#);
        assert_eq!(after("--system-prompt"), keeper::SORTING);
        assert_eq!(after("--model"), "m");
        for flag in ["-p", "--strict-mcp-config", "--no-session-persistence"] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        for absent in [
            "--mcp-config",
            "--allowedTools",
            "--dangerously-skip-permissions",
            "--resume",
            "--append-system-prompt",
            "earlier",
        ] {
            assert!(!args.iter().any(|a| a == absent), "{absent}");
        }
        // The workspace's socket and token are taken out of the environment it inherits.
        let envs: Vec<_> = cmd.get_envs().collect();
        for name in ["WORKSPACE_SOCKET", "WORKSPACE_TOKEN"] {
            assert!(envs.contains(&(std::ffi::OsStr::new(name), None)), "{name}");
        }
        let codex = Turn {
            kind: TurnKind::Sort,
            ..turn(Provider::Codex, None, vec![])
        };
        let refused = command(&codex).unwrap_err().to_string();
        assert!(
            refused.starts_with("Only Claude can sort comments"),
            "{refused}"
        );
    }

    fn turn(provider: Provider, resume: Option<&str>, attachments: Vec<Attachment>) -> Turn {
        Turn {
            run_id: "run".into(),
            kind: TurnKind::Prompt,
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
            instructions: skill(Role::ProjectOrchestrator),
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
    fn a_compaction_sends_the_bare_command_with_a_turns_arguments() {
        let mut compaction = turn(Provider::Claude, Some("session"), vec![]);
        compaction.kind = TurnKind::Compaction(None);
        assert_eq!(input(&compaction).unwrap(), b"/compact");
        compaction.kind = TurnKind::Compaction(Some("the failing test names".into()));
        assert_eq!(
            input(&compaction).unwrap(),
            b"/compact the failing test names"
        );
        // Everything but the input is a turn's, so the summary request shares its prefix.
        let args = arguments(&compaction);
        assert_eq!(
            args,
            arguments(&turn(Provider::Claude, Some("session"), vec![]))
        );
        let after = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        assert_eq!(after("--resume"), "session");
        assert_eq!(after("--model"), "m");
        assert!(after("--mcp-config").contains("agent_workspace"));
        assert!(!args.iter().any(|a| a == "--input-format"));
    }

    #[test]
    fn a_codex_compaction_starts_the_app_server_with_a_turns_overrides_and_no_prompt() {
        let turn = turn(Provider::Codex, Some("thread"), vec![]);
        let mut compaction = self::turn(Provider::Codex, Some("thread"), vec![]);
        compaction.kind = TurnKind::CodexCompaction { before: None };
        let overrides = |turn: &Turn| -> Vec<String> {
            let args = arguments(turn);
            let pairs = args.iter().zip(args.iter().skip(1));
            let values = pairs.filter(|(flag, _)| *flag == "-c");
            values.map(|(_, value)| value.clone()).collect()
        };
        let args = arguments(&compaction);
        // The reasoning effort, the role instructions and the workspace's tool server are a
        // turn's own, so the summary request shares the turns' prefix.
        let shared = overrides(&turn);
        assert_eq!(overrides(&compaction), shared);
        for (value, name) in shared.iter().zip([
            "model_reasoning_effort=",
            "developer_instructions=",
            "mcp_servers.agent_workspace=",
        ]) {
            assert!(value.starts_with(name), "{value}");
        }
        // Nothing else: `exec`'s flags are not the app-server's, and there is no prompt.
        assert_eq!(args[0], "app-server");
        assert_eq!(args.len(), 1 + 2 * shared.len());
        for exec_only in ["exec", "resume", "thread", "-", "-m", "--json"] {
            assert!(!args.iter().any(|a| a == exec_only), "{exec_only}");
        }
    }

    #[test]
    fn a_codex_compaction_writes_four_requests_and_resumes_the_thread_as_a_turn_runs_it() {
        let mut compaction = turn(Provider::Codex, Some("thread"), vec![]);
        compaction.kind = TurnKind::CodexCompaction { before: None };
        let written = String::from_utf8(input(&compaction).unwrap()).unwrap();
        assert!(written.ends_with('\n'));
        let lines: Vec<Value> = written
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(
            (&lines[0]["id"], &lines[0]["method"]),
            (&json!(1), &json!("initialize"))
        );
        assert_eq!(lines[0]["params"]["clientInfo"]["name"], "wiffletree");
        assert_eq!(lines[1], json!({"method":"initialized"}));
        assert_eq!(
            lines[2],
            json!({"id":2,"method":"thread/resume","params":{
                "threadId":"thread",
                "cwd":"/tmp",
                "model":"m",
                "sandbox":"danger-full-access",
                "approvalPolicy":"never",
                "excludeTurns":true,
            }})
        );
        assert_eq!(
            lines[3],
            json!({"id":3,"method":"thread/compact/start","params":{"threadId":"thread"}})
        );
        // An agent that has run no turn has no thread to compact.
        compaction.provider_session = None;
        assert!(input(&compaction).is_err());
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
    fn providers_and_the_tool_commands_they_start_run_at_low_priority() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("provider");
        // The provider prints its own niceness, then starts a tool command the way a Bash tool
        // does, leading its own process group, which prints its pid, group and niceness.
        fs::write(
            &script,
            "#!/bin/sh\nps -o nice= -p $$\nset -m\nsh -c 'ps -o pid=,pgid=,nice= -p $$' &\nwait\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        for provider in [Provider::Claude, Provider::Codex] {
            let mut cmd = ProcessCommand::new(&script);
            configure(&mut cmd, &turn(provider, None, vec![]));
            let output = cmd.stdin(Stdio::null()).output().unwrap();
            let output = String::from_utf8(output.stdout).unwrap();
            let lines: Vec<Vec<&str>> = output
                .lines()
                .map(|line| line.split_whitespace().collect())
                .collect();
            let [own, tool] = lines.as_slice() else {
                panic!("{provider:?}: {output}");
            };
            let [pid, group, tool] = tool.as_slice() else {
                panic!("{provider:?}: {output}");
            };
            assert_eq!(own.as_slice(), ["10"], "{provider:?}");
            assert_eq!(
                pid, group,
                "{provider:?}: the tool command leads its own group"
            );
            assert_eq!(*tool, "10", "{provider:?}");
        }
    }

    /// A stand-in provider that prints the descriptors it holds, one per line. `ls` lists its
    /// own table: everything the provider passed on, plus the two it opens to read `/dev/fd`.
    fn descriptor_lister(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("provider");
        fs::write(&script, "#!/bin/sh\nexec /bin/ls /dev/fd\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[test]
    fn providers_started_together_hold_only_their_own_descriptors() {
        const PROVIDERS: usize = 8;
        const ROUNDS: usize = 25;
        let dir = tempfile::tempdir().unwrap();
        let script = descriptor_lister(dir.path());
        let turn = turn(Provider::Claude, None, vec![]);
        // Every round starts all of them at once. Nothing may panic between two waits, or the
        // others would wait for it forever.
        let together = std::sync::Barrier::new(PROVIDERS + 1);
        let (outputs, git) = std::thread::scope(|scope| {
            let providers: Vec<_> = (0..PROVIDERS)
                .map(|_| {
                    scope.spawn(|| {
                        let mut outputs = vec![];
                        for _ in 0..ROUNDS {
                            let mut cmd = ProcessCommand::new(&script);
                            configure(&mut cmd, &turn);
                            together.wait();
                            // Waiting for the output reaps the child, which exits on its own.
                            outputs.push(cmd.spawn().and_then(|child| child.wait_with_output()));
                        }
                        outputs
                    })
                })
                .collect();
            // Git queries make and close pipes of their own at the same moments.
            let git = scope.spawn(|| {
                let mut answers = vec![];
                for _ in 0..ROUNDS {
                    let mut git = ProcessCommand::new("git");
                    git.arg("--version");
                    together.wait();
                    let timeout = Duration::from_secs(30);
                    answers.push(crate::runtime::bounded_output(git, timeout, None));
                }
                answers
            });
            let outputs: Vec<_> = providers
                .into_iter()
                .flat_map(|provider| provider.join().unwrap())
                .collect();
            (outputs, git.join().unwrap())
        });
        for answer in git {
            assert!(answer.unwrap().0.success());
        }
        let held: Vec<_> = outputs
            .into_iter()
            .map(|output| String::from_utf8(output.unwrap().stdout).unwrap())
            .collect();
        assert_eq!(held.len(), PROVIDERS * ROUNDS);
        let own = "0\n1\n2\n3\n4\n";
        let leaked: Vec<_> = held.iter().filter(|held| *held != own).collect();
        assert!(
            leaked.is_empty(),
            "{} of {} providers held descriptors they were not given: {leaked:?}",
            leaked.len(),
            held.len()
        );
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
