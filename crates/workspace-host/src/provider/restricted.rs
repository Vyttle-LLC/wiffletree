//! The launch of an assistant thread, the one turn kind besides comment sorting that does not
//! run in YOLO mode. The provider holds the thread to reading, the web and its scratch folder.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use workspace_core::Source;

/// The thread tools, as MCP lists them.
pub(crate) const THREAD_TOOLS: [&str; 3] = ["save_output", "suggest_space", "project_brief"];
/// The built-in Claude tools every thread has; `Bash` joins them only for a CLI source.
const CLAUDE_TOOLS: [&str; 7] = [
    "Read",
    "Glob",
    "Grep",
    "WebSearch",
    "WebFetch",
    "Write",
    "Edit",
];
/// Codex features that act outside the sandbox or start other agents, turned off for threads.
const CODEX_FEATURES_OFF: [&str; 11] = [
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "in_app_browser",
    "in_app_local_automation",
    "multi_agent",
    "plugins",
    "remote_plugin",
    "hooks",
    "worktrees",
];

/// What a thread's turn may use beyond reading: its scratch folder and its Space's sources.
#[derive(Clone, Debug, Default)]
pub struct ThreadLaunch {
    pub scratch: PathBuf,
    pub sources: Vec<Source>,
    /// Each MCP source's server as the provider's own configuration defines it, by name; a
    /// source with no definition there is not offered.
    pub servers: BTreeMap<String, Value>,
}
impl ThreadLaunch {
    fn mcp_sources(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.sources.iter().filter_map(|source| match source {
            Source::Mcp { server, tools, .. } if self.servers.contains_key(server) => {
                Some((server.as_str(), tools.as_slice()))
            }
            _ => None,
        })
    }
    fn cli_prefixes(&self) -> impl Iterator<Item = &str> {
        self.sources
            .iter()
            .flat_map(|source| match source {
                Source::Cli { prefixes, .. } => prefixes.as_slice(),
                Source::Mcp { .. } => &[],
            })
            .map(String::as_str)
    }
    /// The MCP servers the run may report: the workspace's and the sources'.
    fn servers_given(&self) -> BTreeSet<&str> {
        std::iter::once("agent_workspace")
            .chain(self.mcp_sources().map(|(server, _)| server))
            .collect()
    }
    /// The built-in Claude tools the run is given.
    fn claude_tools(&self) -> Vec<&'static str> {
        let bash = self.cli_prefixes().next().is_some();
        CLAUDE_TOOLS
            .into_iter()
            .chain(bash.then_some("Bash"))
            .collect()
    }
    /// What Claude may do without asking; everything else is denied.
    pub(crate) fn claude_allowed(&self) -> Vec<String> {
        let scratch = self.scratch.display();
        let mut allowed: Vec<String> = ["Read", "Glob", "Grep", "WebSearch", "WebFetch"]
            .map(String::from)
            .into();
        // `//` makes a rule's path absolute; Edit rules cover every tool that writes files.
        for tool in ["Write", "Edit", "MultiEdit"] {
            allowed.push(format!("{tool}(/{scratch}/**)"));
        }
        allowed.extend(THREAD_TOOLS.map(|tool| format!("mcp__agent_workspace__{tool}")));
        for (server, tools) in self.mcp_sources() {
            allowed.extend(tools.iter().map(|tool| format!("mcp__{server}__{tool}")));
        }
        allowed.extend(
            self.cli_prefixes()
                .map(|prefix| format!("Bash({prefix}:*)")),
        );
        allowed
    }
    /// Refuses a Claude run whose start reports a tool or an MCP server it was not given.
    pub(crate) fn check_claude_start(&self, init: &Value) -> Result<()> {
        let given = self.servers_given();
        let reported: Vec<&str> = (init["mcp_servers"].as_array().into_iter().flatten())
            .filter_map(|server| server["name"].as_str())
            .collect();
        let unlisted: Vec<&str> = (reported.iter().copied())
            .filter(|server| !given.contains(server))
            .collect();
        ensure!(
            unlisted.is_empty(),
            "Stopped: the run was offered an MCP server its Space's sources do not list ({})",
            unlisted.join(", ")
        );
        let tools = self.claude_tools();
        let unexpected: Vec<&str> = (init["tools"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .filter(|tool| match tool.strip_prefix("mcp__") {
                Some(rest) => !given.iter().any(|s| rest.starts_with(&format!("{s}__"))),
                None => !tools.contains(tool),
            })
            .collect();
        ensure!(
            unexpected.is_empty(),
            "Stopped: the run was offered tools a thread is not given ({})",
            unexpected.join(", ")
        );
        ensure!(
            init["permissionMode"]
                .as_str()
                .is_none_or(|mode| mode == "dontAsk"),
            "Stopped: the run started in permission mode {} instead of denying what is not allowed",
            init["permissionMode"]
        );
        // Auto memory lives outside scratch and carries into later sessions.
        ensure!(
            init["memory_paths"]["auto"].is_null(),
            "Stopped: the run started with auto memory on, which writes outside the scratch folder"
        );
        Ok(())
    }
    /// Refuses a Codex tool call on an MCP server the thread was not given.
    /// Codex reports no tools or MCP servers when a run starts, so each MCP call is checked as
    /// it is made: one to a server or a tool the thread was not given stops the run.
    pub(crate) fn check_codex_item(&self, item: &Value) -> Result<()> {
        if item["type"] != "mcp_tool_call" {
            return Ok(());
        }
        let server = item["server"].as_str().unwrap_or_default();
        let tool = item["tool"].as_str().unwrap_or_default();
        ensure!(
            self.servers_given().contains(server),
            "Stopped: the run called an MCP server its Space's sources do not list ({server})"
        );
        let given = match server {
            "agent_workspace" => THREAD_TOOLS.contains(&tool),
            _ => (self.mcp_sources())
                .any(|(source, tools)| source == server && tools.iter().any(|t| t == tool)),
        };
        ensure!(
            given,
            "Stopped: the run called a tool it was not given ({server} {tool})"
        );
        Ok(())
    }
}

/// What Claude may never write, whatever else allows it: its own folder under the home
/// folder, where `dontAsk` still lets the session write its auto memory.
fn claude_denied() -> Vec<String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let claude = home.join(".claude");
    ["Write", "Edit", "MultiEdit"]
        .map(|tool| format!("{tool}(/{}/**)", claude.display()))
        .into()
}

/// Claude in `dontAsk` mode, which denies without asking every tool not allowed, with no
/// settings file that could allow more: the human's own, or one the thread wrote in scratch.
pub(super) fn configure_claude(cmd: &mut ProcessCommand, turn: &Turn, launch: &ThreadLaunch) {
    let mut servers = serde_json::Map::new();
    servers.insert(
        "agent_workspace".into(),
        json!({"command":turn.helper,"args":["thread-mcp"]}),
    );
    for (server, _) in launch.mcp_sources() {
        servers.insert(server.into(), launch.servers[server].clone());
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
    .args(["--setting-sources", ""])
    // Auto memory would load and invite writes to a folder outside scratch; the setting and
    // the variable each turn it off in Claude Code 2.1.310.
    .args(["--settings", r#"{"autoMemoryEnabled":false}"#])
    .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
    .args(["--permission-mode", "dontAsk"])
    .args(["--tools", &launch.claude_tools().join(",")])
    .args(["--allowedTools", &launch.claude_allowed().join(",")])
    .args(["--disallowedTools", &claude_denied().join(",")])
    .args(["--mcp-config", &json!({"mcpServers":servers}).to_string()]);
    if images(turn).next().is_some() {
        cmd.args(["--input-format", "stream-json"]);
    }
    if let Some(id) = &turn.provider_session {
        cmd.args(["--resume", id]);
    }
}

/// Codex's workspace-write sandbox rooted at the scratch folder, with no temporary folder, no
/// shell network and no approvals; only the thread's own MCP servers, limited to their tools.
pub(super) fn configure_codex(cmd: &mut ProcessCommand, turn: &Turn, launch: &ThreadLaunch) {
    cmd.args([
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--color",
        "never",
    ])
    .args(["--ignore-user-config", "--ignore-rules"])
    .args(["--sandbox", "workspace-write"])
    .arg("--cd")
    .arg(&launch.scratch)
    .args(["-m", &turn.profile.model]);
    for setting in [
        "approval_policy=\"never\"".to_owned(),
        "sandbox_workspace_write.network_access=false".into(),
        "sandbox_workspace_write.exclude_slash_tmp=true".into(),
        "sandbox_workspace_write.exclude_tmpdir_env_var=true".into(),
        "sandbox_workspace_write.writable_roots=[]".into(),
        "web_search=\"live\"".into(),
        format!("model_reasoning_effort={}", json!(turn.profile.effort)),
        format!("developer_instructions={}", json!(turn.instructions)),
        format!(
            "mcp_servers.agent_workspace={{required=true, default_tools_approval_mode=\"approve\", command={}, args=[\"thread-mcp\"], env_vars=[\"WORKSPACE_SOCKET\",\"WORKSPACE_TOKEN\"] }}",
            json!(turn.helper)
        ),
    ] {
        cmd.arg("-c").arg(setting);
    }
    for (server, tools) in launch.mcp_sources() {
        let definition = toml_inline(&launch.servers[server]);
        cmd.arg("-c")
            .arg(format!("mcp_servers.{server}={definition}"));
        cmd.arg("-c").arg(format!(
            "mcp_servers.{server}.enabled_tools={}",
            json!(tools)
        ));
        cmd.arg("-c").arg(format!(
            "mcp_servers.{server}.default_tools_approval_mode=\"approve\""
        ));
    }
    for feature in CODEX_FEATURES_OFF {
        cmd.args(["--disable", feature]);
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

/// A JSON value as a TOML inline value, as Codex's `-c` reads one.
fn toml_inline(value: &Value) -> String {
    match value {
        Value::Object(fields) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("{}={}", json!(key), toml_inline(value)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(toml_inline).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Null => "\"\"".into(),
        other => other.to_string(),
    }
}

/// The definition of the MCP server `name` in `provider`'s own configuration under `home`:
/// Claude's `~/.claude.json`, Codex's `config.toml` in `$CODEX_HOME` or `~/.codex`.
pub fn server_definition(provider: Provider, home: &Path, name: &str) -> Option<Value> {
    match provider {
        Provider::Claude => {
            let config: Value =
                serde_json::from_slice(&fs::read(home.join(".claude.json")).ok()?).ok()?;
            config["mcpServers"].get(name).cloned()
        }
        Provider::Codex => {
            let folder = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            let text = fs::read_to_string(folder.join("config.toml")).ok()?;
            let config: toml::Table = text.parse().ok()?;
            let server = config.get("mcp_servers")?.get(name)?.clone();
            serde_json::to_value(server).ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(sources: Vec<Source>) -> ThreadLaunch {
        let servers = [("linear".to_owned(), json!({"command":"linear-mcp"}))].into();
        ThreadLaunch {
            scratch: "/data/assistant/threads/t1".into(),
            sources,
            servers,
        }
    }
    fn linear() -> Source {
        Source::Mcp {
            server: "linear".into(),
            tools: vec!["list_issues".into()],
            extra: Default::default(),
        }
    }
    fn gh() -> Source {
        Source::Cli {
            command: "gh".into(),
            prefixes: vec!["gh pr list".into(), "gh pr view".into()],
            check: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn claude_may_write_only_under_scratch_and_run_only_a_sources_commands() {
        let allowed = launch(vec![]).claude_allowed();
        assert!(allowed.contains(&"Write(//data/assistant/threads/t1/**)".into()));
        assert!(allowed.contains(&"Edit(//data/assistant/threads/t1/**)".into()));
        assert!(!allowed.iter().any(|rule| rule.starts_with("Bash")));
        assert!(!allowed.iter().any(|rule| rule.contains("create_ticket")));
        assert!(!launch(vec![]).claude_tools().contains(&"Bash"));
        let sourced = launch(vec![gh(), linear()]);
        let allowed = sourced.claude_allowed();
        assert!(allowed.contains(&"Bash(gh pr list:*)".into()));
        assert!(allowed.contains(&"mcp__linear__list_issues".into()));
        assert!(sourced.claude_tools().contains(&"Bash"));
    }

    #[test]
    fn claude_may_never_write_its_own_folder_under_home() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            claude_denied(),
            ["Write", "Edit", "MultiEdit"].map(|tool| format!("{tool}(/{home}/.claude/**)"))
        );
    }

    #[test]
    fn a_claude_start_with_an_unlisted_server_or_tool_is_refused() {
        let launch = launch(vec![linear()]);
        let init = |servers: Value, tools: Value| json!({"mcp_servers":servers,"tools":tools,"permissionMode":"dontAsk"});
        let ours = json!([{"name":"agent_workspace","status":"connected"},{"name":"linear","status":"connected"}]);
        let tools = json!([
            "Read",
            "Write",
            "mcp__agent_workspace__save_output",
            "mcp__linear__create_issue"
        ]);
        launch
            .check_claude_start(&init(ours.clone(), tools))
            .unwrap();
        let extra = json!([{"name":"agent_workspace"},{"name":"github"}]);
        let refused = launch
            .check_claude_start(&init(extra, json!([])))
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("MCP server its Space's sources do not list (github)")
        );
        let refused = launch
            .check_claude_start(&init(ours.clone(), json!(["Bash"])))
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("tools a thread is not given (Bash)")
        );
        let mut bypass = init(ours.clone(), json!([]));
        bypass["permissionMode"] = json!("bypassPermissions");
        assert!(launch.check_claude_start(&bypass).is_err());
        let mut remembering = init(ours, json!([]));
        remembering["memory_paths"] = json!({"auto": "/Users/x/.claude/projects/-scratch/memory/"});
        let refused = launch.check_claude_start(&remembering).unwrap_err();
        assert!(refused.to_string().contains("auto memory on"));
    }

    #[test]
    fn the_rules_allow_exactly_the_tools_a_thread_is_offered() {
        let offered: Vec<String> = (crate::mcp::thread_tools().as_array().unwrap().iter())
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(offered, THREAD_TOOLS);
    }

    #[test]
    fn a_codex_call_to_an_unlisted_server_or_tool_is_refused() {
        let launch = launch(vec![linear()]);
        let call =
            |server: &str, tool: &str| json!({"type":"mcp_tool_call","server":server,"tool":tool});
        launch
            .check_codex_item(&call("agent_workspace", "save_output"))
            .unwrap();
        launch
            .check_codex_item(&call("linear", "list_issues"))
            .unwrap();
        assert!(
            launch
                .check_codex_item(&call("github", "list_issues"))
                .is_err()
        );
        assert!(
            launch
                .check_codex_item(&call("linear", "create_issue"))
                .is_err()
        );
        let refused = launch.check_codex_item(&call("agent_workspace", "create_ticket"));
        assert!(
            refused
                .unwrap_err()
                .to_string()
                .contains("(agent_workspace create_ticket)")
        );
        launch
            .check_codex_item(&json!({"type":"command_execution"}))
            .unwrap();
    }

    #[test]
    fn a_server_definition_becomes_an_inline_table() {
        let definition = json!({"command":"npx","args":["-y","linear"],"env":{"A":"1"}});
        let inline = toml_inline(&definition);
        let parsed: toml::Table = format!("server={inline}").parse().unwrap();
        assert_eq!(serde_json::to_value(&parsed["server"]).unwrap(), definition);
    }
}
