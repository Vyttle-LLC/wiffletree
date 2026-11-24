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
//!
//! A claude.ai connector, such as "claude.ai Gmail", cannot be named in an MCP configuration:
//! Claude loads the human's connectors itself, only without `--strict-mcp-config`. So when a
//! Space lists one, the conversation starts without it, and Claude also loads the human's
//! other servers: every other connector, the user's own servers and the plugins'. Each server
//! no Space lists is denied whole, by name, once Claude has reported it; until then a start
//! that offers one stops before the model acts and starts again with it denied, as for a
//! source's tools not seen before. Claude loads connectors after its start unless
//! `MCP_CONNECTION_NONBLOCKING` is false, so they are reported when it starts.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Claude's tools that read a server's resources, which no source allows; a resource is text
/// written by others that no read list names.
const RESOURCE_TOOLS: [&str; 2] = ["ListMcpResourcesTool", "ReadMcpResourceTool"];
/// The prefix of the servers Claude reaches through the human's claude.ai account.
const CLAUDE_AI_PREFIX: &str = "claude.ai ";

/// Whether a source's server is a claude.ai connector, which only Claude reaches and loads
/// itself, rather than a server some configuration defines.
pub fn is_claude_ai(server: &str) -> bool {
    server.starts_with(CLAUDE_AI_PREFIX)
}

/// A server's name as Claude puts it in its tools' names: each character but a letter, digit,
/// `_` or `-` becomes `_`, and for a claude.ai server runs of `_` are one, with none at
/// either end.
pub(super) fn claude_server_name(server: &str) -> String {
    let name: String = (server.chars())
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                true => c,
                false => '_',
            },
        )
        .collect();
    if !is_claude_ai(server) {
        return name;
    }
    let words: Vec<&str> = name.split('_').filter(|word| !word.is_empty()).collect();
    words.join("_")
}

/// What a Claude start reported of the servers it was not given in full, to deny by name from
/// the next start on: each source server's tools, and each server no Space lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceTools {
    pub tools: BTreeMap<String, BTreeSet<String>>,
    pub other_servers: BTreeSet<String>,
}
impl SourceTools {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.other_servers.is_empty()
    }
}

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
    /// Each MCP source's server the provider's configuration defines, or that Claude loads as
    /// a claude.ai connector, by name.
    pub servers: BTreeMap<String, SourceServer>,
    /// The servers Claude reported at an earlier start that no Space lists, each denied whole.
    pub other_servers: BTreeSet<String>,
}
impl ConversationLaunch {
    /// Adds one Space's MCP source; a claude.ai connector has no definition.
    pub fn add(&mut self, server: &str, definition: Value, reads: &[String], acts: &[String]) {
        let entry = self.servers.entry(server.to_owned()).or_default();
        entry.definition = definition;
        entry.reads.extend(reads.iter().cloned());
        entry.acts.extend(acts.iter().cloned());
    }
    /// Records what Claude reported at an earlier start.
    pub fn reported(&mut self, seen: SourceTools) {
        for (server, tools) in seen.tools {
            if let Some(source) = self.servers.get_mut(&server) {
                source.reported.extend(tools);
            }
        }
        self.other_servers.extend(seen.other_servers);
    }
    /// Whether a Space lists a claude.ai connector, which Claude loads only without
    /// `--strict-mcp-config`.
    fn loads_connectors(&self) -> bool {
        self.servers.keys().any(|server| is_claude_ai(server))
    }
    /// The source whose server is `name`, as configured or as Claude puts it in tool names.
    fn source(&self, name: &str) -> Option<(&String, &SourceServer)> {
        (self.servers.get_key_value(name)).or_else(|| {
            (self.servers.iter()).find(|(server, _)| claude_server_name(server) == name)
        })
    }
    /// Claude's tools denied by name: working outside the folder Wiffletree assigned, reading a
    /// server's resources, every source tool but its read tools: its acts and each other tool
    /// Claude reported for it, and every tool of a server no Space lists.
    fn claude_denied(&self) -> Vec<String> {
        let sources = (self.servers.iter()).flat_map(|(server, source)| {
            let offered: BTreeSet<&String> = source.offered().collect();
            let prefix = claude_server_name(server);
            (source.acts.iter().chain(&source.reported))
                .filter(move |tool| !offered.contains(tool))
                .map(move |tool| format!("mcp__{prefix}__{tool}"))
        });
        let others = (self.other_servers.iter())
            .filter(|server| !self.servers.contains_key(*server))
            .map(|server| format!("mcp__{}", claude_server_name(server)));
        ["EnterWorktree", "ExitWorktree"]
            .into_iter()
            .chain(RESOURCE_TOOLS)
            .map(str::to_owned)
            .chain(sources.chain(others).collect::<BTreeSet<_>>())
            .collect()
    }
    /// What Claude's start reports: every `mcp__‹server›__‹tool›` of a source's server, by the
    /// source's name, and every server besides the workspace's that no Space lists.
    pub(crate) fn reported_by_claude(&self, init: &Value) -> SourceTools {
        let mut reported = SourceTools::default();
        let names = (init["tools"].as_array().into_iter().flatten()).filter_map(Value::as_str);
        for name in names {
            if let Some((server, tool)) =
                name.strip_prefix("mcp__").and_then(|n| n.split_once("__"))
                && let Some((source, _)) = self.source(server)
            {
                (reported.tools.entry(source.clone()).or_default()).insert(tool.into());
            }
        }
        let servers = (init["mcp_servers"].as_array().into_iter().flatten())
            .filter_map(|server| server["name"].as_str());
        reported.other_servers = servers
            .filter(|server| *server != "agent_workspace" && !self.servers.contains_key(*server))
            .map(str::to_owned)
            .collect();
        reported
    }
    /// What Claude's start offers that is neither a read tool nor denied: tools of a source's
    /// server it had not reported before, and servers no Space lists that it had not reported
    /// before. The turn stops before the model acts, and starts again with them denied.
    pub(crate) fn unseen_by_claude(&self, init: &Value) -> Vec<String> {
        let reported = self.reported_by_claude(init);
        let tools = (reported.tools.into_iter())
            .flat_map(|(server, tools)| tools.into_iter().map(move |tool| (server.clone(), tool)))
            .filter(|(server, tool)| {
                let source = &self.servers[server];
                !source.offered().any(|offered| offered == tool)
                    && !source.acts.contains(tool)
                    && !source.reported.contains(tool)
            })
            .map(|(server, tool)| format!("{server} {tool}"));
        let servers = (reported.other_servers.into_iter())
            .filter(|server| !self.other_servers.contains(server))
            .map(|server| format!("the server {server}"));
        tools.chain(servers).collect()
    }
    /// Each source's server as Claude starts it: a command-line server through the helper's
    /// `mcp-filter`, which lists and passes on only its read tools; a server reached by URL as
    /// configured, held to its read tools by `claude_denied` and `check_call`. A claude.ai
    /// connector is not among them: Claude loads it itself.
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
            .filter(|(server, _)| !is_claude_ai(server))
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
        let (server, source) = self.source(server).with_context(|| {
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
            ToolCall::Mcp { server, tool } => {
                let (server, _) = self.source(server)?;
                Some(format!("{server} {tool}"))
            }
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
                "--no-chrome",
            ]);
            match launch.loads_connectors() {
                true => cmd.env("MCP_CONNECTION_NONBLOCKING", "false"),
                false => cmd.arg("--strict-mcp-config"),
            };
            cmd.args([
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
        assert_eq!(reported.tools.keys().collect::<Vec<_>>(), ["calendar"]);
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
        launch.reported(reported);
        assert!(launch.unseen_by_claude(&init).is_empty());
        let denied = launch.claude_denied();
        assert!(denied.iter().any(|d| d == "mcp__calendar__delete_calendar"));
        assert!(!denied.iter().any(|d| d == "mcp__calendar__list_events"));
    }

    #[test]
    fn a_listed_connector_is_offered_its_reads_and_every_unlisted_server_is_denied_whole() {
        let args = |launch: &ConversationLaunch| {
            let mut cmd = ProcessCommand::new("claude");
            configure(
                &mut cmd,
                &super::super::tests::turn(Provider::Claude, None, vec![]),
                launch,
            );
            let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into()).collect();
            let blocking = (cmd.get_envs()).any(|(name, value)| {
                name == "MCP_CONNECTION_NONBLOCKING" && value == Some("false".as_ref())
            });
            (args, blocking)
        };
        let (strict, blocking) = args(&gmail());
        assert!(strict.contains(&"--strict-mcp-config".into()) && !blocking);

        let mut launch = ConversationLaunch::default();
        let (reads, acts) = (["search_threads".into()], ["send_message".into()]);
        launch.add("claude.ai Gmail", Value::Null, &reads, &acts);
        // Claude loads the connector itself, only without a strict MCP configuration, and
        // reports it at its start only when connecting blocks.
        let (loose, blocking) = args(&launch);
        assert!(!loose.contains(&"--strict-mcp-config".into()) && blocking);
        assert!(launch.claude_servers(Path::new("/h")).is_empty());

        // Its first start offers the connector's other tools, the human's other connectors and
        // their own servers; the turn stops before the model acts, and once they are recorded
        // each is denied: the connector's tools by name, every other server whole.
        let init = json!({
            "mcp_servers": [
                {"name": "agent_workspace", "status": "connected"},
                {"name": "claude.ai Gmail", "status": "connected"},
                {"name": "claude.ai Slack", "status": "connected"},
                {"name": "plugin:design:figma", "status": "needs-auth"},
            ],
            "tools": [
                "mcp__agent_workspace__act",
                "mcp__claude_ai_Gmail__search_threads",
                "mcp__claude_ai_Gmail__delete_thread",
                "mcp__claude_ai_Slack__slack_send_message",
            ],
        });
        assert_eq!(
            launch.unseen_by_claude(&init),
            [
                "claude.ai Gmail delete_thread",
                "the server claude.ai Slack",
                "the server plugin:design:figma"
            ]
        );
        let reported = launch.reported_by_claude(&init);
        assert_eq!(
            reported.tools["claude.ai Gmail"],
            ["delete_thread".to_owned(), "search_threads".to_owned()].into()
        );
        launch.reported(reported);
        assert!(launch.unseen_by_claude(&init).is_empty());
        let denied = launch.claude_denied();
        for tool in [
            "mcp__claude_ai_Gmail__delete_thread",
            "mcp__claude_ai_Gmail__send_message",
            "mcp__claude_ai_Slack",
            "mcp__plugin_design_figma",
        ] {
            assert!(denied.iter().any(|d| d == tool), "{tool}");
        }
        for offered in [
            "mcp__claude_ai_Gmail__search_threads",
            "mcp__claude_ai_Gmail",
        ] {
            assert!(!denied.iter().any(|d| d == offered), "{offered}");
        }

        // Calls are named as Claude names the server; a read holds the turn.
        let call = |server: &str, tool: &str| ToolCall::Mcp {
            server: server.into(),
            tool: tool.into(),
        };
        launch
            .check_call(&call("claude_ai_Gmail", "search_threads"))
            .unwrap();
        assert!(
            launch
                .check_call(&call("claude_ai_Gmail", "send_message"))
                .is_err()
        );
        assert!(
            launch
                .check_call(&call("claude_ai_Slack", "slack_send_message"))
                .is_err()
        );
        assert_eq!(
            launch
                .others_text(&call("claude_ai_Gmail", "search_threads"))
                .as_deref(),
            Some("claude.ai Gmail search_threads")
        );

        // A server a Space lists later is no longer denied whole.
        launch.add(
            "claude.ai Slack",
            Value::Null,
            &["slack_read_channel".into()],
            &[],
        );
        assert!(
            !launch
                .claude_denied()
                .iter()
                .any(|d| d == "mcp__claude_ai_Slack")
        );
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
