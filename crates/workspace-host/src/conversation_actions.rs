//! The Assistant's acts in other apps, through the Spaces' sources: `read_source` runs a CLI
//! source's read command, and `act` asks for an act tool or command, which the host runs itself
//! on the human's Run, or at once for a kind marked Do without asking in a turn that is not
//! held. The provider is never offered an act tool, so an MCP source's acts are held by the
//! host; a CLI's only by the Assistant's instructions, since it has a shell.
use crate::conversation::TurnGuard;
use crate::*;
use anyhow::anyhow;
use std::time::Duration;
use workspace_core::assistant::conversation::{
    ActCall, ActionCard, ActionState, CardKind, ConversationCard,
};

/// How long a source's command or MCP call may run.
const SOURCE_LIMIT: Duration = Duration::from_secs(60);
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
        }
    }
}

/// One of a Space's sources by name, with the error the Assistant reads when it has none.
fn source<'a>(space: &'a Space, name: &str) -> Result<&'a Source> {
    (space.sources.iter())
        .find(|source| source.name() == name)
        .with_context(|| format!("{} lists no source named {name}", space.name))
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
        Ok((card, run))
    }
    /// What an action runs: its CLI command, or its MCP tool on the server the Assistant's
    /// provider's configuration defines, started as that provider would.
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
                let definition = provider::server_definition(assistant.provider, &env.home, &action.source)
                    .with_context(|| {
                        format!(
                            "The {} source in {} is not configured for {}; add it to {}'s MCP configuration",
                            action.source,
                            space.name,
                            assistant.provider.label(),
                            assistant.provider.label()
                        )
                    })?;
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
        let run = self.action_run(&assistant, &space, &action)?;
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
