//! What occupies a provider session's context window. Read-only: no inference, no session writes.
//!
//! Claude reports its own categories through `/context`. Codex reports only how full the window
//! is, so its categories are estimated from the sizes of the items in its session record.
use crate::provider::{executable, skill};
use crate::runtime::{LowPriority as _, OwnDescriptors as _};
use anyhow::{Context as _, Result, bail, ensure};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use workspace_core::{ModelProfile, Provider, Role};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextCategory {
    pub name: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextUsage {
    pub model: Option<String>,
    pub used: u64,
    pub window: u64,
    /// Where the provider compacts the conversation, when it says.
    pub compacts_at: Option<u64>,
    pub categories: Vec<ContextCategory>,
    /// True when the categories are sized by this app rather than reported by the provider.
    pub estimated: bool,
    pub observed_at: i64,
}

/// The session to inspect, as the host last ran it.
pub struct Probe<'a> {
    pub role: Role,
    pub profile: &'a ModelProfile,
    pub provider_session: &'a str,
    pub directory: &'a Path,
}

pub fn usage(probe: &Probe) -> Result<ContextUsage> {
    match probe.profile.provider {
        Provider::Claude => claude(probe),
        Provider::Codex => codex(probe.provider_session, &codex_home()?),
    }
}

/// Reads token counts such as `24.3k`, `1m`, `601` and `~50`.
fn tokens(text: &str) -> Option<u64> {
    let text = text.trim().trim_start_matches('~').to_ascii_lowercase();
    let (number, scale) = match text.chars().last()? {
        'k' => (&text[..text.len() - 1], 1e3),
        'm' => (&text[..text.len() - 1], 1e6),
        _ => (text.as_str(), 1.),
    };
    Some((number.parse::<f64>().ok()? * scale).round() as u64)
}

fn claude(probe: &Probe) -> Result<ContextUsage> {
    // The same static inputs as a turn, so the system prompt and tools are sized alike. The
    // workspace's own tool server needs a live turn's credentials and is left out.
    let output = Command::new(executable(Provider::Claude)?)
        .own_descriptors_only()
        .at_low_priority()
        .current_dir(probe.directory)
        .args(["-p", "/context", "--output-format", "json"])
        .args(["--resume", probe.provider_session])
        .arg("--no-session-persistence")
        .args(["--model", &probe.profile.model])
        .args(["--append-system-prompt", &skill(probe.role)])
        .args([
            "--strict-mcp-config",
            "--mcp-config",
            r#"{"mcpServers":{}}"#,
        ])
        .args(["--tools", "default", "--no-chrome"])
        .stdin(Stdio::null())
        .output()
        .context("Run Claude context report")?;
    ensure!(
        output.status.success(),
        "Claude could not report this session’s context: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let report: Value = serde_json::from_slice(&output.stdout).context("Read context report")?;
    parse_claude(report["result"].as_str().context("Empty context report")?)
}

/// Parses the Markdown that `/context` prints: a totals line and a category table.
fn parse_claude(report: &str) -> Result<ContextUsage> {
    let field = |label: &str| {
        report
            .lines()
            .find_map(|line| line.strip_prefix(label))
            .map(str::trim)
    };
    let totals = field("**Tokens:**").context("Context report has no totals")?;
    let (used, rest) = totals
        .split_once('/')
        .context("Unreadable context totals")?;
    let window = rest.split_whitespace().next().unwrap_or_default();
    let mut usage = ContextUsage {
        model: field("**Model:**").map(str::to_owned),
        used: tokens(used).context("Unreadable context total")?,
        window: tokens(window).context("Unreadable context window")?,
        compacts_at: None,
        categories: vec![],
        estimated: false,
        observed_at: crate::now(),
    };
    let rows = report
        .lines()
        .skip_while(|line| !line.starts_with("| Category"))
        .skip(2)
        .take_while(|line| line.starts_with('|'));
    for row in rows {
        let mut cells = row.trim_matches('|').split('|').map(str::trim);
        let (Some(name), Some(size)) = (cells.next(), cells.next().and_then(tokens)) else {
            continue;
        };
        match name {
            "Free space" => {}
            "Autocompact buffer" => usage.compacts_at = Some(usage.window.saturating_sub(size)),
            // Deferred tools are listed but not loaded into the window.
            name if name.contains("(deferred)") => {}
            name => usage.categories.push(ContextCategory {
                name: name.into(),
                tokens: size,
            }),
        }
    }
    Ok(usage)
}

pub(crate) fn codex_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return Ok(home.into());
    }
    let home = std::env::var_os("HOME").context("Home directory unavailable")?;
    Ok(PathBuf::from(home).join(".codex"))
}

fn session_file(directory: &Path, thread: &str) -> Option<PathBuf> {
    let suffix = format!("{thread}.jsonl");
    let mut entries: Vec<_> = fs::read_dir(directory).ok()?.flatten().collect();
    // Newest dated folders first: the session is usually recent.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
    entries.into_iter().find_map(|entry| {
        let path = entry.path();
        if path.is_dir() {
            session_file(&path, thread)
        } else {
            path.to_string_lossy().ends_with(&suffix).then_some(path)
        }
    })
}

/// Characters of model-visible content in one session record item, by category.
fn codex_item(item: &Value) -> Option<(&'static str, usize)> {
    let length = |value: &Value| match value {
        Value::String(text) => text.len(),
        Value::Null => 0,
        other => other.to_string().len(),
    };
    let text = |value: &Value| {
        value
            .as_array()
            .into_iter()
            .flatten()
            .map(|part| length(&part["text"]))
            .sum::<usize>()
    };
    Some(match item["type"].as_str()? {
        "message" => match item["role"].as_str()? {
            "developer" | "system" => ("Instructions", text(&item["content"])),
            _ => ("Messages", text(&item["content"])),
        },
        "function_call" | "custom_tool_call" => (
            "Tool calls",
            length(&item["arguments"]) + length(&item["input"]),
        ),
        "function_call_output" | "custom_tool_call_output" => {
            ("Tool results", length(&item["output"]))
        }
        // Reasoning is carried forward encrypted; base64 inflates it by a third.
        "reasoning" => ("Reasoning", length(&item["encrypted_content"]) * 3 / 4),
        // A compaction's summary is carried the same way, in place of the messages it replaced.
        "compaction" => ("Messages", length(&item["encrypted_content"]) * 3 / 4),
        _ => return None,
    })
}

const CODEX_CATEGORIES: [&str; 6] = [
    "System prompt",
    "Instructions",
    "Messages",
    "Tool calls",
    "Tool results",
    "Reasoning",
];
const CHARACTERS_PER_TOKEN: usize = 4;

fn codex(thread: &str, home: &Path) -> Result<ContextUsage> {
    let Some(path) = session_file(&home.join("sessions"), thread) else {
        bail!("Codex has no session record for this conversation yet");
    };
    let mut characters = [0usize; CODEX_CATEGORIES.len()];
    let count = |characters: &mut [usize; CODEX_CATEGORIES.len()], item: &Value| {
        if let Some((name, size)) = codex_item(item)
            && let Some(index) = CODEX_CATEGORIES.iter().position(|c| *c == name)
        {
            characters[index] += size;
        }
    };
    let (mut used, mut window, mut model) = (0, 0, None);
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let Ok(record) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        let payload = &record["payload"];
        match record["type"].as_str().unwrap_or_default() {
            "session_meta" => {
                characters[0] = payload["base_instructions"]["text"]
                    .as_str()
                    .or(payload["base_instructions"].as_str())
                    .map_or(0, str::len)
            }
            // Compaction replaces the conversation so far with what it kept: the messages it
            // retained and a summary, which is an item of its own or the entry's `message`.
            "compacted" => {
                characters[1..].fill(0);
                characters[2] = payload["message"].as_str().map_or(0, str::len);
                let kept = payload["replacement_history"].as_array();
                for item in kept.into_iter().flatten() {
                    count(&mut characters, item);
                }
            }
            "turn_context" => model = payload["model"].as_str().map(str::to_owned).or(model),
            "response_item" => count(&mut characters, payload),
            "event_msg" if payload["type"] == "token_count" => {
                let info = &payload["info"];
                used = info["last_token_usage"]["total_tokens"]
                    .as_u64()
                    .unwrap_or(used);
                window = info["model_context_window"].as_u64().unwrap_or(window);
            }
            _ => {}
        }
    }
    ensure!(
        window > 0,
        "Codex has not reported this session’s context yet"
    );
    Ok(ContextUsage {
        model,
        used,
        window,
        compacts_at: None,
        categories: estimate(&characters, used),
        estimated: true,
        observed_at: crate::now(),
    })
}

/// The usage of the one model request Codex recorded for `turn` of `thread`. Codex's
/// notifications and its running total leave a compaction's request out; this entry of its
/// session record is the only place its tokens are. Only an entry's own `usage` is taken,
/// never its running sums, and only when exactly one entry carries the turn's id: anything
/// else is an error saying what the record held.
pub(crate) fn codex_turn_usage(thread: &str, turn: &str, home: &Path) -> Result<Value> {
    let Some(path) = session_file(&home.join("sessions"), thread) else {
        bail!("Codex has no session record for this conversation");
    };
    let mut entries = vec![];
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let Ok(record) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if record["type"] == "token_usage_record" && record["payload"]["turn_id"] == turn {
            entries.push(record["payload"]["usage"].clone());
        }
    }
    ensure!(
        entries.len() == 1,
        "Codex's session record has {} usage entries for the compaction's turn",
        entries.len()
    );
    Ok(entries.remove(0))
}

/// Splits the reported total across categories in proportion to their content. Whatever the
/// record does not account for is tool definitions and provider overhead.
fn estimate(characters: &[usize; CODEX_CATEGORIES.len()], used: u64) -> Vec<ContextCategory> {
    let sized: Vec<u64> = characters
        .iter()
        .map(|c| (c / CHARACTERS_PER_TOKEN) as u64)
        .collect();
    let total: u64 = sized.iter().sum();
    // An over-count means older items were compacted away; scale to what is actually in use.
    let scale = if total > used && total > 0 {
        used as f64 / total as f64
    } else {
        1.
    };
    let mut categories: Vec<_> = CODEX_CATEGORIES
        .iter()
        .zip(sized)
        .map(|(name, tokens)| ContextCategory {
            name: (*name).into(),
            tokens: (tokens as f64 * scale).round() as u64,
        })
        .filter(|category| category.tokens > 0)
        .collect();
    let counted: u64 = categories.iter().map(|c| c.tokens).sum();
    if used > counted {
        categories.push(ContextCategory {
            name: "Tools and overhead".into(),
            tokens: used - counted,
        });
    }
    categories
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "## Context Usage\n\n**Model:** claude-opus-5-5  \n**Tokens:** 24.3k / 1m (2%)\n\n### Estimated usage by category\n\n| Category | Tokens | Percentage |\n|----------|--------|------------|\n| System prompt | 3.3k | 0.3% |\n| System tools | 601 | 0.1% |\n| System tools (deferred) | 16.6k | 1.7% |\n| Skills | ~50 | 0.0% |\n| Messages | 6k | 0.6% |\n| Free space | 942.7k | 94.3% |\n| Autocompact buffer | 33k | 3.3% |\n\n### MCP Tools\n\n| Tool | Server | Tokens |\n|------|--------|--------|\n| batch | docs | 165 |\n";

    #[test]
    fn claude_report_yields_loaded_categories_and_the_compaction_point() {
        let usage = parse_claude(REPORT).unwrap();
        assert_eq!((usage.used, usage.window), (24_300, 1_000_000));
        assert_eq!(usage.compacts_at, Some(967_000));
        assert_eq!(usage.model.as_deref(), Some("claude-opus-5-5"));
        assert!(!usage.estimated);
        let names: Vec<_> = usage.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["System prompt", "System tools", "Skills", "Messages"]
        );
        assert_eq!(usage.categories[2].tokens, 50);
    }

    #[test]
    fn unreadable_claude_report_is_an_error_not_an_empty_bar() {
        assert!(parse_claude("Unknown command: /context").is_err());
    }

    fn record(home: &Path, thread: &str, lines: &[Value]) {
        let folder = home.join("sessions/2026/10/06");
        fs::create_dir_all(&folder).unwrap();
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(
            folder.join(format!("rollout-2026-10-06-{thread}.jsonl")),
            body,
        )
        .unwrap();
    }

    #[test]
    fn codex_fill_is_reported_and_categories_are_estimated_to_match_it() {
        let home = tempfile::tempdir().unwrap();
        record(
            home.path(),
            "thread-1",
            &[
                serde_json::json!({"type":"session_meta","payload":{"base_instructions":{"text":"x".repeat(400)}}}),
                serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}),
                serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"y".repeat(200)}]}}),
                serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"z".repeat(800)}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":1000},"model_context_window":258400}}}),
            ],
        );
        let usage = codex("thread-1", home.path()).unwrap();
        assert_eq!((usage.used, usage.window), (1000, 258_400));
        assert!(usage.estimated);
        assert_eq!(usage.model.as_deref(), Some("gpt-6.1-sol"));
        let size = |name: &str| {
            usage
                .categories
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.tokens)
        };
        assert_eq!(size("System prompt"), Some(100));
        assert_eq!(size("Messages"), Some(50));
        assert_eq!(size("Tool results"), Some(200));
        assert_eq!(size("Tools and overhead"), Some(650));
        assert_eq!(usage.categories.iter().map(|c| c.tokens).sum::<u64>(), 1000);
    }

    /// A compaction's entry as Codex 0.160 writes it: an empty `message`, and what it kept
    /// in `replacement_history`.
    fn compacted(kept: Value) -> Value {
        serde_json::json!({"type":"compacted","payload":{"message":"","replacement_history":kept}})
    }

    #[test]
    fn codex_measures_what_a_compaction_kept_and_nothing_before_it() {
        let home = tempfile::tempdir().unwrap();
        let user = |text: String| serde_json::json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]});
        record(
            home.path(),
            "thread-3",
            &[
                serde_json::json!({"type":"session_meta","payload":{"base_instructions":{"text":"x".repeat(400)}}}),
                serde_json::json!({"type":"response_item","payload":user("y".repeat(4000))}),
                serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"z".repeat(8000)}}),
                compacted(serde_json::json!([
                    user("y".repeat(200)),
                    user("y".repeat(120)),
                    {"type":"compaction","encrypted_content":"e".repeat(640)},
                ])),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":6550},"model_context_window":475000}}}),
            ],
        );
        let usage = codex("thread-3", home.path()).unwrap();
        assert_eq!((usage.used, usage.window), (6550, 475_000));
        let sizes: Vec<_> = usage
            .categories
            .iter()
            .map(|c| (c.name.as_str(), c.tokens))
            .collect();
        // The two kept messages and the summary, whose base64 is three quarters content.
        let messages = (200 + 120 + 640 * 3 / 4) / 4;
        assert_eq!(
            sizes,
            [
                ("System prompt", 100),
                ("Messages", messages),
                ("Tools and overhead", 6550 - 100 - messages)
            ]
        );
    }

    #[test]
    fn a_turns_usage_is_its_one_entry_and_never_another_turns() {
        let home = tempfile::tempdir().unwrap();
        let entry = |turn: &str, input: u64| {
            serde_json::json!({"type":"token_usage_record","payload":{
                "turn_id":turn,
                "usage":{"input_tokens":input,"cached_input_tokens":0,"output_tokens":1},
                "thread_token_usage":{"input_tokens":input + 1000,"output_tokens":1},
            }})
        };
        record(
            home.path(),
            "thread-4",
            &[
                entry("turn", 7),
                entry("compaction", 29_552),
                entry("twice", 3),
                entry("twice", 4),
                // The compaction's own entry repeats its usage; only a usage entry counts.
                serde_json::json!({"type":"compacted","payload":{"latest_token_usage_record":entry("compaction", 29_552)["payload"]}}),
            ],
        );
        let usage = |turn: &str| codex_turn_usage("thread-4", turn, home.path());
        // The entry's own usage, not a running sum beside it.
        assert_eq!(
            usage("compaction").unwrap(),
            serde_json::json!({"input_tokens":29_552,"cached_input_tokens":0,"output_tokens":1})
        );
        let none = usage("another").unwrap_err().to_string();
        assert!(none.contains("0 usage entries"), "{none}");
        let two = usage("twice").unwrap_err().to_string();
        assert!(two.contains("2 usage entries"), "{two}");
        let missing = codex_turn_usage("no-thread", "turn", home.path()).unwrap_err();
        assert!(
            missing.to_string().contains("no session record"),
            "{missing}"
        );
    }

    #[test]
    fn codex_compaction_drops_earlier_items_from_the_estimate() {
        let home = tempfile::tempdir().unwrap();
        record(
            home.path(),
            "thread-2",
            &[
                serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"z".repeat(8000)}}),
                serde_json::json!({"type":"compacted","payload":{"message":"s".repeat(400)}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":100},"model_context_window":1000}}}),
            ],
        );
        let usage = codex("thread-2", home.path()).unwrap();
        assert_eq!(
            usage.categories,
            [ContextCategory {
                name: "Messages".into(),
                tokens: 100
            }]
        );
    }

    #[test]
    fn codex_categories_shrink_to_the_reported_total_after_compaction() {
        let scaled = estimate(&[0, 0, 4000, 0, 4000, 0], 500);
        assert_eq!(scaled.iter().map(|c| c.tokens).sum::<u64>(), 500);
        assert_eq!(scaled.len(), 2);
    }

    #[test]
    fn codex_without_a_session_record_says_so() {
        let home = tempfile::tempdir().unwrap();
        let error = codex("missing", home.path()).unwrap_err().to_string();
        assert!(error.contains("no session record"), "{error}");
    }
}
