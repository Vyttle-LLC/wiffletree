//! The assistant's threads: sessions of role `Assistant` owned by a Space, with no project,
//! parent or ticket, each in its own scratch folder, kept only on this Mac.
use crate::*;
use workspace_core::assistant::proposal::Attached;
use workspace_core::assistant::{
    CardKind, JobRunOf, Thread, ThreadCard, ThreadHit, ThreadTab, title_from_message,
    validate_title,
};
use workspace_core::job::{OutputKind, Recipe, RecipePreview, Sharing, job_id};

/// The message a thread is sent when the human asks for Make this a project's brief.
pub(crate) const BRIEF_REQUEST: &str = "Make this a project: write a project brief from our conversation, with a short project name, and show it to me with project_brief.";
/// The largest file `save_output` copies.
const MAX_OUTPUT_BYTES: u64 = 20 * 1024 * 1024;
/// At most this many scratch files are listed for Make this a project.
const MAX_SCRATCH_FILES: usize = 200;
const MAX_CARD_TEXT: usize = 16 * 1024;

/// Threads and their scratch folders sit under `assistant/` in the data folder.
pub(crate) fn scratch_folder(home: &Path, thread: &str) -> PathBuf {
    home.join("assistant").join("threads").join(thread)
}
/// Where a message to a thread keeps its attachments.
pub(crate) fn attachments_folder(home: &Path, message: &str) -> Result<PathBuf> {
    ensure!(
        Path::new(message).file_name() == Some(message.as_ref()),
        "Message ID cannot name a folder"
    );
    Ok(home.join("assistant").join("attachments").join(message))
}
/// This Mac's private board store, a local Git repository with a folder per Space.
pub(crate) fn board_store(home: &Path) -> PathBuf {
    home.join("boards")
}

/// A new thread's first message.
pub(crate) struct FirstMessage<'a> {
    pub id: &'a str,
    pub body: &'a str,
    pub files: &'a [PathBuf],
}

/// The tables a thread's session writes to that named a project for every row.
const PROJECT_OPTIONAL: [&str; 5] = [
    "sessions",
    "messages",
    "activity",
    "usage_requests",
    "work_logs",
];

/// The thread table, and sessions, messages, activity, usage and log entries that need no
/// project, so that a thread's session holds no project record.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS threads (id TEXT PRIMARY KEY REFERENCES sessions(id), space_id TEXT NOT NULL, data TEXT NOT NULL);",
    )?;
    for table in PROJECT_OPTIONAL {
        if project_required(db, table)? {
            allow_no_project(db, table)?;
        }
    }
    Ok(())
}
fn project_required(db: &Connection, table: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT \"notnull\" FROM pragma_table_info(?1) WHERE name='project_id'",
        [table],
        |r| r.get(0),
    )?)
}
/// Rebuilds `table` with a nullable `project_id`, as SQLite changes a column's constraint:
/// a copy under a new name with its rows, indexes, triggers and AUTOINCREMENT counter, with
/// foreign keys checked once at the end. The legacy rename leaves the views and the other
/// tables' triggers that name `table` as they are, to find the copy by its name.
fn allow_no_project(db: &Connection, table: &str) -> Result<()> {
    let schema: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |r| r.get(0),
    )?;
    // The name is written as created, quoted or not; the columns follow it.
    let columns = schema.find('(').context("Unexpected schema")?;
    let copy = format!("CREATE TABLE {table}_rebuilt {}", &schema[columns..]);
    let rebuilt = copy.replacen("project_id TEXT NOT NULL", "project_id TEXT", 1);
    ensure!(rebuilt != copy, "Unexpected schema for {table}");
    let dependents: Vec<String> = db
        .prepare("SELECT sql FROM sqlite_master WHERE tbl_name=?1 AND type IN ('index','trigger') AND sql IS NOT NULL")?
        .query_map([table], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let counter: Option<i64> = db
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name=?1",
            [table],
            |r| r.get(0),
        )
        .optional()
        .unwrap_or(None);
    db.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA legacy_alter_table=ON;")?;
    let result = (|| -> Result<()> {
        db.execute_batch("BEGIN IMMEDIATE;")?;
        db.execute_batch(&format!(
            "{rebuilt}; INSERT INTO {table}_rebuilt SELECT * FROM {table}; DROP TABLE {table}; ALTER TABLE {table}_rebuilt RENAME TO {table};"
        ))?;
        for sql in &dependents {
            db.execute_batch(sql)?;
        }
        if let Some(counter) = counter {
            db.execute(
                "UPDATE sqlite_sequence SET seq=max(seq,?2) WHERE name=?1",
                params![table, counter],
            )?;
        }
        let broken: i64 =
            db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        ensure!(broken == 0, "Rebuilding {table} broke {broken} references");
        db.execute_batch("COMMIT;")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = db.execute_batch("ROLLBACK;");
    }
    db.execute_batch("PRAGMA legacy_alter_table=OFF; PRAGMA foreign_keys=ON;")?;
    result
}

/// `project`, or NULL for the empty project id a thread's session carries.
pub(crate) fn project_column(project: &str) -> Option<&str> {
    (!project.is_empty()).then_some(project)
}

/// What a thread may do in its Space: a thread's tool name and arguments.
fn card_text<'a>(args: &'a Value, key: &str, limit: usize) -> Result<&'a str> {
    let text = args[key].as_str().map(str::trim).unwrap_or_default();
    ensure!(!text.is_empty(), "Give {key}");
    ensure!(text.len() <= limit, "{key} is too long");
    Ok(text)
}

/// A title's folder in the board store: lowercase words joined by hyphens.
fn slug(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    match words.join("-") {
        slug if slug.is_empty() => "output".into(),
        slug => slug.chars().take(60).collect(),
    }
}

/// Matches `query` anywhere, with LIKE's own wildcards taken literally.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// The line of `body` that holds `query`, at most 120 characters.
fn excerpt(body: &str, query: &str) -> Option<String> {
    let query = query.to_lowercase();
    let line = body.lines().find(|l| l.to_lowercase().contains(&query))?;
    Some(line.trim().chars().take(120).collect())
}

impl Host {
    pub fn assistant_name(&self) -> String {
        self.shared().assistant_name()
    }
    pub fn set_assistant_name(&mut self, name: &str) -> Result<String> {
        self.shared().set_assistant_name(name)?;
        Ok(self.assistant_name())
    }
    pub fn thread(&self, id: &str) -> Result<Thread> {
        self.db
            .query_row("SELECT data FROM threads WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?
            .context("Thread not found")
    }
    pub(crate) fn threads(&self) -> Result<Vec<Thread>> {
        self.list_data("SELECT data FROM threads ORDER BY rowid", [])
    }
    pub(crate) fn save_thread(&self, thread: &Thread) -> Result<()> {
        self.db.execute(
            "UPDATE threads SET data=?2 WHERE id=?1",
            params![thread.id, encode(thread)?],
        )?;
        Ok(())
    }
    /// The threads in the tab strip at `now`, in its order, with their sessions' state.
    pub fn thread_tabs(&self, now: i64) -> Result<Vec<ThreadTab>> {
        let mut threads: Vec<Thread> = (self.threads()?.into_iter())
            .filter(|t| t.in_strip(now))
            .collect();
        threads.sort_by_key(|t| (t.position, t.created_at));
        threads
            .into_iter()
            .map(|thread| {
                let session = self.shown(self.session(&thread.id)?);
                let runtime = self.session_runtime(&thread.id)?;
                Ok(ThreadTab {
                    message_revision: self.message_revision(&thread.id)?,
                    thread,
                    status: session.status,
                    profile: runtime.profile,
                    selection: runtime.selection,
                })
            })
            .collect()
    }
    /// Starts a thread in `space` and sends it its first message, which titles it. Starting the
    /// same id again returns the thread it started.
    pub fn start_thread(
        &mut self,
        id: &str,
        space: &SpaceId,
        body: &str,
        files: &[PathBuf],
        chosen: Option<ModelProfile>,
    ) -> Result<Thread> {
        let first = FirstMessage {
            id: &new_id(),
            body,
            files,
        };
        self.begin_thread(id, space, &title_from_message(body), first, chosen)
    }
    /// Starts a thread titled `title` in `space` with its `first` message.
    pub(crate) fn begin_thread(
        &mut self,
        id: &str,
        space: &SpaceId,
        title: &str,
        first: FirstMessage,
        chosen: Option<ModelProfile>,
    ) -> Result<Thread> {
        ensure!(
            Path::new(id).file_name() == Some(id.as_ref())
                && !id.starts_with('.')
                && id.len() <= 64,
            "A thread id is a plain name"
        );
        if let Ok(thread) = self.thread(id) {
            ensure!(
                &thread.space_id == space,
                "Thread ID reused in another Space"
            );
            return Ok(thread);
        }
        let record = self.space(space)?;
        let model = self.thread_model(&record, chosen)?;
        self.open_thread(id, space, title, None, model, first)
    }
    /// Starts a thread, or with `job` a job's run, on `model` and sends it its `first`
    /// message.
    pub(crate) fn open_thread(
        &mut self,
        id: &str,
        space: &SpaceId,
        title: &str,
        job: Option<JobRunOf>,
        model: (ModelProfile, Chooser, String, bool),
        first: FirstMessage,
    ) -> Result<Thread> {
        let kind = (job, None);
        self.create_thread(id, space, title, kind, model, Some(first))
    }
    /// Creates a thread, a job's run with `job` or an issue thread with `attached`, on its
    /// model; with `first`, sends it that message, else it waits for the human's.
    fn create_thread(
        &mut self,
        id: &str,
        space: &SpaceId,
        title: &str,
        (job, attached): (Option<JobRunOf>, Option<Attached>),
        (profile, chooser, reason, waits): (ModelProfile, Chooser, String, bool),
        first: Option<FirstMessage>,
    ) -> Result<Thread> {
        let record = self.space(space)?;
        ensure!(
            record.local.enabled,
            "{} is off on this Mac",
            record.space.name
        );
        let at = now();
        let title = title.to_owned();
        let session = Session {
            id: id.into(),
            project_id: String::new(),
            parent_id: None,
            repository_id: None,
            name: title.clone(),
            role: match job {
                Some(_) => Role::JobRun,
                None => Role::Assistant,
            },
            provider: profile.provider,
            status: Status::Ready,
            archived: false,
            owner: Some(SessionOwner::Space(space.clone())),
        };
        let position: i64 = self.db.query_row(
            "SELECT coalesce(max(json_extract(data,'$.position')),0)+1 FROM threads",
            [],
            |r| r.get(0),
        )?;
        let thread = Thread {
            id: id.into(),
            space_id: space.clone(),
            title,
            created_at: at,
            last_message_at: at,
            opened_at: at,
            closed: false,
            position,
            project_id: None,
            cards: vec![],
            routed: vec![],
            moved: vec![],
            job,
            attached,
            proposals: vec![],
        };
        let scratch = scratch_folder(&self.home, id);
        fs::create_dir_all(&scratch)?;
        self.atomically(|host| {
            host.insert_thread_session(&session)?;
            host.db.execute(
                "INSERT INTO threads VALUES (?1,?2,?3)",
                params![thread.id, space.as_str(), encode(&thread)?],
            )?;
            let mut runtime = host.pin(&session.id, profile.clone(), chooser, &reason)?;
            runtime.waiting = waits;
            runtime.workdir = Some(scratch.to_string_lossy().into_owned());
            host.save_runtime(&runtime)?;
            if let Some(first) = first {
                host.send_with_files(first.id, None, &session.id, first.body, first.files)?;
            }
            Ok(())
        })?;
        Ok(thread)
    }
    /// Ask the assistant: the issue thread of `attached` in the issue's Space, opened again
    /// when that Space has one, else started with no message. The Space is `filter` when it
    /// shares the tracker, else the one of those last used for a new project; a thread never
    /// changes its Space, so a tracker two Spaces share has a thread in each.
    pub fn open_issue_thread(
        &mut self,
        attached: &Attached,
        filter: Option<&SpaceId>,
    ) -> Result<Thread> {
        let (space, title) = match attached {
            Attached::Issue { id } => {
                let space = self.issue_space(id.prefix(), filter)?;
                let issue = self.read_issue(id)?;
                let named = (issue.issue.as_ref()).map_or(String::new(), |i| i.title.clone());
                (space, title_from_message(&format!("{id} · {named}")))
            }
            Attached::Issues => {
                let space = match filter {
                    Some(space) => self.space(space)?.space,
                    None => (self.spaces_by_use()?.into_iter())
                        .find(|space| self.tracker(space).is_ok())
                        .context("No Space here has issues")?,
                };
                let prefix = (self.tracker(&space))
                    .with_context(|| format!("{} has no issues", space.name))?
                    .prefix;
                let space = self.issue_space(&prefix, Some(&space.id))?;
                let title = format!("Issues · {}", space.name);
                (space, title)
            }
        };
        let existing = (self.threads()?.into_iter()).find(|thread| {
            thread.space_id == space.id && thread.attached.as_ref() == Some(attached)
        });
        if let Some(thread) = existing {
            return self.set_thread_open(&thread.id, true);
        }
        let record = self.space(&space.id)?;
        let model = self.thread_model(&record, None)?;
        let kind = (None, Some(attached.clone()));
        self.create_thread(&new_id(), &space.id, &title, kind, model, None)
    }
    fn insert_thread_session(&self, session: &Session) -> Result<()> {
        self.db.execute(
            "INSERT INTO sessions VALUES (?1,NULL,NULL,?2,?3)",
            params![session.id, tag(&session.role)?, encode(session)?],
        )?;
        Self::event(&self.db, "", Some(&session.id), "thread_started", "")
    }
    /// The model a new thread starts on: the one the user chose, else the Assistant row as its
    /// Space resolves it, which says when the Space's row fell back to this Mac's.
    fn thread_model(
        &self,
        record: &SpaceRecord,
        chosen: Option<ModelProfile>,
    ) -> Result<(ModelProfile, Chooser, String, bool)> {
        if let Some(profile) = chosen {
            return Ok((profile, Chooser::Human, "Chosen by you".into(), false));
        }
        let pick = self.resolve_job(Some(&record.space.id), &Job::Assistant, false, false)?;
        Ok((pick.profile, Chooser::Wiffletree, pick.reason, pick.waits))
    }
    /// Whether this Mac may run `profile`, a model chosen by hand: its provider is not found
    /// signed out here. A provider not detected yet counts as signed in.
    pub(crate) fn may_use(&self, profile: &ModelProfile) -> bool {
        self.detected
            .get(&profile.provider)
            .is_none_or(|signed_in| *signed_in)
    }
    /// Called as a message is queued for a thread's session: the human's message counts as its
    /// latest, and a thread that became a project takes no more.
    pub(crate) fn thread_receives(
        &mut self,
        session: &Session,
        sender: Option<&str>,
    ) -> Result<()> {
        let mut thread = self.thread(&session.id)?;
        ensure!(
            thread.project_id.is_none(),
            "This thread became a project and is read-only"
        );
        if thread.job.is_some() {
            let sent: i64 = self.db.query_row(
                "SELECT count(*) FROM messages WHERE recipient=?1 AND sender IS NULL",
                [&session.id],
                |r| r.get(0),
            )?;
            ensure!(sent == 0, "A job's run takes no message but its prompt");
            return Ok(());
        }
        if sender.is_none() {
            thread.last_message_at = now();
            thread.closed = false;
            self.save_thread(&thread)?;
        }
        Ok(())
    }
    pub fn rename_thread(&mut self, id: &str, title: &str) -> Result<Thread> {
        let title = validate_title(title)?.to_owned();
        let mut thread = self.thread(id)?;
        let mut session = self.session(id)?;
        thread.title = title.clone();
        session.name = title;
        self.atomically(|host| {
            host.save_thread(&thread)?;
            host.db.execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![session.id, encode(&session)?],
            )?;
            Ok(())
        })?;
        Ok(thread)
    }
    /// Closes a thread's tab, or opens it again, as a search result does.
    pub fn set_thread_open(&mut self, id: &str, open: bool) -> Result<Thread> {
        let mut thread = self.thread(id)?;
        thread.closed = !open;
        if open {
            thread.opened_at = now();
        }
        self.save_thread(&thread)?;
        Ok(thread)
    }
    pub fn order_threads(&mut self, ids: &[String]) -> Result<()> {
        self.atomically(|host| {
            for (position, id) in ids.iter().enumerate() {
                let mut thread = host.thread(id)?;
                thread.position = position as i64;
                host.save_thread(&thread)?;
            }
            Ok(())
        })
    }
    /// Removes a thread's conversation, with every record its session has, and its scratch
    /// folder. What it logged to a brain stays.
    pub fn delete_thread(&mut self, id: &str) -> Result<()> {
        self.thread(id)?;
        ensure!(
            !self.in_turn.contains(id),
            "Stop the thread's turn before deleting it"
        );
        let messages: Vec<String> = self
            .db
            .prepare("SELECT id FROM messages WHERE recipient=?1")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        // Its brain entries stay and are sent: its batch closes now, and a session with entries
        // keeps its bare row, which they name, and loses everything else.
        let closed = self.atomically(|host| {
            let closed = memory::close_log_batches(&host.db, memory::Closing::Session(id), now())?;
            let logged: bool = host.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM work_logs WHERE session_id=?1)",
                [id],
                |r| r.get(0),
            )?;
            match logged {
                true => delete_dependents(&host.db, "sessions", "id=?1", id, &["work_logs"])?,
                false => delete_with_dependents(&host.db, "sessions", "id=?1", id)?,
            }
            Ok(closed)
        })?;
        self.brains_now |= closed > 0;
        for message in messages {
            if let Ok(folder) = attachments_folder(&self.home, &message) {
                let _ = fs::remove_dir_all(folder);
            }
        }
        match fs::remove_dir_all(scratch_folder(&self.home, id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
    /// Threads whose title or messages hold `query`, newest message first, in `space` if
    /// given. An empty query lists every thread.
    pub fn search_threads(&self, query: &str, space: Option<&SpaceId>) -> Result<Vec<ThreadHit>> {
        let query = query.trim();
        let pattern = like_pattern(query);
        let threads: Vec<Thread> = self.list_data(
            "SELECT t.data FROM threads t WHERE (?2 IS NULL OR t.space_id=?2) AND json_extract(t.data,'$.job') IS NULL AND (json_extract(t.data,'$.title') LIKE ?1 ESCAPE '\\' OR EXISTS (SELECT 1 FROM messages m WHERE m.recipient=t.id AND m.body LIKE ?1 ESCAPE '\\')) ORDER BY json_extract(t.data,'$.last_message_at') DESC LIMIT 100",
            params![pattern, space.map(SpaceId::as_str)],
        )?;
        threads
            .into_iter()
            .map(|thread| {
                let excerpt = match thread.title.to_lowercase().contains(&query.to_lowercase()) {
                    true => None,
                    false => self
                        .db
                        .prepare("SELECT body FROM messages WHERE recipient=?1 AND body LIKE ?2 ESCAPE '\\' ORDER BY sequence DESC LIMIT 1")?
                        .query_row(params![thread.id, pattern], |r| r.get::<_, String>(0))
                        .optional()?
                        .and_then(|body| excerpt(&body, query)),
                };
                Ok(ThreadHit { thread, excerpt })
            })
            .collect()
    }
    /// Asks a thread for Make this a project's brief, which it shows with `project_brief`.
    pub fn draft_project_brief(&mut self, id: &str) -> Result<Message> {
        self.thread(id)?;
        self.send(&new_id(), None, id, BRIEF_REQUEST)
    }
    /// Make this a project: a project in the thread's Space with that Space's repositories on
    /// this Mac, started on `brief` with a link back to the thread and copies of the scratch
    /// `files`. The thread stays, read-only, and links to the project.
    pub fn make_project(
        &mut self,
        id: &str,
        name: &str,
        brief: &str,
        files: &[String],
    ) -> Result<Project> {
        let mut thread = self.thread(id)?;
        ensure!(
            thread.project_id.is_none(),
            "This thread is already a project"
        );
        ensure!(!brief.trim().is_empty(), "The brief must not be empty");
        let scratch = fs::canonicalize(scratch_folder(&self.home, id))?;
        let files = files
            .iter()
            .map(|file| inside_scratch(&scratch, file))
            .collect::<Result<Vec<_>>>()?;
        let body = format!(
            "{}\n\nFrom the assistant thread “{}” (wiffletree://thread/{id}).",
            brief.trim(),
            thread.title
        );
        let project = self.atomically(|host| {
            let (project, coordinator) = host.insert_project(name, &thread.space_id, None)?;
            host.send_with_files(&new_id(), None, &coordinator.id, &body, &files)?;
            host.set_live(&project.id, true)?;
            thread.project_id = Some(project.id.clone());
            host.save_thread(&thread)?;
            Ok(project)
        })?;
        Ok(project)
    }
    /// The regular files in a thread's scratch folder, relative to it, hidden ones left out.
    pub fn scratch_files(&self, id: &str) -> Result<Vec<String>> {
        self.thread(id)?;
        let root = scratch_folder(&self.home, id);
        let mut files = vec![];
        let mut folders = vec![root.clone()];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    folders.push(entry.path());
                } else if kind.is_file() && files.len() < MAX_SCRATCH_FILES {
                    let relative = entry.path().strip_prefix(&root)?.to_owned();
                    files.push(relative.to_string_lossy().into_owned());
                }
            }
        }
        files.sort();
        Ok(files)
    }
    pub fn dismiss_thread_card(&mut self, id: &str, card: &str) -> Result<Thread> {
        let mut thread = self.thread(id)?;
        thread.cards.retain(|c| c.id != card);
        self.save_thread(&thread)?;
        Ok(thread)
    }
    fn add_card(&self, thread: &mut Thread, kind: CardKind) -> Result<Value> {
        let card = ThreadCard {
            id: new_id(),
            created_at: now(),
            kind,
        };
        let result = json!({"card":card.id, "shown":true});
        thread.cards.push(card);
        self.save_thread(thread)?;
        Ok(result)
    }
    /// What a thread's instructions say about the issue it was opened on, if any. A job's run
    /// gets the Job run core in place of the Assistant core, which its role already chooses.
    pub(crate) fn attached_section(&self, session: &Session) -> Result<String> {
        let thread = self.thread(&session.id)?;
        // Only the ID: the issue's text is untrusted, and the thread reads it with read_issue.
        Ok(match &thread.attached {
            Some(Attached::Issue { id }) => format!(
                "\n## This issue thread\nThe human opened this thread on {id}, an issue of your Space's tracker. \"This issue\" means {id}: read it with `read_issue` before you answer about it.\n"
            ),
            Some(Attached::Issues) => "\n## This issue thread\nThe human opened this thread on your Space's tracker's list of issues. Read it with `list_issues` before you answer about it.\n".into(),
            None => String::new(),
        })
    }
    /// Who last changed a shared Space's `space.yaml`, as its storage clone's history names
    /// them; `None` for a Space only this Mac keeps, or one whose history cannot be read.
    pub(crate) fn space_changed_by(&self, space: &Space) -> Option<String> {
        if matches!(space.storage.repository, StorageRepository::Local { .. }) {
            return None;
        }
        let clone = self.storage_folder(&space.storage.repository);
        let file = format!("spaces/{}/space.yaml", space.storage.folder);
        let file = format!(
            ":(literal){}",
            folder::Root::new(&space.storage.root).join(&file)
        );
        let args = ["log", "-1", "--format=%an%x00%s", "--", file.as_str()];
        let line = runtime::git_output(&clone, &args).ok()?;
        let (author, subject) = line.trim_end().split_once('\0')?;
        Some(sync::named(author, subject).to_owned())
    }
    /// What a thread's turn may use beyond reading: its scratch folder and its Space's
    /// sources, or for a job's run only its recipe's, with each MCP source's definition from
    /// the provider's own configuration. An issue thread reads untrusted text, so it gets no
    /// source and no web: nothing it could send what it read through.
    pub(crate) fn thread_launch(&self, session: &Session) -> Result<provider::ThreadLaunch> {
        let thread = self.thread(&session.id)?;
        let sources = match (&thread.job, &thread.attached) {
            (Some(job), _) => self.recipe(&job.job)?.sources,
            (None, Some(_)) => vec![],
            (None, None) => self.space(&thread.space_id)?.space.sources,
        };
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let servers = (sources.iter())
            .filter_map(|source| match source {
                Source::Mcp { server, .. } => Some(server),
                Source::Cli { .. } => None,
            })
            .filter_map(|server| {
                let definition = provider::server_definition(session.provider, &home, server)?;
                Some((server.clone(), definition))
            })
            .collect();
        let scratch = scratch_folder(&self.home, &session.id);
        fs::create_dir_all(&scratch)?;
        Ok(provider::ThreadLaunch {
            scratch: fs::canonicalize(scratch)?,
            sources,
            servers,
            job_run: thread.job.is_some(),
            issue_thread: thread.attached.is_some(),
            compile: None,
        })
    }
    /// A thread's workspace tool. No other tool is offered to a thread, and none of these
    /// coordinates, writes to a repository or changes a Space or a setting.
    pub(crate) fn thread_tool(
        &mut self,
        session: &Session,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        let mut thread = self.thread(&session.id)?;
        // A job's run saves its output and may read its Space's brain, and does nothing else.
        if let Some(job) = thread.job.clone() {
            if name == "query_brain" {
                return self.query_brain(session, args);
            }
            ensure!(
                name == "save_output",
                "{name} is not one of a job run's tools"
            );
            let title = card_text(args, "title", 200)?;
            let source = self.output_source(&thread, card_text(args, "path", 1024)?)?;
            let output = self.save_job_output(&thread, &job, title, &source)?;
            return Ok(json!({"version": output.version, "kept": output.kept}));
        }
        match name {
            "save_output" => {
                let title = card_text(args, "title", 200)?;
                let path = card_text(args, "path", 1024)?;
                let (file, version) = self.save_output(&thread, title, path)?;
                let mut result = self.add_card(
                    &mut thread,
                    CardKind::SavedOutput {
                        title: title.into(),
                        file,
                        version,
                    },
                )?;
                result["version"] = json!(version);
                result["kept"] = json!("Only on this Mac");
                Ok(result)
            }
            "suggest_space" => {
                let name = card_text(args, "name", 80)?.into();
                let description = card_text(args, "description", MAX_CARD_TEXT)?.into();
                self.add_card(&mut thread, CardKind::SuggestSpace { name, description })
            }
            "project_brief" => {
                let name = card_text(args, "name", 128)?.into();
                let brief = card_text(args, "brief", MAX_CARD_TEXT)?.into();
                self.add_card(&mut thread, CardKind::ProjectBrief { name, brief })
            }
            "propose_issue" | "propose_edit" | "propose_comment" | "propose_split"
            | "propose_duplicate" => self.propose_change(&mut thread, name, args),
            "list_issues" | "read_issue" => self.issue_tool(session, name, args),
            "log_work" if thread.attached.is_some() => {
                bail!("An issue thread writes to no repository, so it has no log_work")
            }
            "log_work" => self.log_work(session, args),
            "query_brain" => self.query_brain(session, args),
            other => bail!("{other} is not one of a thread's tools"),
        }
    }
    /// The recipe a thread's `propose_job` describes, with its card's preview waiting for its
    /// sources' checks, which the service runs off the host thread.
    pub(crate) fn propose_job(
        &self,
        session: &Session,
        args: &Value,
    ) -> Result<(Recipe, jobs::PendingPreview)> {
        let thread = self.thread(&session.id)?;
        ensure!(
            thread.job.is_none(),
            "propose_job is not one of a job run's tools"
        );
        let recipe = proposed_recipe(&thread, args)?;
        self.refuse_taken_job(&recipe)?;
        let preview = self.preview_job(&recipe)?;
        Ok((recipe, preview))
    }
    /// Shows a proposed recipe's card, its sources checked, in the thread.
    pub(crate) fn add_recipe_card(
        &self,
        thread: &str,
        recipe: Recipe,
        preview: RecipePreview,
    ) -> Result<Value> {
        let mut thread = self.thread(thread)?;
        let recipe = Box::new(recipe);
        self.add_card(&mut thread, CardKind::Recipe { recipe, preview })
    }
    /// Copies a scratch file into the board store under the thread's Space and commits it; a
    /// title saved before gets a new version. Each output is a folder holding only its file,
    /// with its metadata beside the folder as `<folder>.json`, so no file name collides with
    /// it. Returns the file's name and the version.
    fn save_output(&self, thread: &Thread, title: &str, path: &str) -> Result<(String, u32)> {
        let source = self.output_source(thread, path)?;
        let about = json!({"title":title,"thread_id":thread.id,"saved_at":now()});
        store_output(
            &board_store(&self.home),
            thread.space_id.as_str(),
            title,
            &source,
            about,
        )
    }
    /// A file of the thread's scratch folder that `save_output` may copy.
    fn output_source(&self, thread: &Thread, path: &str) -> Result<PathBuf> {
        let scratch = fs::canonicalize(scratch_folder(&self.home, &thread.id))?;
        let source = inside_scratch(&scratch, path)?;
        let size = fs::metadata(&source)?.len();
        ensure!(size <= MAX_OUTPUT_BYTES, "An output can be at most 20 MB");
        Ok(source)
    }
}

/// The folder of a Space's part of the board store that holds its jobs' outputs. A title's
/// folder is never named so, since titles' folder names hold no underscore.
pub(crate) const JOB_OUTPUTS: &str = "_jobs";

/// Copies `source` into the repository at `repository`, under `base`, as `title`'s output and
/// commits it; a title saved before gets a new version. Each output is a folder holding only
/// its file, with `about` and the file's name beside the folder as `<folder>.json`, so no
/// file name collides with it. Returns the file's name and the version.
pub(crate) fn store_output(
    repository: &Path,
    base: &str,
    title: &str,
    source: &Path,
    about: Value,
) -> Result<(String, u32)> {
    let root = folder::Root::default();
    store_output_in(&root, repository, base, title, source, about)
}

/// `store_output` for a repository whose Wiffletree folder is `root`, whose versions from before
/// its move still count.
pub(crate) fn store_output_in(
    root: &folder::Root,
    repository: &Path,
    base: &str,
    title: &str,
    source: &Path,
    mut about: Value,
) -> Result<(String, u32)> {
    if !repository.join(".git").exists() {
        fs::create_dir_all(repository)?;
        runtime::git_output(repository, &["init", "-q", "-b", "main"])?;
    }
    let relative =
        output_folder(&repository.join(base), title).map(|folder| format!("{base}/{folder}"))?;
    let metadata = format!("{relative}.json");
    let folder = repository.join(&relative);
    if folder.exists() {
        fs::remove_dir_all(&folder)?;
    }
    fs::create_dir_all(&folder)?;
    let file = source
        .file_name()
        .context("Name a file")?
        .to_string_lossy()
        .into_owned();
    fs::copy(source, folder.join(&file))?;
    about["file"] = json!(file);
    shared::write_atomically(
        &repository.join(&metadata),
        &serde_json::to_vec_pretty(&about)?,
    )?;
    let author = [
        "-c",
        "user.name=Wiffletree",
        "-c",
        "user.email=wiffletree@localhost",
        "-c",
        "commit.gpgsign=false",
    ];
    let mut add = author.to_vec();
    add.extend(["add", "-A", "--", relative.as_str(), metadata.as_str()]);
    runtime::git_output(repository, &add)?;
    let mut commit = author.to_vec();
    let message = format!("Save {relative}");
    commit.extend([
        "commit",
        "-q",
        "--no-verify",
        "-m",
        message.as_str(),
        "--",
        relative.as_str(),
        metadata.as_str(),
    ]);
    runtime::git_output(repository, &commit)?;
    // Each save rewrites the metadata, so its commits count the versions.
    Ok((file, folder::saves(repository, root, &metadata)?))
}

/// The recipe a thread's `propose_job` describes, in the thread's own Space, for its card.
/// A recipe names its own sources; none are taken from elsewhere.
fn proposed_recipe(thread: &Thread, args: &Value) -> Result<Recipe> {
    let name = card_text(args, "name", 200)?.to_owned();
    let prompt = card_text(args, "prompt", MAX_CARD_TEXT)?.to_owned();
    let cron = card_text(args, "cron", 200)?.to_owned();
    let timezone = match args["timezone"].as_str().map(str::trim) {
        Some(zone) if !zone.is_empty() => zone.to_owned(),
        _ => jobs::local_timezone(),
    };
    let sources: Vec<Source> = match args.get("sources") {
        Some(sources) if !sources.is_null() => serde_json::from_value(sources.clone())
            .context("Each source is an MCP server with its tools or a CLI with its commands")?,
        _ => vec![],
    };
    let output = match args["output_title"].as_str().map(str::trim) {
        Some(title) if !title.is_empty() => Some(OutputKind {
            title: title.into(),
            file_type: args["output_type"].as_str().unwrap_or("md").trim().into(),
        }),
        _ => None,
    };
    let sharing = match args["sharing"].as_str() {
        Some("everyone") => Sharing::Everyone,
        _ => Sharing::OneRunner,
    };
    let recipe = Recipe {
        id: job_id(&name),
        name,
        space_id: thread.space_id.clone(),
        prompt,
        schedule: workspace_core::job::Schedule { cron, timezone },
        model: None,
        sources,
        output,
        sharing,
        runner: None,
        enabled: true,
        extra: Default::default(),
    };
    jobs::validate(&recipe)?;
    Ok(recipe)
}

/// The folder in a Space's part of the board store that `title` saves to: its slug, or the
/// slug with `-2`, `-3`… when another title with the same slug holds it.
pub(crate) fn output_folder(space: &Path, title: &str) -> Result<String> {
    let slug = slug(title);
    let held_by = |folder: &str| -> Option<String> {
        let about: Value =
            serde_json::from_slice(&fs::read(space.join(format!("{folder}.json"))).ok()?).ok()?;
        about["title"].as_str().map(str::to_owned)
    };
    (1..)
        .map(|n| match n {
            1 => slug.clone(),
            n => format!("{slug}-{n}"),
        })
        .take(1000)
        .find(|folder| held_by(folder).is_none_or(|held| held == title))
        .context("Too many outputs share this title's name")
}

/// `path`, absolute or relative to `scratch`, as a regular file inside it, links resolved.
fn inside_scratch(scratch: &Path, path: &str) -> Result<PathBuf> {
    let resolved = fs::canonicalize(scratch.join(path))
        .with_context(|| format!("{path} is not in the scratch folder"))?;
    ensure!(
        resolved.starts_with(scratch) && resolved.is_file(),
        "{path} is not a file in the scratch folder"
    );
    Ok(resolved)
}

/// Deletes the rows of `table` matching `filter` (on `?1`) and, first, every row that refers
/// to them through a foreign key, however deep.
fn delete_with_dependents(db: &Connection, table: &str, filter: &str, param: &str) -> Result<()> {
    delete_dependents(db, table, filter, param, &[])?;
    db.execute(&format!("DELETE FROM {table} WHERE {filter}"), [param])?;
    Ok(())
}
/// Deletes the rows of other tables that refer to `table`'s rows matching `filter`, and
/// theirs in turn, except those of the tables in `keep`.
fn delete_dependents(
    db: &Connection,
    table: &str,
    filter: &str,
    param: &str,
    keep: &[&str],
) -> Result<()> {
    let references: Vec<(String, String, String)> = db
        .prepare("SELECT m.name,f.\"from\",f.\"to\" FROM sqlite_master m JOIN pragma_foreign_key_list(m.name) f WHERE m.type='table' AND f.\"table\"=?1 AND m.name<>?1")?
        .query_map([table], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (child, from, to) in references {
        if keep.contains(&child.as_str()) {
            continue;
        }
        let to = if to.is_empty() {
            "rowid".to_owned()
        } else {
            to
        };
        let nested = format!("{from} IN (SELECT {to} FROM {table} WHERE {filter})");
        delete_with_dependents(db, &child, &nested, param)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Host, SpaceId) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let sage = host
            .create_space("Sage", "Sage work", "Treat production as read-only.", None)
            .unwrap()
            .space
            .id;
        (home, host, sage)
    }
    fn start(host: &mut Host, id: &str, space: &SpaceId, body: &str) -> Thread {
        host.start_thread(id, space, body, &[], None).unwrap()
    }

    #[test]
    fn an_older_store_takes_sessions_messages_and_activity_without_a_project() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.create_project("Calls").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        host.send("m1", None, &coordinator.id, "hello").unwrap();
        // As a store from before threads holds them.
        drop(host);
        let db = Connection::open(home.path().join(STORE_FILE)).unwrap();
        for table in PROJECT_OPTIONAL {
            let schema: String = db
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            let required = schema.replacen("project_id TEXT", "project_id TEXT NOT NULL", 1);
            db.execute_batch("PRAGMA writable_schema=ON;").unwrap();
            db.execute(
                "UPDATE sqlite_master SET sql=?1 WHERE name=?2",
                params![required, table],
            )
            .unwrap();
            db.execute_batch("PRAGMA writable_schema=OFF;").unwrap();
        }
        drop(db);
        let mut host = Host::open_set_up(home.path()).unwrap();
        for table in PROJECT_OPTIONAL {
            assert!(!project_required(&host.db, table).unwrap(), "{table}");
        }
        assert_eq!(host.message("m1").unwrap().body, "hello");
        let next = host.send("m2", None, &coordinator.id, "again").unwrap();
        assert!(next.sequence > host.message("m1").unwrap().sequence);
        // Foreign keys still hold.
        assert!(host.send("m3", None, "no-such-session", "x").is_err());
        let personal = host.default_space().unwrap();
        start(&mut host, "t1", &personal, "Hi");
    }

    #[test]
    fn a_thread_starts_on_its_spaces_default_unless_this_mac_cannot_run_it() {
        let (_home, mut host, sage) = store();
        let opus = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "high".into(),
        };
        let mut space = host.space(&sage).unwrap().space;
        space.models.threads = Some(opus.clone().into());
        host.update_space(space).unwrap();
        start(&mut host, "t1", &sage, "Hi");
        let runtime = host.session_runtime("t1").unwrap();
        assert_eq!(runtime.profile, Some(opus.clone()));
        assert_eq!(
            runtime.selection.unwrap().reason,
            "Sage's Assistant row: First choice"
        );
        let signed_out = ProviderStatus {
            installed: true,
            signed_in: false,
            plan: None,
        };
        host.record_detection(Provider::Codex, &signed_out);
        start(&mut host, "t2", &sage, "Hi");
        let runtime = host.session_runtime("t2").unwrap();
        let row = host.model_selection().unwrap();
        let row = &row.row(&Job::Assistant).unwrap().main.first;
        assert_eq!(runtime.profile.as_ref(), Some(row));
        assert_eq!(
            runtime.selection.unwrap().reason,
            "Sage's Assistant row falls back to this Mac's: Codex is not signed in on this Mac. \
             First choice"
        );
        let chosen = host
            .start_thread("t3", &sage, "Hi", &[], Some(opus.clone()))
            .unwrap();
        assert_eq!(
            host.session_runtime(&chosen.id).unwrap().profile,
            Some(opus.clone())
        );
        // As anywhere in the table, a provider not detected yet is not chosen.
        host.detected.remove(&Provider::Codex);
        start(&mut host, "t4", &sage, "Hi");
        assert_eq!(
            host.session_runtime("t4").unwrap().profile.as_ref(),
            Some(row)
        );
    }

    #[test]
    fn a_space_saved_with_a_row_the_table_refuses_is_refused() {
        let (_home, mut host, sage) = store();
        let profile = |model: &str| ModelProfile {
            provider: Provider::Claude,
            model: model.into(),
            effort: "high".into(),
        };
        let mut space = host.space(&sage).unwrap().space;
        let row = JobRow {
            job: Job::Coordinator,
            main: Line {
                first: profile("sonnet"),
                then: Then::Model(profile("opus")),
            },
            small: None,
        };
        space.models.set_overrides(vec![row]);
        let error = host.update_space(space.clone()).unwrap_err().to_string();
        assert!(
            error.contains("both choices use the Claude subscription"),
            "{error}"
        );
        space.models.set_overrides(vec![]);
        space.reviews.max_rounds = Some(99);
        assert!(host.update_space(space).is_err());
        assert_eq!(host.space(&sage).unwrap().space.reviews.max_rounds, None);
    }

    /// A Claude row for `job`, on a line that breaks the table's rules when `broken`.
    fn space_row(job: Job, broken: bool) -> JobRow {
        let opus = ModelProfile {
            provider: Provider::Claude,
            model: "opus".into(),
            effort: "high".into(),
        };
        JobRow {
            job,
            main: Line {
                first: opus.clone(),
                then: if broken {
                    Then::Model(opus)
                } else {
                    Then::Wait
                },
            },
            small: None,
        }
    }

    #[test]
    fn a_space_cannot_gain_a_router_or_compiler_row_through_either_save() {
        let (_home, mut host, sage) = store();
        for job in [Job::Router, Job::Compiler] {
            let name = job.name();
            let row = space_row(job, false);
            let mut space = host.space(&sage).unwrap().space;
            space.models.set_overrides(vec![row.clone()]);
            let error = host.update_space(space).unwrap_err().to_string();
            assert!(
                error.contains(&format!("The {name} row belongs")),
                "{error}"
            );

            let settings = cascade::SpaceModelSettings {
                rows: vec![row],
                ..Default::default()
            };
            let error = host.set_space_model_settings(&sage, settings).unwrap_err();
            assert!(error.to_string().contains("belongs to this Mac"), "{error}");
            assert!(
                host.space(&sage)
                    .unwrap()
                    .space
                    .models
                    .overrides()
                    .is_empty()
            );
        }
    }

    /// Writes `rows` straight into the Space, as a teammate's older host saved them.
    fn saved_by_an_older_host(host: &mut Host, sage: &SpaceId, rows: Vec<JobRow>) {
        let mut space = host.space(sage).unwrap().space;
        space.models.set_overrides(rows);
        host.db
            .execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![sage.as_str(), encode(&space).unwrap()],
            )
            .unwrap();
    }

    #[test]
    fn an_old_compiler_row_changes_nothing_and_blocks_no_other_save() {
        let (_home, mut host, sage) = store();
        // It also breaks the table's rules, which must not count against it.
        let old = vec![
            space_row(Job::Compiler, true),
            space_row(Job::Router, false),
        ];
        saved_by_an_older_host(&mut host, &sage, old.clone());

        let this_mac = host.resolve_job(None, &Job::Router, false, false).unwrap();
        let in_sage = host
            .resolve_job(Some(&sage), &Job::Router, false, false)
            .unwrap();
        assert_ne!(old[1].main.first, this_mac.profile);
        assert_eq!(in_sage.profile, this_mac.profile);
        let table = host.space_table(Some(&sage), &Default::default()).unwrap();
        assert!(
            table
                .rows
                .iter()
                .all(|row| row.resolved.from != Layer::Space)
        );
        assert!(table.fallbacks(false).is_empty(), "no banner for it");

        // An unrelated save, through each command, keeps it.
        let mut settings = host.space_model_settings(&sage).unwrap();
        settings.rows.push(space_row(Job::Implementer, false));
        let saved = host.set_space_model_settings(&sage, settings).unwrap();
        assert!(old.iter().all(|row| saved.rows.contains(row)));
        let mut space = host.space(&sage).unwrap().space;
        space.description = "Changed".into();
        let saved = host.update_space(space).unwrap().space;
        assert_eq!(saved.models.ignored(), old);

        // Changing it is refused, and leaving it out removes it.
        let mut settings = host.space_model_settings(&sage).unwrap();
        settings.rows.retain(|row| row.job != Job::Compiler);
        settings.rows.push(space_row(Job::Compiler, false));
        assert!(host.set_space_model_settings(&sage, settings).is_err());
        let mut settings = host.space_model_settings(&sage).unwrap();
        settings.rows.retain(|row| row.job.space_may_set());
        let saved = host.set_space_model_settings(&sage, settings).unwrap();
        assert_eq!(saved.rows.len(), 1);
        assert!(host.space(&sage).unwrap().space.models.ignored().is_empty());
    }

    #[test]
    fn a_spaces_list_of_this_macs_models_limits_nothing_and_is_noted_once() {
        let (home, mut host, sage) = store();
        let sol = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "high".into(),
        };
        let mut space = host.space(&sage).unwrap().space;
        space.models.threads = Some(sol.clone().into());
        host.update_space(space).unwrap();
        let mut local = host.space(&sage).unwrap().local;
        local.models = Some(vec![ModelProfile {
            provider: Provider::Claude,
            model: "sonnet".into(),
            effort: "medium".into(),
        }]);
        host.db
            .execute(
                "UPDATE spaces SET local=?2 WHERE id=?1",
                params![sage.as_str(), encode(&local).unwrap()],
            )
            .unwrap();
        start(&mut host, "t1", &sage, "Hi");
        assert_eq!(host.session_runtime("t1").unwrap().profile, Some(sol));

        // An earlier store, before the note.
        host.db
            .execute(
                "DELETE FROM policy_migrations WHERE id='space-model-lists'",
                [],
            )
            .unwrap();
        drop(host);
        let host = Host::open_set_up(home.path()).unwrap();
        let noted = |host: &Host| {
            (host.model_selection().unwrap().notes.iter())
                .filter(|note| note.starts_with("The Space Sage had a list"))
                .count()
        };
        assert_eq!(noted(&host), 1);
        assert_eq!(host.space(&sage).unwrap().local.models, local.models);
        drop(host);
        assert_eq!(noted(&Host::open_set_up(home.path()).unwrap()), 1);
    }

    #[test]
    fn a_shared_spaces_instructions_say_who_last_changed_them() {
        let (home, mut host, sage) = store();
        start(&mut host, "t1", &sage, "Hi");
        let session = host.session("t1").unwrap();
        // Kept only on this Mac, no one is named.
        let local = host.session_instructions(&session).unwrap();
        assert!(local.contains("adds these to every agent in it. They cannot widen"));
        assert!(!local.contains("last changed by"));
        // Stored on GitHub, the clone's last commit to its space.yaml names the machine.
        let mut record = host.space(&sage).unwrap().space;
        record.storage.repository = StorageRepository::GitHub {
            owner: "Sage-Social".into(),
            name: "wiffletree-storage".into(),
        };
        record.storage.root = String::new();
        host.db
            .execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![sage.as_str(), encode(&record).unwrap()],
            )
            .unwrap();
        let clone = repos::clone_folder(home.path(), "Sage-Social/wiffletree-storage");
        let folder = clone.join("spaces").join(&record.storage.folder);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("space.yaml"), "name: Sage\n").unwrap();
        crate::test_support::git(&clone, &["init", "-q"]);
        crate::test_support::git(&clone, &["add", "-A"]);
        crate::test_support::git(&clone, &["commit", "-qm", "Edit Sage · Jamie's Mac"]);
        let shared = host.session_instructions(&session).unwrap();
        assert!(
            shared.contains("adds these to every agent in it. They were last changed by Jamie's Mac. They cannot widen"),
            "{shared}"
        );
    }

    #[test]
    fn idle_and_closed_threads_leave_the_strip_and_search_brings_them_back() {
        let (_home, mut host, sage) = store();
        let personal = (host.spaces().unwrap().into_iter())
            .find(|record| record.space.name == workspace_core::PERSONAL)
            .unwrap()
            .space
            .id;
        start(
            &mut host,
            "dates",
            &sage,
            "What date format did we settle on for the API?",
        );
        start(
            &mut host,
            "dentist",
            &personal,
            "Remind me to book the dentist",
        );
        let ids = |host: &Host, at: i64| -> Vec<String> {
            (host.thread_tabs(at).unwrap().into_iter())
                .map(|t| t.thread.id)
                .collect()
        };
        assert_eq!(ids(&host, now()), ["dates", "dentist"]);
        host.order_threads(&["dentist".into(), "dates".into()])
            .unwrap();
        assert_eq!(ids(&host, now()), ["dentist", "dates"]);
        assert_eq!(
            ids(&host, now() + 30 * 60 * 60 * 1000),
            Vec::<String>::new()
        );
        host.set_thread_open("dates", false).unwrap();
        assert_eq!(ids(&host, now()), ["dentist"]);

        let found = |query: &str, space: Option<&SpaceId>| -> Vec<String> {
            (host.search_threads(query, space).unwrap().into_iter())
                .map(|hit| hit.thread.id)
                .collect()
        };
        assert_eq!(found("date format", None), ["dates"]);
        assert_eq!(found("DENTIST", None), ["dentist"]);
        assert!(found("dentist", Some(&sage)).is_empty());
        assert!(found("100%", None).is_empty());
        host.send("reply", Some("dates"), "dates", "ISO 8601 in UTC")
            .unwrap();
        let hits = host.search_threads("iso 8601", None).unwrap();
        assert_eq!(hits[0].excerpt.as_deref(), Some("ISO 8601 in UTC"));
        host.set_thread_open("dates", true).unwrap();
        assert_eq!(ids(&host, now()), ["dentist", "dates"]);
    }

    #[test]
    fn deleting_a_thread_removes_its_conversation_and_scratch_folder_only() {
        let (home, mut host, sage) = store();
        start(&mut host, "t1", &sage, "Draft a memo");
        start(&mut host, "t2", &sage, "Another");
        let scratch = scratch_folder(home.path(), "t1");
        fs::write(scratch.join("memo.md"), "draft").unwrap();
        host.send("reply", Some("t1"), "t1", "Drafted").unwrap();
        host.delete_thread("t1").unwrap();
        assert!(host.thread("t1").is_err() && host.session("t1").is_err());
        assert!(!scratch.exists());
        let left: i64 = host
            .db
            .query_row(
                "SELECT count(*) FROM messages WHERE recipient='t1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        assert_eq!(host.thread_tabs(now()).unwrap().len(), 1);
    }

    #[test]
    fn make_this_a_project_starts_one_in_the_threads_space_and_leaves_it_read_only() {
        let (home, mut host, sage) = store();
        start(&mut host, "t1", &sage, "The sign-in retry is broken");
        fs::write(
            scratch_folder(home.path(), "t1").join("findings.md"),
            "notes",
        )
        .unwrap();
        assert_eq!(host.scratch_files("t1").unwrap(), ["findings.md"]);
        assert!(
            host.make_project("t1", "Sign-in", "Fix it", &["../../x".into()])
                .is_err()
        );
        let project = host
            .make_project(
                "t1",
                "Sign-in retry",
                "Fix the retry.",
                &["findings.md".into()],
            )
            .unwrap();
        assert_eq!(project.space, sage);
        assert!(
            project.added_repositories.is_empty(),
            "it inherits its Space's"
        );
        assert!(host.live_projects().unwrap().contains(&project.id));
        let coordinator = (host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project.id)
            .unwrap();
        let brief = host.messages(&coordinator.id, None, 10).unwrap().remove(0);
        assert!(brief.sender.is_none());
        assert!(brief.body.starts_with("Fix the retry."));
        assert!(brief.body.contains("wiffletree://thread/t1"));
        assert_eq!(brief.attachments[0].name(), "findings.md");
        assert_eq!(host.thread("t1").unwrap().project_id, Some(project.id));
        let refused = host.send("late", None, "t1", "One more thing").unwrap_err();
        assert!(format!("{refused}").contains("read-only"));
    }

    #[test]
    fn a_threads_tools_show_cards_and_nothing_else_is_offered() {
        let (_home, mut host, sage) = store();
        start(&mut host, "t1", &sage, "Woodworking plans");
        let session = host.session("t1").unwrap();
        let args = json!({"name":"Woodworking","description":"Shop projects"});
        host.thread_tool(&session, "suggest_space", &args).unwrap();
        let args = json!({"name":"Sign-in","brief":"Fix it"});
        host.thread_tool(&session, "project_brief", &args).unwrap();
        let kinds: Vec<CardKind> = (host.thread("t1").unwrap().cards.into_iter())
            .map(|card| card.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                CardKind::SuggestSpace {
                    name: "Woodworking".into(),
                    description: "Shop projects".into()
                },
                CardKind::ProjectBrief {
                    name: "Sign-in".into(),
                    brief: "Fix it".into()
                }
            ]
        );
        assert_eq!(
            host.spaces().unwrap().len(),
            2,
            "suggesting creates no Space"
        );
        for tool in [
            "create_ticket",
            "report",
            "send_message",
            "workspace_context",
        ] {
            assert!(
                host.thread_tool(&session, tool, &json!({})).is_err(),
                "{tool}"
            );
        }
        let outside = json!({"path":"/etc/hosts","title":"Hosts"});
        assert!(host.thread_tool(&session, "save_output", &outside).is_err());
    }

    #[test]
    fn each_title_keeps_its_own_output_whatever_its_file_is_named() {
        let (home, mut host, sage) = store();
        start(&mut host, "t1", &sage, "Exports");
        let session = host.session("t1").unwrap();
        let scratch = scratch_folder(home.path(), "t1");
        let mut save = |file: &str, body: &str, title: &str| {
            fs::write(scratch.join(file), body).unwrap();
            let args = json!({"path": file, "title": title});
            host.thread_tool(&session, "save_output", &args).unwrap()["version"].clone()
        };
        assert_eq!(save("board.json", "{\"rows\":1}", "JSON export"), 1);
        assert_eq!(save("one.md", "one", "A B"), 1);
        assert_eq!(save("two.md", "two", "A-B"), 1);
        assert_eq!(save("one.md", "one again", "A B"), 2);
        let space = board_store(home.path()).join(sage.as_str());
        let read = |path: &str| fs::read_to_string(space.join(path)).unwrap();
        assert_eq!(read("json-export/board.json"), "{\"rows\":1}");
        assert_eq!(read("a-b/one.md"), "one again");
        assert_eq!(read("a-b-2/two.md"), "two");
        assert!(read("a-b-2.json").contains("\"A-B\""));
    }

    #[test]
    fn a_title_becomes_a_folder_name() {
        assert_eq!(slug("AWS costs"), "aws-costs");
        assert_eq!(slug("Q3: “plan”/draft"), "q3-plan-draft");
        assert_eq!(slug("…"), "output");
    }

    #[test]
    fn a_search_takes_wildcards_literally() {
        assert_eq!(like_pattern("50%_off"), "%50\\%\\_off%");
        assert_eq!(
            excerpt("Hi\nAPI dates are ISO 8601 in UTC\nbye", "iso"),
            Some("API dates are ISO 8601 in UTC".into())
        );
    }
}
