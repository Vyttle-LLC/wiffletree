//! The Assistant's one conversation (OpenSpec change `assistant-does-the-work`): the session
//! this install owns, working in `<data folder>/assistant/workspace/` in YOLO mode; what holds
//! its turns to cards; the cards its tools raise; and its tools that act in a named Space.
use crate::*;
use workspace_core::assistant::conversation::{
    CardKind, ConversationCard, ConversationState, Route,
};

/// The Assistant's working directory.
pub(crate) fn workspace_folder(home: &Path) -> PathBuf {
    home.join("assistant").join("workspace")
}

/// The conversation's cards, the tags on the human's messages it passed to work, the Space the
/// human picked in the composer for a message, and the human's unsent draft.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversation_cards (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS conversation_routes (message_id TEXT PRIMARY KEY, data TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS conversation_picks (message_id TEXT PRIMARY KEY, space_id TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS conversation_source_tools (server TEXT NOT NULL, tool TEXT NOT NULL, PRIMARY KEY(server,tool));
         CREATE TABLE IF NOT EXISTS conversation_other_servers (server TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS conversation_draft (id INTEGER PRIMARY KEY CHECK (id=1), body TEXT NOT NULL);",
    )?;
    draft_held_messages(db)
}

/// Migration (`assistant-does-the-work`): the messages an earlier release's router held unsent
/// become the conversation's draft, oldest first, and their table goes.
fn draft_held_messages(db: &Connection) -> Result<()> {
    let held: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='held_messages')",
        [],
        |r| r.get(0),
    )?;
    if !held {
        return Ok(());
    }
    let tx = db.unchecked_transaction()?;
    let bodies: Vec<Option<String>> = tx
        .prepare("SELECT json_extract(data,'$.body') FROM held_messages ORDER BY rowid")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let bodies: Vec<String> = (bodies.into_iter().flatten())
        .filter(|body| !body.trim().is_empty())
        .collect();
    if !bodies.is_empty() {
        add_to_draft(&tx, &bodies.join("\n\n"))?;
    }
    tx.execute("DROP TABLE held_messages", [])?;
    tx.commit()?;
    Ok(())
}
fn add_to_draft(db: &Connection, text: &str) -> Result<()> {
    db.execute(
        "INSERT INTO conversation_draft VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET body=body||char(10)||char(10)||excluded.body",
        [text],
    )?;
    Ok(())
}

/// The Assistant's workspace tools, as MCP lists them. It has none of the coordination tools.
pub(crate) const TOOLS: [&str; 26] = [
    "start_work",
    "message_work",
    "pause_work",
    "resume_work",
    "stop_work",
    "list_work",
    "work_status",
    "ask_human",
    "create_loop",
    "edit_loop",
    "pause_loop",
    "resume_loop",
    "delete_loop",
    "run_loop",
    "list_loops",
    "list_issues",
    "read_issue",
    "file_issue",
    "edit_issue",
    "comment_issue",
    "save_output",
    "query_brain",
    "log_work",
    "suggest_space",
    "read_source",
    "act",
];
/// The largest file `save_output` copies.
const MAX_OUTPUT_BYTES: u64 = 20 * 1024 * 1024;

/// What holds a turn of the Assistant's to cards: the text written by others it has read, as
/// the host sees it. A held turn's loop changes and app actions, even a kind marked Do without
/// asking, wait on a card for the human (assistant, "Text written by others never triggers an
/// action").
#[derive(Clone, Debug, Default)]
pub(crate) struct TurnGuard {
    /// What the turn has read that others wrote, in order; the turn is held when there is any.
    pub read: Vec<String>,
    /// The Space the human picked in the composer for the message that started the turn.
    pub picked: Option<SpaceId>,
    /// The human's message the turn started on, which `message_work` tags.
    pub human_message: Option<String>,
}
impl TurnGuard {
    pub(crate) fn held(&self) -> bool {
        !self.read.is_empty()
    }
    pub(crate) fn note(&mut self, what: String) {
        if !self.read.contains(&what) {
            self.read.push(what);
        }
    }
}

/// `args` without `space`, which the Assistant's tools take beside their own arguments.
pub(crate) fn without_space(args: &Value) -> Value {
    let mut rest = args.clone();
    if let Some(fields) = rest.as_object_mut() {
        fields.remove("space");
    }
    rest
}

impl Host {
    /// The Assistant's session, once its first message started it.
    pub fn assistant_session(&self) -> Result<Option<Session>> {
        Ok(self
            .db
            .query_row(
                "SELECT data FROM sessions WHERE role=?1",
                [tag(&Role::Conversation)?],
                |r| decode(r, 0),
            )
            .optional()?)
    }
    /// The conversation as the snapshot carries it, once its first message started it.
    pub(crate) fn conversation_state(&self) -> Result<Option<ConversationState>> {
        let Some(session) = self.assistant_session()? else {
            return Ok(None);
        };
        let session = self.shown(session);
        let runtime = self.session_runtime(&session.id)?;
        Ok(Some(ConversationState {
            status: session.status,
            turn_started_at: (session.status == Status::Working)
                .then_some(runtime.last_started_at)
                .flatten(),
            message_revision: self.message_revision(&session.id)?,
            profile: runtime.profile,
            cards: self.conversation_cards()?,
            routes: self.conversation_routes()?,
            session_id: session.id,
        }))
    }
    /// The Assistant's session, started now on this Mac's Assistant row when it has none.
    pub(crate) fn assistant(&mut self) -> Result<Session> {
        if let Some(session) = self.assistant_session()? {
            return Ok(session);
        }
        let pick = self.resolve_job(None, &Job::Assistant, false, false)?;
        let session = Session {
            id: new_id(),
            project_id: String::new(),
            parent_id: None,
            repository_id: None,
            name: self.assistant_name(),
            role: Role::Conversation,
            provider: pick.profile.provider,
            status: Status::Ready,
            archived: false,
            owner: Some(SessionOwner::Install),
        };
        let folder = workspace_folder(&self.home);
        fs::create_dir_all(&folder)?;
        self.atomically(|host| {
            host.db.execute(
                "INSERT INTO sessions VALUES (?1,NULL,NULL,?2,?3)",
                params![session.id, tag(&session.role)?, encode(&session)?],
            )?;
            Self::event(&host.db, "", Some(&session.id), "assistant_started", "")?;
            host.pin_pick(&session, &pick)?;
            let mut runtime = host.session_runtime(&session.id)?;
            runtime.workdir = Some(folder.to_string_lossy().into_owned());
            host.save_runtime(&runtime)
        })?;
        Ok(session)
    }
    /// The human's message to the Assistant, which starts its session the first time. With
    /// `space`, the Space they picked in the composer: in the turn it starts, a tool naming
    /// another Space is refused.
    pub fn send_to_assistant(
        &mut self,
        id: &str,
        body: &str,
        files: &[PathBuf],
        space: Option<&SpaceId>,
    ) -> Result<Message> {
        if let Some(space) = space {
            self.space(space)?;
        }
        let assistant = self.assistant()?;
        self.atomically(|host| {
            let message = host.send_with_files(id, None, &assistant.id, body, files)?;
            if let Some(space) = space {
                host.db.execute(
                    "INSERT OR IGNORE INTO conversation_picks VALUES (?1,?2)",
                    params![id, space.as_str()],
                )?;
            }
            host.record_human_message(&assistant.id)?;
            host.db.execute("DELETE FROM conversation_draft", [])?;
            Ok(message)
        })
    }
    /// The human's unsent draft for the conversation, which their next message replaces.
    pub fn conversation_draft(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT body FROM conversation_draft", [], |r| r.get(0))
            .optional()?)
    }
    /// The conversation's cards, oldest first.
    pub fn conversation_cards(&self) -> Result<Vec<ConversationCard>> {
        self.list_data(
            "SELECT data FROM conversation_cards ORDER BY created_at, rowid",
            [],
        )
    }
    pub fn conversation_card(&self, id: &str) -> Result<ConversationCard> {
        self.db
            .query_row(
                "SELECT data FROM conversation_cards WHERE id=?1",
                [id],
                |r| decode(r, 0),
            )
            .optional()?
            .context("Card not found")
    }
    /// Puts a card in the conversation, naming what text written by others its turn had read.
    pub(crate) fn raise_card(&self, kind: CardKind, guard: &TurnGuard) -> Result<ConversationCard> {
        let card = ConversationCard {
            id: new_id(),
            created_at: now(),
            read: guard.read.clone(),
            kind,
        };
        self.db.execute(
            "INSERT INTO conversation_cards VALUES (?1,?2,?3)",
            params![card.id, card.created_at, encode(&card)?],
        )?;
        Ok(card)
    }
    pub(crate) fn save_card(&self, card: &ConversationCard) -> Result<()> {
        self.db.execute(
            "UPDATE conversation_cards SET data=?2 WHERE id=?1",
            params![card.id, encode(card)?],
        )?;
        Ok(())
    }
    /// The tags on the human's messages the Assistant passed to its work.
    pub fn conversation_routes(&self) -> Result<Vec<Route>> {
        self.list_data("SELECT data FROM conversation_routes ORDER BY rowid", [])
    }
    pub(crate) fn save_route(&self, route: &Route) -> Result<()> {
        self.db.execute(
            "INSERT INTO conversation_routes VALUES (?1,?2) ON CONFLICT(message_id) DO UPDATE SET data=excluded.data",
            params![route.message_id, encode(route)?],
        )?;
        Ok(())
    }
    fn picked_space(&self, message: &str) -> Result<Option<SpaceId>> {
        Ok(self
            .db
            .query_row(
                "SELECT space_id FROM conversation_picks WHERE message_id=?1",
                [message],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(SpaceId))
    }
    /// What holds a turn of `input` from its start: a turn no message of the human's started,
    /// and every other message beside the human's, such as a report from work, a loop's result
    /// or Wiffletree's notice, which a card then names.
    pub(crate) fn turn_guard(&self, input: &[Message]) -> Result<TurnGuard> {
        let mut guard = TurnGuard::default();
        for message in input {
            if message.from_human() {
                guard.human_message = Some(message.id.clone());
                guard.picked = self.picked_space(&message.id)?;
            } else {
                guard.note(self.sent_by(message)?);
            }
        }
        if guard.human_message.is_none() {
            guard.note("no message from you started this turn".into());
        }
        Ok(guard)
    }
    /// Who sent a message to the Assistant that is not the human's, as a card names it.
    fn sent_by(&self, message: &Message) -> Result<String> {
        let Some(sender) = &message.sender else {
            return Ok("a message from Wiffletree".into());
        };
        let work = self.project(&self.session(sender)?.project_id)?.name;
        Ok(match message.id.starts_with("report:") {
            true => format!("a report from work “{work}”"),
            false => format!("a message from work “{work}”"),
        })
    }
    /// The Assistant's turn: each message under who sent it, the human's first.
    pub(crate) fn conversation_prompt(&self, input: &[Message]) -> Result<String> {
        let parts = (input.iter())
            .map(|message| {
                let from = match message.from_human() {
                    true => match self.picked_space(&message.id)? {
                        Some(space) => format!("the human, for {}", self.space(&space)?.space.name),
                        None => "the human".into(),
                    },
                    false => self.sent_by(message)?,
                };
                Ok(format!(
                    "From: {from}\nMessage ID: {}\n\n{}{}",
                    message.id,
                    message.body,
                    provider::attachment_list(&message.attachments)
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(parts.join("\n\n---\n\n"))
    }
    /// The Spaces the Assistant serves, after its instructions: each with its id, description,
    /// repositories and sources, saying which ones `provider` cannot reach.
    pub(crate) fn conversation_section(&self, provider: Provider) -> Result<String> {
        let mut text = String::from("\n## Spaces\n");
        let repositories = self.repositories()?;
        for record in self.spaces()?.into_iter().filter(|r| r.local.enabled) {
            let space = record.space;
            text.push_str(&format!(
                "\n### {} (space `{}`)\n{}\n",
                space.name,
                space.id.as_str(),
                space.description.trim()
            ));
            let ids = self.space_repositories(&space.id)?.repository_ids;
            let named: Vec<String> = (repositories.iter())
                .filter(|r| ids.contains(&r.id))
                .map(|r| format!("{} (`{}`)", r.name, r.id))
                .collect();
            if !named.is_empty() {
                text.push_str(&format!("Repositories: {}\n", named.join(", ")));
            }
            for source in &space.sources {
                let kind = match source {
                    Source::Mcp { .. } => "MCP server",
                    Source::Cli { .. } => "CLI",
                };
                if provider != Provider::Claude && provider::is_claude_ai(source.name()) {
                    text.push_str(&format!(
                        "Source {} ({kind}): unavailable here; it is a claude.ai connector, which only Claude reaches\n",
                        source.name()
                    ));
                    continue;
                }
                text.push_str(&format!(
                    "Source {} ({kind}): reads {}; acts {}\n",
                    source.name(),
                    source.reads().join(", "),
                    match source.acts() {
                        [] => "none".to_owned(),
                        acts => acts.join(", "),
                    }
                ));
            }
        }
        Ok(text)
    }
    /// The servers of every Space's MCP sources the Assistant's provider can start, as its own
    /// configuration defines them, and on Claude the claude.ai connectors they list, each
    /// offered for its read tools.
    pub(crate) fn conversation_launch(
        &self,
        session: &Session,
    ) -> Result<provider::ConversationLaunch> {
        let home = self.source_env().home;
        let mut launch = provider::ConversationLaunch::default();
        for record in self.spaces()?.into_iter().filter(|r| r.local.enabled) {
            for source in &record.space.sources {
                let Source::Mcp {
                    server, tools, act, ..
                } = source
                else {
                    continue;
                };
                let definition = match provider::is_claude_ai(server) {
                    true => (session.provider == Provider::Claude).then_some(Value::Null),
                    false => provider::server_definition(session.provider, &home, server),
                };
                if let Some(definition) = definition {
                    launch.add(server, definition, tools, act);
                }
            }
        }
        launch.reported(self.source_tools_seen()?);
        Ok(launch)
    }
    /// What Claude starts have reported, by the Assistant's turns, acts' runs and loop runs
    /// that load connectors.
    pub(crate) fn source_tools_seen(&self) -> Result<provider::SourceTools> {
        let mut seen = provider::SourceTools::default();
        let reported: Vec<(String, String)> = (self.db)
            .prepare("SELECT server,tool FROM conversation_source_tools")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (server, tool) in reported {
            seen.tools.entry(server).or_default().insert(tool);
        }
        seen.other_servers = (self.db)
            .prepare("SELECT server FROM conversation_other_servers")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(seen)
    }
    /// Keeps what a Claude start reported, by the Assistant's turn, an act's or a loop's run: each
    /// source's server's tools, so the ones that are not read tools are denied by name from the
    /// next start on, and the servers no Space lists, which are denied whole.
    pub(crate) fn record_source_tools(&self, reported: &provider::SourceTools) -> Result<()> {
        for (server, tools) in &reported.tools {
            for tool in tools {
                self.db.execute(
                    "INSERT OR IGNORE INTO conversation_source_tools VALUES (?1,?2)",
                    params![server, tool],
                )?;
            }
        }
        for server in &reported.other_servers {
            self.db.execute(
                "INSERT OR IGNORE INTO conversation_other_servers VALUES (?1)",
                params![server],
            )?;
        }
        Ok(())
    }
    /// The Space a tool call names with `space`: one this Mac has, and the one the human
    /// picked in the composer for this turn's message, when they picked one.
    pub(crate) fn named_space(&self, args: &Value, guard: &TurnGuard) -> Result<Space> {
        let id = (args["space"].as_str())
            .filter(|id| !id.is_empty())
            .context("Name the Space with space, its id")?;
        let space = (self.find_space(&SpaceId(id.into()))?)
            .with_context(|| format!("No Space here has the id {id}"))?
            .space;
        if let Some(picked) = &guard.picked
            && *picked != space.id
        {
            let chosen = self.space(picked)?.space.name;
            bail!(
                "The human chose {chosen} for this message, so this turn acts only in {chosen}, not {}",
                space.name
            );
        }
        Ok(space)
    }
    /// One of the Assistant's tools that answers at once; `read_source` and `act`, which run a
    /// source, are the service's. Reads of text written by others hold the rest of the turn.
    pub(crate) fn conversation_tool(
        &mut self,
        session: &Session,
        name: &str,
        args: &Value,
        guard: &mut TurnGuard,
    ) -> Result<Value> {
        match name {
            "start_work" => self.start_work(session, args, guard),
            "message_work" => self.message_work(session, args, guard),
            "pause_work" | "resume_work" | "stop_work" => self.steer_work(session, name, args),
            "list_work" => self.list_work(session),
            "work_status" => {
                let (status, work) = self.work_status(session, args)?;
                guard.note(format!("the status of work “{work}”"));
                Ok(status)
            }
            "ask_human" => self.raise_decision(session, args, guard),
            "list_loops" => self.list_loops(),
            "create_loop" | "edit_loop" | "pause_loop" | "resume_loop" | "delete_loop"
            | "run_loop" => self.loop_tool(name, args, guard),
            "list_issues" | "read_issue" => {
                let space = self.named_space(args, guard)?;
                let read =
                    self.issue_tool_in(&space, None, &session.id, name, &without_space(args))?;
                guard.note(match name {
                    "read_issue" => format!("issue {}", args["id"].as_str().unwrap_or_default()),
                    _ => format!("{}'s issues", space.name),
                });
                Ok(read)
            }
            "file_issue" | "edit_issue" | "comment_issue" => {
                let space = self.named_space(args, guard)?;
                let (issue, written) =
                    self.write_for_assistant(&space, name, &without_space(args))?;
                let kind = CardKind::IssueWritten {
                    space_id: space.id,
                    issue_id: issue.to_string(),
                    written: written.into(),
                };
                let card = self.raise_card(kind, guard)?;
                Ok(json!({"issue": issue.to_string(), "written": written, "card": card.id}))
            }
            "save_output" => self.save_assistant_output(args, guard),
            "query_brain" => {
                let space = self.named_space(args, guard)?;
                let answer = self.query_space_brain(&space.id, &without_space(args))?;
                guard.note(format!("{}'s brain", space.name));
                Ok(answer)
            }
            "log_work" => {
                let space = self.named_space(args, guard)?;
                self.log_work_in(session, &space.id, &without_space(args))
            }
            "suggest_space" => {
                service::only_arguments(name, args, &["name", "description"])?;
                let text = |key: &str, limit: usize| -> Result<String> {
                    let text = args[key].as_str().map(str::trim).unwrap_or_default();
                    ensure!(!text.is_empty() && text.len() <= limit, "Give {key}");
                    Ok(text.to_owned())
                };
                let (name, description) = (text("name", 80)?, text("description", 16 * 1024)?);
                let card = self.raise_card(CardKind::SuggestSpace { name, description }, guard)?;
                Ok(json!({"card": card.id, "shown": true}))
            }
            other => bail!("{other} is not one of the Assistant's tools"),
        }
    }
    /// `save_output`: copies a file, anywhere on this Mac, to this Mac's private board store
    /// in the Space named, as the title's next version.
    fn save_assistant_output(&mut self, args: &Value, guard: &TurnGuard) -> Result<Value> {
        service::only_arguments("save_output", args, &["space", "path", "title"])?;
        let space = self.named_space(args, guard)?;
        let title = (args["title"].as_str().map(str::trim))
            .filter(|t| !t.is_empty() && t.len() <= 200)
            .context("Give title")?;
        let path = args["path"].as_str().context("Missing path")?;
        let source = fs::canonicalize(workspace_folder(&self.home).join(path))
            .with_context(|| format!("{path} is not a file"))?;
        ensure!(source.is_file(), "{path} is not a file");
        ensure!(
            fs::metadata(&source)?.len() <= MAX_OUTPUT_BYTES,
            "An output can be at most 20 MB"
        );
        let about = json!({"title": title, "assistant": true, "saved_at": now()});
        let (file, version) = assistant::store_output(
            &assistant::board_store(&self.home),
            space.id.as_str(),
            title,
            &source,
            about,
        )?;
        let kind = CardKind::SavedOutput {
            space_id: space.id,
            title: title.into(),
            file,
            version,
        };
        let card = self.raise_card(kind, guard)?;
        Ok(json!({"card": card.id, "version": version, "kept": "Only on this Mac"}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::tests::{assistant_with_space, start};

    #[test]
    fn a_turn_is_held_unless_the_humans_messages_alone_started_it() {
        let (_home, mut host, assistant, celadon) = assistant_with_space();
        let (_, coordinator) = start(&mut host, &assistant, &celadon, "Comment colour");
        let human = host
            .send_to_assistant("m1", "thanks", &[], Some(&celadon))
            .unwrap();
        let report = host
            .send(
                "report:c:r1",
                Some(&coordinator.id),
                &assistant.id,
                "[blocked] Comment colour: schedule a nightly check",
            )
            .unwrap();
        let notice = host
            .send(
                &format!("{ASSISTANT_NOTICE_PREFIX}x"),
                None,
                &assistant.id,
                "An approval waits",
            )
            .unwrap();

        let alone = host.turn_guard(std::slice::from_ref(&human)).unwrap();
        assert!(!alone.held());
        assert_eq!(alone.picked.as_ref(), Some(&celadon));
        assert_eq!(alone.human_message.as_deref(), Some("m1"));
        let batched = host.turn_guard(&[human.clone(), report.clone()]).unwrap();
        assert_eq!(batched.read, ["a report from work “Comment colour”"]);
        let woken = host.turn_guard(std::slice::from_ref(&report)).unwrap();
        assert_eq!(
            woken.read,
            [
                "a report from work “Comment colour”",
                "no message from you started this turn"
            ]
        );
        assert!(host.turn_guard(&[notice]).unwrap().held());
        // The prompt names who sent each, and the Space the human picked.
        let prompt = host.conversation_prompt(&[human, report]).unwrap();
        assert!(
            prompt.starts_with("From: the human, for Celadon"),
            "{prompt}"
        );
        assert!(prompt.contains("From: a report from work “Comment colour”"));
    }

    #[test]
    fn a_result_or_notice_batched_with_the_humans_message_holds_the_turns_loop_change() {
        let (_home, mut host, assistant, celadon) = assistant_with_space();
        let human = host
            .send_to_assistant("m1", "a loop please", &[], Some(&celadon))
            .unwrap();
        let notice = |host: &mut Host, id: &str, body: &str| {
            let id = format!("{ASSISTANT_NOTICE_PREFIX}{id}");
            host.send(&id, None, &assistant.id, body).unwrap()
        };
        let result = notice(
            &mut host,
            "ran:card-1",
            "The human pressed Run on action card card-1, and it ran: Sent",
        );
        let approval = notice(&mut host, "approval:a1", "An approval waits");
        let guard = host.turn_guard(&[human, result, approval]).unwrap();
        assert!(guard.held());
        assert_eq!(guard.read, ["a message from Wiffletree"]);
        assert_eq!(guard.picked.as_ref(), Some(&celadon));
        assert_eq!(guard.human_message.as_deref(), Some("m1"));
        // So a loop change the turn asks for waits for the human's Apply.
        let recipe = json!({"space": celadon, "name": "Digest", "prompt": "Summarise", "cron": "0 7 * * *", "timezone": "UTC"});
        let card = host.loop_tool("create_loop", &recipe, &guard).unwrap();
        assert_eq!(card["applied"], false);
        assert!(host.recipes().unwrap().is_empty());
    }

    #[test]
    fn the_conversation_gets_every_spaces_additions_each_under_its_heading() {
        let (_home, mut host, assistant, celadon) = assistant_with_space();
        host.create_space("Sage", "", "Treat production as read-only.", None)
            .unwrap();
        let mut space = host.space(&celadon).unwrap().space;
        space.additions = "Keep contrast at AA.".into();
        host.update_space(space).unwrap();
        let text = host.session_instructions(&assistant).unwrap();
        let order = [
            "# Wiffletree Assistant contract",
            "# Assistant core",
            "# Assisting",
            "\n## Spaces\n",
            "## Additions from the Celadon Space\nThe human's Celadon Space adds these for requests in it.",
            "Keep contrast at AA.",
            "## Additions from the Sage Space\nThe human's Sage Space adds these for requests in it.",
            "Treat production as read-only.",
        ];
        let at: Vec<usize> = (order.iter())
            .map(|part| text.find(part).unwrap_or_else(|| panic!("{part}: {text}")))
            .collect();
        assert!(at.is_sorted(), "{at:?}");
        // Personal has none, so it has no heading.
        assert!(!text.contains("## Additions from the Personal Space"));
    }

    #[test]
    fn an_output_is_kept_on_this_mac_in_the_space_named_and_a_space_is_only_suggested() {
        let (home, mut host, assistant, celadon) = assistant_with_space();
        let chart = home.path().join("costs.html");
        fs::write(&chart, "<p>costs</p>").unwrap();
        let mut guard = TurnGuard::default();
        let saved = json!({"space": celadon, "path": chart, "title": "AWS costs"});
        let answer = host
            .conversation_tool(&assistant, "save_output", &saved, &mut guard)
            .unwrap();
        assert_eq!(
            (answer["version"].as_u64(), answer["kept"].as_str()),
            (Some(1), Some("Only on this Mac"))
        );
        let store = assistant::board_store(&host.home).join(celadon.as_str());
        assert!(fs::read_dir(store).unwrap().count() > 0);
        let suggested = json!({"name": "Woodworking", "description": "Shop projects"});
        host.conversation_tool(&assistant, "suggest_space", &suggested, &mut guard)
            .unwrap();
        assert!(
            host.spaces()
                .unwrap()
                .iter()
                .all(|r| r.space.name != "Woodworking")
        );
        let kinds: Vec<CardKind> = host
            .conversation_cards()
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert!(matches!(
            &kinds[..],
            [CardKind::SavedOutput { .. }, CardKind::SuggestSpace { .. }]
        ));
    }
}
