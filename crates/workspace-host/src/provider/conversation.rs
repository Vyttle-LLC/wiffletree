//! The Assistant conversation's launch: YOLO, as a coordinator's, in its workspace folder, with
//! the Assistant's tools and every Space's MCP sources offered for their read tools alone. An
//! act tool is never offered: the host calls it itself, after the human's Run (see
//! `conversation_actions`).
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// An MCP server some Space lists as a source, with what the Spaces listing it allow.
#[derive(Clone, Debug, Default)]
pub struct SourceServer {
    /// The server as the provider's own configuration defines it.
    pub definition: Value,
    pub reads: BTreeSet<String>,
    pub acts: BTreeSet<String>,
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
    /// Claude's tools denied by name: working outside the folder Wiffletree assigned, and every
    /// source's act tools, so they are never offered.
    fn claude_denied(&self) -> Vec<String> {
        let acts = (self.servers.iter())
            .flat_map(|(server, source)| (source.acts.iter()).map(move |t| (server, t)))
            .map(|(server, tool)| format!("mcp__{server}__{tool}"));
        ["EnterWorktree".to_owned(), "ExitWorktree".to_owned()]
            .into_iter()
            .chain(acts)
            .collect()
    }
    /// Refuses a call to an MCP tool the provider was not offered, as it is made: only the
    /// workspace's tools and the sources' read tools. Claude is offered every tool of a source's
    /// server that is not denied by name, and Codex only the enabled ones, so this is what holds
    /// a server's other tools back.
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
            for (server, source) in &launch.servers {
                servers.insert(server.clone(), source.definition.clone());
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
