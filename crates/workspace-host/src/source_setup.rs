//! Add source on a Space's page (OpenSpec change `assistant-does-the-work`, spaces "Connecting
//! a source is one form"): the MCP servers this Mac's Claude and Codex are configured with and
//! the well-known CLIs installed here, and what one of them offers once started. Wiffletree
//! runs no app's sign-in itself; a signed-out source names the command its own tool gives.
use crate::jobs::{SourceEnv, known_check, mcp_exchange};
use crate::*;
use workspace_core::job::{JOB_CLIS, cli_reads};
use workspace_core::{SourceCandidate, SourceKind, SourceProbe, SuggestedTool, suggests_act};

/// The act commands Add source suggests for a CLI it knows. Its reads are the loops' own.
const CLI_ACTS: [(&str, &[&str]); 2] = [
    (
        "gh",
        &[
            "pr comment",
            "pr create",
            "pr review",
            "issue create",
            "issue comment",
            "issue close",
        ],
    ),
    (
        "linctl",
        &["issue create", "issue update", "comment create"],
    ),
];

/// A server reached by URL (HTTP or SSE) rather than started as a command.
fn by_url(definition: &Value) -> bool {
    definition["command"].as_str().is_none_or(str::is_empty)
}

/// The MCP servers configured for Claude and Codex, each once, then the well-known CLIs found
/// on this Mac.
pub(crate) fn candidates(env: &SourceEnv) -> Vec<SourceCandidate> {
    let mut found: Vec<SourceCandidate> = vec![];
    for provider in [Provider::Claude, Provider::Codex] {
        for (name, definition) in provider::configured_servers(provider, &env.home) {
            match found.iter_mut().find(|c| c.name == name) {
                Some(known) => known.configured_for.push(provider),
                None => found.push(SourceCandidate {
                    kind: SourceKind::Mcp,
                    by_url: by_url(&definition),
                    name,
                    configured_for: vec![provider],
                }),
            }
        }
    }
    let clis = (JOB_CLIS.iter()).filter(|command| env.find(command).is_some());
    found.extend(clis.map(|command| SourceCandidate {
        kind: SourceKind::Cli,
        name: (*command).into(),
        configured_for: vec![],
        by_url: false,
    }));
    found
}

/// Starts the MCP server `name` as Claude, else Codex, would start it and lists its tools, or
/// checks the CLI `name` is installed and signed in and lists the commands Wiffletree knows.
pub(crate) fn probe(env: &SourceEnv, kind: SourceKind, name: &str) -> SourceProbe {
    match kind {
        SourceKind::Mcp => probe_server(env, name),
        SourceKind::Cli => probe_cli(env, name),
    }
}

fn probe_server(env: &SourceEnv, name: &str) -> SourceProbe {
    let definition = [Provider::Claude, Provider::Codex]
        .into_iter()
        .find_map(|provider| provider::server_definition(provider, &env.home, name));
    let Some(definition) = definition else {
        return problem(format!(
            "The {name} MCP server is not configured for Claude or Codex on this Mac."
        ));
    };
    if by_url(&definition) {
        return problem(format!(
            "The {name} MCP server is reached by URL, which Wiffletree cannot start to list its tools. Type its tools by hand. Its act tools are refused for now; its read tools work."
        ));
    }
    let list = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    match mcp_exchange(env, &definition, list) {
        Ok(listed) => SourceProbe {
            tools: server_tools(&listed),
            ..Default::default()
        },
        Err(why) => problem(format!("The {name} MCP server {why}.")),
    }
}

/// A `tools/list` result's tools with the kind each suggests.
fn server_tools(listed: &Value) -> Vec<SuggestedTool> {
    (listed["tools"].as_array().into_iter().flatten())
        .filter_map(|tool| {
            let name = tool["name"].as_str()?;
            let read_only = tool["annotations"]["readOnlyHint"].as_bool();
            Some(SuggestedTool {
                name: name.into(),
                act: suggests_act(name, read_only),
                description: tool["description"].as_str().unwrap_or_default().into(),
            })
        })
        .collect()
}

fn probe_cli(env: &SourceEnv, command: &str) -> SourceProbe {
    let Some(program) = env.find(command) else {
        return problem(format!("{command} is not installed on this Mac."));
    };
    let as_tools = |operations: &[&str], act: bool| -> Vec<SuggestedTool> {
        (operations.iter())
            .map(|operation| SuggestedTool {
                name: format!("{command} {operation}"),
                act,
                description: String::new(),
            })
            .collect()
    };
    let acts = (CLI_ACTS.iter()).find(|(cli, _)| *cli == command);
    let mut tools = as_tools(cli_reads(command), false);
    tools.extend(
        acts.map(|(_, acts)| as_tools(acts, true))
            .unwrap_or_default(),
    );
    let mut probe = SourceProbe {
        tools,
        ..Default::default()
    };
    // aws names its profile in each command, so it has no check before one is typed.
    if let Some((args, fix)) = known_check(command, &[]) {
        let mut check = std::process::Command::new(program);
        check
            .args(&args[1..])
            .env("PATH", std::env::join_paths(&env.path).unwrap_or_default());
        let passed = runtime::run_limited(check, jobs::CHECK_LIMIT)
            .ok()
            .flatten()
            .is_some_and(|status| status.success());
        if !passed {
            probe.problem = Some(format!("{command} is not signed in on this Mac."));
            probe.sign_in = Some(fix);
        }
    }
    probe
}

fn problem(why: String) -> SourceProbe {
    SourceProbe {
        problem: Some(why),
        ..Default::default()
    }
}

impl Host {
    pub(crate) fn source_candidates(&self) -> Vec<SourceCandidate> {
        candidates(&self.source_env())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &Path, path: Vec<PathBuf>) -> SourceEnv {
        SourceEnv {
            path,
            home: home.into(),
            mcp_limit: std::time::Duration::from_secs(5),
        }
    }

    #[test]
    fn candidates_list_each_configured_server_once_and_the_clis_found() {
        let home = tempfile::tempdir().unwrap();
        let claude = json!({"mcpServers": {
            "calendar": {"command": "calendar-mcp"},
            "gmail": {"type": "http", "url": "https://example.com/mcp"},
        }});
        fs::write(home.path().join(".claude.json"), claude.to_string()).unwrap();
        let codex = home.path().join(".codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("config.toml"),
            "[mcp_servers.calendar]\ncommand = \"calendar-mcp\"\n",
        )
        .unwrap();
        let bin = home.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("gh"), "").unwrap();
        let found = candidates(&env(home.path(), vec![bin]));
        let calendar = found.iter().find(|c| c.name == "calendar").unwrap();
        assert_eq!(calendar.configured_for, [Provider::Claude, Provider::Codex]);
        assert!(found.iter().any(|c| c.name == "gmail" && c.by_url));
        assert!(
            found
                .iter()
                .any(|c| c.name == "gh" && c.kind == SourceKind::Cli)
        );
        assert!(!found.iter().any(|c| c.name == "linctl"));
    }

    #[test]
    fn a_server_lists_its_tools_with_suggested_kinds() {
        let home = tempfile::tempdir().unwrap();
        let server = home.path().join("calendar-mcp");
        let tools = json!([
            {"name": "list_events", "description": "Events in a range"},
            {"name": "create_event"},
            {"name": "summarize_day", "annotations": {"readOnlyHint": true}},
        ]);
        let script = format!(
            "#!/usr/bin/env python3\nimport json, sys\nfor line in sys.stdin:\n    m = json.loads(line)\n    if 'id' not in m: continue\n    result = {{'tools': json.loads('{tools}')}} if m['method'] == 'tools/list' else {{}}\n    print(json.dumps({{'jsonrpc': '2.0', 'id': m['id'], 'result': result}}), flush=True)\n"
        );
        fs::write(&server, script).unwrap();
        fs::set_permissions(&server, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let claude = json!({"mcpServers": {"calendar": {"command": server}}});
        fs::write(home.path().join(".claude.json"), claude.to_string()).unwrap();
        // The server's `python3` comes from the test's own PATH.
        let path = std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect();
        let probe = probe(&env(home.path(), path), SourceKind::Mcp, "calendar");
        assert_eq!(probe.problem, None);
        let kinds: Vec<(&str, bool)> = (probe.tools.iter())
            .map(|tool| (tool.name.as_str(), tool.act))
            .collect();
        assert_eq!(
            kinds,
            [
                ("list_events", false),
                ("create_event", true),
                ("summarize_day", false)
            ]
        );
        assert_eq!(probe.tools[0].description, "Events in a range");
    }

    #[test]
    fn a_signed_out_cli_names_its_own_sign_in_command() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let gh = bin.join("gh");
        fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let probe = probe(&env(home.path(), vec![bin]), SourceKind::Cli, "gh");
        assert_eq!(
            probe.problem.as_deref(),
            Some("gh is not signed in on this Mac.")
        );
        assert_eq!(probe.sign_in.as_deref(), Some("gh auth login"));
        let act = probe.tools.iter().find(|t| t.name == "gh pr comment");
        assert!(act.is_some_and(|tool| tool.act));
        assert!(probe.tools.iter().any(|t| t.name == "gh pr list" && !t.act));
    }

    #[test]
    fn a_server_reached_by_url_is_typed_by_hand() {
        let home = tempfile::tempdir().unwrap();
        let claude =
            json!({"mcpServers": {"gmail": {"type": "http", "url": "https://example.com"}}});
        fs::write(home.path().join(".claude.json"), claude.to_string()).unwrap();
        let probe = probe(&env(home.path(), vec![]), SourceKind::Mcp, "gmail");
        assert!(probe.tools.is_empty());
        assert!(probe.problem.unwrap().contains("reached by URL"));
    }
}
