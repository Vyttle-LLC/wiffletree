//! The claude.ai connectors this Mac's Claude loads, with their tools, for Add source. Claude
//! reports them only in a start's init event, which it writes once it has input; so a short
//! `claude -p` run is given a one-word message and stopped at that event, before the model
//! answers. It is offered no built-in tool and allowed no tool at all, and it loads none of the
//! human's settings, so even a model that answered could call nothing.
use super::conversation::{claude_server_name, is_claude_ai};
use super::*;

/// A claude.ai connector as a Claude start reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connector {
    /// As Claude lists it, such as "claude.ai Gmail".
    pub name: String,
    /// Claude's word for it, such as `connected` or `needs-auth`.
    pub status: String,
    /// Its tools, by tool name; none unless it is connected.
    pub tools: Vec<String>,
}
impl Connector {
    pub fn connected(&self) -> bool {
        self.status == "connected"
    }
}

/// Starts Claude at `claude` within `limit` and lists the claude.ai connectors its start
/// reports, without calling any of them.
pub fn claude_ai_connectors(claude: &Path, limit: Duration) -> Result<Vec<Connector>> {
    answer_run(claude, configure, b"OK".to_vec(), limit, |value| {
        match value["type"] == "system" && value["subtype"] == "init" {
            true => reported(value).map(Some),
            false => Ok(None),
        }
    })
}

fn configure(cmd: &mut ProcessCommand, _folder: &Path) -> Result<()> {
    cmd.env_remove("WORKSPACE_SOCKET")
        .env_remove("WORKSPACE_TOKEN")
        // Claude loads connectors after its start unless connecting is blocking.
        .env("MCP_CONNECTION_NONBLOCKING", "false")
        .args(["-p", "--tools", "", "--no-session-persistence"])
        .args(["--permission-mode", "dontAsk"])
        .args(["--setting-sources", "local"])
        .args(["--output-format", "stream-json", "--verbose", "--no-chrome"])
        .args(["--model", "haiku"])
        .args(["--system-prompt", "Reply OK."]);
    Ok(())
}

/// The connectors a start's init event reports. A start that would not deny every tool is
/// refused, though it is stopped before the model answers.
fn reported(init: &Value) -> Result<Vec<Connector>> {
    ensure!(
        init["permissionMode"] == "dontAsk",
        "Claude started in permission mode {} instead of denying every tool",
        init["permissionMode"]
    );
    let tools: Vec<&str> = (init["tools"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let servers = (init["mcp_servers"].as_array().into_iter().flatten())
        .filter_map(|server| Some((server["name"].as_str()?, server["status"].as_str()?)));
    Ok(servers
        .filter(|(name, _)| is_claude_ai(name))
        .map(|(name, status)| {
            let prefix = format!("mcp__{}__", claude_server_name(name));
            Connector {
                name: name.into(),
                status: status.into(),
                tools: (tools.iter())
                    .filter_map(|tool| tool.strip_prefix(&prefix))
                    .map(str::to_owned)
                    .collect(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_allows_no_tool_and_loads_none_of_the_humans_settings() {
        let mut cmd = ProcessCommand::new("claude");
        configure(&mut cmd, Path::new("/f")).unwrap();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into()).collect();
        let after = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--setting-sources"), "local");
        assert!(
            !args
                .iter()
                .any(|a| a.contains("allowedTools") || a.contains("dangerously"))
        );
        assert!(
            (cmd.get_envs()).any(|(name, value)| name == "MCP_CONNECTION_NONBLOCKING"
                && value == Some("false".as_ref()))
        );
    }

    #[test]
    fn a_start_reports_each_connector_with_its_own_tools() {
        let init = json!({
            "permissionMode": "dontAsk",
            "mcp_servers": [
                {"name": "claude.ai Gmail", "status": "connected"},
                {"name": "claude.ai Google Calendar", "status": "connected"},
                {"name": "claude.ai Slack", "status": "needs-auth"},
                {"name": "termic", "status": "connected"},
            ],
            "tools": [
                "mcp__claude_ai_Gmail__search_threads",
                "mcp__claude_ai_Gmail__send_message",
                "mcp__claude_ai_Google_Calendar__list_events",
                "mcp__termic__task_list",
            ],
        });
        let found = reported(&init).unwrap();
        let named: Vec<(&str, bool, Vec<&str>)> = (found.iter())
            .map(|c| {
                let tools = c.tools.iter().map(String::as_str).collect();
                (c.name.as_str(), c.connected(), tools)
            })
            .collect();
        assert_eq!(
            named,
            [
                (
                    "claude.ai Gmail",
                    true,
                    vec!["search_threads", "send_message"]
                ),
                ("claude.ai Google Calendar", true, vec!["list_events"]),
                ("claude.ai Slack", false, vec![]),
            ]
        );
        let bypassing = json!({"permissionMode": "bypassPermissions", "mcp_servers": []});
        assert!(reported(&bypassing).is_err());
    }
}
