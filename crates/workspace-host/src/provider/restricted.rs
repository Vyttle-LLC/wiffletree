//! The launch of an assistant thread or a brain compile, the turn kinds besides comment sorting
//! that do not run in YOLO mode. The provider holds a thread to reading, the web and its scratch
//! folder, and a compile to reading and its worktree, where Claude writes only the wiki.
//!
//! A loop run on Claude may read a claude.ai connector its recipe lists. Claude loads
//! connectors only without `--strict-mcp-config` and with the human's user settings, so such a
//! run also loads the human's other servers and permission rules: each server no source lists
//! is denied whole once Claude has reported it (a start that offers one not yet denied stops
//! and starts again with it denied), the connector's tools but its reads are denied by name,
//! and each allow rule of the human's that the run is not given is denied, so it grants nothing.
use super::conversation::{SourceTools, claude_server_name, is_claude_ai};
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use workspace_core::Source;

/// An issue thread's tools, as MCP lists them: its outputs, cards and brain reads, its tracker's
/// and repositories' reads, and its proposals. It has no `log_work`, as it writes to no
/// repository. Issue threads are the only threads besides job runs that still run.
pub(crate) const ISSUE_THREAD_TOOLS: [&str; 12] = [
    "save_output",
    "suggest_space",
    "propose_issue",
    "query_brain",
    "list_issues",
    "read_issue",
    "search_repositories",
    "read_repository_file",
    "propose_edit",
    "propose_comment",
    "propose_split",
    "propose_duplicate",
];
/// The built-in Claude tools that reach the web, which an issue thread is never given: issue
/// text is untrusted, and the web would let it send what the thread read off this Mac.
const WEB_TOOLS: [&str; 2] = ["WebSearch", "WebFetch"];
/// Shell text a job run's commands may never hold: redirection, chaining, pipes,
/// substitution, variables, grouping, escapes and a second line.
const SHELL_TEXT: [char; 13] = [
    '>', '<', ';', '&', '|', '$', '`', '(', ')', '{', '}', '\\', '\n',
];

/// Why the hook before a job run's shell command refuses it, given the hook's input; `None`
/// lets Claude's own rules decide. Unreadable input is refused.
pub fn shell_text_refused(input: &[u8]) -> Option<String> {
    let input: Value = serde_json::from_slice(input).unwrap_or_default();
    let Some(command) = input["tool_input"]["command"].as_str() else {
        return Some(
            "Wiffletree could not read this command, so a job's run may not run it.".into(),
        );
    };
    command.contains(SHELL_TEXT).then(|| {
        "A job's run may run only its listed commands as they are, with no redirection, chaining, pipe, substitution or variable.".into()
    })
}
/// A job run's tools: it saves its output and reads its Space's brain, and offers no card.
pub(crate) const JOB_RUN_TOOLS: [&str; 2] = ["save_output", "query_brain"];
/// A brain compile's tools: it queues conflicts and says it is done, and has no other write.
pub(crate) const COMPILE_TOOLS: [&str; 2] = ["queue_conflict", "finish_compile"];
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
pub(super) const CODEX_FEATURES_OFF: [&str; 11] = [
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
    /// Each MCP source's server as the provider's own configuration defines it, by name, and
    /// on Claude each claude.ai connector, with no definition; a source not here is not
    /// offered.
    pub servers: BTreeMap<String, Value>,
    /// What earlier Claude starts that loaded connectors reported, to deny from the first start.
    pub seen: SourceTools,
    /// The allow rules in the human's own Claude settings, which a run that loads connectors
    /// loads too; each the run is not given is denied.
    pub user_allowed: Vec<String>,
    /// A job's run, which has only `JOB_RUN_TOOLS` and its recipe's sources.
    pub job_run: bool,
    /// An issue thread, which has `ISSUE_THREAD_TOOLS`, no source and no web.
    pub issue_thread: bool,
    /// A brain compile, which has `COMPILE_TOOLS`, no source and no web, and in its worktree,
    /// `scratch`, writes only this brain folder's `notes/` and `STATUS.md`.
    pub compile: Option<PathBuf>,
}
impl ThreadLaunch {
    /// Whether Codex may run it: not a job's run with a CLI source, since Codex's command rules
    /// cannot hold a CLI to its listed commands (task 3.6), or with a claude.ai connector,
    /// which only Claude reaches.
    pub(crate) fn codex_may_run(&self) -> bool {
        let connector = (self.sources.iter())
            .any(|source| matches!(source, Source::Mcp { server, .. } if is_claude_ai(server)));
        !self.job_run || (self.cli_prefixes().next().is_none() && !connector)
    }
    /// The settings a Claude run starts with: auto memory off and, for a job's run with a CLI
    /// source, a hook before every shell command that refuses any holding shell text (see
    /// `shell_text_refused`). Claude itself denies `;`, `&&` and `|` after an allowed prefix,
    /// but runs a redirection to a path it may write, and a deny rule cannot match one.
    fn claude_settings(&self, helper: &Path) -> Value {
        let mut settings = json!({"autoMemoryEnabled": false});
        if self.job_run && self.cli_prefixes().next().is_some() {
            let quoted = format!("'{}'", helper.display().to_string().replace('\'', r"'\''"));
            let check = json!({"type":"command","command":format!("{quoted} job-bash-check")});
            settings["hooks"] = json!({"PreToolUse":[{"matcher":"Bash","hooks":[check]}]});
        }
        settings
    }
    /// The workspace tools the run is offered.
    fn workspace_tools(&self) -> &'static [&'static str] {
        match (self.compile.is_some(), self.job_run, self.issue_thread) {
            (true, _, _) => &COMPILE_TOOLS,
            (false, true, _) => &JOB_RUN_TOOLS,
            (false, false, _) => &ISSUE_THREAD_TOOLS,
        }
    }
    /// The helper's argument that serves those tools.
    fn helper_arg(&self) -> &'static str {
        match (self.compile.is_some(), self.job_run, self.issue_thread) {
            (true, _, _) => "compile-mcp",
            (false, true, _) => "job-mcp",
            (false, false, _) => "issue-thread-mcp",
        }
    }
    /// Whether the run may search and fetch the web.
    fn web(&self) -> bool {
        !self.issue_thread && self.compile.is_none()
    }
    /// The files Claude may write: a compile's wiki, or else all of scratch.
    fn writable(&self) -> Vec<String> {
        match &self.compile {
            Some(brain) => vec![
                format!("/{}/notes/**", brain.display()),
                format!("/{}/STATUS.md", brain.display()),
            ],
            None => vec![format!("/{}/**", self.scratch.display())],
        }
    }
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
            .filter(|tool| self.web() || !WEB_TOOLS.contains(tool))
            .chain(bash.then_some("Bash"))
            .collect()
    }
    /// What Claude may do without asking; everything else is denied.
    pub(crate) fn claude_allowed(&self) -> Vec<String> {
        let mut allowed: Vec<String> = ["Read", "Glob", "Grep", "WebSearch", "WebFetch"]
            .into_iter()
            .filter(|tool| self.web() || !WEB_TOOLS.contains(tool))
            .map(String::from)
            .collect();
        // `//` makes a rule's path absolute; Edit rules cover every tool that writes files.
        for tool in ["Write", "Edit", "MultiEdit"] {
            allowed.extend(self.writable().iter().map(|path| format!("{tool}({path})")));
        }
        allowed.extend(
            (self.workspace_tools().iter()).map(|tool| format!("mcp__agent_workspace__{tool}")),
        );
        for (server, tools) in self.mcp_sources() {
            let server = claude_server_name(server);
            allowed.extend(tools.iter().map(|tool| format!("mcp__{server}__{tool}")));
        }
        allowed.extend(
            self.cli_prefixes()
                .map(|prefix| format!("Bash({prefix}:*)")),
        );
        allowed
    }
    /// Whether a source is a claude.ai connector, which Claude loads only without
    /// `--strict-mcp-config` and with the human's user settings.
    pub(crate) fn loads_connectors(&self) -> bool {
        self.mcp_sources().any(|(server, _)| is_claude_ai(server))
    }
    /// The connector sources and their read tools.
    fn connectors(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.mcp_sources()
            .filter(|(server, _)| is_claude_ai(server))
    }
    /// What a start that loads connectors reports: each connector's tools, by its source's
    /// name, and every server besides the workspace's that no source lists.
    pub(crate) fn reported_by_claude(&self, init: &Value) -> SourceTools {
        let mut reported = SourceTools::default();
        let names = (init["tools"].as_array().into_iter().flatten()).filter_map(Value::as_str);
        for name in names {
            let Some((server, tool)) = name.strip_prefix("mcp__").and_then(|n| n.split_once("__"))
            else {
                continue;
            };
            if let Some((source, _)) =
                (self.connectors()).find(|(source, _)| claude_server_name(source) == server)
            {
                (reported.tools.entry(source.into()).or_default()).insert(tool.into());
            }
        }
        let given = self.servers_given();
        reported.other_servers = (init["mcp_servers"].as_array().into_iter().flatten())
            .filter_map(|server| server["name"].as_str())
            .filter(|server| !given.contains(server))
            .map(str::to_owned)
            .collect();
        reported
    }
    /// What a start that loads connectors offers that is neither a read tool nor denied: a
    /// connector's other tools and servers no source lists, not reported before.
    pub(crate) fn unseen_by_claude(&self, init: &Value) -> Vec<String> {
        let reported = self.reported_by_claude(init);
        let tools = (reported.tools.into_iter())
            .flat_map(|(server, tools)| tools.into_iter().map(move |tool| (server.clone(), tool)))
            .filter(|(server, tool)| {
                let reads = (self.connectors()).find(|(source, _)| source == server);
                let seen = self.seen.tools.get(server);
                !reads.is_some_and(|(_, reads)| reads.contains(tool))
                    && !seen.is_some_and(|seen| seen.contains(tool))
            })
            .map(|(server, tool)| format!("{server} {tool}"));
        let servers = (reported.other_servers.into_iter())
            .filter(|server| !self.seen.other_servers.contains(server))
            .map(|server| format!("the server {server}"));
        tools.chain(servers).collect()
    }
    /// Refuses a Claude run whose start reports a tool or an MCP server it was not given, or a
    /// connector it reads that is not connected. A server denied whole may be listed, with no
    /// tools.
    pub(crate) fn check_claude_start(&self, init: &Value) -> Result<()> {
        let given = self.servers_given();
        let servers: Vec<&Value> = (init["mcp_servers"].as_array().into_iter().flatten()).collect();
        let unlisted: Vec<&str> = (servers.iter())
            .filter_map(|server| server["name"].as_str())
            .filter(|server| !given.contains(server))
            .filter(|server| !self.loads_connectors() || !self.seen.other_servers.contains(*server))
            .collect();
        ensure!(
            unlisted.is_empty(),
            "Stopped: the run was offered an MCP server its Space's sources do not list ({})",
            unlisted.join(", ")
        );
        for (connector, _) in self.connectors() {
            let status = (servers.iter())
                .find(|server| server["name"] == connector)
                .and_then(|server| server["status"].as_str())
                .unwrap_or("missing");
            ensure!(
                status == "connected",
                "The {connector} connector is not connected for Claude ({status}), such as when its sign-in has expired. Check it at claude.ai/customize/connectors."
            );
        }
        let tools = self.claude_tools();
        let prefixes: Vec<String> = (given.iter())
            .map(|server| format!("{}__", claude_server_name(server)))
            .collect();
        let connector_reads: BTreeSet<String> = (self.connectors())
            .flat_map(|(server, reads)| {
                let server = claude_server_name(server);
                reads
                    .iter()
                    .map(move |tool| format!("mcp__{server}__{tool}"))
            })
            .collect();
        let connector_prefixes: Vec<String> = (self.connectors())
            .map(|(server, _)| format!("mcp__{}__", claude_server_name(server)))
            .collect();
        let unexpected: Vec<&str> = (init["tools"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .filter(|tool| match tool.strip_prefix("mcp__") {
                // A connector's other tools are denied, so none may be offered.
                Some(_) if connector_prefixes.iter().any(|p| tool.starts_with(p)) => {
                    !connector_reads.contains(*tool)
                }
                Some(rest) => !prefixes.iter().any(|prefix| rest.starts_with(prefix)),
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
            "agent_workspace" => self.workspace_tools().contains(&tool),
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

/// What Claude may never do, whatever else allows it: write its own folder under the home
/// folder, where `dontAsk` still lets the session write its auto memory, and for an issue
/// thread reach the web. A run that loads connectors also may not use a connector's tools but
/// its reads, any server no source lists, or anything the human's own allow rules allow that
/// the run is not given.
fn claude_denied(launch: &ThreadLaunch) -> Vec<String> {
    let mut denied = claude_home_denied();
    if !launch.web() {
        denied.extend(WEB_TOOLS.map(String::from));
    }
    if !launch.loads_connectors() {
        return denied;
    }
    for (server, reads) in launch.connectors() {
        let name = claude_server_name(server);
        let seen = launch.seen.tools.get(server).into_iter().flatten();
        denied.extend(
            seen.filter(|tool| !reads.contains(tool))
                .map(|tool| format!("mcp__{name}__{tool}")),
        );
    }
    let given = launch.servers_given();
    denied.extend(
        (launch.seen.other_servers.iter())
            .filter(|server| !given.contains(server.as_str()))
            .map(|server| format!("mcp__{}", claude_server_name(server))),
    );
    let allowed = launch.claude_allowed();
    denied.extend(
        (launch.user_allowed.iter())
            .filter(|rule| !allowed.contains(rule))
            .cloned(),
    );
    denied
}

/// The allow rules in the human's own Claude settings under `home`.
pub fn user_allow_rules(home: &Path) -> Vec<String> {
    let path = home.join(".claude").join("settings.json");
    let settings: Value = (fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    (settings["permissions"]["allow"]
        .as_array()
        .into_iter()
        .flatten())
    .filter_map(Value::as_str)
    .map(str::to_owned)
    .collect()
}
/// Writes to Claude's own folder under the home folder, where `dontAsk` still lets a session
/// write its auto memory.
pub(super) fn claude_home_denied() -> Vec<String> {
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
        json!({"command":turn.helper,"args":[launch.helper_arg()]}),
    );
    for (server, _) in launch
        .mcp_sources()
        .filter(|(server, _)| !is_claude_ai(server))
    {
        servers.insert(server.into(), launch.servers[server].clone());
    }
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--no-chrome",
    ]);
    // Claude loads connectors itself, with the user's settings and only from a configuration
    // that is not strict, and reports them at its start only when connecting blocks.
    let setting_sources = match launch.loads_connectors() {
        true => {
            cmd.env("MCP_CONNECTION_NONBLOCKING", "false");
            "user"
        }
        false => {
            cmd.arg("--strict-mcp-config");
            ""
        }
    };
    cmd.args([
        "--model",
        &turn.profile.model,
        "--effort",
        &turn.profile.effort,
    ])
    .args(["--append-system-prompt", &turn.instructions])
    .args(["--system-prompt-snapshot", "off"])
    .args(["--setting-sources", setting_sources])
    // Auto memory would load and invite writes to a folder outside scratch; the setting and
    // the variable each turn it off in Claude Code 2.1.310.
    .args([
        "--settings",
        &launch.claude_settings(&turn.helper).to_string(),
    ])
    .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
    .args(["--permission-mode", "dontAsk"])
    .args(["--tools", &launch.claude_tools().join(",")])
    .args(["--allowedTools", &launch.claude_allowed().join(",")])
    .args(["--disallowedTools", &claude_denied(launch).join(",")])
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
        match launch.web() {
            true => "web_search=\"live\"".into(),
            false => "web_search=\"disabled\"".into(),
        },
        format!("model_reasoning_effort={}", json!(turn.profile.effort)),
        format!("developer_instructions={}", json!(turn.instructions)),
        format!(
            "mcp_servers.agent_workspace={{required=true, default_tools_approval_mode=\"approve\", command={}, args=[{}], env_vars=[\"WORKSPACE_SOCKET\",\"WORKSPACE_TOKEN\"] }}",
            json!(turn.helper),
            json!(launch.helper_arg())
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
pub(super) fn toml_inline(value: &Value) -> String {
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
            job_run: false,
            issue_thread: false,
            compile: None,
            ..Default::default()
        }
    }

    /// A morning brief's run on Claude that reads Gmail through its claude.ai connector, after
    /// an earlier start reported Gmail's other tools and the human's other servers.
    fn morning_brief() -> ThreadLaunch {
        let gmail: Source = serde_json::from_value(
            json!({"kind":"mcp","server":"claude.ai Gmail","tools":["search_threads"]}),
        )
        .unwrap();
        let mut run = ThreadLaunch {
            job_run: true,
            ..launch(vec![gmail, linear()])
        };
        run.servers.insert("claude.ai Gmail".into(), Value::Null);
        run.seen.tools.insert(
            "claude.ai Gmail".into(),
            ["search_threads".into(), "send_message".into()].into(),
        );
        run.seen.other_servers = ["claude.ai Slack".into(), "termic".into()].into();
        run.user_allowed = vec!["Edit".into(), "mcp__linear__list_issues".into()];
        run
    }

    #[test]
    fn a_loop_run_reads_a_connector_with_every_other_server_and_tool_denied() {
        let run = morning_brief();
        assert!(run.loads_connectors());
        let turn = super::super::tests::turn(Provider::Claude, None, vec![]);
        let mut cmd = ProcessCommand::new("claude");
        configure_claude(&mut cmd, &turn, &run);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into()).collect();
        let after = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        // Claude loads the connector itself, with the user's settings and not strictly.
        assert!(!args.contains(&"--strict-mcp-config".into()));
        assert_eq!(after("--setting-sources"), "user");
        assert!((cmd.get_envs()).any(|(name, value)| {
            name == "MCP_CONNECTION_NONBLOCKING" && value == Some("false".as_ref())
        }));
        let servers: Value = serde_json::from_str(after("--mcp-config")).unwrap();
        assert!(servers["mcpServers"].get("claude.ai Gmail").is_none());
        assert!(servers["mcpServers"].get("linear").is_some());
        // Only its read tools are allowed, never an act; the rest is denied by name.
        let allowed: Vec<&str> = after("--allowedTools").split(',').collect();
        assert!(allowed.contains(&"mcp__claude_ai_Gmail__search_threads"));
        assert!(!allowed.iter().any(|rule| rule.contains("send_message")));
        let denied: Vec<&str> = after("--disallowedTools").split(',').collect();
        for rule in [
            "mcp__claude_ai_Gmail__send_message",
            "mcp__claude_ai_Slack",
            "mcp__termic",
            // The human's own allow rule would let the run write anywhere.
            "Edit",
        ] {
            assert!(denied.contains(&rule), "{rule}: {denied:?}");
        }
        for kept in [
            "mcp__claude_ai_Gmail__search_threads",
            "mcp__linear__list_issues",
        ] {
            assert!(!denied.contains(&kept), "{kept}");
        }
        // A run without a connector stays strict and loads no settings of the human's.
        let strict = launch(vec![linear()]);
        let mut cmd = ProcessCommand::new("claude");
        configure_claude(&mut cmd, &turn, &strict);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into()).collect();
        assert!(args.contains(&"--strict-mcp-config".into()));
        assert!(args.windows(2).any(|w| w == ["--setting-sources", ""]));
    }

    #[test]
    fn a_loop_runs_start_that_offers_a_server_or_tool_not_yet_denied_starts_again() {
        let run = morning_brief();
        let connected = |name: &str| json!({"name": name, "status": "connected"});
        let init = |servers: Vec<Value>, tools: Value| json!({"permissionMode": "dontAsk", "mcp_servers": servers, "tools": tools});
        let ours = || {
            vec![
                connected("agent_workspace"),
                connected("linear"),
                connected("claude.ai Gmail"),
                connected("claude.ai Slack"),
                connected("termic"),
            ]
        };
        let reads = json!([
            "Read",
            "mcp__agent_workspace__save_output",
            "mcp__linear__list_issues",
            "mcp__claude_ai_Gmail__search_threads"
        ]);
        let start = init(ours(), reads.clone());
        assert!(run.unseen_by_claude(&start).is_empty());
        run.check_claude_start(&start).unwrap();

        // A new connector of the human's and a new Gmail tool stop the start, to be recorded.
        let mut more = ours();
        more.push(connected("claude.ai Google Drive"));
        let mut tools = reads.as_array().unwrap().clone();
        tools.push(json!("mcp__claude_ai_Gmail__trash_thread"));
        let start = init(more, json!(tools));
        assert_eq!(
            run.unseen_by_claude(&start),
            [
                "claude.ai Gmail trash_thread",
                "the server claude.ai Google Drive"
            ]
        );
        let reported = run.reported_by_claude(&start);
        assert!(reported.tools["claude.ai Gmail"].contains("trash_thread"));
        assert!(reported.other_servers.contains("claude.ai Google Drive"));
        assert!(!reported.other_servers.contains("linear"));

        // A denied tool still offered, or a signed-out connector, fails the run.
        let mut leaked = reads.as_array().unwrap().clone();
        leaked.push(json!("mcp__claude_ai_Gmail__send_message"));
        assert!(
            run.check_claude_start(&init(ours(), json!(leaked)))
                .is_err()
        );
        let mut signed_out = ours();
        signed_out[2] = json!({"name": "claude.ai Gmail", "status": "needs-auth"});
        let refused = run
            .check_claude_start(&init(signed_out, reads))
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("claude.ai/customize/connectors"),
            "{refused}"
        );
    }

    #[test]
    fn a_compile_writes_only_its_wiki_with_its_two_tools_and_no_web() {
        let compile = ThreadLaunch {
            scratch: "/data/compile-worktrees/r1".into(),
            compile: Some("/data/compile-worktrees/r1/second-brain".into()),
            ..launch(vec![])
        };
        let allowed = compile.claude_allowed();
        for tool in ["Write", "Edit", "MultiEdit"] {
            assert!(allowed.contains(&format!(
                "{tool}(//data/compile-worktrees/r1/second-brain/notes/**)"
            )));
            assert!(allowed.contains(&format!(
                "{tool}(//data/compile-worktrees/r1/second-brain/STATUS.md)"
            )));
        }
        assert!(!allowed.iter().any(|rule| rule.ends_with("/r1/**)")));
        let workspace: Vec<&String> = (allowed.iter())
            .filter(|rule| rule.starts_with("mcp__agent_workspace__"))
            .collect();
        assert_eq!(
            workspace,
            [
                "mcp__agent_workspace__queue_conflict",
                "mcp__agent_workspace__finish_compile"
            ]
        );
        for tool in WEB_TOOLS {
            assert!(!compile.claude_tools().contains(&tool));
            assert!(claude_denied(&compile).contains(&tool.to_owned()));
        }
        assert_eq!(compile.helper_arg(), "compile-mcp");
    }
    fn linear() -> Source {
        Source::Mcp {
            server: "linear".into(),
            tools: vec!["list_issues".into()],
            act: vec![],
            without_asking: vec![],
            extra: Default::default(),
        }
    }
    fn gh() -> Source {
        Source::Cli {
            command: "gh".into(),
            prefixes: vec!["gh pr list".into(), "gh pr view".into()],
            act: vec![],
            without_asking: vec![],
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
    fn a_loop_run_is_never_given_a_sources_act_tools_or_commands() {
        let acting = |source: Value| -> Source { serde_json::from_value(source).unwrap() };
        let run = ThreadLaunch {
            job_run: true,
            ..launch(vec![
                acting(
                    json!({"kind":"mcp","server":"linear","tools":["list_issues"],"act":["create_issue"],"without_asking":["create_issue"]}),
                ),
                acting(
                    json!({"kind":"cli","command":"gh","prefixes":["gh pr list"],"act":["gh pr comment"]}),
                ),
            ])
        };
        let allowed = run.claude_allowed();
        assert!(allowed.contains(&"mcp__linear__list_issues".into()));
        assert!(allowed.contains(&"Bash(gh pr list:*)".into()));
        assert!(
            !allowed
                .iter()
                .any(|rule| rule.contains("create_issue") || rule.contains("comment"))
        );
        let call = json!({"type":"mcp_tool_call","server":"linear","tool":"create_issue"});
        assert!(run.check_codex_item(&call).is_err());
    }

    #[test]
    fn claude_may_never_write_its_own_folder_under_home() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            claude_denied(&launch(vec![])),
            ["Write", "Edit", "MultiEdit"].map(|tool| format!("{tool}(/{home}/.claude/**)"))
        );
    }

    #[test]
    fn an_issue_thread_has_no_web_tool_on_either_provider() {
        let issue_thread = ThreadLaunch {
            issue_thread: true,
            ..launch(vec![])
        };
        for tool in WEB_TOOLS {
            assert!(!issue_thread.claude_tools().contains(&tool));
            assert!(!issue_thread.claude_allowed().contains(&tool.to_owned()));
            assert!(claude_denied(&issue_thread).contains(&tool.to_owned()));
            assert!(
                launch(vec![]).claude_tools().contains(&tool),
                "a thread keeps the web"
            );
        }
        assert!(
            (issue_thread.claude_allowed().iter())
                .any(|rule| rule == "mcp__agent_workspace__propose_edit")
        );
        assert!(
            !issue_thread
                .claude_allowed()
                .iter()
                .any(|rule| rule.contains("link_issue"))
        );
        let turn = super::super::tests::turn(Provider::Codex, None, vec![]);
        let mut codex = ProcessCommand::new("codex");
        configure_codex(&mut codex, &turn, &issue_thread);
        let args: Vec<String> = (codex.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.contains(&"web_search=\"disabled\"".to_owned()),
            "{args:?}"
        );
        assert!(!args.contains(&"web_search=\"live\"".to_owned()));
        assert!(args.iter().any(|arg| arg.contains("\"issue-thread-mcp\"")));
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
        let offered: Vec<String> = (crate::mcp::issue_thread_tools().as_array().unwrap().iter())
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(offered, ISSUE_THREAD_TOOLS);
        let offered: Vec<String> = (crate::mcp::job_run_tools().as_array().unwrap().iter())
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(offered, JOB_RUN_TOOLS);
    }

    #[test]
    fn a_job_runs_shell_commands_hold_no_shell_text() {
        let input = |command: &str| json!({"tool_input":{"command":command}}).to_string();
        assert_eq!(
            shell_text_refused(input("gh pr list --state open").as_bytes()),
            None
        );
        for command in [
            "gh pr list > /tmp/x",
            "gh pr list; touch x",
            "gh pr list && touch x",
            "gh pr list | tee x",
            "gh pr list $(touch x)",
            "gh pr list `touch x`",
            "gh pr list\ntouch x",
            "gh pr list < x",
        ] {
            assert!(
                shell_text_refused(input(command).as_bytes()).is_some(),
                "{command}"
            );
        }
        // Input that could not be read reaches it as nothing, and is refused.
        for unreadable in [&b"not json"[..], b"", b"{}"] {
            assert!(shell_text_refused(unreadable).is_some());
        }
        // Only a job's run with a CLI source gets the hook, which runs Wiffletree's helper.
        let helper = Path::new("/Apps/Wiffle tree/wiffletree-host");
        let mut run = launch(vec![gh()]);
        assert!(run.claude_settings(helper)["hooks"].is_null());
        run.job_run = true;
        let settings = run.claude_settings(helper);
        assert_eq!(settings["autoMemoryEnabled"], false);
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "'/Apps/Wiffle tree/wiffletree-host' job-bash-check"
        );
        run.sources.clear();
        assert!(run.claude_settings(helper)["hooks"].is_null());
    }

    #[test]
    fn a_job_run_is_offered_only_save_output_and_its_recipes_sources() {
        let mut run = launch(vec![gh()]);
        run.job_run = true;
        let allowed = run.claude_allowed();
        assert!(allowed.contains(&"mcp__agent_workspace__save_output".into()));
        for tool in ["propose_issue", "suggest_space", "read_issue"] {
            assert!(
                !allowed.contains(&format!("mcp__agent_workspace__{tool}")),
                "{tool}"
            );
        }
        assert!(allowed.contains(&"Bash(gh pr list:*)".into()));
        assert!(
            !allowed
                .iter()
                .any(|rule| rule == "Bash" || rule == "Bash(gh:*)")
        );
        let call =
            json!({"type":"mcp_tool_call","server":"agent_workspace","tool":"propose_issue"});
        assert!(run.check_codex_item(&call).is_err());
        // Codex cannot hold gh to its commands, so a run with a CLI source never starts on it.
        assert!(!run.codex_may_run());
        assert!(launch(vec![gh()]).codex_may_run());
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
