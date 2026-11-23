//! An act tool of an MCP source reached by URL, which only Claude can call, since Claude holds
//! its sign-in. The host starts a short `claude -p` run offered that one tool of that one
//! server and no built-in tool, told to call it once with an action card's exact arguments.
//!
//! Two checks hold the run to that call. Before any tool runs, Claude's `PreToolUse` hook,
//! the helper's `act-check` (`act_call_refused`), refuses every call but the first one of
//! that tool with exactly those arguments; a hook runs whatever permission rules say, and
//! exit 2 refuses even an allowed tool. And the host reads the run's stream, stopping it at
//! the first other call. Only the tool's result is kept; the run's own text is never read.
use super::*;
use std::collections::BTreeSet;

/// The expected call, which the hook reads from the run's folder.
const EXPECTED: &str = "act-call.json";
/// Made by the hook when it allows the call, so a second call finds it.
const CALLED: &str = "act-called";
/// Claude's tools that read a server's resources, which an act run never needs.
const RESOURCE_TOOLS: [&str; 2] = ["ListMcpResourcesTool", "ReadMcpResourceTool"];

/// One act of a source reached by URL, as its action card shows it.
#[derive(Clone, Debug)]
pub struct UrlAct {
    pub server: String,
    /// The server as Claude's own configuration defines it.
    pub definition: Value,
    pub tool: String,
    pub arguments: Value,
    /// The server's other tools Claude has reported, denied by name.
    pub others: BTreeSet<String>,
}
impl UrlAct {
    fn name(&self) -> String {
        format!("mcp__{}__{}", self.server, self.tool)
    }
    /// Why a call is not this act's, given its tool name and input.
    fn refused(&self, name: &str, input: &Value) -> Option<String> {
        if name != self.name() {
            return Some(format!(
                "Stopped: the run called {name}, not {}",
                self.name()
            ));
        }
        (*input != self.arguments).then(|| {
            format!(
                "Stopped: the run called {} with arguments other than the card's",
                self.tool
            )
        })
    }
    /// Sets up the run, then writes the call the hook expects; without it, the hook refuses
    /// every call.
    fn configure(&self, cmd: &mut ProcessCommand, folder: &Path, helper: &Path) -> Result<()> {
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', r"'\''"));
        let check = json!({"type": "command", "command": format!("{} act-check {}", quote(helper), quote(folder))});
        let settings = json!({
            "autoMemoryEnabled": false,
            "disableAllHooks": false,
            "hooks": {"PreToolUse": [{"matcher": "*", "hooks": [check]}]},
        });
        let denied = (RESOURCE_TOOLS.into_iter().map(str::to_owned))
            .chain((self.others.iter()).map(|tool| format!("mcp__{}__{tool}", self.server)))
            .filter(|tool| *tool != self.name())
            .collect::<Vec<_>>();
        let servers = json!({"mcpServers": {&self.server: self.definition}});
        let prompt = format!(
            "You make one tool call for Wiffletree. Call {} once, with exactly the JSON arguments you are given, unchanged. Then stop. Do nothing else.",
            self.name()
        );
        cmd.env_remove("WORKSPACE_SOCKET")
            .env_remove("WORKSPACE_TOKEN")
            .args(["-p", "--tools", "", "--no-session-persistence"])
            .args(["--strict-mcp-config", "--mcp-config", &servers.to_string()])
            .args(["--allowedTools", &self.name()])
            .args(["--disallowedTools", &denied.join(",")])
            .args(["--settings", &settings.to_string()])
            .args(["--output-format", "stream-json", "--verbose", "--no-chrome"])
            .args(["--model", "haiku"])
            .args(["--system-prompt", &prompt]);
        let expected = json!({"name": self.name(), "arguments": self.arguments});
        Ok(fs::write(folder.join(EXPECTED), expected.to_string())?)
    }
}

/// Why the hook refuses a call, given the run's folder and the hook's input; `None` allows the
/// first call of the expected tool with exactly the expected arguments. Unreadable input or an
/// unreadable expectation is refused.
pub fn act_call_refused(folder: &Path, input: &[u8]) -> Option<String> {
    let expected: Value = (fs::read(folder.join(EXPECTED)).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let input: Value = serde_json::from_slice(input).unwrap_or_default();
    let (Some(name), Some(expected_name)) =
        (input["tool_name"].as_str(), expected["name"].as_str())
    else {
        return Some("Wiffletree could not read this call, so it may not run.".into());
    };
    if name != expected_name {
        return Some(format!("This run may call only {expected_name}."));
    }
    if input["tool_input"] != expected["arguments"] {
        return Some(format!(
            "This run may call {expected_name} only with the action card's exact arguments."
        ));
    }
    let first = (fs::File::options().write(true).create_new(true)).open(folder.join(CALLED));
    first
        .is_err()
        .then(|| format!("This run may call {expected_name} only once."))
}

/// Runs `act` through Claude at `claude`, whose hook is `helper`'s `act-check`, and returns
/// the tool's text; an error says why the call did not go through, or what the server said.
pub fn act_run(claude: &Path, helper: &Path, act: &UrlAct, limit: Duration) -> Result<String> {
    let mut prepared = Ok(());
    let configure = |cmd: &mut ProcessCommand, folder: &Path| {
        prepared = act.configure(cmd, folder, helper);
    };
    let input = format!(
        "Call {} once with exactly these arguments:\n{}",
        act.name(),
        act.arguments
    );
    let mut called: Option<String> = None;
    let ran = answer_run(
        claude,
        configure,
        input.into_bytes(),
        limit,
        |value| match value["type"].as_str() {
            Some("system") if value["subtype"] == "init" => started(act, value).map(|_| None),
            Some("assistant") => {
                for call in tool_uses(value) {
                    let name = call["name"].as_str().unwrap_or_default();
                    if let Some(why) = act.refused(name, &call["input"]) {
                        bail!("{why}");
                    }
                    ensure!(
                        called.is_none(),
                        "Stopped: the run called {} a second time; the first call may have gone through",
                        act.tool
                    );
                    called = Some(call["id"].as_str().unwrap_or_default().to_owned());
                }
                Ok(None)
            }
            Some("user") => {
                let result = (tool_results(value))
                    .find(|result| called.as_deref() == result["tool_use_id"].as_str());
                let Some(result) = result else {
                    return Ok(None);
                };
                let text = result_text(&result["content"]);
                ensure!(result["is_error"] != true, "{text}");
                Ok(Some(text))
            }
            Some("result") => bail!("Claude ended without calling {}", act.tool),
            _ => Ok(None),
        },
    );
    prepared?;
    ran
}

/// Checks the run's start: it offers no tool but the server's, which is connected and offers
/// the act tool. A server that is not connected is most often signed out.
fn started(act: &UrlAct, init: &Value) -> Result<()> {
    let tools: Vec<&str> = (init["tools"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let prefix = format!("mcp__{}__", act.server);
    if let Some(other) = tools.iter().find(|tool| !tool.starts_with(&prefix)) {
        bail!("Stopped: the run started with {other}, a tool other than the server's");
    }
    let status = (init["mcp_servers"].as_array().into_iter().flatten())
        .find(|server| server["name"] == act.server.as_str())
        .and_then(|server| server["status"].as_str())
        .unwrap_or("missing");
    ensure!(
        status == "connected",
        "The server is not connected for Claude ({status}), such as when its sign-in has expired. Sign in to {} in Claude Code with /mcp.",
        act.server
    );
    ensure!(
        tools.contains(&act.name().as_str()),
        "The {} server offers no tool {}",
        act.server,
        act.tool
    );
    Ok(())
}
fn tool_uses(message: &Value) -> impl Iterator<Item = &Value> {
    (message["message"]["content"]
        .as_array()
        .into_iter()
        .flatten())
    .filter(|block| block["type"] == "tool_use")
}
fn tool_results(message: &Value) -> impl Iterator<Item = &Value> {
    (message["message"]["content"]
        .as_array()
        .into_iter()
        .flatten())
    .filter(|block| block["type"] == "tool_result")
}
/// A tool result's text: a string, or its text blocks joined.
fn result_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.into();
    }
    (content.as_array().into_iter().flatten())
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_event() -> UrlAct {
        UrlAct {
            server: "calendar".into(),
            definition: json!({"type": "http", "url": "https://calendar.example/mcp"}),
            tool: "create_event".into(),
            arguments: json!({"title": "Hold", "start": "2026-11-27T15:00"}),
            others: ["list_events".into(), "create_event".into()].into(),
        }
    }

    #[test]
    fn the_run_is_offered_the_one_act_tool_and_hooked_before_every_call() {
        let folder = tempfile::tempdir().unwrap();
        let mut cmd = ProcessCommand::new("claude");
        (create_event())
            .configure(&mut cmd, folder.path(), Path::new("/Apps/wiffletree-host"))
            .unwrap();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into()).collect();
        let after = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--allowedTools"), "mcp__calendar__create_event");
        assert_eq!(
            after("--disallowedTools"),
            "ListMcpResourcesTool,ReadMcpResourceTool,mcp__calendar__list_events"
        );
        let servers: Value = serde_json::from_str(after("--mcp-config")).unwrap();
        assert_eq!(
            servers,
            json!({"mcpServers": {"calendar": {"type": "http", "url": "https://calendar.example/mcp"}}})
        );
        let settings: Value = serde_json::from_str(after("--settings")).unwrap();
        let hook = &settings["hooks"]["PreToolUse"][0];
        assert_eq!(hook["matcher"], "*");
        assert_eq!(
            hook["hooks"][0]["command"],
            format!(
                "'/Apps/wiffletree-host' act-check '{}'",
                folder.path().display()
            )
        );
        assert!(!args.iter().any(|a| a.contains("dangerously")));
    }

    #[test]
    fn the_hook_allows_only_the_first_call_of_the_act_with_the_cards_arguments() {
        let folder = tempfile::tempdir().unwrap();
        let act = create_event();
        act.configure(
            &mut ProcessCommand::new("claude"),
            folder.path(),
            Path::new("/h"),
        )
        .unwrap();
        let call = |name: &str, input: Value| {
            let hook = json!({"tool_name": name, "tool_input": input}).to_string();
            act_call_refused(folder.path(), hook.as_bytes())
        };
        let other_arguments = json!({"title": "Hold", "start": "2026-11-27T16:00"});
        assert!(call("mcp__calendar__create_event", other_arguments).is_some());
        assert!(call("mcp__calendar__delete_calendar", act.arguments.clone()).is_some());
        assert!(call("Bash", json!({"command": "ls"})).is_some());
        assert!(act_call_refused(folder.path(), b"not json").is_some());
        assert_eq!(
            call("mcp__calendar__create_event", act.arguments.clone()),
            None
        );
        let again = call("mcp__calendar__create_event", act.arguments.clone());
        assert!(again.unwrap().contains("only once"));
        let elsewhere = tempfile::tempdir().unwrap();
        let hook = json!({"tool_name": act.name(), "tool_input": act.arguments}).to_string();
        assert!(
            act_call_refused(elsewhere.path(), hook.as_bytes()).is_some(),
            "no expectation, no call"
        );
    }
}
