//! Decodes provider streams into provider-neutral work steps and the turn's final reply.
//! Unknown or partial events produce no step; they never fail a turn.
use serde_json::Value;
use std::{collections::HashMap, path::Path};
use workspace_core::{StepKind, StepState};

/// What an adapter knows about a step. The service assigns its order, revision and times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepUpdate {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: StepKind,
    pub state: StepState,
    pub title: String,
    pub note: Option<String>,
    pub detail: Option<String>,
}
impl StepUpdate {
    fn new(id: impl Into<String>, kind: StepKind, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            parent_id: None,
            kind,
            state: StepState::Running,
            title: title.into(),
            note: None,
            detail: None,
        }
    }
}

/// Claude stream-json. Text and thinking stream as deltas; each tool call arrives as a complete
/// assistant block, and its result in the next user message. Subagent calls carry
/// `parent_tool_use_id` and arrive only as complete messages.
pub struct ClaudeSteps {
    cwd: String,
    message: String,
    open_blocks: HashMap<u64, StepUpdate>,
    tools: HashMap<String, StepUpdate>,
    /// Text blocks seen as stream events and as complete messages. A complete text block that
    /// was never streamed (partial messages unavailable) still becomes narration.
    streamed_text: usize,
    completed_text: usize,
    last_text: Option<String>,
}
impl ClaudeSteps {
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_string_lossy().into_owned(),
            message: String::new(),
            open_blocks: HashMap::new(),
            tools: HashMap::new(),
            streamed_text: 0,
            completed_text: 0,
            last_text: None,
        }
    }

    pub fn event(&mut self, value: &Value) -> Vec<StepUpdate> {
        let parent = value["parent_tool_use_id"].as_str();
        match value["type"].as_str() {
            Some("stream_event") if parent.is_none() => self.stream(&value["event"]),
            Some("assistant") => blocks(value)
                .filter_map(|block| self.assistant_block(block, parent))
                .collect(),
            Some("user") => blocks(value)
                .filter_map(|block| self.tool_result(block, &value["tool_use_result"]))
                .collect(),
            _ => vec![],
        }
    }

    /// The final text: the provider's `result`, or the last text block when it has none.
    pub fn reply(&self, result: &Value) -> Option<String> {
        result["result"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| self.last_text.clone())
    }

    fn stream(&mut self, event: &Value) -> Vec<StepUpdate> {
        let index = event["index"].as_u64().unwrap_or_default();
        match event["type"].as_str() {
            Some("message_start") => {
                self.message = event["message"]["id"].as_str().unwrap_or_default().into();
                vec![]
            }
            Some("content_block_start") => {
                let block = &event["content_block"];
                let id = format!("{}:{index}", self.message);
                let kind = match block["type"].as_str() {
                    Some("text") => {
                        self.streamed_text += 1;
                        StepKind::Narration
                    }
                    Some("thinking") => StepKind::Thinking,
                    Some("tool_use") => {
                        return self.assistant_block(block, None).into_iter().collect();
                    }
                    _ => return vec![],
                };
                self.open_blocks
                    .insert(index, StepUpdate::new(id, kind, ""));
                vec![]
            }
            Some("content_block_delta") => {
                let delta = &event["delta"];
                let text = delta["text"].as_str().or(delta["thinking"].as_str());
                match (self.open_blocks.get_mut(&index), text) {
                    (Some(step), Some(text)) if !text.is_empty() => {
                        let detail = step.detail.get_or_insert_default();
                        detail.push_str(text);
                        step.title.clone_from(detail);
                        vec![step.clone()]
                    }
                    _ => vec![],
                }
            }
            Some("content_block_stop") => self
                .open_blocks
                .remove(&index)
                .filter(|step| !step.title.trim().is_empty())
                .map(|mut step| {
                    step.state = StepState::Succeeded;
                    step
                })
                .into_iter()
                .collect(),
            _ => vec![],
        }
    }

    fn assistant_block(&mut self, block: &Value, parent: Option<&str>) -> Option<StepUpdate> {
        match block["type"].as_str()? {
            "text" if parent.is_none() => {
                let text = block["text"].as_str()?;
                self.last_text = Some(text.to_owned());
                self.completed_text += 1;
                if self.completed_text <= self.streamed_text || text.trim().is_empty() {
                    return None;
                }
                self.streamed_text = self.completed_text;
                let mut step = StepUpdate::new(
                    format!("text:{}", self.completed_text),
                    StepKind::Narration,
                    text,
                );
                step.detail = Some(text.to_owned());
                step.state = StepState::Succeeded;
                Some(step)
            }
            "tool_use" => {
                let id = block["id"].as_str()?;
                let mut step = claude_tool(id, block["name"].as_str()?, &block["input"], &self.cwd);
                step.parent_id = parent.map(str::to_owned);
                // A tool announced while its input streamed keeps its place; the full input
                // only sharpens the title.
                self.tools.insert(id.into(), step.clone());
                Some(step)
            }
            _ => None,
        }
    }

    fn tool_result(&mut self, block: &Value, result: &Value) -> Option<StepUpdate> {
        if block["type"] != "tool_result" {
            return None;
        }
        let mut step = self.tools.remove(block["tool_use_id"].as_str()?)?;
        let failed = block["is_error"] == true;
        step.state = if failed {
            StepState::Failed
        } else {
            StepState::Succeeded
        };
        let output = result_text(&block["content"]);
        match step.kind {
            // The plan itself is the useful detail, not the tool's acknowledgement.
            StepKind::Plan => {}
            StepKind::FileEdit if !failed => {
                step.note = patch_counts(&result["structuredPatch"]).or(step.note);
            }
            _ => {
                if failed {
                    step.note = exit_code(&output).map(|code| format!("exit {code}"));
                }
                step.detail = Some(output).filter(|o| !o.trim().is_empty());
            }
        }
        Some(step)
    }
}

/// Codex `exec --json`. Every item streams as started, updated and completed snapshots.
pub struct CodexSteps {
    cwd: String,
    last_message: Option<String>,
}
impl CodexSteps {
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_string_lossy().into_owned(),
            last_message: None,
        }
    }

    pub fn event(&mut self, value: &Value) -> Vec<StepUpdate> {
        let completed = match value["type"].as_str() {
            Some("item.started" | "item.updated") => false,
            Some("item.completed") => true,
            _ => return vec![],
        };
        self.item(&value["item"], completed).into_iter().collect()
    }

    /// The last agent message of the turn.
    pub fn reply(&self) -> Option<String> {
        self.last_message.clone()
    }

    fn item(&mut self, item: &Value, completed: bool) -> Option<StepUpdate> {
        let id = item["id"].as_str()?;
        let status = item["status"].as_str().unwrap_or(if completed {
            "completed"
        } else {
            "in_progress"
        });
        let mut state = match status {
            "completed" => StepState::Succeeded,
            "failed" | "declined" => StepState::Failed,
            _ => StepState::Running,
        };
        let mut step = match item["type"].as_str()? {
            "agent_message" | "reasoning" => {
                let text = item["text"].as_str().filter(|t| !t.trim().is_empty())?;
                let kind = if item["type"] == "agent_message" {
                    if completed {
                        self.last_message = Some(text.to_owned());
                    }
                    StepKind::Narration
                } else {
                    StepKind::Thinking
                };
                let mut step = StepUpdate::new(id, kind, text.replace("**", ""));
                step.detail = Some(text.to_owned());
                step
            }
            "command_execution" => {
                let mut step = StepUpdate::new(
                    id,
                    StepKind::Command,
                    unwrap_shell(item["command"].as_str()?),
                );
                if let Some(code) = item["exit_code"].as_i64().filter(|&code| code != 0) {
                    state = StepState::Failed;
                    step.note = Some(format!("exit {code}"));
                }
                step.detail = item["aggregated_output"]
                    .as_str()
                    .filter(|o| !o.trim().is_empty())
                    .map(str::to_owned);
                step
            }
            "file_change" => {
                let changes = item["changes"].as_array()?;
                let paths: Vec<_> = changes
                    .iter()
                    .filter_map(|c| c["path"].as_str())
                    .map(|p| relative(p, &self.cwd))
                    .collect();
                let mut step = StepUpdate::new(id, StepKind::FileEdit, paths.join(", "));
                if changes.len() > 1 {
                    step.note = Some(format!("{} files", changes.len()));
                }
                step.detail = Some(
                    changes
                        .iter()
                        .filter_map(|c| {
                            Some(format!(
                                "{} {}",
                                c["kind"].as_str()?,
                                relative(c["path"].as_str()?, &self.cwd)
                            ))
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                step
            }
            "mcp_tool_call" => {
                let mut step = StepUpdate::new(
                    id,
                    StepKind::Tool,
                    tool_title(item["server"].as_str()?, item["tool"].as_str()?),
                );
                step.detail = item["error"]["message"].as_str().map(str::to_owned);
                step
            }
            "web_search" => StepUpdate::new(id, StepKind::Search, item["query"].as_str()?),
            "todo_list" => {
                let todos = item["items"].as_array()?.iter().map(|todo| {
                    let done = todo["completed"] == true;
                    (done, todo["text"].as_str().unwrap_or_default())
                });
                // A plan is a statement, not an action in progress.
                state = StepState::Succeeded;
                plan(id, todos)
            }
            _ => return None,
        };
        step.state = state;
        Some(step)
    }
}

fn blocks(value: &Value) -> impl Iterator<Item = &Value> {
    value["message"]["content"].as_array().into_iter().flatten()
}

fn claude_tool(id: &str, name: &str, input: &Value, cwd: &str) -> StepUpdate {
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let path = |key: &str| relative(text(key), cwd);
    let (kind, title) = match name {
        "Bash" => (StepKind::Command, text("command").to_owned()),
        "Read" => (StepKind::Read, path("file_path")),
        "Edit" | "MultiEdit" | "Write" => (StepKind::FileEdit, path("file_path")),
        "NotebookEdit" => (StepKind::FileEdit, path("notebook_path")),
        "Grep" | "Glob" => (StepKind::Search, text("pattern").to_owned()),
        "WebSearch" => (StepKind::Search, text("query").to_owned()),
        "WebFetch" => (StepKind::Search, text("url").to_owned()),
        "TodoWrite" => {
            let todos = input["todos"].as_array().into_iter().flatten().map(|todo| {
                let done = todo["status"] == "completed";
                (done, todo["content"].as_str().unwrap_or_default())
            });
            let mut step = plan(id, todos);
            step.state = StepState::Succeeded;
            return step;
        }
        "Agent" | "Task" => (StepKind::Tool, format!("Agent · {}", text("description"))),
        _ => match name.strip_prefix("mcp__").and_then(|n| n.split_once("__")) {
            Some((server, tool)) => (StepKind::Tool, tool_title(server, tool)),
            None => (StepKind::Tool, name.to_owned()),
        },
    };
    let mut step = StepUpdate::new(id, kind, title);
    if step.title.trim().is_empty() {
        name.clone_into(&mut step.title);
    }
    if name == "Write" {
        let lines = text("content").lines().count();
        step.note = (lines > 0).then(|| format!("+{lines}"));
    }
    step
}

/// "Plan · 1 of 3 done", with each item listed in the detail.
fn plan<'a>(id: &str, todos: impl Iterator<Item = (bool, &'a str)>) -> StepUpdate {
    let todos: Vec<_> = todos.collect();
    let done = todos.iter().filter(|(done, _)| *done).count();
    let mut step = StepUpdate::new(
        id,
        StepKind::Plan,
        format!("Plan · {done} of {} done", todos.len()),
    );
    step.detail = Some(
        todos
            .iter()
            .map(|(done, text)| format!("{} {text}", if *done { "✓" } else { "◻" }))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    step
}

/// The workspace's own tools read as plain verbs; other servers keep their name.
fn tool_title(server: &str, tool: &str) -> String {
    if server == "agent_workspace" {
        tool.to_owned()
    } else {
        format!("{server} · {tool}")
    }
}

fn relative(path: &str, cwd: &str) -> String {
    path.strip_prefix(cwd)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
        .to_owned()
}

/// A tool result's text, whether given as a string or as content blocks.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// "+12 −4" from an edit's structured patch.
fn patch_counts(patch: &Value) -> Option<String> {
    let lines = patch
        .as_array()?
        .iter()
        .flat_map(|hunk| hunk["lines"].as_array().into_iter().flatten())
        .filter_map(Value::as_str);
    let (added, removed) = lines.fold((0, 0), |(a, r), line| match line.as_bytes().first() {
        Some(b'+') => (a + 1, r),
        Some(b'-') => (a, r + 1),
        _ => (a, r),
    });
    Some(format!("+{added} −{removed}"))
}

/// Claude reports a failed command as "Exit code N" on its first line.
fn exit_code(output: &str) -> Option<i64> {
    output
        .lines()
        .next()?
        .strip_prefix("Exit code ")?
        .trim()
        .parse()
        .ok()
}

/// Codex runs commands through a login shell; the title shows only the command.
fn unwrap_shell(command: &str) -> String {
    for flag in [" -lc ", " -c "] {
        let Some((shell, script)) = command.split_once(flag) else {
            continue;
        };
        if !shell.ends_with("sh") || shell.contains(' ') {
            continue;
        }
        let quoted = |q: char| script.len() >= 2 && script.starts_with(q) && script.ends_with(q);
        return if quoted('\'') {
            script[1..script.len() - 1].replace("'\\''", "'")
        } else if quoted('"') {
            script[1..script.len() - 1].replace("\\\"", "\"")
        } else {
            script.to_owned()
        };
    }
    command.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    const CWD: &str = "/work/repo";

    fn lines(name: &str) -> Vec<Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/streams")
            .join(name);
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// Every update, and the last state of each step in order of first appearance.
    fn replay(
        name: &str,
        mut decode: impl FnMut(&Value) -> Vec<StepUpdate>,
    ) -> (Vec<StepUpdate>, Vec<StepUpdate>) {
        let updates: Vec<_> = lines(name).iter().flat_map(&mut decode).collect();
        let mut last: Vec<StepUpdate> = vec![];
        for update in &updates {
            match last.iter_mut().find(|s| s.id == update.id) {
                Some(step) => *step = update.clone(),
                None => last.push(update.clone()),
            }
        }
        (updates, last)
    }

    fn summary(steps: &[StepUpdate]) -> Vec<(StepKind, StepState, &str)> {
        steps
            .iter()
            .map(|s| (s.kind, s.state, s.title.as_str()))
            .collect()
    }

    #[test]
    fn fixtures_load() {
        for name in [
            "claude-recorded.jsonl",
            "claude-synthetic.jsonl",
            "codex-recorded.jsonl",
            "codex-synthetic.jsonl",
        ] {
            assert!(!lines(name).is_empty(), "{name}");
        }
    }

    #[test]
    fn recorded_claude_turn_becomes_one_step_per_tool_call() {
        let mut claude = ClaudeSteps::new(Path::new(CWD));
        let (_, steps) = replay("claude-recorded.jsonl", |v| claude.event(v));
        let actions: Vec<_> = steps.iter().filter(|s| s.kind.is_action()).collect();
        use StepKind::*;
        use StepState::*;
        assert_eq!(
            actions
                .iter()
                .map(|s| (s.kind, s.state, s.title.as_str(), s.parent_id.is_some()))
                .collect::<Vec<_>>(),
            [
                (Tool, Succeeded, "ToolSearch", false),
                (Command, Succeeded, "ls", false),
                (Command, Failed, "cat missing-file.txt", false),
                (Read, Succeeded, "notes.txt", false),
                (FileEdit, Succeeded, "notes.txt", false),
                (Command, Succeeded, "grep -rn \"gamma\" notes.txt", false),
                (Tool, Succeeded, "Agent · Count lines in notes.txt", false),
                (Command, Succeeded, "wc -l notes.txt", true),
            ]
        );
        let failed = actions.iter().find(|s| s.state == Failed).unwrap();
        assert_eq!(failed.note.as_deref(), Some("exit 1"));
        assert!(failed.detail.as_deref().unwrap().contains("No such file"));
        let edit = actions.iter().find(|s| s.kind == FileEdit).unwrap();
        assert_eq!(edit.note.as_deref(), Some("+1 −1"));
        // Redacted thinking produces no step; every narration block finished.
        assert!(steps.iter().all(|s| s.kind != Thinking));
        assert!(
            steps
                .iter()
                .filter(|s| s.kind == Narration)
                .all(|s| s.state == Succeeded)
        );
        assert_eq!(steps.iter().filter(|s| s.kind == Narration).count(), 5);
    }

    #[test]
    fn claude_reply_is_only_the_result_text() {
        let mut claude = ClaudeSteps::new(Path::new(CWD));
        let events = lines("claude-recorded.jsonl");
        events.iter().for_each(|v| drop(claude.event(v)));
        let result = events.iter().find(|v| v["type"] == "result").unwrap();
        let reply = claude.reply(result).unwrap();
        assert!(reply.starts_with("Ran ls"));
        assert!(!reply.contains("I'll start by checking"));
    }

    #[test]
    fn claude_narration_streams_as_it_is_written() {
        let mut claude = ClaudeSteps::new(Path::new(CWD));
        let (updates, _) = replay("claude-synthetic.jsonl", |v| claude.event(v));
        let titles: Vec<_> = updates
            .iter()
            .filter(|u| u.kind == StepKind::Narration)
            .map(|u| (u.title.as_str(), u.state))
            .collect();
        assert_eq!(
            &titles[..3],
            [
                ("Planning the ", StepState::Running),
                ("Planning the work first:", StepState::Running),
                ("Planning the work first:", StepState::Succeeded),
            ]
        );
    }

    #[test]
    fn synthetic_claude_shapes() {
        let mut claude = ClaudeSteps::new(Path::new(CWD));
        let (_, steps) = replay("claude-synthetic.jsonl", |v| claude.event(v));
        use StepKind::*;
        use StepState::*;
        assert_eq!(
            summary(&steps),
            [
                (Thinking, Succeeded, "Checking the plan before editing."),
                (Narration, Succeeded, "Planning the work first:"),
                (Plan, Succeeded, "Plan · 1 of 3 done"),
                (FileEdit, Succeeded, "src/new.rs"),
                (Tool, Failed, "report"),
                (Search, Running, "TODO"),
                (Narration, Succeeded, "All set."),
            ]
        );
        assert_eq!(
            steps[2].detail.as_deref(),
            Some("✓ Write the module\n◻ Report progress\n◻ Search for TODOs")
        );
        assert_eq!(steps[3].note.as_deref(), Some("+3"));
        assert_eq!(
            steps[4].detail.as_deref(),
            Some("Report rejected: missing message_id")
        );
        assert_eq!(
            claude.reply(&json!({"result": ""})).as_deref(),
            Some("All set.")
        );
    }

    #[test]
    fn claude_without_partial_messages_still_narrates() {
        let mut claude = ClaudeSteps::new(Path::new(CWD));
        let steps = claude.event(&json!({"type":"assistant","message":{"content":[{"type":"text","text":"MAIN_READY"}]}}));
        assert_eq!(
            summary(&steps),
            [(StepKind::Narration, StepState::Succeeded, "MAIN_READY")]
        );
        assert_eq!(
            claude.reply(&json!({"type":"result"})).as_deref(),
            Some("MAIN_READY")
        );
    }

    #[test]
    fn recorded_codex_turn() {
        let mut codex = CodexSteps::new(Path::new(CWD));
        let (_, steps) = replay("codex-recorded.jsonl", |v| codex.event(v));
        use StepKind::*;
        use StepState::*;
        let actions: Vec<_> = steps.iter().filter(|s| s.kind.is_action()).collect();
        assert_eq!(
            actions
                .iter()
                .map(|s| (s.kind, s.state, s.title.as_str()))
                .collect::<Vec<_>>(),
            [
                (Command, Succeeded, "ls"),
                (Command, Failed, "cat missing-file.txt"),
                (FileEdit, Succeeded, "notes.txt"),
                (Command, Succeeded, "grep -n gamma notes.txt"),
            ]
        );
        assert_eq!(actions[1].note.as_deref(), Some("exit 1"));
        assert!(codex.reply().unwrap().starts_with("Ran `ls`"));
    }

    #[test]
    fn synthetic_codex_shapes() {
        let mut codex = CodexSteps::new(Path::new(CWD));
        let (updates, steps) = replay("codex-synthetic.jsonl", |v| codex.event(v));
        use StepKind::*;
        use StepState::*;
        assert_eq!(
            summary(&steps),
            [
                (Thinking, Succeeded, "Planning the change"),
                (Plan, Succeeded, "Plan · 1 of 2 done"),
                (Tool, Succeeded, "report"),
                (Tool, Failed, "github · search_issues"),
                (Search, Succeeded, "rust char_indices"),
                (Command, Failed, "cargo test -p core"),
                (FileEdit, Succeeded, "src/lib.rs, tests/parse.rs"),
                (
                    Narration,
                    Succeeded,
                    "Fixed the parser and added a regression test."
                ),
            ]
        );
        // The command shows its output while it runs.
        assert!(updates.iter().any(|u| u.id == "item_test"
            && u.state == Running
            && u.detail.as_deref() == Some("running 3 tests\n")));
        assert_eq!(steps[5].note.as_deref(), Some("exit 101"));
        assert_eq!(steps[6].note.as_deref(), Some("2 files"));
        assert_eq!(
            codex.reply().as_deref(),
            Some("Fixed the parser and added a regression test.")
        );
    }

    #[test]
    fn shell_wrappers_are_removed() {
        assert_eq!(unwrap_shell("/bin/zsh -lc ls"), "ls");
        assert_eq!(unwrap_shell("/bin/zsh -lc 'cat a.txt'"), "cat a.txt");
        assert_eq!(
            unwrap_shell("/bin/zsh -lc 'echo '\\''hi'\\'''"),
            "echo 'hi'"
        );
        assert_eq!(unwrap_shell("bash -c \"say \\\"x\\\"\""), "say \"x\"");
        assert_eq!(unwrap_shell("git commit -c x"), "git commit -c x");
    }
}
