//! Read-only feasibility probes. Provider inference is intentionally absent.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use workspace_core::{ModelCapability, Provider};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub trait OwnDescriptors {
    /// Leaves the child only its standard input, output and error. macOS cannot make a pipe
    /// that is close-on-exec from the start, so a child started while another thread is making
    /// one would otherwise hold it for as long as it and everything it started run.
    fn own_descriptors_only(&mut self) -> &mut Self;
}
impl OwnDescriptors for Command {
    fn own_descriptors_only(&mut self) -> &mut Self {
        // SAFETY: `close_others_on_exec` is async-signal-safe.
        unsafe {
            self.pre_exec(|| {
                close_others_on_exec::<1024>();
                Ok(())
            })
        }
    }
}

/// How nice every provider process runs, so agents' builds and tests yield to the desktop.
const AGENT_NICENESS: libc::c_int = 10;

pub(crate) trait LowPriority {
    /// Runs the child, and so everything it starts, at `AGENT_NICENESS`. A niceness is
    /// inherited across fork and exec, so it reaches tool commands that lead their own process
    /// group or session too.
    fn at_low_priority(&mut self) -> &mut Self;
}
impl LowPriority for Command {
    fn at_low_priority(&mut self) -> &mut Self {
        // SAFETY: `setpriority` is async-signal-safe. Its failure is ignored: it fails only for a
        // host already running nicer than this, which the child then inherits.
        unsafe {
            self.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS as _, 0, AGENT_NICENESS);
                Ok(())
            })
        }
    }
}

/// Marks every descriptor above stderr close-on-exec, from a list of up to `LISTED` of them.
///
/// It runs between fork and exec in a copy of a process that had other threads, any of which
/// may have held a lock, the allocator's included. So it only makes system calls: it must not
/// allocate, lock, log or panic.
fn close_others_on_exec<const LISTED: usize>() {
    let close_on_exec = |descriptor| unsafe {
        libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC);
    };
    #[cfg(target_os = "macos")]
    {
        let mut open: [libc::proc_fdinfo; LISTED] = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&open) as libc::c_int;
        let list = libc::PROC_PIDLISTFDS;
        let read =
            unsafe { libc::proc_pidinfo(libc::getpid(), list, 0, open.as_mut_ptr().cast(), size) };
        // A list that filled the buffer may be a cut one. That and a failed call mark every
        // possible descriptor below, so none stays inheritable.
        if (1..size).contains(&read) {
            let listed = read as usize / std::mem::size_of::<libc::proc_fdinfo>();
            let above_stderr = open
                .iter()
                .take(listed)
                .map(|d| d.proc_fd)
                .filter(|d| *d > 2);
            return above_stderr.for_each(close_on_exec);
        }
    }
    (3..unsafe { libc::getdtablesize() }).for_each(close_on_exec);
}

pub fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let answer = git_answer(path, args)?;
    ensure!(
        answer.status.success(),
        "Git query failed{}",
        answer.reason()
    );
    Ok(answer.output)
}

/// A finished Git query. Its exit status can be part of the answer, as with `merge-tree`.
pub struct GitAnswer {
    pub status: ExitStatus,
    pub output: String,
    /// Git's first `fatal:` or `error:` line, or else the last line of its error output.
    pub error: String,
}
impl GitAnswer {
    /// The error line as a suffix for a failure message, or nothing.
    pub fn reason(&self) -> String {
        if self.error.is_empty() {
            String::new()
        } else {
            format!(": {}", self.error)
        }
    }
}

pub fn git_answer(path: &Path, args: &[&str]) -> Result<GitAnswer> {
    git_query(path, args, None)
}

/// `git_answer` with `input` on Git's standard input, as `git patch-id` reads it.
pub fn git_answer_with_input(path: &Path, args: &[&str], input: &str) -> Result<GitAnswer> {
    git_query(path, args, Some(input.as_bytes().to_vec()))
}

fn git_query(path: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<GitAnswer> {
    let mut command = git_command(path, args);
    // Queries must not take index.lock in checkouts people and agents are working in.
    command.env("GIT_OPTIONAL_LOCKS", "0");
    git_run(command, GIT_TIMEOUT, input)
}

/// Runs `git push` without prompts. A push runs the repository's pre-push hooks, so it gets
/// minutes where a query gets seconds.
pub fn git_push(path: &Path, args: &[&str]) -> Result<GitAnswer> {
    git_run(git_command(path, args), PUSH_TIMEOUT, None)
}

fn git_command(path: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn git_run(command: Command, timeout: Duration, input: Option<Vec<u8>>) -> Result<GitAnswer> {
    let (status, output, error) = bounded_output(command, timeout, input)?;
    Ok(GitAnswer {
        status,
        output: String::from_utf8(output)?,
        error: error_line(&error),
    })
}

/// A Git query's output as bytes, as file contents are, with `input` on its standard input.
pub fn git_bytes(path: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut command = git_command(path, args);
    command.env("GIT_OPTIONAL_LOCKS", "0");
    let (status, output, error) = bounded_output(command, GIT_TIMEOUT, Some(input.to_vec()))?;
    ensure!(
        status.success(),
        "git {} failed: {}",
        args[0],
        error_line(&error)
    );
    Ok(output)
}

fn error_line(error: &[u8]) -> String {
    let error = String::from_utf8_lossy(error);
    let lines = || error.lines().map(str::trim).filter(|l| !l.is_empty());
    lines()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or_else(|| lines().next_back())
        .unwrap_or_default()
        .to_owned()
}

/// Runs GitHub's `gh` in `path` without prompts and returns its output, or its last error line.
pub fn gh_output(path: &Path, args: &[&str]) -> Result<String> {
    tool_output("gh", path, args)
}

/// A finished `gh` call. Its output can matter although it failed: `gh api graphql` exits
/// non-zero when one alias cannot be resolved and still prints the others' data.
pub struct GhAnswer {
    pub status: ExitStatus,
    pub output: String,
    /// The last line of its error output.
    pub error: String,
}

/// `gh_output` that also answers for a call that exited non-zero.
pub fn gh_answer(path: &Path, args: &[&str]) -> Result<GhAnswer> {
    tool_answer("gh", path, args)
}

fn tool_output(program: &str, path: &Path, args: &[&str]) -> Result<String> {
    let answer = tool_answer(program, path, args)?;
    ensure!(
        answer.status.success(),
        "{program} failed: {}",
        match answer.error.as_str() {
            "" => "no error output",
            error => error,
        }
    );
    Ok(answer.output)
}

fn tool_answer(program: &str, path: &Path, args: &[&str]) -> Result<GhAnswer> {
    let mut command = Command::new(program);
    command
        .current_dir(path)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    let (status, output, error) = bounded_output(command, GH_TIMEOUT, None)
        .with_context(|| format!("{program} could not run"))?;
    let error = String::from_utf8_lossy(&error);
    let last = error.lines().map(str::trim).rfind(|l| !l.is_empty());
    Ok(GhAnswer {
        status,
        output: String::from_utf8(output)?,
        error: last.unwrap_or_default().to_owned(),
    })
}

const GH_TIMEOUT: Duration = Duration::from_secs(30);
const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const PUSH_TIMEOUT: Duration = Duration::from_secs(600);
const OUTPUT_BOUND: u64 = 4 * 1024 * 1024;

/// Runs the command with one deadline for its output and its exit; a command still running
/// at the deadline is killed and reaped.
pub(crate) fn bounded_output(
    mut command: Command,
    timeout: Duration,
    input: Option<Vec<u8>>,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    let deadline = Instant::now() + timeout;
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = Process(
        command
            .own_descriptors_only()
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    if let (Some(input), Some(mut pipe)) = (input, child.0.stdin.take()) {
        // Written beside the readers so a full output pipe cannot stall the input.
        std::thread::spawn(move || pipe.write_all(&input));
    }
    let read = |pipe: Option<_>| -> Result<mpsc::Receiver<std::io::Result<Vec<u8>>>> {
        let pipe: Box<dyn Read + Send> = pipe.context("Missing Git pipe")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe
                .take(OUTPUT_BOUND + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = sender.send(result);
        });
        Ok(receiver)
    };
    let stdout = read(child.0.stdout.take().map(|p| Box::new(p) as _))?;
    let stderr = read(child.0.stderr.take().map(|p| Box::new(p) as _))?;
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let output = stdout
        .recv_timeout(remaining())
        .context("Git query timed out")??;
    ensure!(
        output.len() as u64 <= OUTPUT_BOUND,
        "Git output exceeds bound"
    );
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        ensure!(!remaining().is_zero(), "Git query timed out");
        std::thread::sleep(Duration::from_millis(5));
    };
    // Error output is only a hint; a descendant still holding it open must not hold us.
    let error = stderr
        .recv_timeout(remaining().min(Duration::from_millis(100)))
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    Ok((status, output, error))
}

/// Runs `command` in a process group of its own, with no input and its output discarded, for
/// at most `limit`. Returns its exit status, or `None` when it ran out of time. Either way every
/// process left in the group is killed, so nothing it started outlives it.
pub(crate) fn run_limited(mut command: Command, limit: Duration) -> Result<Option<ExitStatus>> {
    let mut child = command
        .own_descriptors_only()
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let group = child.id() as libc::pid_t;
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // SAFETY: `killpg` only sends a signal; the group is the child's own.
    unsafe { libc::killpg(group, libc::SIGKILL) };
    if status.is_none() {
        child.wait()?;
    }
    Ok(status)
}

#[derive(Clone, Debug)]
pub struct ModelOption {
    pub provider: Provider,
    pub model: String,
    pub label: String,
    pub efforts: Vec<String>,
}

pub fn models(provider: Provider) -> Result<Vec<ModelOption>> {
    match provider {
        Provider::Claude => claude_models(),
        Provider::Codex => codex_catalog(),
    }
}

fn json_lines(stdout: impl Read + Send + 'static) -> mpsc::Receiver<serde_json::Result<Value>> {
    let (sender, receiver) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            let result = reader
                .by_ref()
                .take(2 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut bytes);
            match result {
                Ok(0) => break,
                Ok(_) if bytes.len() <= 2 * 1024 * 1024 => {
                    if sender
                        .send(serde_json::from_slice::<Value>(&bytes))
                        .is_err()
                    {
                        break;
                    }
                }
                _ => break,
            }
        }
    });
    receiver
}

pub fn codex_models() -> Result<Vec<ModelCapability>> {
    Ok(codex_catalog()?
        .into_iter()
        .map(|model| ModelCapability {
            provider: model.provider,
            model: model.model,
            efforts: model.efforts,
        })
        .collect())
}

fn with_codex<T>(
    read: impl FnOnce(&mut dyn FnMut(u64, &str, Value) -> Result<Value>) -> Result<T>,
) -> Result<T> {
    let mut child = Process(
        Command::new(crate::provider::executable(Provider::Codex)?)
            .own_descriptors_only()
            .at_low_priority()
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Install Codex CLI to probe app-server")?,
    );
    let mut input = child.0.stdin.take().context("Missing provider stdin")?;
    let stdout = child.0.stdout.take().context("Missing provider stdout")?;
    let receiver = json_lines(stdout);
    let mut request = |id: u64, method: &str, params: Value| -> Result<Value> {
        writeln!(
            input,
            "{}",
            json!({"id":id,"method":method,"params":params})
        )?;
        input.flush()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let value = receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .context("App-server response timed out")??;
            if value.get("id") == Some(&json!(id)) {
                ensure!(
                    value.get("error").is_none(),
                    "App-server request failed: {}",
                    value["error"]
                );
                if method == "initialize" {
                    writeln!(input, "{}", json!({"method":"initialized"}))?;
                    input.flush()?;
                }
                return Ok(value["result"].clone());
            }
        }
    };
    request(
        1,
        "initialize",
        json!({"clientInfo":{"name":"agent_workspace_probe","title":"Wiffletree feasibility","version":"0.1.0"},"capabilities":{"experimentalApi":false}}),
    )?;
    read(&mut request)
}

pub fn codex_quota() -> Result<workspace_core::QuotaReading> {
    with_codex(|request| {
        let result = request(2, "account/rateLimits/read", json!({}))?;
        let mut reading = crate::telemetry::codex_quota(&result, crate::now())?;
        if reading.account.is_none()
            && let Ok(account) = request(3, "account/read", json!({"refreshToken":false}))
        {
            reading.account = account["account"]["email"].as_str().map(str::to_owned);
        }
        Ok(reading)
    })
}

pub fn claude_account(cwd: &Path) -> Result<Option<String>> {
    let mut child = Process(
        Command::new(crate::provider::executable(Provider::Claude)?)
            .own_descriptors_only()
            .at_low_priority()
            .current_dir(cwd)
            .args(["auth", "status", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let stdout = child.0.stdout.take().context("Missing provider stdout")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(65_537).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let bytes = receiver
        .recv_timeout(Duration::from_secs(5))
        .context("Authentication status timed out")??;
    ensure!(bytes.len() <= 65_536, "Authentication status exceeds bound");
    let value: Value = serde_json::from_slice(&bytes)?;
    Ok(value["email"].as_str().map(|email| {
        value["orgId"]
            .as_str()
            .map(|org| format!("{email} · {org}"))
            .unwrap_or_else(|| email.to_owned())
    }))
}

fn codex_catalog() -> Result<Vec<ModelOption>> {
    with_codex(|request| {
        let mut models = Vec::new();
        let mut cursor = Value::Null;
        for page in 0..10 {
            let result = request(
                2 + page,
                "model/list",
                json!({"limit":100,"cursor":cursor,"includeHidden":false}),
            )?;
            let data = result["data"].as_array().context("Invalid model catalog")?;
            for model in data {
                models.push(codex_option(model)?);
            }
            cursor = result["nextCursor"].clone();
            if cursor.is_null() {
                return Ok(models);
            }
        }
        anyhow::bail!("Model catalog exceeds page bound")
    })
}

fn codex_option(model: &Value) -> Result<ModelOption> {
    let id = model["model"].as_str().context("Missing model ID")?;
    Ok(ModelOption {
        provider: Provider::Codex,
        model: id.into(),
        label: model["displayName"]
            .as_str()
            .filter(|name| *name != id)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                id.split('-')
                    .map(|part| {
                        if part == "gpt" {
                            return "GPT".to_owned();
                        }
                        let mut chars = part.chars();
                        chars
                            .next()
                            .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        efforts: model["supportedReasoningEfforts"]
            .as_array()
            .context("Missing supported efforts")?
            .iter()
            .filter_map(|effort| effort["reasoningEffort"].as_str().map(str::to_owned))
            .collect(),
    })
}

fn claude_models() -> Result<Vec<ModelOption>> {
    let mut child = Process(
        Command::new(crate::provider::executable(Provider::Claude)?)
            .own_descriptors_only()
            .at_low_priority()
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--tools",
                "",
                "--no-session-persistence",
                "--safe-mode",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut input = child.0.stdin.take().context("Missing provider stdin")?;
    let receiver = json_lines(child.0.stdout.take().context("Missing provider stdout")?);
    // Initialize only: no user message, inference, tools, hooks or persisted conversation.
    writeln!(
        input,
        "{}",
        json!({"type":"control_request","request_id":"models",
        "request":{"subtype":"initialize","hooks":{}}})
    )?;
    input.flush()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let value = receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .context("Claude model catalog timed out")??;
        if value["type"] == "control_response" && value["response"]["request_id"] == "models" {
            ensure!(
                value["response"]["subtype"] == "success",
                "Claude model catalog failed"
            );
            return claude_options(&value["response"]["response"]["models"]);
        }
    }
}

fn claude_options(data: &Value) -> Result<Vec<ModelOption>> {
    let mut models = Vec::new();
    for item in data.as_array().context("Missing Claude model catalog")? {
        let alias = item["value"].as_str().context("Missing Claude model ID")?;
        // Provider-chosen defaults and hybrid routing do not identify an approved model family.
        if matches!(alias, "default" | "best" | "opusplan") {
            continue;
        }
        let name = item["displayName"].as_str().unwrap_or(alias);
        let efforts = item["supportedEffortLevels"]
            .as_array()
            .map(|levels| {
                levels
                    .iter()
                    .filter_map(|level| level.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        models.push(ModelOption {
            provider: Provider::Claude,
            model: alias.into(),
            label: if alias.starts_with("claude-") {
                name.into()
            } else {
                format!("{name} · latest")
            },
            efforts,
        });
        if let Some(id) = item["resolvedModel"].as_str().filter(|id| *id != alias) {
            models.push(ModelOption {
                provider: Provider::Claude,
                model: id.into(),
                label: format!("{} · pinned", claude_version_label(id)),
                efforts: models.last().unwrap().efforts.clone(),
            });
        }
    }
    ensure!(!models.is_empty(), "Claude returned no selectable models");
    Ok(models)
}

fn claude_version_label(id: &str) -> String {
    let Some(rest) = id.strip_prefix("claude-") else {
        return id.into();
    };
    let mut parts = rest.split('-');
    let name = parts.next().unwrap_or(rest);
    let mut chars = name.chars();
    let name = chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default();
    let version = parts
        .take_while(|part| part.len() != 8)
        .collect::<Vec<_>>()
        .join(".");
    format!("{name} {version}").trim().to_owned()
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn a_child_whose_descriptor_list_may_be_cut_short_marks_every_descriptor() {
        // More inheritable descriptors than a list of four can hold, so the list fills and the
        // child marks every number up to its limit instead.
        let mut pipes = [[0; 2]; 4];
        for ends in &mut pipes {
            assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        }
        let mut ls = Command::new("/bin/ls");
        ls.arg("/dev/fd");
        // SAFETY: as in `own_descriptors_only`.
        unsafe {
            ls.pre_exec(|| {
                close_others_on_exec::<4>();
                Ok(())
            })
        };
        let output = ls.output();
        for descriptor in pipes.iter().flatten() {
            unsafe { libc::close(*descriptor) };
        }
        // `ls` holds the standard three and the two it opens to read `/dev/fd`.
        let held = String::from_utf8(output.unwrap().stdout).unwrap();
        assert_eq!(held, "0\n1\n2\n3\n4\n");
    }

    #[test]
    fn a_command_that_cannot_run_gives_an_error() {
        let command = Command::new("/nonexistent/cli");
        assert!(bounded_output(command, Duration::from_secs(1), None).is_err());
    }

    #[test]
    fn a_finished_command_gives_both_streams() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "echo out; echo err >&2"]);
        let (_, stdout, stderr) = bounded_output(command, Duration::from_secs(10), None).unwrap();
        assert_eq!(
            (stdout.as_slice(), stderr.as_slice()),
            (&b"out\n"[..], &b"err\n"[..])
        );
    }

    #[test]
    fn a_hanging_command_is_given_up_on_at_the_deadline_and_killed() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            &format!("echo $$ > '{}'; exec sleep 30", pid_file.display()),
        ]);
        let started = Instant::now();
        assert!(bounded_output(command, Duration::from_millis(300), None).is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "the hanging command was left running");
    }

    #[test]
    fn a_command_that_closes_its_output_then_hangs_is_killed_at_the_deadline() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec >&- 2>&-; exec sleep 30"]);
        let started = Instant::now();
        let error = bounded_output(command, Duration::from_secs(1), None).unwrap_err();
        assert_eq!(error.to_string(), "Git query timed out");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_missing_or_failing_gh_says_why() {
        let folder = tempfile::tempdir().unwrap();
        let missing = tool_output("wiffletree-no-such-gh", folder.path(), &[]).unwrap_err();
        assert!(
            format!("{missing:#}").starts_with("wiffletree-no-such-gh could not run: "),
            "{missing:#}"
        );
        let failed = tool_output(
            "sh",
            folder.path(),
            &[
                "-c",
                "echo 'first' >&2; echo 'To get started, run: gh auth login' >&2; exit 4",
            ],
        )
        .unwrap_err();
        assert_eq!(
            failed.to_string(),
            "sh failed: To get started, run: gh auth login"
        );
    }

    #[test]
    fn a_failed_call_still_answers_with_its_output() {
        let folder = tempfile::tempdir().unwrap();
        let answer = tool_answer(
            "sh",
            folder.path(),
            &[
                "-c",
                "echo '{\"data\":{}}'; echo 'gh: Could not resolve' >&2; exit 1",
            ],
        )
        .unwrap();
        assert!(!answer.status.success());
        assert_eq!(answer.output.trim(), r#"{"data":{}}"#);
        assert_eq!(answer.error, "gh: Could not resolve");
    }

    #[test]
    fn a_failing_push_gives_gits_error_and_a_hanging_one_is_killed_at_its_deadline() {
        let folder = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let answer = git_answer(folder.path(), args).unwrap();
            assert!(answer.status.success(), "{}", answer.error);
        };
        git(&["init", "-q", "-b", "main"]);
        git(&[
            "-c",
            "user.name=F",
            "-c",
            "user.email=f@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "start",
        ]);
        let missing = folder.path().join("no-such-remote.git");
        let args = ["push", missing.to_str().unwrap(), "HEAD:refs/heads/main"];
        let failed = git_push(folder.path(), &args).unwrap();
        assert!(!failed.status.success());
        assert!(failed.error.starts_with("fatal: "), "{}", failed.error);
        // A pre-push hook that never returns is given up on. Git's kill does not reach the
        // hook, so it waits only for as long as Git runs and cannot outlive the test.
        let remote = folder.path().join("remote.git");
        git(&["init", "-q", "--bare", remote.to_str().unwrap()]);
        let hook = folder.path().join(".git/hooks/pre-push");
        std::fs::write(
            &hook,
            "#!/bin/sh\nwhile kill -0 $PPID 2>/dev/null; do sleep 0.1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let args = ["push", remote.to_str().unwrap(), "HEAD:refs/heads/main"];
        let started = Instant::now();
        let hung = git_run(
            git_command(folder.path(), &args),
            Duration::from_millis(500),
            None,
        );
        assert_eq!(hung.err().unwrap().to_string(), "Git query timed out");
        assert!(started.elapsed() < Duration::from_secs(10));
        let refs = git_output(&remote, &["for-each-ref"]).unwrap();
        assert_eq!(refs, "", "the killed push sent nothing");
    }

    #[test]
    fn a_failed_git_query_says_why() {
        let folder = tempfile::tempdir().unwrap();
        let error = git_output(folder.path(), &["rev-parse", "HEAD"]).unwrap_err();
        assert!(
            error.to_string().starts_with("Git query failed: fatal:"),
            "{error}"
        );
    }

    #[test]
    fn claude_catalog_exposes_pinned_versions_and_skips_automatic_routing() {
        let options = claude_options(&json!([
            {"value":"default", "resolvedModel":"claude-opus-5-5"},
            {"value":"opus", "resolvedModel":"claude-opus-5-5", "displayName":"Opus",
             "description":"Opus 5.5 · Complex tasks", "supportedEffortLevels":["high","max"]},
            {"value":"opusplan"}
        ]))
        .unwrap();
        assert_eq!(
            options.iter().map(|m| m.model.as_str()).collect::<Vec<_>>(),
            ["opus", "claude-opus-5-5"]
        );
        assert_eq!(options[1].label, "Opus 5.5 · pinned");
        assert_eq!(options[1].efforts, ["high", "max"]);
        assert!(claude_options(&json!({})).is_err());
        let fixed = claude_options(
            &json!([{"value":"claude-sonnet-5", "displayName":"Sonnet 5",
            "description":"Efficient for routine tasks"}]),
        )
        .unwrap();
        assert_eq!(fixed[0].label, "Sonnet 5");
        assert_eq!(
            claude_version_label("claude-haiku-4-5-20251001"),
            "Haiku 4.5"
        );
    }

    #[test]
    fn codex_catalog_keeps_the_exact_model_and_supported_efforts() {
        let model = codex_option(&json!({"model":"gpt-6.1-sol", "displayName":"GPT 6.1 Sol",
            "supportedReasoningEfforts":[{"reasoningEffort":"high"},{"reasoningEffort":"ultra"}]}))
        .unwrap();
        assert_eq!(model.model, "gpt-6.1-sol");
        assert_eq!(model.label, "GPT 6.1 Sol");
        assert_eq!(model.efforts, ["high", "ultra"]);
        assert!(codex_option(&json!({"displayName":"Missing ID"})).is_err());
    }
}
