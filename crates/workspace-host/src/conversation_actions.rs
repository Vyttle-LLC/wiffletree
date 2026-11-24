//! The Assistant's acts in other apps, through the Spaces' sources: `read_source` runs a CLI
//! source's read command, and `act` asks for an act tool or command, which the host runs itself
//! on the human's Run, or at once for a kind marked Do without asking in a turn that is not
//! held. The provider is never offered an act tool, so an MCP source's acts are held by the
//! host; a CLI's only by the Assistant's instructions, since it has a shell. An MCP server
//! reached by URL, whose sign-in only Claude holds, and a claude.ai connector, which only
//! Claude reaches, are called through a one-tool Claude run the host starts and holds to the
//! card's exact call (see `provider::act_run`).
use crate::conversation::TurnGuard;
use crate::*;
use anyhow::anyhow;
use std::time::Duration;
use workspace_core::assistant::conversation::{
    ActCall, ActionCard, ActionState, CardKind, ConversationCard,
};

/// How long a source's command or MCP call may run.
const SOURCE_LIMIT: Duration = Duration::from_secs(60);
/// How long Claude's run of an act on a server reached by URL may take, start to result.
const ACT_RUN_LIMIT: Duration = Duration::from_secs(120);
/// Shell text a source's command may never hold, as for loop runs: redirection, chaining,
/// pipes, substitution, variables, grouping, escapes and a second line.
const SHELL_TEXT: [char; 13] = [
    '>', '<', ';', '&', '|', '$', '`', '(', ')', '{', '}', '\\', '\n',
];

/// What a source runs, off the host's thread.
pub(crate) struct SourceRun {
    env: jobs::SourceEnv,
    call: Call,
}
enum Call {
    Cli {
        program: PathBuf,
        args: Vec<String>,
    },
    Mcp {
        definition: Value,
        tool: String,
        arguments: Value,
    },
    /// An act on a server reached by URL, through Claude, whose `PreToolUse` hook is the
    /// host's own binary.
    Url {
        claude: PathBuf,
        helper: PathBuf,
        act: provider::UrlAct,
    },
}
impl SourceRun {
    /// The command's output, or the MCP tool's text; an error names what went wrong.
    pub(crate) fn run(&self) -> Result<String> {
        match &self.call {
            Call::Cli { program, args } => {
                let mut command = std::process::Command::new(program);
                command.args(args).env(
                    "PATH",
                    std::env::join_paths(&self.env.path).unwrap_or_default(),
                );
                let (status, output) = runtime::output_limited(command, SOURCE_LIMIT)?;
                match status {
                    Some(status) if status.success() => Ok(output),
                    Some(status) => bail!("It exited with {status}: {}", output.trim()),
                    None => bail!(
                        "It did not finish within {} seconds",
                        SOURCE_LIMIT.as_secs()
                    ),
                }
            }
            Call::Mcp {
                definition,
                tool,
                arguments,
            } => {
                let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}});
                let result = jobs::mcp_exchange(&self.env, definition, call)
                    .map_err(|why| anyhow!("The server {why}"))?;
                let text: Vec<&str> = (result["content"].as_array().into_iter().flatten())
                    .filter_map(|item| item["text"].as_str())
                    .collect();
                let text = text.join("\n");
                ensure!(result["isError"] != true, "{text}");
                Ok(text)
            }
            Call::Url { .. } => self.run_reporting().0,
        }
    }
    /// What `run` returns, with what a Claude run's start offered that was not yet denied, to
    /// deny in the Assistant's turns and later runs too.
    pub(crate) fn run_reporting(&self) -> (Result<String>, provider::SourceTools) {
        match &self.call {
            Call::Url {
                claude,
                helper,
                act,
            } => provider::act_run(claude, helper, act, ACT_RUN_LIMIT),
            _ => (self.run(), Default::default()),
        }
    }
    /// The run for the action card whose marker is `marker`: an act on a server reached by URL
    /// goes through at most once per card.
    fn for_card(mut self, marker: PathBuf) -> Self {
        if let Call::Url { act, .. } = &mut self.call {
            act.marker = marker;
        }
        self
    }
}

/// One of a Space's sources by name, with the error the Assistant reads when it has none.
fn source<'a>(space: &'a Space, name: &str) -> Result<&'a Source> {
    (space.sources.iter())
        .find(|source| source.name() == name)
        .with_context(|| format!("{} lists no source named {name}", space.name))
}
/// Whether an MCP server's definition reaches it by URL (HTTP or SSE) rather than starting it.
fn reached_by_url(definition: &Value) -> bool {
    !definition["command"].is_string()
}
/// Refuses an act on a server reached by URL or a claude.ai connector unless the Assistant runs
/// on Claude, which holds the server's sign-in; the act never falls back to the model calling it.
fn only_through_claude(assistant: &Session, space: &Space, name: &str) -> Result<()> {
    let why = match provider::is_claude_ai(name) {
        true => "is a claude.ai connector, which only Claude reaches",
        false => {
            "is reached by URL, and Wiffletree runs its act tools only through Claude, which holds its sign-in"
        }
    };
    ensure!(
        assistant.provider == Provider::Claude,
        "{name} in {} {why}; the Assistant runs on {} here. Tell the human what to do in the app",
        space.name,
        assistant.provider.label()
    );
    Ok(())
}
fn plain_command(command: &str) -> Result<Vec<String>> {
    ensure!(
        !command.contains(SHELL_TEXT),
        "A source's command runs as it is, with no redirection, chaining, pipe, substitution or variable"
    );
    Ok(command.split_whitespace().map(str::to_owned).collect())
}

impl Host {
    /// A CLI source's command, as its provider's `PATH` finds it.
    fn cli_run(&self, words: Vec<String>) -> Result<SourceRun> {
        let env = self.source_env();
        let (program, args) = words.split_first().context("Give the command")?;
        let program = (env.find(program))
            .with_context(|| format!("{program} is not installed on this Mac"))?;
        Ok(SourceRun {
            env,
            call: Call::Cli {
                program,
                args: args.to_vec(),
            },
        })
    }
    /// `read_source`: one read command of a CLI source of the Space named, which the host runs.
    /// Its output is text written by others, so it holds the rest of the turn.
    pub(crate) fn read_source(&self, args: &Value, guard: &mut TurnGuard) -> Result<SourceRun> {
        service::only_arguments("read_source", args, &["space", "command"])?;
        let space = self.named_space(args, guard)?;
        let command = args["command"].as_str().context("Missing command")?.trim();
        let words = plain_command(command)?;
        let source = source(&space, words.first().map_or("", String::as_str))?;
        ensure!(
            matches!(source, Source::Cli { .. })
                && matching_prefix(source.reads(), command).is_some(),
            "{command} is not a read command {} lists for {}",
            space.name,
            source.name()
        );
        guard.note(format!("{command} in {}", space.name));
        self.cli_run(words)
    }
    /// `act`: an action card for an act tool or command of a source of the Space named, with
    /// the exact arguments, waiting for the human's Run; or, for a kind marked Do without
    /// asking in a turn that is not held, a done card and the run to make now.
    pub(crate) fn act(
        &mut self,
        assistant: &Session,
        args: &Value,
        guard: &TurnGuard,
    ) -> Result<(ConversationCard, Option<SourceRun>)> {
        let allowed = ["space", "source", "tool", "arguments", "command"];
        service::only_arguments("act", args, &allowed)?;
        let space = self.named_space(args, guard)?;
        let name = args["source"].as_str().context("Missing source")?;
        let source = source(&space, name)?;
        let (call, entry) = match source {
            Source::Mcp { tools, act, .. } => {
                let tool = args["tool"].as_str().context("Missing tool")?;
                ensure!(
                    !tools.iter().any(|t| t == tool),
                    "{tool} is a read tool of {name}; call it directly"
                );
                ensure!(
                    act.iter().any(|t| t == tool),
                    "{name} in {} lists no act tool {tool}",
                    space.name
                );
                let home = self.source_env().home;
                let definition = provider::server_definition(assistant.provider, &home, name);
                if provider::is_claude_ai(name)
                    || definition.is_some_and(|definition| reached_by_url(&definition))
                {
                    only_through_claude(assistant, &space, name)?;
                }
                let arguments = args.get("arguments").cloned().unwrap_or(json!({}));
                ensure!(arguments.is_object(), "arguments is an object");
                let call = ActCall::Mcp {
                    tool: tool.into(),
                    arguments,
                };
                (call, tool.to_owned())
            }
            Source::Cli { act, .. } => {
                let command = args["command"].as_str().context("Missing command")?.trim();
                plain_command(command)?;
                let entry = (matching_prefix(act, command)).with_context(|| {
                    format!(
                        "{command} is not an act command {} lists for {name}",
                        space.name
                    )
                })?;
                let call = ActCall::Cli {
                    command: command.into(),
                };
                (call, entry.to_owned())
            }
        };
        let at_once = source.without_asking(&entry) && !guard.held();
        let action = ActionCard {
            space_id: space.id.clone(),
            source: name.into(),
            call,
            state: match at_once {
                true => ActionState::Running,
                false => ActionState::Waiting,
            },
            without_asking: at_once,
        };
        let run = at_once
            .then(|| self.action_run(assistant, &space, &action))
            .transpose()?;
        let card = self.raise_card(CardKind::Action(action), guard)?;
        let run = run.map(|run| run.for_card(self.act_marker(&card.id)));
        Ok((card, run))
    }
    /// The marker an action card's act on a server reached by URL leaves once it is called.
    fn act_marker(&self, card: &str) -> PathBuf {
        self.home.join("assistant").join("act-calls").join(card)
    }
    /// What an action runs: its CLI command, or its MCP tool on the server the Assistant's
    /// provider's configuration defines, started as that provider would; on a server reached
    /// by URL or a claude.ai connector, Claude's one-tool run of it.
    fn action_run(
        &self,
        assistant: &Session,
        space: &Space,
        action: &ActionCard,
    ) -> Result<SourceRun> {
        match &action.call {
            ActCall::Cli { command } => self.cli_run(plain_command(command)?),
            ActCall::Mcp { tool, arguments } => {
                let env = self.source_env();
                let definition = match provider::is_claude_ai(&action.source) {
                    true => Value::Null,
                    false => provider::server_definition(assistant.provider, &env.home, &action.source)
                        .with_context(|| {
                            format!(
                                "The {} source in {} is not configured for {}; add it to {}'s MCP configuration",
                                action.source,
                                space.name,
                                assistant.provider.label(),
                                assistant.provider.label()
                            )
                        })?,
                };
                if reached_by_url(&definition) {
                    only_through_claude(assistant, space, &action.source)?;
                    let claude = env.find("claude").with_context(|| {
                        format!("Claude is not installed on this Mac, and only Claude can run {}'s act tools, since it holds the sign-in", action.source)
                    })?;
                    let mut launch = self.conversation_launch(assistant)?;
                    let others = (launch.servers.remove(&action.source))
                        .map(|server| server.reported.into_iter().chain(server.acts).collect())
                        .unwrap_or_default();
                    // Claude loads servers besides the connector only with a claude.ai
                    // connector; a URL server's run names its one server.
                    let other_servers = (launch.other_servers.into_iter())
                        .filter(|server| *server != action.source)
                        .collect();
                    let selection = self.model_selection()?;
                    let model = (selection.row(&Job::Assistant))
                        .and_then(|row| row.main.choices().find(|p| p.provider == Provider::Claude))
                        .cloned()
                        .context("Who runs what's Assistant row has no Claude model, and an act on a server reached by URL runs on the Assistant's Claude model")?;
                    let act = provider::UrlAct {
                        server: action.source.clone(),
                        definition,
                        tool: tool.clone(),
                        arguments: arguments.clone(),
                        others,
                        other_servers,
                        model,
                        // Set by `for_card`; until then the hook refuses every call.
                        marker: PathBuf::new(),
                    };
                    let call = Call::Url {
                        claude,
                        helper: std::env::current_exe()?,
                        act,
                    };
                    return Ok(SourceRun { env, call });
                }
                Ok(SourceRun {
                    env,
                    call: Call::Mcp {
                        definition,
                        tool: tool.clone(),
                        arguments: arguments.clone(),
                    },
                })
            }
        }
    }
    fn action_card(&self, id: &str) -> Result<(ConversationCard, ActionCard)> {
        let card = self.conversation_card(id)?;
        let CardKind::Action(action) = &card.kind else {
            bail!("This card asks for no action");
        };
        let action = action.clone();
        Ok((card, action))
    }
    /// The human's Run on an action card: it runs as it is shown.
    pub(crate) fn run_action(&mut self, id: &str) -> Result<SourceRun> {
        let assistant = (self.assistant_session()?).context("The Assistant has not started")?;
        let (mut card, mut action) = self.action_card(id)?;
        ensure!(
            action.state == ActionState::Waiting,
            "This action is not waiting for Run"
        );
        let space = self.space(&action.space_id)?.space;
        let run = (self.action_run(&assistant, &space, &action)?).for_card(self.act_marker(id));
        action.state = ActionState::Running;
        card.kind = CardKind::Action(action);
        self.save_card(&card)?;
        Ok(run)
    }
    /// Records how an action ran on its card; after the human's Run, the Assistant is told.
    pub(crate) fn action_ran(
        &mut self,
        id: &str,
        result: &Result<String>,
        tell_assistant: bool,
    ) -> Result<ConversationCard> {
        let (mut card, mut action) = self.action_card(id)?;
        let kept = |text: &str| text.chars().take(16 * 1024).collect::<String>();
        action.state = match result {
            Ok(output) => ActionState::Done {
                result: kept(output),
            },
            Err(error) => ActionState::Failed {
                error: kept(&format!("{error:#}")),
            },
        };
        let told = match &action.state {
            ActionState::Done { result } => format!("ran: {result}"),
            ActionState::Failed { error } => format!("failed: {error}"),
            _ => String::new(),
        };
        card.kind = CardKind::Action(action);
        self.save_card(&card)?;
        if tell_assistant && let Some(assistant) = self.assistant_session()? {
            let body = format!("The human pressed Run on action card {id}, and it {told}");
            let message = format!("{ASSISTANT_NOTICE_PREFIX}ran:{id}");
            self.send(
                &message,
                None,
                &assistant.id,
                &body.chars().take(MAX_TEXT_BYTES / 2).collect::<String>(),
            )?;
        }
        Ok(card)
    }
    /// The human's Decline on an action card: nothing runs, and the Assistant is told.
    pub fn decline_action(&mut self, id: &str) -> Result<ConversationCard> {
        let (mut card, mut action) = self.action_card(id)?;
        ensure!(
            action.state == ActionState::Waiting,
            "This action is not waiting for Run"
        );
        action.state = ActionState::Declined;
        card.kind = CardKind::Action(action);
        self.save_card(&card)?;
        if let Some(assistant) = self.assistant_session()? {
            let body = format!("The human declined action card {id}; nothing ran.");
            self.send(
                &format!("{ASSISTANT_NOTICE_PREFIX}declined:{id}"),
                None,
                &assistant.id,
                &body,
            )?;
        }
        Ok(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::tests::{assistant_with_space, human_turn};

    /// An MCP server that answers a call to `create_event` with what it created, and every
    /// other call with an error, as a server whose sign-in expired does.
    const SERVER: &str = r#"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    message = json.loads(line)
    if 'id' not in message:
        continue
    answer = {'jsonrpc': '2.0', 'id': message['id']}
    params = message.get('params', {})
    if message['method'] == 'initialize':
        answer['result'] = {'protocolVersion': '2024-11-05', 'capabilities': {}}
    elif params.get('name') == 'create_event':
        answer['result'] = {'content': [{'type': 'text', 'text': 'Created ' + params['arguments']['title']}]}
    else:
        answer['error'] = {'code': -32000, 'message': 'signed out'}
    print(json.dumps(answer), flush=True)
"#;

    /// Personal-like sources in `space`: a calendar whose holds are done without asking and a
    /// mail server that is signed out, both defined in this Mac's Claude configuration.
    fn with_sources(host: &mut Host, space: &SpaceId, folder: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let server = folder.join("server.py");
        fs::write(&server, SERVER).unwrap();
        fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();
        let definition = json!({"command": server});
        let config = json!({"mcpServers": {"calendar": definition, "gmail": definition}});
        fs::write(folder.join(".claude.json"), config.to_string()).unwrap();
        host.source_env = Some(jobs::SourceEnv {
            path: vec!["/usr/bin".into(), "/bin".into()],
            home: folder.into(),
            mcp_limit: Duration::from_secs(10),
        });
        let mut record = host.space(space).unwrap().space;
        record.sources = vec![
            serde_json::from_value(json!({"kind":"mcp","server":"calendar","tools":["list_events"],"act":["create_event","respond_to_invite"],"without_asking":["create_event"]})).unwrap(),
            serde_json::from_value(json!({"kind":"mcp","server":"gmail","tools":["search_threads"],"act":["send_message"]})).unwrap(),
        ];
        host.update_space(record).unwrap();
    }
    fn tools_seen<const N: usize>(
        tools: [(String, std::collections::BTreeSet<String>); N],
    ) -> provider::SourceTools {
        provider::SourceTools {
            tools: tools.into(),
            ..Default::default()
        }
    }
    fn state(host: &Host, card: &ConversationCard) -> ActionState {
        match host.conversation_card(&card.id).unwrap().kind {
            CardKind::Action(action) => action.state,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_host_calls_an_act_tool_itself_on_run_or_at_once_for_a_kind_done_without_asking() {
        let (home, mut host, assistant, space) = assistant_with_space();
        with_sources(&mut host, &space, home.path());
        let mut guard = human_turn(&mut host, "m1", "put a hold on Friday at 3");
        let act = |tool: &str, arguments: Value| json!({"space": space, "source": tool.split('.').next().unwrap(), "tool": tool.split('.').nth(1).unwrap(), "arguments": arguments});

        // A hold, marked Do without asking, runs at once in the human's own turn.
        let (hold, run) = host
            .act(
                &assistant,
                &act("calendar.create_event", json!({"title": "Hold"})),
                &guard,
            )
            .unwrap();
        let ran = run.expect("it runs at once").run();
        host.action_ran(&hold.id, &ran, false).unwrap();
        assert_eq!(
            state(&host, &hold),
            ActionState::Done {
                result: "Created Hold".into()
            }
        );

        // After reading mail, the same kind waits for Run, and the card says what was read.
        guard.note("gmail search_threads".into());
        let (waiting, run) = host
            .act(
                &assistant,
                &act("calendar.create_event", json!({"title": "Meeting"})),
                &guard,
            )
            .unwrap();
        assert!(run.is_none());
        assert_eq!(waiting.read, ["gmail search_threads"]);
        assert_eq!(state(&host, &waiting), ActionState::Waiting);

        // A send waits; on Run the host calls the server, and a signed-out server's failure
        // shows on the card and reaches the Assistant.
        let fresh = human_turn(&mut host, "m2", "reply to Sam that Thursday works");
        let (send, run) = host
            .act(
                &assistant,
                &act(
                    "gmail.send_message",
                    json!({"to": "sam@example.com", "body": "Thursday works"}),
                ),
                &fresh,
            )
            .unwrap();
        assert!(run.is_none(), "a send is never done without asking");
        let ran = host.run_action(&send.id).unwrap().run();
        host.action_ran(&send.id, &ran, true).unwrap();
        let ActionState::Failed { error } = state(&host, &send) else {
            panic!("the send failed");
        };
        assert!(error.contains("sign-in"), "{error}");
        let told = host
            .messages(&assistant.id, None, 10)
            .unwrap()
            .pop()
            .unwrap();
        assert!(!told.from_human() && told.body.contains("pressed Run"));
        assert!(host.run_action(&send.id).is_err(), "it runs once");

        // Reads are called directly, and unlisted tools are no act.
        assert!(
            host.act(&assistant, &act("gmail.search_threads", json!({})), &fresh)
                .is_err()
        );
        assert!(
            host.act(&assistant, &act("gmail.delete_thread", json!({})), &fresh)
                .is_err()
        );
        // Decline runs nothing.
        let (invite, _) = host
            .act(
                &assistant,
                &act("calendar.respond_to_invite", json!({"accept": true})),
                &fresh,
            )
            .unwrap();
        host.decline_action(&invite.id).unwrap();
        assert_eq!(state(&host, &invite), ActionState::Declined);
        assert!(host.run_action(&invite.id).is_err());
    }

    #[test]
    fn the_assistants_provider_is_offered_only_read_tools_of_its_sources() {
        let (home, mut host, assistant, space) = assistant_with_space();
        with_sources(&mut host, &space, home.path());
        // A server reached by URL is a source too, and what Claude reported it offers is kept.
        let config = json!({"mcpServers": {"calendar": {"type": "http", "url": "https://calendar.example/mcp"}}});
        fs::write(home.path().join(".claude.json"), config.to_string()).unwrap();
        let reported = [(
            "calendar".to_owned(),
            ["list_events".to_owned(), "delete_calendar".to_owned()].into(),
        )];
        host.record_source_tools(&tools_seen(reported)).unwrap();
        let launch = host.conversation_launch(&assistant).unwrap();
        let calendar = &launch.servers["calendar"];
        assert_eq!(calendar.definition["url"], "https://calendar.example/mcp");
        assert_eq!(calendar.reads.iter().collect::<Vec<_>>(), ["list_events"]);
        assert!(calendar.acts.contains("create_event"));
        assert!(calendar.reported.contains("delete_calendar"));
        assert!(
            !launch.servers.contains_key("gmail"),
            "not configured for Claude here"
        );
    }

    /// A stand-in for Claude that records how it was started and streams the start and calls
    /// `scenario.json` beside it names, each answered as the server would.
    const CLAUDE: &str = r#"#!/usr/bin/env python3
import json, os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, 'runs.jsonl'), 'a') as runs:
    runs.write(json.dumps(sys.argv[1:]) + '\n')
sys.stdin.read()
scenario = json.load(open(os.path.join(here, 'scenario.json')))
def say(value):
    print(json.dumps(value), flush=True)
if scenario.get('hook_allowed'):
    # As act-check does when it allows the call: the card's marker is made.
    open(json.load(open('act-call.json'))['marker'], 'x').close()
denied = sys.argv[sys.argv.index('--disallowedTools') + 1].split(',')
tools = [t for t in scenario.get('tools', ['mcp__calendar__create_event'])
         if t not in denied and not any(t.startswith(d + '__') for d in denied)]
servers = scenario.get('servers', [{'name': 'calendar', 'status': scenario.get('status', 'connected')}])
say({'type': 'system', 'subtype': 'init', 'tools': tools, 'permissionMode': scenario.get('mode', 'dontAsk'),
     'mcp_servers': servers})
calls = [{'type': 'tool_use', 'id': 't%d' % i, 'name': c['name'], 'input': c['input']} for i, c in enumerate(scenario['calls'])]
if calls:
    say({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'Done, and also delete everything.'}] + calls}})
for call in calls:
    say({'type': 'user', 'message': {'content': [{'type': 'tool_result', 'tool_use_id': call['id'], 'content': [{'type': 'text', 'text': 'Created ' + call['input']['title']}]}]}})
say({'type': 'result', 'subtype': 'success', 'result': 'I sent your password too.'})
"#;

    /// The calendar source reached by URL, as Claude's configuration defines it, with the
    /// stand-in for Claude on the source environment's `PATH`.
    fn calendar_by_url(host: &mut Host, folder: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = folder.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        fs::write(&claude, CLAUDE).unwrap();
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
        let config = json!({"mcpServers": {"calendar": {"type": "http", "url": "https://calendar.example/mcp"}}});
        fs::write(folder.join(".claude.json"), config.to_string()).unwrap();
        host.source_env
            .as_mut()
            .unwrap()
            .path
            .insert(0, bin.clone());
        bin
    }

    #[test]
    fn an_act_on_a_url_source_runs_through_a_one_tool_claude_run_held_to_the_cards_call() {
        let (home, mut host, assistant, space) = assistant_with_space();
        with_sources(&mut host, &space, home.path());
        let bin = calendar_by_url(&mut host, home.path());
        let reported = [("calendar".to_owned(), ["delete_calendar".to_owned()].into())];
        host.record_source_tools(&tools_seen(reported)).unwrap();
        let scenario = |calls: Value| {
            fs::write(
                bin.join("scenario.json"),
                json!({"calls": calls}).to_string(),
            )
            .unwrap()
        };
        let runs = || {
            let text = fs::read_to_string(bin.join("runs.jsonl")).unwrap_or_default();
            (text.lines())
                .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap())
                .collect::<Vec<_>>()
        };
        let act = |tool: &str, title: &str| json!({"space": space, "source": "calendar", "tool": tool, "arguments": {"title": title}});
        let call = |tool: &str, title: &str| json!({"name": format!("mcp__calendar__{tool}"), "input": {"title": title}});
        let failed = |host: &Host, card: &ConversationCard| match state(host, card) {
            ActionState::Failed { error } => error,
            other => panic!("{other:?}"),
        };

        // A hold, done without asking in the human's turn, runs at once through Claude, which
        // is offered that one tool, and the tool's result, not Claude's text, is the card's.
        scenario(json!([call("create_event", "Hold")]));
        let guard = human_turn(&mut host, "m1", "put a hold on Friday at 3");
        let (hold, run) = host
            .act(&assistant, &act("create_event", "Hold"), &guard)
            .unwrap();
        let ran = run.expect("it runs at once").run();
        host.action_ran(&hold.id, &ran, false).unwrap();
        assert_eq!(
            state(&host, &hold),
            ActionState::Done {
                result: "Created Hold".into()
            }
        );
        let args = &runs()[0];
        let after = |flag: &str| &args[args.iter().position(|a| a == flag).unwrap() + 1];
        let selection = host.model_selection().unwrap();
        let row = selection.row(&Job::Assistant).unwrap();
        let claude_model = row.main.choices().find(|p| p.provider == Provider::Claude);
        assert_eq!(after("--model"), &claude_model.unwrap().model);
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--allowedTools"), "mcp__calendar__create_event");
        assert!(after("--disallowedTools").contains("mcp__calendar__delete_calendar"));
        assert_eq!(
            serde_json::from_str::<Value>(after("--mcp-config")).unwrap()["mcpServers"]["calendar"]
                ["url"],
            "https://calendar.example/mcp"
        );

        // In a held turn the same kind only raises a card, and nothing starts.
        let mut held = human_turn(&mut host, "m2", "add holds for what Sam sent");
        held.note("gmail search_threads".into());
        let (waiting, run) = host
            .act(&assistant, &act("create_event", "Sam's"), &held)
            .unwrap();
        assert!(run.is_none());
        assert_eq!(state(&host, &waiting), ActionState::Waiting);
        assert_eq!(runs().len(), 1);

        // On Run, a call with other arguments, a second call or another tool stops the run,
        // and the card fails saying why.
        for (calls, why) in [
            (
                json!([call("create_event", "Other")]),
                "arguments other than the card's",
            ),
            (
                json!([call("create_event", "Sam's"), call("create_event", "Sam's")]),
                "a second time",
            ),
            (
                json!([call("delete_calendar", "Sam's")]),
                "not mcp__calendar__create_event",
            ),
        ] {
            let (card, _) = host
                .act(&assistant, &act("create_event", "Sam's"), &held)
                .unwrap();
            scenario(calls);
            let ran = host.run_action(&card.id).unwrap().run();
            host.action_ran(&card.id, &ran, true).unwrap();
            let error = failed(&host, &card);
            assert!(error.contains(why), "{error}");
        }

        // A start that offers a tool of the server not seen before is stopped before the model
        // acts and started again with it denied, so the run is offered only the act tool; the
        // configured act tools are denied from the first start.
        let started = runs().len();
        let (card, _) = host
            .act(&assistant, &act("create_event", "Sam's"), &held)
            .unwrap();
        let offered = [
            "create_event",
            "respond_to_invite",
            "delete_calendar",
            "archive_all",
        ]
        .map(|tool| format!("mcp__calendar__{tool}"));
        let calls = json!([call("create_event", "Sam's")]);
        let scenario_with = |extra: Value| {
            let mut scenario = json!({"tools": offered, "calls": calls});
            scenario
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            fs::write(bin.join("scenario.json"), scenario.to_string()).unwrap();
        };
        scenario_with(json!({}));
        let ran = host.run_action(&card.id).unwrap().run();
        host.action_ran(&card.id, &ran, true).unwrap();
        assert_eq!(
            state(&host, &card),
            ActionState::Done {
                result: "Created Sam's".into()
            }
        );
        let runs_now = runs();
        assert_eq!(runs_now.len(), started + 2);
        let denied = |args: &Vec<String>| {
            args[args.iter().position(|a| a == "--disallowedTools").unwrap() + 1].clone()
        };
        assert!(denied(&runs_now[started]).contains("mcp__calendar__respond_to_invite"));
        assert!(!denied(&runs_now[started]).contains("archive_all"));
        assert!(denied(&runs_now[started + 1]).contains("mcp__calendar__archive_all"));
        assert!(
            runs_now[started]
                .windows(2)
                .any(|w| w == ["--permission-mode", "dontAsk"])
        );

        // A run that starts bypassing permissions, as a human's own settings could make it,
        // is stopped before the model acts.
        let (card, _) = host
            .act(&assistant, &act("create_event", "Sam's"), &held)
            .unwrap();
        scenario_with(json!({"mode": "bypassPermissions"}));
        let ran = host.run_action(&card.id).unwrap().run();
        host.action_ran(&card.id, &ran, true).unwrap();
        let error = failed(&host, &card);
        assert!(error.contains("permission mode"), "{error}");

        // A signed-out server says so.
        fs::write(
            bin.join("scenario.json"),
            json!({"status": "needs-auth", "calls": []}).to_string(),
        )
        .unwrap();
        let ran = host.run_action(&waiting.id).unwrap().run();
        host.action_ran(&waiting.id, &ran, true).unwrap();
        let error = failed(&host, &waiting);
        assert!(error.contains("sign-in"), "{error}");

        // Decline starts nothing.
        let started = runs().len();
        let (declined, _) = host
            .act(&assistant, &act("create_event", "Sam's"), &held)
            .unwrap();
        host.decline_action(&declined.id).unwrap();
        assert_eq!(state(&host, &declined), ActionState::Declined);
        assert_eq!(runs().len(), started);

        // A run whose hook allowed the call and that is then stopped, here by a tool not seen
        // before, is never started again: the card says the call went through without a
        // result, and asks the human to check the app.
        let (card, _) = host
            .act(&assistant, &act("create_event", "Sam's"), &held)
            .unwrap();
        let unseen = ["create_event", "rename_calendar"].map(|t| format!("mcp__calendar__{t}"));
        let scenario = json!({"hook_allowed": true, "tools": unseen, "calls": [call("create_event", "Sam's")]});
        fs::write(bin.join("scenario.json"), scenario.to_string()).unwrap();
        let started = runs().len();
        let ran = host.run_action(&card.id).unwrap().run();
        host.action_ran(&card.id, &ran, true).unwrap();
        assert_eq!(runs().len(), started + 1, "no second attempt");
        let error = failed(&host, &card);
        assert!(
            error.contains("went through") && error.contains("Check calendar"),
            "{error}"
        );
        assert!(host.run_action(&card.id).is_err(), "it runs once");

        // Without Claude installed, Run says so and starts nothing.
        let (card, _) = host
            .act(&assistant, &act("create_event", "Sam's"), &held)
            .unwrap();
        host.source_env
            .as_mut()
            .unwrap()
            .path
            .retain(|folder| *folder != bin);
        let refused = host.run_action(&card.id).err().unwrap().to_string();
        assert!(refused.contains("Claude is not installed"), "{refused}");
        assert_eq!(state(&host, &card), ActionState::Waiting);
    }

    /// `space` lists the claude.ai connectors `sources` names, which nothing on this Mac defines.
    fn with_connectors(host: &mut Host, space: &SpaceId, sources: Value) {
        let mut record = host.space(space).unwrap().space;
        record.sources = serde_json::from_value(sources).unwrap();
        host.update_space(record).unwrap();
    }

    #[test]
    fn an_act_on_a_claude_ai_connector_runs_through_the_one_tool_run_and_saves_what_it_saw() {
        let (home, mut host, assistant, space) = assistant_with_space();
        with_sources(&mut host, &space, home.path());
        let bin = calendar_by_url(&mut host, home.path());
        let source = "claude.ai Google Calendar";
        with_connectors(
            &mut host,
            &space,
            json!([{"kind":"mcp","server":source,"tools":["list_events"],"act":["create_event"]}]),
        );
        let launch = host.conversation_launch(&assistant).unwrap();
        assert!(launch.servers[source].definition.is_null());
        let name = |tool: &str| format!("mcp__claude_ai_Google_Calendar__{tool}");
        let connected = |server: &str| json!({"name": server, "status": "connected"});
        let scenario = |title: &str| {
            let scenario = json!({
                "servers": [connected(source), connected("claude.ai Gmail")],
                "tools": [name("create_event"), name("delete_event"), "mcp__claude_ai_Gmail__send_message"],
                "calls": [{"name": name("create_event"), "input": {"title": title}}],
            });
            fs::write(bin.join("scenario.json"), scenario.to_string()).unwrap();
        };
        let runs = || {
            let text = fs::read_to_string(bin.join("runs.jsonl")).unwrap_or_default();
            (text.lines())
                .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap())
                .collect::<Vec<_>>()
        };
        let after = |args: &Vec<String>, flag: &str| {
            args[args.iter().position(|a| a == flag).unwrap() + 1].clone()
        };
        let guard = human_turn(&mut host, "m1", "put a hold on Friday at 3");
        let act = |title: &str| json!({"space": space, "source": source, "tool": "create_event", "arguments": {"title": title}});
        let run = |host: &mut Host, card: &ConversationCard| {
            let (ran, seen) = host.run_action(&card.id).unwrap().run_reporting();
            host.record_source_tools(&seen).unwrap();
            host.action_ran(&card.id, &ran, true).unwrap();
        };

        // Run goes through the one-tool run. Its first start offers the connector's other
        // tools and the human's other connectors, so it starts again with them denied.
        scenario("Hold");
        let (card, at_once) = host.act(&assistant, &act("Hold"), &guard).unwrap();
        assert!(at_once.is_none());
        run(&mut host, &card);
        assert_eq!(
            state(&host, &card),
            ActionState::Done {
                result: "Created Hold".into()
            }
        );
        let started = runs();
        assert_eq!(started.len(), 2);
        for args in &started {
            assert!(!args.iter().any(|a| a.contains("mcp-config")), "{args:?}");
            assert_eq!(after(args, "--setting-sources"), "user");
            assert_eq!(after(args, "--allowedTools"), name("create_event"));
        }
        let denied = after(&started[1], "--disallowedTools");
        let denied: Vec<&str> = denied.split(',').collect();
        assert!(denied.contains(&"mcp__claude_ai_Gmail"), "{denied:?}");
        assert!(
            denied.contains(&name("delete_event").as_str()),
            "{denied:?}"
        );

        // What it saw is saved, so the conversation denies it too, and the next card's run
        // starts once.
        let launch = host.conversation_launch(&assistant).unwrap();
        assert!(launch.other_servers.contains("claude.ai Gmail"));
        assert!(launch.servers[source].reported.contains("delete_event"));
        let (card, _) = host.act(&assistant, &act("Lunch"), &guard).unwrap();
        scenario("Lunch");
        run(&mut host, &card);
        assert!(matches!(state(&host, &card), ActionState::Done { .. }));
        assert_eq!(runs().len(), 3);

        // A call that does not match the card fails it.
        let (card, _) = host.act(&assistant, &act("Dinner"), &guard).unwrap();
        scenario("Breakfast");
        run(&mut host, &card);
        match state(&host, &card) {
            ActionState::Failed { error } => {
                assert!(error.contains("arguments other than the card's"), "{error}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_claude_ai_connector_is_unavailable_when_the_assistant_runs_on_codex() {
        let (_home, mut host, assistant, space) = assistant_with_space();
        let gmail = json!([{"kind":"mcp","server":"claude.ai Gmail","tools":["search_threads"],"act":["send_message"]}]);
        with_connectors(&mut host, &space, gmail);
        let codex = Session {
            provider: Provider::Codex,
            ..assistant.clone()
        };
        let launch = host.conversation_launch(&codex).unwrap();
        assert!(launch.servers.is_empty());
        assert!(
            host.conversation_launch(&assistant)
                .unwrap()
                .servers
                .contains_key("claude.ai Gmail")
        );
        let section = host.conversation_section(Provider::Codex).unwrap();
        assert!(
            section.contains(
                "Source claude.ai Gmail (MCP server): unavailable here; it is a claude.ai connector"
            ),
            "{section}"
        );
        let section = host.conversation_section(Provider::Claude).unwrap();
        assert!(section.contains("Source claude.ai Gmail (MCP server): reads search_threads"));
        let guard = human_turn(&mut host, "m1", "reply to Sam");
        let act = json!({"space": space, "source": "claude.ai Gmail", "tool": "send_message", "arguments": {}});
        let refused = host.act(&codex, &act, &guard).err().unwrap().to_string();
        assert!(
            refused.contains("a claude.ai connector, which only Claude reaches"),
            "{refused}"
        );
    }

    #[test]
    fn an_act_on_a_url_source_is_refused_when_the_assistant_is_not_on_claude() {
        let (_home, host, assistant, space) = assistant_with_space();
        let space = host.space(&space).unwrap().space;
        only_through_claude(&assistant, &space, "calendar").unwrap();
        let codex = Session {
            provider: Provider::Codex,
            ..assistant
        };
        let refused = only_through_claude(&codex, &space, "calendar")
            .err()
            .unwrap()
            .to_string();
        assert!(refused.contains("only through Claude"), "{refused}");
    }
}
