//! The Assistant conversation's launch: YOLO, as a coordinator's, in its workspace folder, with
//! the Assistant's tools and every Space's MCP sources offered for their read tools alone. An
//! act tool is never offered: the host calls it itself, after the human's Run (see
//! `conversation_actions`).
//!
//! Claude offers every tool a server lists. A command-line server is started through the
//! helper's `mcp-filter`, which lists only its read tools. A server reached by URL (HTTP or SSE)
//! is given to Claude as configured, since Claude holds its sign-in: its act tools and every
//! other tool Claude has reported for it are denied by name, and a call to a tool not yet
//! reported stops the turn as it is made.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Claude's tools that read a server's resources, which no source allows; a resource is text
/// written by others that no read list names.
const RESOURCE_TOOLS: [&str; 2] = ["ListMcpResourcesTool", "ReadMcpResourceTool"];

/// An MCP server some Space lists as a source, with what the Spaces listing it allow.
#[derive(Clone, Debug, Default)]
pub struct SourceServer {
    /// The server as the provider's own configuration defines it.
    pub definition: Value,
    pub reads: BTreeSet<String>,
    pub acts: BTreeSet<String>,
    /// Every tool Claude reported the server offers, at an earlier turn's start.
    pub reported: BTreeSet<String>,
}
impl SourceServer {
    /// Its tools the provider is offered: read tools no Space lists as an act.
    fn offered(&self) -> impl Iterator<Item = &String> {
        self.reads.iter().filter(|tool| !self.acts.contains(*tool))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ConversationLaunch {
    /// Each MCP source's server the provider's configuration defines, by name.
    pub servers: BTreeMap<String, SourceServer>,
}
impl ConversationLaunch {
    /// Adds one Space's MCP source.
    pub fn add(&mut self, server: &str, definition: Value, reads: &[String], acts: &[String]) {
        let entry = self.servers.entry(server.to_owned()).or_default();
        entry.definition = definition;
        entry.reads.extend(reads.iter().cloned());
        entry.acts.extend(acts.iter().cloned());
    }
    /// Records the tools Claude reported a source's server offers.
    pub fn reported(&mut self, server: &str, tools: impl IntoIterator<Item = String>) {
        if let Some(source) = self.servers.get_mut(server) {
            source.reported.extend(tools);
        }
    }
    /// Claude's tools denied by name: working outside the folder Wiffletree assigned, reading a
    /// server's resources, and every source tool but its read tools: its acts and each other
    /// tool Claude reported for it.
    fn claude_denied(&self) -> Vec<String> {
        let sources = (self.servers.iter()).flat_map(|(server, source)| {
            let offered: BTreeSet<&String> = source.offered().collect();
            (source.acts.iter().chain(&source.reported))
                .filter(move |tool| !offered.contains(tool))
                .map(move |tool| format!("mcp__{server}__{tool}"))
        });
        ["EnterWorktree", "ExitWorktree"]
            .into_iter()
            .chain(RESOURCE_TOOLS)
            .map(str::to_owned)
            .chain(sources.collect::<BTreeSet<_>>())
            .collect()
    }
    /// The source tools Claude's start reports, by server: every `mcp__‹server›__‹tool›` of a
    /// source's server, to deny the ones not read by name from the next turn on.
    pub(crate) fn reported_by_claude(&self, init: &Value) -> BTreeMap<String, BTreeSet<String>> {
        let mut reported: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let names = (init["tools"].as_array().into_iter().flatten()).filter_map(Value::as_str);
        for name in names {
            if let Some((server, tool)) =
                name.strip_prefix("mcp__").and_then(|n| n.split_once("__"))
                && self.servers.contains_key(server)
            {
                reported
                    .entry(server.into())
                    .or_default()
                    .insert(tool.into());
            }
        }
        reported
    }
    /// The source tools Claude's start offers that are neither read tools nor denied: tools of a
    /// URL server it had not reported before. The turn stops before the model acts, and starts
    /// again with them denied.
    pub(crate) fn unseen_by_claude(&self, init: &Value) -> Vec<String> {
        (self.reported_by_claude(init).into_iter())
            .flat_map(|(server, tools)| tools.into_iter().map(move |tool| (server.clone(), tool)))
            .filter(|(server, tool)| {
                let source = &self.servers[server];
                !source.offered().any(|offered| offered == tool)
                    && !source.acts.contains(tool)
                    && !source.reported.contains(tool)
            })
            .map(|(server, tool)| format!("{server} {tool}"))
            .collect()
    }
    /// Each source's server as Claude starts it: a command-line server through the helper's
    /// `mcp-filter`, which lists and passes on only its read tools; a server reached by URL as
    /// configured, held to its read tools by `claude_denied` and `check_call`.
    fn claude_servers(&self, helper: &Path) -> Vec<(&str, Value)> {
        let filtered = |source: &SourceServer| -> Value {
            let Some(command) = source.definition["command"].as_str() else {
                return source.definition.clone();
            };
            let offered: Vec<&String> = source.offered().collect();
            let mut args = vec![
                json!("mcp-filter"),
                json!(json!(offered).to_string()),
                json!(command),
            ];
            args.extend(
                source.definition["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            let mut wrapped = json!({"command": helper, "args": args});
            if let Some(env) = source.definition.get("env") {
                wrapped["env"] = env.clone();
            }
            wrapped
        };
        (self.servers.iter())
            .map(|(server, source)| (server.as_str(), filtered(source)))
            .collect()
    }
    /// Refuses a call to an MCP tool the provider was not offered, as it is made: only the
    /// workspace's tools and the sources' read tools. Claude's filter and denials and Codex's
    /// enabled tools offer no others; this stops a turn that reaches one anyway, such as a tool
    /// of a URL server Claude had not reported before.
    pub(crate) fn check_call(&self, call: &ToolCall) -> Result<()> {
        let ToolCall::Mcp { server, tool } = call else {
            return Ok(());
        };
        if server == "agent_workspace" {
            return Ok(());
        }
        let source = self.servers.get(server).with_context(|| {
            format!(
                "Stopped: the Assistant called an MCP server no Space lists as a source ({server})"
            )
        })?;
        ensure!(
            source.offered().any(|offered| offered == tool),
            "Stopped: the Assistant called {server} {tool}, which is not one of that source's read tools; acting in another app goes through act"
        );
        Ok(())
    }
    /// What a call reads that others wrote, which holds the rest of its turn to cards: a
    /// source's read tool, a web search or a web page.
    pub(crate) fn others_text(&self, call: &ToolCall) -> Option<String> {
        match call {
            ToolCall::Mcp { server, tool } if self.servers.contains_key(server) => {
                Some(format!("{server} {tool}"))
            }
            ToolCall::Mcp { .. } => None,
            ToolCall::WebSearch => Some("a web search".into()),
            ToolCall::WebFetch => Some("a web page".into()),
        }
    }
}

/// A call the provider's stream announces that the host looks at in the Assistant's turns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToolCall {
    Mcp { server: String, tool: String },
    WebSearch,
    WebFetch,
}
/// The calls one record of the provider's stream announces: Claude's tool uses, as they start
/// streaming and in the finished message, and Codex's MCP calls and web searches.
pub(crate) fn tool_calls(provider: Provider, value: &Value) -> Vec<ToolCall> {
    let claude = |name: &str| match name {
        "WebSearch" => Some(ToolCall::WebSearch),
        "WebFetch" => Some(ToolCall::WebFetch),
        _ => {
            let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
            Some(ToolCall::Mcp {
                server: server.into(),
                tool: tool.into(),
            })
        }
    };
    match provider {
        Provider::Claude => {
            let started = &value["event"]["content_block"];
            let blocks = (value["message"]["content"].as_array().into_iter().flatten())
                .chain((value["type"] == "stream_event").then_some(started));
            blocks
                .filter(|block| block["type"] == "tool_use")
                .filter_map(|block| claude(block["name"].as_str()?))
                .collect()
        }
        Provider::Codex => {
            let item = &value["item"];
            match item["type"].as_str() {
                Some("mcp_tool_call") => vec![ToolCall::Mcp {
                    server: item["server"].as_str().unwrap_or_default().into(),
                    tool: item["tool"].as_str().unwrap_or_default().into(),
                }],
                Some("web_search") => vec![ToolCall::WebSearch],
                _ => vec![],
            }
        }
    }
}

/// YOLO, as a coordinator runs, with the Assistant's tools and the sources' servers. Codex runs
/// with no user configuration, so no MCP server of the human's own reaches the Assistant with
/// its act tools.
pub(super) fn configure(cmd: &mut ProcessCommand, turn: &Turn, launch: &ConversationLaunch) {
    let workspace = json!({"command":turn.helper,"args":["assistant-mcp"]});
    match turn.session.provider {
        Provider::Claude => {
            let mut servers = serde_json::Map::new();
            servers.insert("agent_workspace".into(), workspace);
            for (server, filtered) in launch.claude_servers(&turn.helper) {
                servers.insert(server.into(), filtered);
            }
            cmd.args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--strict-mcp-config",
                "--no-chrome",
            ])
            .args([
                "--model",
                &turn.profile.model,
                "--effort",
                &turn.profile.effort,
            ])
            .args(["--append-system-prompt", &turn.instructions])
            .args(["--system-prompt-snapshot", "off"])
            .args(["--mcp-config", &json!({"mcpServers":servers}).to_string()])
            .args(["--allowedTools", "mcp__agent_workspace__*"])
            .args(["--disallowedTools", &launch.claude_denied().join(",")])
            .args(["--dangerously-skip-permissions", "--tools", "default"]);
            if images(turn).next().is_some() {
                cmd.args(["--input-format", "stream-json"]);
            }
            if let Some(id) = &turn.provider_session {
                cmd.args(["--resume", id]);
            }
        }
        Provider::Codex => {
            cmd.args([
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--color",
                "never",
            ])
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("--ignore-user-config")
            .args(["-m", &turn.profile.model]);
            for setting in [
                format!("model_reasoning_effort={}", json!(turn.profile.effort)),
                format!("developer_instructions={}", json!(turn.instructions)),
                format!(
                    "mcp_servers.agent_workspace={{required=true, default_tools_approval_mode=\"approve\", command={}, args=[\"assistant-mcp\"], env_vars=[\"WORKSPACE_SOCKET\",\"WORKSPACE_TOKEN\"] }}",
                    json!(turn.helper)
                ),
            ] {
                cmd.arg("-c").arg(setting);
            }
            for (server, source) in &launch.servers {
                let offered: Vec<&String> = source.offered().collect();
                cmd.arg("-c").arg(format!(
                    "mcp_servers.{server}={}",
                    restricted::toml_inline(&source.definition)
                ));
                cmd.arg("-c").arg(format!(
                    "mcp_servers.{server}.enabled_tools={}",
                    json!(offered)
                ));
                cmd.arg("-c").arg(format!(
                    "mcp_servers.{server}.default_tools_approval_mode=\"approve\""
                ));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn gmail() -> ConversationLaunch {
        let mut launch = ConversationLaunch::default();
        let (reads, acts) = (["search_threads".into()], ["send_message".into()]);
        launch.add("gmail", json!({"command":"gmail-mcp"}), &reads, &acts);
        launch
    }

    #[test]
    fn a_sources_act_tools_are_denied_and_its_other_tools_stop_the_turn() {
        let launch = gmail();
        assert!((launch.claude_denied().iter()).any(|tool| tool == "mcp__gmail__send_message"));
        let call = |server: &str, tool: &str| ToolCall::Mcp {
            server: server.into(),
            tool: tool.into(),
        };
        // Claude starts the server through the filter, given only its read tools.
        let servers = launch.claude_servers(Path::new("/app/wiffletree-host"));
        assert_eq!(
            servers,
            [(
                "gmail",
                json!({"command": "/app/wiffletree-host", "args": ["mcp-filter", "[\"search_threads\"]", "gmail-mcp"]})
            )]
        );
        launch.check_call(&call("gmail", "search_threads")).unwrap();
        launch.check_call(&call("agent_workspace", "act")).unwrap();
        for (server, tool) in [
            ("gmail", "send_message"),
            ("gmail", "delete_thread"),
            ("github", "x"),
        ] {
            assert!(
                launch.check_call(&call(server, tool)).is_err(),
                "{server} {tool}"
            );
        }
        assert_eq!(
            launch
                .others_text(&call("gmail", "search_threads"))
                .as_deref(),
            Some("gmail search_threads")
        );
        assert_eq!(
            launch.others_text(&call("agent_workspace", "list_work")),
            None
        );
    }

    #[test]
    fn a_url_server_is_given_to_claude_with_every_tool_but_its_reads_denied_once_reported() {
        let mut launch = gmail();
        let calendar = json!({"type": "http", "url": "https://calendar.example/mcp", "headers": {"X-Key": "k"}});
        let (reads, acts) = (["list_events".into()], ["create_event".into()]);
        launch.add("calendar", calendar.clone(), &reads, &acts);
        let servers = launch.claude_servers(Path::new("/app/wiffletree-host"));
        assert_eq!(
            servers[0],
            ("calendar", calendar),
            "as configured: Claude holds its sign-in"
        );
        let denied = launch.claude_denied();
        for tool in [
            "mcp__calendar__create_event",
            "ListMcpResourcesTool",
            "ReadMcpResourceTool",
        ] {
            assert!(denied.iter().any(|d| d == tool), "{tool}");
        }
        // Claude's start reports every tool the server offers; from then on each but the
        // reads is denied by name, and until then a call to one stops the turn.
        let init = json!({"tools": ["Bash", "mcp__agent_workspace__act", "mcp__calendar__list_events", "mcp__calendar__delete_calendar", "mcp__other__x"]});
        let reported = launch.reported_by_claude(&init);
        assert_eq!(reported.keys().collect::<Vec<_>>(), ["calendar"]);
        let unseen = ToolCall::Mcp {
            server: "calendar".into(),
            tool: "delete_calendar".into(),
        };
        assert!(launch.check_call(&unseen).is_err());
        assert!(
            !launch
                .claude_denied()
                .iter()
                .any(|d| d == "mcp__calendar__delete_calendar")
        );
        // So the turn stops at its start, before the model acts, until the tool is denied.
        assert_eq!(launch.unseen_by_claude(&init), ["calendar delete_calendar"]);
        launch.reported("calendar", reported["calendar"].clone());
        assert!(launch.unseen_by_claude(&init).is_empty());
        let denied = launch.claude_denied();
        assert!(denied.iter().any(|d| d == "mcp__calendar__delete_calendar"));
        assert!(!denied.iter().any(|d| d == "mcp__calendar__list_events"));
    }

    #[test]
    fn claude_and_codex_announce_web_and_source_reads() {
        let streamed = json!({"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"tool_use","name":"WebFetch"}}});
        assert_eq!(
            tool_calls(Provider::Claude, &streamed),
            [ToolCall::WebFetch]
        );
        let message = json!({"type":"assistant","message":{"content":[{"type":"text","text":"x"},{"type":"tool_use","name":"mcp__gmail__search_threads"}]}});
        assert_eq!(
            tool_calls(Provider::Claude, &message),
            [ToolCall::Mcp {
                server: "gmail".into(),
                tool: "search_threads".into()
            }]
        );
        let search = json!({"type":"item.started","item":{"type":"web_search","query":"q"}});
        assert_eq!(tool_calls(Provider::Codex, &search), [ToolCall::WebSearch]);
    }
}
