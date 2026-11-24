//! The launch of a UI evaluator run (OpenSpec change `ui-evaluators`): blind, on the model it
//! was started with, with no shell, web or workspace tool, no MCP server but an app run's
//! driver, and none of the human's provider instructions, settings or skills. The provider
//! holds it to its run folder.
use super::*;
use crate::app_driver;

/// The built-in Claude tools an evaluator has: reading and writing files, which its rules hold
/// to the run folder.
const CLAUDE_TOOLS: [&str; 5] = ["Read", "Glob", "Grep", "Write", "Edit"];
/// Codex features an evaluator never has: the threads' (`restricted::CODEX_FEATURES_OFF`), then
/// its shell, its image reader, which reads any path, and image generation. Code mode is left
/// with `apply_patch`, which the sandbox holds to the run folder.
const CODEX_FEATURES_OFF: [&str; 4] = [
    "shell_tool",
    "unified_exec",
    "view_image",
    "image_generation",
];
/// The only variables an evaluator's provider is given: none of Wiffletree's, such as
/// `WORKSPACE_TOKEN` and `WORKSPACE_SOCKET`, nor any other the host inherited.
const ENVIRONMENT: [&str; 7] = ["HOME", "USER", "LOGNAME", "PATH", "SHELL", "TMPDIR", "LANG"];

/// What an evaluator run's turn may use: its run folder alone, and for Codex a home of its own.
#[derive(Clone, Debug)]
pub struct EvaluatorLaunch {
    /// Canonical; it holds `prompt.md`.
    pub folder: PathBuf,
    /// Codex's home for evaluators, which holds only a link to the human's `auth.json`, so the
    /// human's `AGENTS.md`, config, skills and history do not reach the run.
    pub codex_home: PathBuf,
    /// An app run's test copy, which its turn starts, and the driver server that reaches it.
    pub app: Option<AppLaunch>,
}
impl EvaluatorLaunch {
    /// The empty folder an evaluator's Codex runs with as `HOME`, beside its Codex home: Codex
    /// also finds the human's skills under `~/.agents/skills`, whatever its own home says.
    pub fn codex_user_home(&self) -> PathBuf {
        self.codex_home.with_file_name("home")
    }
    /// `Read`, `Glob` and `Grep` of the folder and `Write`, `Edit` and `MultiEdit` in it;
    /// everything else is denied without asking.
    /// An app run's driver tools too.
    pub(crate) fn claude_allowed(&self) -> Vec<String> {
        // `//` makes a rule's path absolute.
        let folder = format!("/{}/**", self.folder.display());
        let files = ["Read", "Glob", "Grep", "Write", "Edit", "MultiEdit"]
            .map(|tool| format!("{tool}({folder})"));
        files.into_iter().chain(self.driver_tools()).collect()
    }
    /// The driver's tools as Claude names them; none for a run without a test copy.
    fn driver_tools(&self) -> Vec<String> {
        match self.app {
            Some(_) => (app_driver::TOOLS.iter())
                .map(|tool| format!("mcp__{}__{tool}", app_driver::SERVER))
                .collect(),
            None => vec![],
        }
    }
    /// The run's MCP servers: an app run's driver, connected to its copy's socket, alone.
    fn servers(&self, helper: &Path) -> Value {
        match &self.app {
            Some(app) => json!({app_driver::SERVER: {
                "command": helper,
                "args": ["app-driver-mcp", app.socket, self.folder],
            }}),
            None => json!({}),
        }
    }
    /// Refuses a Claude run that started with anything an evaluator is not given.
    pub(crate) fn check_claude_start(&self, init: &Value) -> Result<()> {
        let servers: Vec<(&Value, &Value)> = (init["mcp_servers"].as_array().into_iter())
            .flatten()
            .map(|server| (&server["name"], &server["status"]))
            .collect();
        let (name, connected) = (json!(app_driver::SERVER), json!("connected"));
        let expected: Vec<(&Value, &Value)> = match self.app {
            Some(_) => vec![(&name, &connected)],
            None => vec![],
        };
        ensure!(
            servers == expected,
            "Stopped: the evaluator was offered other MCP servers than its driver, or its driver did not connect ({})",
            init["mcp_servers"]
        );
        let driver = self.driver_tools();
        let unexpected: Vec<&str> = (init["tools"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .filter(|tool| !CLAUDE_TOOLS.contains(tool) && !driver.iter().any(|d| d == tool))
            .collect();
        ensure!(
            unexpected.is_empty(),
            "Stopped: the evaluator was offered tools it is not given ({})",
            unexpected.join(", ")
        );
        ensure!(
            init["permissionMode"] == "dontAsk",
            "Stopped: the evaluator started in permission mode {} instead of denying what is not allowed",
            init["permissionMode"]
        );
        ensure!(
            init["memory_paths"]["auto"].is_null(),
            "Stopped: the evaluator started with auto memory on, which writes outside its folder"
        );
        Ok(())
    }
    /// Refuses any MCP call but to an app run's driver: anything else reached the run.
    pub(crate) fn check_codex_item(&self, item: &Value) -> Result<()> {
        let to_driver = self.app.is_some()
            && item["server"] == app_driver::SERVER
            && (item["tool"].as_str()).is_some_and(|tool| app_driver::TOOLS.contains(&tool));
        ensure!(
            item["type"] != "mcp_tool_call" || to_driver,
            "Stopped: the evaluator called an MCP tool it was not given ({} {})",
            item["server"].as_str().unwrap_or_default(),
            item["tool"].as_str().unwrap_or_default()
        );
        Ok(())
    }
    /// Refuses a model other than the full ID asked for. An alias, such as `opus`, stands for
    /// whichever model the provider names, which the run records.
    pub(crate) fn check_model(&self, profile: &ModelProfile, reported: &str) -> Result<()> {
        ensure!(
            !pins_exactly(profile.provider, &profile.model) || reported == profile.model,
            "Stopped: {} ran {reported}, not {}, the model the run was started with. Wiffletree never runs an evaluator on another model.",
            profile.provider.label(),
            profile.model
        );
        Ok(())
    }
    /// The model Codex's session record for `thread` says its turn runs, once it is written.
    pub(crate) fn codex_model(&self, thread: &str) -> Option<String> {
        let file = rollout(
            &self.codex_home.join("sessions"),
            &format!("-{thread}.jsonl"),
        )?;
        let text = fs::read_to_string(file).ok()?;
        text.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|record| record["type"] == "turn_context")
            .and_then(|record| record["payload"]["model"].as_str().map(str::to_owned))
    }
}

/// Whether `model` names one model rather than an alias: Claude's aliases, such as `opus`,
/// move to newer models, and its full IDs start with `claude-`. Codex's models have no alias.
pub fn pins_exactly(provider: Provider, model: &str) -> bool {
    match provider {
        Provider::Claude => model.starts_with("claude-"),
        Provider::Codex => true,
    }
}

/// The file under `folder`, at any depth, whose name ends with `suffix`. Codex files a session
/// under `sessions/<year>/<month>/<day>/`.
fn rollout(folder: &Path, suffix: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = rollout(&path, suffix) {
                return Some(found);
            }
        } else if path.to_string_lossy().ends_with(suffix) {
            return Some(path);
        }
    }
    None
}

/// Clears the environment the host inherited and passes on only `ENVIRONMENT` and the locale.
pub(super) fn clear_environment(cmd: &mut ProcessCommand) {
    cmd.env_clear();
    for (name, value) in std::env::vars_os() {
        let name_text = name.to_string_lossy();
        if ENVIRONMENT.contains(&name_text.as_ref()) || name_text.starts_with("LC_") {
            cmd.env(name, value);
        }
    }
}

/// Claude in `dontAsk` mode with no settings, instructions, memory or MCP server of anyone's
/// but an app run's driver; its first message is the prompt alone.
pub(super) fn configure_claude(cmd: &mut ProcessCommand, turn: &Turn, launch: &EvaluatorLaunch) {
    clear_environment(cmd);
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
    .args(["--setting-sources", ""])
    .args([
        "--settings",
        &json!({"autoMemoryEnabled": false}).to_string(),
    ])
    .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
    .args(["--permission-mode", "dontAsk"])
    .args(["--tools", &CLAUDE_TOOLS.join(",")])
    .args(["--allowedTools", &launch.claude_allowed().join(",")])
    .args([
        "--disallowedTools",
        &restricted::claude_home_denied().join(","),
    ])
    .args([
        "--mcp-config",
        &json!({"mcpServers": launch.servers(&turn.helper)}).to_string(),
    ]);
    if let Some(id) = &turn.provider_session {
        cmd.args(["--resume", id]);
    }
}

/// Codex's workspace-write sandbox rooted at the run folder, with no network, temporary
/// folder, web search, shell or MCP server but an app run's driver, in a home that holds only
/// the human's sign-in and with an empty `HOME`, so none of the human's skills reach it.
pub(super) fn configure_codex(cmd: &mut ProcessCommand, turn: &Turn, launch: &EvaluatorLaunch) {
    clear_environment(cmd);
    cmd.env("CODEX_HOME", &launch.codex_home)
        .env("HOME", launch.codex_user_home())
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--color",
            "never",
        ])
        .args(["--ignore-user-config", "--ignore-rules"])
        .args(["--sandbox", "workspace-write"])
        .arg("--cd")
        .arg(&launch.folder)
        .args(["-m", &turn.profile.model]);
    for setting in [
        "approval_policy=\"never\"".to_owned(),
        "sandbox_workspace_write.network_access=false".into(),
        "sandbox_workspace_write.exclude_slash_tmp=true".into(),
        "sandbox_workspace_write.exclude_tmpdir_env_var=true".into(),
        "sandbox_workspace_write.writable_roots=[]".into(),
        "web_search=\"disabled\"".into(),
        format!("model_reasoning_effort={}", json!(turn.profile.effort)),
    ] {
        cmd.arg("-c").arg(setting);
    }
    if let Some(app) = &launch.app {
        let server = app_driver::SERVER;
        cmd.arg("-c").arg(format!(
            "mcp_servers.{server}={{required=true, default_tools_approval_mode=\"approve\", command={}, args={}}}",
            json!(turn.helper),
            json!(["app-driver-mcp", app.socket, launch.folder])
        ));
        cmd.arg("-c").arg(format!(
            "mcp_servers.{server}.enabled_tools={}",
            json!(app_driver::TOOLS)
        ));
    }
    for feature in restricted::CODEX_FEATURES_OFF
        .iter()
        .chain(&CODEX_FEATURES_OFF)
    {
        cmd.args(["--disable", feature]);
    }
    if let Some(id) = &turn.provider_session {
        cmd.args(["resume", id]);
    }
    cmd.arg("-");
}

/// Makes `home` a Codex home holding only a link to the human's `auth.json`, in `$CODEX_HOME`
/// or `~/.codex`. A link, not a copy, so a token Codex refreshes is the human's one token.
/// Makes the empty `HOME` beside it too (`EvaluatorLaunch::codex_user_home`).
pub fn prepare_codex_home(home: &Path) -> Result<()> {
    fs::create_dir_all(home)?;
    fs::create_dir_all(home.with_file_name("home"))?;
    let source = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .context("Codex's home is unknown: HOME is not set")?;
    let link = home.join("auth.json");
    let target = source.join("auth.json");
    if fs::read_link(&link).ok().as_deref() != Some(target.as_path()) {
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> EvaluatorLaunch {
        EvaluatorLaunch {
            folder: "/eval/runs/run-1".into(),
            codex_home: "/data/evaluators/codex-home".into(),
            app: None,
        }
    }
    /// An app run's launch, with its test copy.
    fn app_launch() -> EvaluatorLaunch {
        EvaluatorLaunch {
            app: Some(AppLaunch {
                target: EvaluationTarget {
                    executable: "/build/workspace-desktop".into(),
                    arguments: vec!["--data-dir".into(), "/tmp/ux".into()],
                    environment: Default::default(),
                },
                socket: "/tmp/wiffletree-run-1.sock".into(),
                log: "/data/evaluators/logs/run-1.log".into(),
            }),
            ..launch()
        }
    }
    fn arguments(provider: Provider) -> (Vec<String>, ProcessCommand) {
        arguments_for(provider, launch())
    }
    fn arguments_for(provider: Provider, launch: EvaluatorLaunch) -> (Vec<String>, ProcessCommand) {
        let mut turn = super::super::tests::turn(provider, None, vec![]);
        turn.instructions = "SECRET LOOP GOALS".into();
        turn.helper = "/app/wiffletree-host".into();
        turn.evaluator = Some(launch);
        let mut cmd = ProcessCommand::new("provider");
        configure(&mut cmd, &turn);
        let args = (cmd.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        (args, cmd)
    }
    fn after<'a>(args: &'a [String], flag: &str) -> &'a str {
        let at = args.iter().position(|arg| arg == flag).unwrap();
        &args[at + 1]
    }
    /// What the command sets and removes: with `env_clear`, every variable not set is gone.
    fn environment(cmd: &ProcessCommand) -> Vec<String> {
        (cmd.get_envs())
            .filter(|(_, value)| value.is_some())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn claude_has_only_its_folders_files_no_shell_web_mcp_settings_or_instructions() {
        let (args, cmd) = arguments(Provider::Claude);
        assert_eq!(after(&args, "--tools"), "Read,Glob,Grep,Write,Edit");
        assert_eq!(
            after(&args, "--allowedTools"),
            "Read(//eval/runs/run-1/**),Glob(//eval/runs/run-1/**),Grep(//eval/runs/run-1/**),Write(//eval/runs/run-1/**),Edit(//eval/runs/run-1/**),MultiEdit(//eval/runs/run-1/**)"
        );
        assert_eq!(after(&args, "--permission-mode"), "dontAsk");
        // No user, project or local settings: no CLAUDE.md, skills, hooks or permissions of
        // the human's.
        assert_eq!(after(&args, "--setting-sources"), "");
        assert!(args.contains(&"--strict-mcp-config".into()));
        assert_eq!(after(&args, "--mcp-config"), r#"{"mcpServers":{}}"#);
        assert_eq!(after(&args, "--settings"), r#"{"autoMemoryEnabled":false}"#);
        assert!(after(&args, "--disallowedTools").contains("/.claude/**)"));
        for absent in [
            "--append-system-prompt",
            "--dangerously-skip-permissions",
            "--resume",
            "--add-dir",
        ] {
            assert!(!args.contains(&absent.into()), "{absent}: {args:?}");
        }
        assert!(!args.iter().any(|arg| arg.contains("SECRET")));
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("Bash") || arg.contains("Web"))
        );
        assert_eq!(after(&args, "--model"), "m");
        let env = environment(&cmd);
        assert!(env.contains(&"CLAUDE_CODE_DISABLE_AUTO_MEMORY".into()));
        assert!(
            !env.iter().any(|name| name.starts_with("WORKSPACE_")),
            "{env:?}"
        );
    }

    #[test]
    fn codex_is_sandboxed_in_its_folder_with_no_shell_web_user_config_or_instructions() {
        let (args, cmd) = arguments(Provider::Codex);
        assert_eq!(after(&args, "--sandbox"), "workspace-write");
        assert_eq!(after(&args, "--cd"), "/eval/runs/run-1");
        for flag in ["--ignore-user-config", "--ignore-rules"] {
            assert!(args.contains(&flag.into()), "{flag}");
        }
        for setting in [
            "sandbox_workspace_write.network_access=false",
            "sandbox_workspace_write.writable_roots=[]",
            "sandbox_workspace_write.exclude_slash_tmp=true",
            "web_search=\"disabled\"",
            "approval_policy=\"never\"",
        ] {
            assert!(args.contains(&setting.into()), "{setting}: {args:?}");
        }
        let disabled: Vec<&str> = (args.windows(2))
            .filter(|pair| pair[0] == "--disable")
            .map(|pair| pair[1].as_str())
            .collect();
        for feature in [
            "shell_tool",
            "unified_exec",
            "view_image",
            "computer_use",
            "browser_use",
            "plugins",
            "multi_agent",
        ] {
            assert!(disabled.contains(&feature), "{feature}");
        }
        assert!(!args.iter().any(|arg| arg.contains("mcp_servers")));
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("developer_instructions"))
        );
        assert!(!args.iter().any(|arg| arg.contains("SECRET")));
        assert!(!args.contains(&"--dangerously-bypass-approvals-and-sandbox".into()));
        assert_eq!(args.last().unwrap(), "-");
        // Its own home keeps the human's AGENTS.md, config and skills out.
        let env: Vec<(String, Option<String>)> = (cmd.get_envs())
            .map(|(name, value)| {
                let value = value.map(|v| v.to_string_lossy().into_owned());
                (name.to_string_lossy().into_owned(), value)
            })
            .collect();
        assert!(env.contains(&(
            "CODEX_HOME".into(),
            Some("/data/evaluators/codex-home".into())
        )));
        // An empty HOME keeps out the skills Codex finds under ~/.agents/skills.
        assert!(env.contains(&("HOME".into(), Some("/data/evaluators/home".into()))));
        assert!(
            !env.iter().any(|(name, _)| name.starts_with("WORKSPACE_")),
            "{env:?}"
        );
    }

    #[test]
    fn a_claude_start_with_any_server_tool_mode_or_memory_is_refused() {
        let launch = launch();
        let init = |servers: Value, tools: Value| json!({"mcp_servers":servers,"tools":tools,"permissionMode":"dontAsk","memory_paths":null});
        let tools = json!(["Read", "Glob", "Grep", "Write", "Edit"]);
        launch
            .check_claude_start(&init(json!([]), tools.clone()))
            .unwrap();
        let server = json!([{"name":"agent_workspace","status":"connected"}]);
        assert!(
            launch
                .check_claude_start(&init(server, tools.clone()))
                .is_err()
        );
        for tool in [
            "Bash",
            "WebFetch",
            "Skill",
            "Agent",
            "mcp__agent_workspace__report",
        ] {
            let refused = launch
                .check_claude_start(&init(json!([]), json!([tool])))
                .unwrap_err();
            assert!(refused.to_string().contains(tool), "{refused}");
        }
        let mut bypass = init(json!([]), tools.clone());
        bypass["permissionMode"] = json!("bypassPermissions");
        assert!(launch.check_claude_start(&bypass).is_err());
        let mut remembering = init(json!([]), tools);
        remembering["memory_paths"] = json!({"auto": "/Users/x/.claude/projects/-eval/memory/"});
        assert!(launch.check_claude_start(&remembering).is_err());
    }

    #[test]
    fn a_full_id_must_be_the_model_the_provider_runs_and_an_alias_takes_its_word() {
        let profile = |provider, model: &str| ModelProfile {
            provider,
            model: model.into(),
            effort: "high".into(),
        };
        let exact = profile(Provider::Claude, "claude-opus-5-5");
        launch().check_model(&exact, "claude-opus-5-5").unwrap();
        let moved = launch()
            .check_model(&exact, "claude-opus-5-6")
            .unwrap_err()
            .to_string();
        assert!(
            moved.contains("Claude ran claude-opus-5-6, not claude-opus-5-5"),
            "{moved}"
        );
        // `opus` names whichever model Claude says; the run records which.
        let alias = profile(Provider::Claude, "opus");
        launch().check_model(&alias, "claude-opus-5-6").unwrap();
        // Codex's models have no alias.
        let codex = profile(Provider::Codex, "gpt-6.1-sol");
        assert!(launch().check_model(&codex, "gpt-6.2-sol").is_err());
        launch().check_model(&codex, "gpt-6.1-sol").unwrap();
    }

    #[test]
    fn codexs_model_is_read_from_its_session_record_once_written() {
        let home = tempfile::tempdir().unwrap();
        let launch = EvaluatorLaunch {
            codex_home: home.path().into(),
            ..launch()
        };
        assert_eq!(launch.codex_model("t1"), None);
        let day = home.path().join("sessions/2026/11/04");
        fs::create_dir_all(&day).unwrap();
        let record = [
            json!({"type":"session_meta","payload":{"id":"t1"}}),
            json!({"type":"turn_context","payload":{"model":"gpt-6-luna","effort":"low"}}),
        ]
        .map(|line| line.to_string())
        .join("\n");
        fs::write(day.join("rollout-2026-11-04T06-25-43-t1.jsonl"), record).unwrap();
        assert_eq!(launch.codex_model("t1").as_deref(), Some("gpt-6-luna"));
        assert_eq!(launch.codex_model("t2"), None);
    }

    #[test]
    fn an_app_runs_claude_gets_the_driver_server_and_its_tools_alone() {
        let (args, _) = arguments_for(Provider::Claude, app_launch());
        let config: Value = serde_json::from_str(after(&args, "--mcp-config")).unwrap();
        assert_eq!(
            config,
            json!({"mcpServers":{"app":{"command":"/app/wiffletree-host","args":["app-driver-mcp","/tmp/wiffletree-run-1.sock","/eval/runs/run-1"]}}})
        );
        assert_eq!(after(&args, "--tools"), "Read,Glob,Grep,Write,Edit");
        let allowed: Vec<&str> = after(&args, "--allowedTools").split(',').collect();
        let driver: Vec<&str> = (allowed.iter().copied())
            .filter(|tool| tool.starts_with("mcp__"))
            .collect();
        assert_eq!(
            driver,
            app_driver::TOOLS.map(|tool| format!("mcp__app__{tool}"))
        );
    }

    #[test]
    fn an_app_runs_claude_start_needs_its_driver_and_nothing_else() {
        let app = app_launch();
        let init = |servers: Value, tools: Value| json!({"mcp_servers":servers,"tools":tools,"permissionMode":"dontAsk","memory_paths":null});
        let tools = json!(["Read", "Write", "mcp__app__screenshot", "mcp__app__click"]);
        let driver = json!([{"name":"app","status":"connected"}]);
        app.check_claude_start(&init(driver.clone(), tools.clone()))
            .unwrap();
        for servers in [
            json!([]),
            json!([{"name":"app","status":"failed"}]),
            json!([{"name":"app","status":"connected"},{"name":"agent_workspace","status":"connected"}]),
        ] {
            assert!(
                app.check_claude_start(&init(servers.clone(), tools.clone()))
                    .is_err(),
                "{servers}"
            );
        }
        for tool in ["mcp__app__drop", "mcp__agent_workspace__report", "Bash"] {
            assert!(
                app.check_claude_start(&init(driver.clone(), json!([tool])))
                    .is_err(),
                "{tool}"
            );
        }
        // A run without a test copy has no driver to connect.
        assert!(launch().check_claude_start(&init(driver, tools)).is_err());
    }

    #[test]
    fn an_app_runs_codex_gets_the_driver_server_and_its_tools_alone() {
        let (args, _) = arguments_for(Provider::Codex, app_launch());
        let settings: Vec<&str> = (args.windows(2))
            .filter(|pair| pair[0] == "-c")
            .map(|pair| pair[1].as_str())
            .filter(|setting| setting.starts_with("mcp_servers"))
            .collect();
        assert_eq!(
            settings,
            [
                r#"mcp_servers.app={required=true, default_tools_approval_mode="approve", command="/app/wiffletree-host", args=["app-driver-mcp","/tmp/wiffletree-run-1.sock","/eval/runs/run-1"]}"#,
                r#"mcp_servers.app.enabled_tools=["screenshot","click","double_click","right_click","move","type_text","key","scroll","drag","resize_window","window"]"#,
            ]
        );
        let call =
            |server: &str, tool: &str| json!({"type":"mcp_tool_call","server":server,"tool":tool});
        let app = app_launch();
        app.check_codex_item(&call("app", "screenshot")).unwrap();
        assert!(app.check_codex_item(&call("app", "drop")).is_err());
        assert!(
            app.check_codex_item(&call("agent_workspace", "report"))
                .is_err()
        );
        assert!(
            launch()
                .check_codex_item(&call("app", "screenshot"))
                .is_err()
        );
    }

    #[test]
    fn a_codex_mcp_call_is_refused() {
        let call = json!({"type":"mcp_tool_call","server":"agent_workspace","tool":"report"});
        assert!(launch().check_codex_item(&call).is_err());
        let patch = json!({"type":"file_change"});
        launch().check_codex_item(&patch).unwrap();
    }

    #[test]
    fn the_codex_home_links_only_the_humans_sign_in() {
        let home = tempfile::tempdir().unwrap();
        let evaluators = home.path().join("codex-home");
        prepare_codex_home(&evaluators).unwrap();
        prepare_codex_home(&evaluators).unwrap();
        let entries: Vec<_> = fs::read_dir(&evaluators).unwrap().collect();
        assert_eq!(entries.len(), 1);
        assert!(fs::read_link(evaluators.join("auth.json")).is_ok());
        let user_home = home.path().join("home");
        assert_eq!(fs::read_dir(user_home).unwrap().count(), 0);
    }
}
