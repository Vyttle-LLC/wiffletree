//! Read-only feasibility probes. Provider inference is intentionally absent.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
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
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Queries must not take index.lock in checkouts people and agents are working in.
        .env("GIT_OPTIONAL_LOCKS", "0");
    let (status, output, error) = bounded_output(command, GIT_TIMEOUT, input)?;
    let error = String::from_utf8_lossy(&error);
    let lines = || error.lines().map(str::trim).filter(|l| !l.is_empty());
    let error = lines()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or_else(|| lines().next_back())
        .unwrap_or_default()
        .to_owned();
    Ok(GitAnswer {
        status,
        output: String::from_utf8(output)?,
        error,
    })
}

const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const OUTPUT_BOUND: u64 = 4 * 1024 * 1024;

/// Runs the command with one deadline for its output and its exit; a command still running
/// at the deadline is killed and reaped.
fn bounded_output(
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
    fn a_command_that_closes_its_output_then_hangs_is_killed_at_the_deadline() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec >&- 2>&-; exec sleep 30"]);
        let started = Instant::now();
        let error = bounded_output(command, Duration::from_secs(1), None).unwrap_err();
        assert_eq!(error.to_string(), "Git query timed out");
        assert!(started.elapsed() < Duration::from_secs(5));
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
