//! Work logs (OpenSpec change `brain-log-capture`): `log_work` and the human's capture form
//! store one entry each, which the host renders as one file in the gate's entry contract
//! (`gate/README.md`, "Log entry contract") and collects into batches. A closed batch goes to
//! the brain's batch flow (`brains.rs`), which opens one pull request for it. Nothing here logs
//! an entry's text; errors and events name entry and batch ids only.
use super::*;
use brains::new_ulid;
use chrono::{Datelike, Local, TimeZone};
use publish::GitTool;

/// The gate's cap on an entry's body, `gate.max_entry_body_chars`'s default.
const MAX_BODY_CHARS: usize = 2000;
const MAX_TITLE_CHARS: usize = 80;
/// A batch holds at most this many entries; the next one starts a new batch.
const MAX_BATCH_ENTRIES: i64 = 50;
/// A batch opens its pull request at the latest this long after its first entry.
const BATCH_FLUSH_MS: i64 = 24 * 60 * 60 * 1000;
const NO_REF: &str = "—";
/// The tool result's key for the milestone prompt, which the MCP bridge moves to the end of the
/// result's text as its last line.
pub(crate) const PROMPT_KEY: &str = "log_work_prompt";
const PROMPT: &str = "Milestone: record it with one terse log_work call (type, title, changed, why, state; the coordinator also gives this ticket's ticket_id). No transcripts, narration or pasted output, and never child personal information, credentials or secrets.";
/// The arguments `log_work` takes. The host fills in the entry's id, date, `ref` and author.
const LOG_WORK_ARGUMENTS: [&str; 8] = [
    "type",
    "title",
    "changed",
    "why",
    "state",
    "repository",
    "ticket_id",
    "milestone",
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS log_batches (id TEXT PRIMARY KEY, brain TEXT NOT NULL COLLATE NOCASE, project_id TEXT NOT NULL, ticket_id TEXT, session_id TEXT, opened_at INTEGER NOT NULL, closed_at INTEGER, brain_batch_id TEXT);
CREATE TABLE IF NOT EXISTS log_prompts (session_id TEXT NOT NULL, ticket_id TEXT NOT NULL, milestone TEXT NOT NULL, prompted_at INTEGER NOT NULL, PRIMARY KEY(session_id,ticket_id,milestone));
";

/// Who may log: the project coordinator and implementers.
pub(crate) fn logs_work(role: Role) -> bool {
    matches!(role, Role::ProjectOrchestrator | Role::Implementer)
}

/// Adds the batch tables and the entries' batch column and, once, carries the rows captured
/// before per-entry files over: an exported row is marked as in its legacy file, and the
/// export receipts and the maintenance runs are dropped. Then queues the unsent entries whose
/// project's Space has a brain.
pub(crate) fn migrate(host: &mut Host) -> Result<()> {
    let has_batches: bool = host.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('work_logs') WHERE name='batch_id')",
        [],
        |r| r.get(0),
    )?;
    let legacy: bool = host.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='log_exports')",
        [],
        |r| r.get(0),
    )?;
    host.atomically(|host| {
        host.db.execute_batch(SCHEMA)?;
        if !has_batches {
            host.db
                .execute_batch("ALTER TABLE work_logs ADD COLUMN batch_id TEXT;")?;
        }
        if legacy {
            host.carry_over_logs()?;
            host.db.execute_batch(
                "DROP TABLE log_exports; DROP INDEX IF EXISTS runs_project; DROP TABLE IF EXISTS maintenance_runs;",
            )?;
        }
        host.queue_unsent()
    })
}

/// The host's local date of `at`, `YYYY-MM-DD`.
fn local_date(at: i64) -> String {
    let at = Local.timestamp_millis_opt(at).single().unwrap_or_default();
    format!("{:04}-{:02}-{:02}", at.year(), at.month(), at.day())
}

/// One line: whitespace runs, newlines included, become one space, and HTML comments are
/// escaped so that no entry carries the gate's `gate:allow` suppression marker.
fn one_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("<!--", "&lt;!--")
        .replace("-->", "--&gt;")
}

/// The input as stored: each field on one line and not empty, and the title at most 80
/// characters.
fn folded(mut input: LogInput) -> Result<LogInput> {
    for (name, value) in [
        ("title", &mut input.title),
        ("changed", &mut input.changed),
        ("why", &mut input.why),
        ("state", &mut input.state),
    ] {
        *value = one_line(value);
        ensure!(!value.is_empty(), "Give {name} some text");
    }
    let title = input.title.chars().count();
    ensure!(
        title <= MAX_TITLE_CHARS,
        "The title is {title} characters; keep it to {MAX_TITLE_CHARS}"
    );
    if let Some(milestone) = &input.milestone {
        text(milestone, 256)?;
    }
    Ok(input)
}

/// A folder name the entry contract allows: a letter or digit, then letters, digits, `.`, `_`
/// and `-`.
fn valid_folder(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A GitHub login as the gate checks it.
fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && !login.starts_with('-')
        && !login.ends_with('-')
        && !login.contains("--")
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A `ref` the entry contract allows: `—`, `<branch> @ —`, or `<branch> @ <sha>[…<sha>]` with an
/// optional ` · PR #NN`, where each SHA is 7–40 lowercase hex characters.
fn valid_ref(reference: &str) -> bool {
    if reference == NO_REF {
        return true;
    }
    let Some((branch, at)) = reference.split_once(" @ ") else {
        return false;
    };
    if branch.is_empty() || branch.contains(char::is_whitespace) {
        return false;
    }
    if at == NO_REF {
        return true;
    }
    let (shas, pull_request) = match at.split_once(" · PR #") {
        Some((shas, number)) => (shas, Some(number)),
        None => (at, None),
    };
    let sha = |s: &str| {
        (7..=40).contains(&s.len()) && s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    };
    let shas_fit = match shas.split_once('…') {
        Some((first, last)) => sha(first) && sha(last),
        None => sha(shas),
    };
    shas_fit && pull_request.is_none_or(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// A ticket's `ref`: its branch with the commits since its base, 7-character SHAs, and its pull
/// request once there are commits.
fn ticket_ref(branch: &str, commits: &[&str], pull_request: Option<u64>) -> String {
    let short = |sha: &str| sha[..7].to_owned();
    let shas = match commits {
        [] => return format!("{branch} @ {NO_REF}"),
        [one] => short(one),
        [first, .., last] => format!("{}…{}", short(first), short(last)),
    };
    let pull_request = pull_request
        .map(|n| format!(" · PR #{n}"))
        .unwrap_or_default();
    format!("{branch} @ {shas}{pull_request}")
}

/// The entry's path in the brain repository.
fn entry_path(folder: &str, repository: &str, date: &str, id: &str) -> String {
    let (year, month) = (&date[..4], &date[5..7]);
    format!("{folder}/logs/{repository}/{year}/{month}/{date}-{id}.md")
}

/// The entry's file in the gate's contract, refused when its body is over the cap.
fn entry_file(log: &WorkLog, author: &str) -> Result<String> {
    let kind = match log.input.kind {
        LogKind::Decision => "decision",
        LogKind::Observation => "observation",
    };
    let input = &log.input;
    let body = format!(
        "## {} — {}\n- type: {kind}\n- ref: {}\n- changed: {}\n- why: {}\n- state: {}\n",
        log.date, input.title, log.reference, input.changed, input.why, input.state
    );
    let length = body.trim().chars().count();
    ensure!(
        length <= MAX_BODY_CHARS,
        "The entry is {length} characters and the cap is {MAX_BODY_CHARS}: cut at least {} characters. Nothing was stored.",
        length - MAX_BODY_CHARS
    );
    Ok(format!(
        "---\nid: {}\nrepo: {}\nauthor: {author}\nsource: wiffletree\n---\n{body}",
        log.id, log.repository
    ))
}

/// The entry's folder for a workspace repository: its GitHub name, or its workspace name.
fn repository_folder(repository: &Repository) -> Result<String> {
    let name = (repository.github.as_ref())
        .map(|github| github.name.clone())
        .unwrap_or_else(|| repository.name.clone());
    ensure!(
        valid_folder(&name),
        "The repository name {name} cannot be a log folder: use letters, digits, '.', '_' or '-'"
    );
    Ok(name)
}

/// The tool arguments as a `LogInput` for `session`. Arguments the host fills in are refused.
fn log_work_input(session: &Session, args: &Value) -> Result<LogInput> {
    service::only_arguments("log_work", args, &LOG_WORK_ARGUMENTS)?;
    let string = |key: &str| -> Result<String> {
        Ok(args[key]
            .as_str()
            .with_context(|| format!("Missing {key}"))?
            .to_owned())
    };
    let optional = |key: &str| args[key].as_str().map(str::to_owned);
    Ok(LogInput {
        session_id: session.id.clone(),
        milestone: optional("milestone"),
        kind: serde_json::from_value(args["type"].clone())
            .context("type is decision or observation")?,
        title: string("title")?,
        repository: optional("repository"),
        ticket_id: optional("ticket_id"),
        changed: string("changed")?,
        why: string("why")?,
        state: string("state")?,
    })
}

/// Which open batches to close.
pub(crate) enum Closing<'a> {
    Batch(&'a str),
    Ticket(&'a str),
    Session(&'a str),
    /// Those whose first entry is at least 24 hours older than this.
    Due(i64),
    /// Send now, for one brain or every brain.
    Now(Option<&'a str>),
}

/// Closes the open batches `closing` names at `at`; answers how many.
pub(crate) fn close_log_batches(db: &Connection, closing: Closing, at: i64) -> Result<usize> {
    let open = "UPDATE log_batches SET closed_at=?1 WHERE closed_at IS NULL AND";
    Ok(match closing {
        Closing::Batch(batch) => db.execute(&format!("{open} id=?2"), params![at, batch])?,
        Closing::Ticket(ticket) => {
            db.execute(&format!("{open} ticket_id=?2"), params![at, ticket])?
        }
        Closing::Session(session) => {
            db.execute(&format!("{open} session_id=?2"), params![at, session])?
        }
        Closing::Due(now) => db.execute(
            &format!("{open} opened_at<=?2"),
            params![at, now - BATCH_FLUSH_MS],
        )?,
        Closing::Now(brain) => db.execute(
            &format!("{open} (?2 IS NULL OR brain=?2)"),
            params![at, brain],
        )?,
    })
}

/// Where an entry collects: a worker's per ticket, the coordinator's and the human's per
/// coordinator session.
enum BatchKey<'a> {
    Ticket(&'a str),
    Session(&'a str),
}

impl Host {
    /// The human's capture form, which logs as the project's coordinator does.
    pub fn append_log(&mut self, mut input: LogInput) -> Result<WorkLog> {
        let project = self.session(&input.session_id)?.project_id;
        let coordinator = (self.coordinator_of(&project)?)
            .context("This project has no coordinator to log as")?;
        input.session_id = coordinator.clone();
        let coordinator = self.session(&coordinator)?;
        self.capture(&coordinator, input)
    }

    /// `log_work`: one entry for the calling coordinator or implementer.
    pub(crate) fn log_work(&mut self, session: &Session, args: &Value) -> Result<Value> {
        let log = self.capture(session, log_work_input(session, args)?)?;
        Ok(json!({"id": log.id, "path": log.path, "ref": log.reference}))
    }

    /// The one capture path. The brain is the caller's Space's; the host mints the id and date,
    /// computes `ref`, takes the author from gh's login and records the provider, model and
    /// machine. A milestone repeated with the same content returns its first entry. Refused,
    /// storing nothing, without a brain, a GitHub login, or room under the cap.
    fn capture(&mut self, session: &Session, input: LogInput) -> Result<WorkLog> {
        ensure!(
            logs_work(session.role),
            "log_work is for the project coordinator and implementers"
        );
        let input = folded(input)?;
        if let Some(milestone) = &input.milestone
            && let Some(existing) = self
                .db
                .query_row(
                    "SELECT sequence,data FROM work_logs WHERE session_id=?1 AND milestone=?2",
                    params![input.session_id, milestone],
                    |r| {
                        let mut log: WorkLog = decode(r, 1)?;
                        log.sequence = r.get(0)?;
                        Ok(log)
                    },
                )
                .optional()?
        {
            ensure!(
                existing.input == input,
                "Milestone identity reused with different content"
            );
            return Ok(existing);
        }
        let project = self.project(&session.project_id)?;
        let space_brain = (self.owner_brain(&SessionOwner::Project(project.id.clone()))?)
            .context("This project's Space has no brain, so nothing was stored. The human can name one in Settings › Spaces.")?;
        let BrainRepository::GitHub { owner, name, .. } = &space_brain.repository else {
            bail!(
                "This project's Space's brain is not a GitHub repository yet, so nothing was stored. See the Brains page."
            );
        };
        let brain = format!("{owner}/{name}");
        let author = (self.gh_login.clone())
            .filter(|login| valid_login(login))
            .context("Wiffletree has not read your GitHub login, so nothing was stored. Sign gh in (gh auth login), then Sync now on the Brains page.")?;
        let ticket = match session.role {
            Role::Implementer => {
                ensure!(
                    input.repository.is_none(),
                    "An implementer logs for its ticket's repository; leave repository out"
                );
                let own = (self.session_runtime(&session.id)?.ticket_id)
                    .context("This implementer has no ticket to log for")?;
                ensure!(
                    input.ticket_id.as_ref().is_none_or(|named| *named == own),
                    "An implementer logs for its own ticket; leave ticket_id out"
                );
                let own = self.ticket(&own)?;
                ensure!(
                    own.is_open(),
                    "This ticket is already {}; its implementer logs nothing more",
                    own.state
                );
                Some(own)
            }
            _ => {
                match &input.ticket_id {
                    Some(named) => {
                        ensure!(
                            input.repository.is_none(),
                            "Give ticket_id or repository, not both: a ticket's entry is for its repository"
                        );
                        let ticket = self.ticket(named).ok().filter(|ticket| {
                            (self.session(&ticket.coordinator_id))
                                .is_ok_and(|c| c.project_id == project.id)
                        });
                        Some(ticket.with_context(|| {
                            format!("{named} is not one of this project's tickets")
                        })?)
                    }
                    None => None,
                }
            }
        };
        let (task_id, repository, reference) = match &ticket {
            Some(ticket) => {
                let folder = repository_folder(&self.ticket_repository(ticket)?)?;
                let reference = self.ticket_reference(ticket)?;
                (Some(ticket.id.clone()), folder, reference)
            }
            None => match &input.repository {
                Some(named) => {
                    let repositories = self.project_repositories(&project.id)?;
                    let repository = (repositories.iter())
                        .find(|r| {
                            r.id == *named
                                || r.name.eq_ignore_ascii_case(named)
                                || r.github.as_ref().is_some_and(|g| {
                                    g.name.eq_ignore_ascii_case(named)
                                        || format!("{}/{}", g.owner, g.name)
                                            .eq_ignore_ascii_case(named)
                                })
                        })
                        .with_context(|| {
                            format!("{named} is not one of this project's repositories")
                        })?;
                    (None, repository_folder(repository)?, NO_REF.to_owned())
                }
                None => (None, name.clone(), NO_REF.to_owned()),
            },
        };
        ensure!(
            valid_folder(&repository),
            "The brain repository's name {repository} cannot be a log folder"
        );
        let folder = self.brain_folder(&brain, &space_brain);
        let at = now();
        let id = new_ulid(at);
        let date = local_date(at);
        let mut log = WorkLog {
            sequence: 0,
            path: Some(entry_path(&folder, &repository, &date, &id)),
            id,
            project_id: project.id.clone(),
            task_id,
            repository,
            created_at: at,
            date,
            input,
            reference,
            space_id: Some(project.space.clone()),
            brain: Some(brain),
            batch_id: None,
            author: Some(author.clone()),
            provider: Some(session.provider),
            model: (self.session_runtime(&session.id)?.profile).map(|p| p.model),
            machine: Some(sync::machine_name()),
            in_legacy_file: false,
            held: None,
        };
        entry_file(&log, &author)?;
        self.atomically(|host| {
            // By role: an implementer's entries per ticket, the coordinator's and the human's
            // per coordinator session, whichever ticket names their folder and `ref`.
            let key = match (session.role, &ticket) {
                (Role::Implementer, Some(ticket)) => BatchKey::Ticket(&ticket.id),
                _ => BatchKey::Session(&session.id),
            };
            let brain = log.brain.clone().unwrap_or_default();
            let batch = host.open_batch(&brain, &project.id, key, at)?;
            log.batch_id = Some(batch.clone());
            host.store_log(&mut log)?;
            Self::event(
                &host.db,
                &log.project_id,
                Some(&log.input.session_id),
                "work_log",
                &log.id,
            )?;
            host.close_full_batch(&batch, at)
        })?;
        Ok(log)
    }

    /// The brain folder: `brain_dir` in the brain's config on `main`, or the Space's.
    fn brain_folder(&self, brain: &str, space_brain: &SpaceBrain) -> String {
        (self.brain(brain).ok())
            .and_then(|b| b.config.config().map(|c| c.brain_dir.clone()))
            .unwrap_or_else(|| space_brain.folder.clone())
    }

    /// Inserts the entry; one without a milestone takes its id as its milestone column, which
    /// is unique per session.
    fn store_log(&mut self, log: &mut WorkLog) -> Result<()> {
        self.db.execute(
            "INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data,space_id,batch_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                log.id,
                log.project_id,
                log.input.session_id,
                log.input.milestone.as_deref().unwrap_or(&log.id),
                log.repository,
                encode(&*log)?,
                log.space_id.as_ref().map(SpaceId::as_str),
                log.batch_id
            ],
        )?;
        log.sequence = self.db.last_insert_rowid();
        Ok(())
    }

    /// The ticket's `ref` from its branch, which its worktree has checked out: the commits since
    /// its base, and its open or merged pull request. Read in the repository, so a ticket whose
    /// worktree was removed on accepting still has one; `—` once its branch is gone.
    fn ticket_reference(&self, ticket: &Ticket) -> Result<String> {
        let repository = PathBuf::from(self.ticket_repository(ticket)?.path);
        let branch = format!("refs/heads/{}", ticket.branch);
        let present =
            runtime::git_answer(&repository, &["rev-parse", "--verify", "--quiet", &branch])?;
        if !present.status.success() {
            return Ok(NO_REF.into());
        }
        let base = self.ticket_base(ticket)?;
        let range = format!("{}..{branch}", base.measured());
        let commits = runtime::git_output(&repository, &["rev-list", "--reverse", &range])?;
        let commits: Vec<&str> = commits.lines().collect();
        let pull_request = (ticket.pull_request.as_ref())
            .filter(|pr| pr.state != PrState::Closed)
            .map(|pr| pr.number);
        Ok(ticket_ref(&ticket.branch, &commits, pull_request))
    }

    /// The open batch of `brain` for `key`, or a new one opened at `at`.
    fn open_batch(&mut self, brain: &str, project: &str, key: BatchKey, at: i64) -> Result<String> {
        let (ticket, session) = match key {
            BatchKey::Ticket(ticket) => (Some(ticket), None),
            BatchKey::Session(session) => (None, Some(session)),
        };
        let open = self
            .db
            .query_row(
                "SELECT id FROM log_batches WHERE brain=?1 AND ticket_id IS ?2 AND session_id IS ?3 AND closed_at IS NULL",
                params![brain, ticket, session],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(open) = open {
            return Ok(open);
        }
        let id = new_id();
        self.db.execute(
            "INSERT INTO log_batches(id,brain,project_id,ticket_id,session_id,opened_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, brain, project, ticket, session, at],
        )?;
        Ok(id)
    }

    /// Closes the batch once it holds the most entries one may, so the next starts another.
    fn close_full_batch(&mut self, batch: &str, at: i64) -> Result<()> {
        let entries: i64 = self.db.query_row(
            "SELECT count(*) FROM work_logs WHERE batch_id=?1",
            [batch],
            |r| r.get(0),
        )?;
        if entries >= MAX_BATCH_ENTRIES {
            self.db.execute(
                "UPDATE log_batches SET closed_at=?2 WHERE id=?1",
                params![batch, at],
            )?;
            self.brains_now = true;
        }
        Ok(())
    }

    /// Closes batches as `closing` says, making the brains' passes due when any closed.
    pub(crate) fn close_logs(&mut self, closing: Closing) -> Result<usize> {
        let closed = close_log_batches(&self.db, closing, now())?;
        self.brains_now |= closed > 0;
        Ok(closed)
    }

    /// Send now.
    pub fn send_logs_now(&mut self, brain: Option<&str>) -> Result<usize> {
        self.close_logs(Closing::Now(brain))
    }

    /// Closes the batches due at `at` and hands each closed batch to its brain's batch flow,
    /// which commits its entry files and opens one pull request on the brain's next pass. A
    /// batch whose brain has not been fetched, or is held, waits for a later call.
    pub(crate) fn send_log_batches(&mut self, at: i64, git: &GitTool) -> Result<()> {
        self.atomically(Self::queue_unsent)?;
        self.brains_now |= close_log_batches(&self.db, Closing::Due(at), at)? > 0;
        let closed: Vec<(String, String, String, Option<String>)> = self
            .db
            .prepare(
                "SELECT id,brain,project_id,ticket_id FROM log_batches WHERE closed_at IS NOT NULL AND brain_batch_id IS NULL ORDER BY closed_at",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, brain, project, ticket) in closed {
            let ready = (self.brain(&brain).ok())
                .is_some_and(|brain| brain.head.is_some() && brain.held.is_none());
            if !ready {
                continue;
            }
            if let Err(error) = self.hand_off(&id, &brain, &project, ticket.as_deref(), git) {
                eprintln!("Log batch {id}: {error:#}");
            }
        }
        Ok(())
    }

    fn hand_off(
        &mut self,
        id: &str,
        brain: &str,
        project: &str,
        ticket: Option<&str>,
        git: &GitTool,
    ) -> Result<()> {
        let logs: Vec<WorkLog> = self.list_data(
            "SELECT data FROM work_logs WHERE batch_id=?1 ORDER BY sequence",
            [id],
        )?;
        let mut files = vec![];
        // A held entry waits for the human and is never sent as it stands.
        for log in logs.iter().filter(|log| log.held.is_none()) {
            // A row carried over from before per-entry files has no author until gh's login is read.
            let Some(author) = log.author.clone().or_else(|| self.gh_login.clone()) else {
                return Ok(());
            };
            let path = log.path.clone().context("An entry in a batch has a path")?;
            files.push((path, entry_file(log, &author)?.into_bytes()));
        }
        if files.is_empty() {
            return Ok(());
        }
        let about = match ticket {
            Some(ticket) => self.ticket(ticket)?.title,
            None => format!("{} coordinator", self.project(project)?.name),
        };
        let entries = match files.len() {
            1 => "1 entry".to_owned(),
            n => format!("{n} entries"),
        };
        let title: String = format!("Log batch: {about} ({entries})");
        let title: String = title.chars().take(256).collect();
        // The brain batch and the record of it are stored together or not at all, so a failure
        // between them neither loses the log batch nor queues it twice.
        self.atomically(|host| {
            let batch = host.queue_brain_batch(
                brain,
                BrainBatchKind::Log,
                BrainSource::Project(project.into()),
                &title,
                &files,
                git,
            )?;
            let recorded = host.db.execute(
                "UPDATE log_batches SET brain_batch_id=?2 WHERE id=?1",
                params![id, batch.id],
            );
            if let Err(error) = recorded {
                let checkout = PathBuf::from(host.brain(brain)?.checkout);
                host.remove_batch_worktree(&checkout, &batch.id);
                return Err(error.into());
            }
            Ok(())
        })
    }

    /// The prompt for one `log_work` call at `milestone` of the ticket, or `None`: when no brain
    /// applies, when this milestone was prompted already, or when the session has logged an
    /// entry for the ticket since its previous prompted milestone of it.
    pub(crate) fn milestone_prompt(
        &mut self,
        session: &Session,
        ticket: &Ticket,
        milestone: &str,
    ) -> Result<Option<&'static str>> {
        if !logs_work(session.role) {
            return Ok(None);
        }
        let project = self.session(&ticket.coordinator_id)?.project_id;
        let brain = self.owner_brain(&SessionOwner::Project(project))?;
        if !brain.is_some_and(|b| matches!(b.repository, BrainRepository::GitHub { .. })) {
            return Ok(None);
        }
        let (prompted, previous): (bool, Option<i64>) = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM log_prompts WHERE session_id=?1 AND ticket_id=?2 AND milestone=?3), (SELECT max(prompted_at) FROM log_prompts WHERE session_id=?1 AND ticket_id=?2)",
            params![session.id, ticket.id, milestone],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if prompted {
            return Ok(None);
        }
        // Only an entry for this ticket counts; another ticket's or a planning entry does not.
        let logged: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM work_logs WHERE session_id=?1 AND json_extract(data,'$.created_at')>=?2 AND json_extract(data,'$.task_id')=?3)",
            params![session.id, previous.unwrap_or(0), ticket.id],
            |r| r.get(0),
        )?;
        if logged {
            return Ok(None);
        }
        self.db.execute(
            "INSERT INTO log_prompts VALUES (?1,?2,?3,?4)",
            params![session.id, ticket.id, milestone, now()],
        )?;
        Ok(Some(PROMPT))
    }

    /// `result` with the milestone prompt, when there is one, for the MCP bridge to append.
    pub(crate) fn with_milestone_prompt(
        &mut self,
        mut result: Value,
        session: &Session,
        ticket: &Ticket,
        milestone: &str,
    ) -> Result<Value> {
        if let Some(prompt) = self.milestone_prompt(session, ticket, milestone)?
            && let Some(fields) = result.as_object_mut()
        {
            fields.insert(PROMPT_KEY.into(), prompt.into());
        }
        Ok(result)
    }

    pub fn logs(&self, project: &str, before: Option<i64>, limit: usize) -> Result<Vec<WorkLog>> {
        self.project(project)?;
        Ok(self.db.prepare("SELECT sequence,data FROM work_logs WHERE project_id=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3")?.query_map(params![project, before.unwrap_or(i64::MAX), page_limit(limit)?], |r| { let mut log: WorkLog = decode(r, 1)?; log.sequence = r.get(0)?; Ok(log) })?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The migration's carry-over of every row captured before per-entry files.
    fn carry_over_logs(&mut self) -> Result<()> {
        type Row = (String, String, Option<String>, bool);
        let rows: Vec<Row> = self
            .db
            .prepare("SELECT id,data,space_id,EXISTS(SELECT 1 FROM log_exports e WHERE e.log_id=l.id) FROM work_logs l ORDER BY sequence")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, data, space, exported) in rows {
            let mut value: Value = serde_json::from_str(&data)?;
            // The old `reference` was the author's own text; `ref` is now the host's.
            let reference = (value["input"].as_object_mut())
                .and_then(|input| input.remove("reference"))
                .and_then(|reference| reference.as_str().map(str::to_owned))
                .unwrap_or_default();
            let mut log: WorkLog = serde_json::from_value(value)?;
            log.reference = reference;
            log.space_id = space.map(SpaceId);
            log.date = local_date(log.created_at);
            log.in_legacy_file = exported;
            self.db.execute(
                "UPDATE work_logs SET data=?2 WHERE id=?1",
                params![id, encode(&log)?],
            )?;
        }
        Ok(())
    }

    /// Queues each unsent entry whose project's Space now has a GitHub brain, giving it an entry
    /// id, its brain and its path. One that breaks the entry contract or the cap as it stands is
    /// queued held, its text unchanged, and is never sent; the project's coordinator gets one
    /// Inbox item saying how many it holds.
    fn queue_unsent(&mut self) -> Result<()> {
        type Row = (String, String);
        let rows: Vec<Row> = self
            .db
            .prepare("SELECT id,data FROM work_logs WHERE batch_id IS NULL AND json_extract(data,'$.in_legacy_file') IS NOT 1 ORDER BY sequence")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut newly_held = std::collections::BTreeSet::new();
        let mut finished_batches = std::collections::BTreeSet::new();
        for (id, data) in rows {
            let mut entry: WorkLog = serde_json::from_str(&data)?;
            let project = self.project(&entry.project_id)?;
            let Some(space_brain) = self.owner_brain(&SessionOwner::Project(project.id.clone()))?
            else {
                continue;
            };
            let BrainRepository::GitHub { owner, name, .. } = &space_brain.repository else {
                continue;
            };
            let brain = format!("{owner}/{name}");
            // The coordinator's rows were logged for the project as a whole.
            if entry.repository == "project" {
                entry.repository = name.clone();
            }
            entry.id = new_ulid(entry.created_at);
            entry.space_id = Some(project.space.clone());
            let folder = self.brain_folder(&brain, &space_brain);
            entry.path = Some(entry_path(
                &folder,
                &entry.repository,
                &entry.date,
                &entry.id,
            ));
            entry.brain = Some(brain.clone());
            entry.held = unfit(&entry).map(str::to_owned);
            if entry.held.is_some() {
                newly_held.insert(project.id.clone());
            }
            let key = match &entry.task_id {
                Some(ticket) => BatchKey::Ticket(ticket),
                None => BatchKey::Session(&entry.input.session_id),
            };
            let batch = self.open_batch(&brain, &project.id, key, now())?;
            entry.batch_id = Some(batch.clone());
            self.db.execute(
                "UPDATE work_logs SET id=?2,data=?3,batch_id=?4,space_id=?5 WHERE id=?1",
                params![id, entry.id, encode(&entry)?, batch, project.space.as_str()],
            )?;
            self.close_full_batch(&batch, now())?;
            // A finished ticket's batch would otherwise wait for the 24-hour flush.
            let finished = (entry.task_id.as_ref())
                .is_some_and(|ticket| self.ticket(ticket).map_or(true, |t| !t.is_open()));
            if finished {
                finished_batches.insert(batch);
            }
        }
        // Once every row is in, so a finished ticket's rows go together in one pull request.
        for batch in &finished_batches {
            close_log_batches(&self.db, Closing::Batch(batch), now())?;
            self.brains_now = true;
        }
        for project in &newly_held {
            self.raise_held_item(project)?;
        }
        Ok(())
    }

    /// One Inbox item for the project's coordinator naming how many entries it holds.
    fn raise_held_item(&mut self, project: &str) -> Result<()> {
        let Some(coordinator) = self.coordinator_of(project)? else {
            return Ok(());
        };
        let held: i64 = self.db.query_row(
            "SELECT count(*) FROM work_logs WHERE project_id=?1 AND json_extract(data,'$.held') IS NOT NULL",
            [project],
            |r| r.get(0),
        )?;
        let entries = match held {
            1 => "1 log entry".to_owned(),
            n => format!("{n} log entries"),
        };
        let prompt = format!(
            "{entries} captured before per-entry files cannot be sent to the brain as they stand: each breaks the entry contract or the 2,000-character cap. They are kept unchanged in this project's memory and are not sent."
        );
        let operation = format!("{BRAIN_NOTICE_PREFIX}held-logs:{held}");
        self.request_attention(&coordinator, "local", &operation, &prompt, &[])?;
        Ok(())
    }
}

/// Why an entry carried over from before per-entry files cannot be sent as it stands, or `None`
/// when it can. Names the rule, never the text.
fn unfit(entry: &WorkLog) -> Option<&'static str> {
    if !valid_folder(&entry.repository) {
        return Some("its folder name is not one the entry contract allows");
    }
    if !valid_ref(&entry.reference) {
        return Some("its ref is not one the entry contract allows");
    }
    if folded(entry.input.clone()).ok().as_ref() != Some(&entry.input) {
        return Some("a field is empty or not on one line, or the title is over 80 characters");
    }
    entry_file(entry, "author")
        .err()
        .map(|_| "it is over the 2,000-character cap")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brains::tests::{GitHub, REPO, name_brain, run};
    use crate::publish::tests::Fixture;
    use crate::test_support::git;

    /// The gate's entry fixtures, read where the gate keeps them.
    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../gate/tests/fixtures/entries"
    );
    /// Where the gate's tests check each fixture entry (`gate/tests/fixtures/README.md`).
    const FIXTURE_PATH: &str =
        "second-brain/logs/sms-service/2026/10/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md";

    /// The publish fixture's project, in a Sage Space whose brain is `REPO`, with gh signed in:
    /// a ticket in `app` with two commits since its base and a passed review.
    fn fixture() -> Fixture {
        let mut f = publish::tests::fixture();
        f.host.legacy_pointer = f.home.path().join("no-pointer");
        let sage = f.host.create_space("Sage", "", "", None).unwrap().space;
        let sage = name_brain(&mut f.host, &sage.id, REPO);
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &sage))
        .unwrap();
        f.host.record_gh_login(Some("mhohlios".into()));
        f
    }

    fn args(title: &str) -> Value {
        json!({"type": "decision", "title": title, "changed": "Retry once.", "why": "Timeouts are transient.", "state": "Committed."})
    }

    fn log_work(f: &mut Fixture, session: &str, args: Value) -> Result<Value> {
        f.host.agent_tool(session, "log_work", args)
    }

    fn stored(f: &Fixture) -> Vec<WorkLog> {
        f.host.logs(&f.coordinator.project_id, None, 100).unwrap()
    }

    /// The brain on GitHub, fetched once, so a closed batch can be handed off.
    fn fetched(f: &mut Fixture) -> (tempfile::TempDir, publish::Tools) {
        let folder = tempfile::tempdir().unwrap();
        let tools = GitHub::new(folder.path()).tools();
        assert!(run(&mut f.host, &tools));
        (folder, tools)
    }

    #[test]
    fn a_ref_names_the_branch_its_commits_and_its_pull_request() {
        let (one, two) = ("3c1e9a0aaaaaaa", "7d03b1ebbbbbbb");
        let branch = "wiffletree/retry-telnyx-timeout";
        assert_eq!(ticket_ref(branch, &[], Some(92)), format!("{branch} @ —"));
        assert_eq!(
            ticket_ref(branch, &[one], None),
            format!("{branch} @ 3c1e9a0")
        );
        assert_eq!(
            ticket_ref(branch, &[one, "1111111", two], Some(92)),
            format!("{branch} @ 3c1e9a0…7d03b1e · PR #92")
        );
        for reference in [
            ticket_ref(branch, &[], None),
            ticket_ref(branch, &[one, two], Some(92)),
            NO_REF.into(),
        ] {
            assert!(valid_ref(&reference), "{reference}");
        }
    }

    #[test]
    fn the_host_computes_each_ref_from_the_tickets_worktree() {
        let mut f = fixture();
        let implementer = f.implementer.id.clone();
        let commits = git(&f.worktree, &["rev-list", "--reverse", "origin/main..HEAD"]);
        let commits: Vec<&str> = commits.lines().collect();
        let branch = f.ticket().branch;
        let expected = format!("{branch} @ {}…{}", &commits[0][..7], &commits[1][..7]);
        let logged = log_work(&mut f, &implementer, args("Two commits")).unwrap();
        assert_eq!(logged["ref"], expected);

        let mut ticket = f.ticket();
        ticket.pull_request = Some(
            serde_json::from_value(json!({
                "number": 92, "url": "https://github.com/allowed-org/app/pull/92", "state": "open",
                "draft": false, "base": "main", "head": commits[1], "merge_state": "clean",
                "checks": "success", "unresolved_threads": 0, "comments": {}
            }))
            .unwrap(),
        );
        f.host.save_ticket(&ticket).unwrap();
        let logged = log_work(&mut f, &implementer, args("With its PR")).unwrap();
        assert_eq!(logged["ref"], format!("{expected} · PR #92"));

        // A new ticket's implementer logs before its first commit.
        let coordinator = f.coordinator.id.clone();
        let repository = f.ticket().repository_id;
        let fresh = f
            .host
            .create_ticket(&coordinator, &repository, "Fresh", "Do")
            .unwrap();
        let worker = (f.host)
            .assign_ticket(&fresh.id, Role::Implementer, Provider::Claude, "Do", None)
            .unwrap();
        let logged = log_work(&mut f, &worker.id, args("Not started")).unwrap();
        assert_eq!(logged["ref"], format!("{} @ —", fresh.branch));

        let planning = log_work(&mut f, &coordinator, args("Plan")).unwrap();
        assert_eq!(planning["ref"], NO_REF);
    }

    #[test]
    fn an_entry_records_what_the_host_knows_and_the_tool_takes_no_ref_author_or_id() {
        let mut f = fixture();
        let implementer = f.implementer.id.clone();
        for key in ["ref", "author", "id", "source", "date"] {
            let mut given = args("Mine");
            given[key] = json!("chosen by the agent");
            let error = format!("{:#}", log_work(&mut f, &implementer, given).unwrap_err());
            assert!(error.contains(&format!("not {key}")), "{error}");
        }
        assert!(stored(&f).is_empty());

        let logged = log_work(&mut f, &implementer, args("SMS retry")).unwrap();
        let log = stored(&f).remove(0);
        let (id, date) = (log.id.clone(), local_date(log.created_at));
        assert_eq!(id.len(), 26);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
        );
        let path = format!(
            "second-brain/logs/app/{}/{}/{date}-{id}.md",
            &date[..4],
            &date[5..7]
        );
        assert_eq!(logged["path"], path);
        assert_eq!(logged["id"], id);
        assert_eq!(
            (log.brain.as_deref(), log.author.as_deref(), log.provider),
            (Some(REPO), Some("mhohlios"), Some(Provider::Claude))
        );
        assert_eq!(log.task_id.as_deref(), Some(f.ticket.as_str()));
        assert_eq!(log.machine, Some(sync::machine_name()));
        assert_eq!(
            log.space_id,
            Some(f.host.project(&log.project_id).unwrap().space)
        );
        assert!(log.batch_id.is_some());
    }

    #[test]
    fn a_capture_is_refused_and_nothing_stored_without_a_brain_a_login_or_room() {
        let mut f = publish::tests::fixture();
        let implementer = f.implementer.id.clone();
        f.host.record_gh_login(Some("mhohlios".into()));
        let error = format!(
            "{:#}",
            log_work(&mut f, &implementer, args("x")).unwrap_err()
        );
        assert!(error.contains("Space has no brain"), "{error}");

        let mut f = fixture();
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        f.host.gh_login = None;
        let error = format!(
            "{:#}",
            log_work(&mut f, &implementer, args("x")).unwrap_err()
        );
        assert!(error.contains("Sign gh in"), "{error}");
        f.host.record_gh_login(Some("mhohlios".into()));

        let mut long = args("x");
        long["changed"] = json!("word ".repeat(500));
        let error = format!("{:#}", log_work(&mut f, &implementer, long).unwrap_err());
        let length: usize = error["The entry is ".len()..]
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(length > MAX_BODY_CHARS, "{error}");
        assert!(
            error.contains(&format!(
                "cut at least {} characters",
                length - MAX_BODY_CHARS
            )),
            "{error}"
        );

        let title = "t".repeat(81);
        assert!(log_work(&mut f, &implementer, args(&title)).is_err());
        let mut named = args("x");
        named["repository"] = json!("app");
        let error = format!(
            "{:#}",
            log_work(&mut f, &implementer, named.clone()).unwrap_err()
        );
        assert!(error.contains("leave repository out"), "{error}");
        named["repository"] = json!("elsewhere");
        let error = format!("{:#}", log_work(&mut f, &coordinator, named).unwrap_err());
        assert!(
            error.contains("not one of this project's repositories"),
            "{error}"
        );
        // A ticket of another project, or another ticket than an implementer's own.
        let other = f.host.create_project("Other").unwrap();
        let other_coordinator = f.host.coordinator_of(&other.id).unwrap().unwrap();
        let repository = f.ticket().repository_id;
        f.host
            .set_project_repositories(&other.id, Some(vec![repository.clone()]))
            .unwrap();
        let foreign = (f.host)
            .create_ticket(&other_coordinator, &repository, "Foreign", "Do")
            .unwrap();
        let mut theirs = args("x");
        theirs["ticket_id"] = json!(foreign.id);
        let error = format!(
            "{:#}",
            log_work(&mut f, &coordinator, theirs.clone()).unwrap_err()
        );
        assert!(
            error.contains("not one of this project's tickets"),
            "{error}"
        );
        let error = format!("{:#}", log_work(&mut f, &implementer, theirs).unwrap_err());
        assert!(error.contains("leave ticket_id out"), "{error}");

        let ticket = f.ticket.clone();
        for role in [Role::Tester, Role::Reviewer] {
            let verifier = (f.host)
                .assign_ticket(&ticket, role, Provider::Claude, "Check", None)
                .unwrap();
            let error = format!(
                "{:#}",
                log_work(&mut f, &verifier.id, args("x")).unwrap_err()
            );
            assert!(error.contains("coordinator and implementers"), "{error}");
        }
        assert!(stored(&f).is_empty());
        let batches: i64 = (f.host.db)
            .query_row("SELECT count(*) FROM log_batches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(batches, 0);
    }

    #[test]
    fn the_cap_allows_2000_characters_of_body_and_refuses_2001() {
        let mut f = fixture();
        let implementer = f.implementer.id.clone();
        log_work(&mut f, &implementer, args("Base")).unwrap();
        let mut log = stored(&f).remove(0);
        let base = entry_file(&log, "mhohlios").unwrap();
        let body = base.split_once("\n---\n").unwrap().1.trim().chars().count();
        let room = MAX_BODY_CHARS - body + log.input.changed.chars().count();
        log.input.changed = "x".repeat(room);
        assert!(entry_file(&log, "mhohlios").is_ok());
        log.input.changed.push('x');
        let error = format!("{:#}", entry_file(&log, "mhohlios").unwrap_err());
        assert!(error.contains("The entry is 2001 characters"), "{error}");
        assert!(error.contains("cut at least 1 characters"), "{error}");
    }

    #[test]
    fn a_repeated_milestone_returns_its_entry_and_changed_content_is_refused() {
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        let mut first = args("Plan");
        first["milestone"] = json!("plan");
        let one = log_work(&mut f, &coordinator, first.clone()).unwrap();
        assert_eq!(
            log_work(&mut f, &coordinator, first.clone()).unwrap()["id"],
            one["id"]
        );
        first["changed"] = json!("Something else.");
        let error = format!("{:#}", log_work(&mut f, &coordinator, first).unwrap_err());
        assert!(error.contains("reused with different content"), "{error}");
        // Without a milestone every call is a new entry, with its own id.
        let two = log_work(&mut f, &coordinator, args("Plan")).unwrap();
        let three = log_work(&mut f, &coordinator, args("Plan")).unwrap();
        assert_ne!(two["id"], three["id"]);
        assert_eq!(stored(&f).len(), 3);
    }

    #[test]
    fn the_coordinator_and_the_humans_form_log_for_the_project_or_a_named_repository() {
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        let planning = log_work(&mut f, &coordinator, args("Plan")).unwrap();
        assert!(
            planning["path"]
                .as_str()
                .unwrap()
                .starts_with("second-brain/logs/sagehaven-docs/")
        );
        let mut named = args("Repo");
        named["repository"] = json!("allowed-org/app");
        let named = log_work(&mut f, &coordinator, named).unwrap();
        assert!(
            named["path"]
                .as_str()
                .unwrap()
                .starts_with("second-brain/logs/app/")
        );
        // The human's form logs as the coordinator, whichever session is open.
        let human = f.host.append_log(LogInput {
            session_id: f.implementer.id.clone(),
            milestone: None,
            kind: LogKind::Observation,
            title: "Seen\nin review".into(),
            repository: None,
            ticket_id: None,
            changed: "Line one\nline two".into(),
            why: "w".into(),
            state: "s".into(),
        });
        let human = human.unwrap();
        assert_eq!(human.input.session_id, coordinator);
        assert_eq!(
            (human.input.title.as_str(), human.input.changed.as_str()),
            ("Seen in review", "Line one line two")
        );
        let batches: i64 = (f.host.db)
            .query_row("SELECT count(DISTINCT batch_id) FROM work_logs", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(batches, 1, "one batch for the coordinator session");
        // Another project sees none of them.
        let other = f.host.create_project("Other").unwrap();
        assert!(f.host.logs(&other.id, None, 100).unwrap().is_empty());
    }

    #[test]
    fn an_entry_cannot_carry_the_gates_suppression_marker() {
        let mut f = fixture();
        let implementer = f.implementer.id.clone();
        let mut sneaky = args("x");
        sneaky["changed"] = json!("Call 555-0100 <!-- gate:allow phone fine -->");
        log_work(&mut f, &implementer, sneaky).unwrap();
        let changed = stored(&f).remove(0).input.changed;
        assert!(
            !changed.contains("<!--") && !changed.contains("-->"),
            "{changed}"
        );
    }

    /// What the renderer writes for the entry `text` describes, and where, or `None` when no
    /// entry the host stores could describe it: exactly the four frontmatter keys with a valid
    /// author, then the heading and the five fields with a valid type and `ref`, each on one
    /// line as the host stores it.
    fn rendered_like(text: &str) -> Option<(String, String)> {
        let (frontmatter, body) = text.strip_prefix("---\n")?.split_once("\n---\n")?;
        let mut lines = frontmatter.lines();
        let [id, repo, author, source] = ["id", "repo", "author", "source"].map(|key| {
            let (name, value) = lines.next()?.split_once(": ")?;
            (name == key).then_some(value)
        });
        let (id, repo, author, source) = (id?, repo?, author?, source?);
        if lines.next().is_some()
            || !valid_login(author)
            || !["wiffletree", "skill"].contains(&source)
        {
            return None;
        }
        let mut lines = body.lines();
        let (date, title) = lines.next()?.strip_prefix("## ")?.split_once(" — ")?;
        let mut fields: Vec<(&str, String)> = vec![];
        for line in lines {
            match line.strip_prefix("  ") {
                Some(more) => fields.last_mut()?.1.push_str(&format!(" {}", more.trim())),
                None => {
                    let (name, value) = line.strip_prefix("- ")?.split_once(": ")?;
                    fields.push((name, value.into()));
                }
            }
        }
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        if names != ["type", "ref", "changed", "why", "state"] || !valid_ref(&fields[1].1) {
            return None;
        }
        let log = WorkLog {
            sequence: 0,
            id: id.into(),
            project_id: String::new(),
            task_id: None,
            repository: repo.into(),
            created_at: 0,
            date: date.into(),
            input: LogInput {
                session_id: String::new(),
                milestone: None,
                kind: serde_json::from_value(json!(fields[0].1)).ok()?,
                title: title.into(),
                repository: None,
                ticket_id: None,
                changed: fields[2].1.clone(),
                why: fields[3].1.clone(),
                state: fields[4].1.clone(),
            },
            reference: fields[1].1.clone(),
            space_id: None,
            brain: None,
            path: None,
            batch_id: None,
            author: None,
            provider: None,
            model: None,
            machine: None,
            in_legacy_file: false,
            held: None,
        };
        let path = entry_path("second-brain", repo, date, id);
        Some((path, entry_file(&log, author).ok()?))
    }

    fn fixture_entries(kind: &str) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = fs::read_dir(format!("{FIXTURES}/{kind}"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|e| e == "md"))
            .map(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, fs::read_to_string(path).unwrap())
            })
            .collect();
        entries.sort();
        entries
    }

    #[test]
    fn the_renderer_writes_the_gates_good_entries_and_none_of_its_bad_ones() {
        let good = fixture_entries("good");
        assert!(good.len() >= 3);
        for (name, text) in good {
            let (path, rendered) = rendered_like(&text).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(path, FIXTURE_PATH, "{name}");
            // The host writes one line per field, and only Wiffletree's source.
            let expected = text
                .replace("\n  ", " ")
                .replace("source: skill", "source: wiffletree");
            assert_eq!(rendered, expected, "{name}");
        }
        let bad = fixture_entries("bad");
        let problems: Value = serde_json::from_str(
            &fs::read_to_string(format!("{FIXTURES}/bad/problems.json")).unwrap(),
        )
        .unwrap();
        let listed: Vec<&String> = problems.as_object().unwrap().keys().collect();
        let names: Vec<&String> = bad.iter().map(|(name, _)| name).collect();
        assert_eq!(listed, names, "every bad entry is covered");
        for (name, text) in bad {
            if let Some((path, rendered)) = rendered_like(&text) {
                assert!(
                    path != FIXTURE_PATH || rendered != text,
                    "{name} rendered as is"
                );
            }
        }
    }

    #[test]
    fn ids_are_lowercase_ulids_that_sort_by_capture_time() {
        let (earlier, later) = (new_ulid(1_793_000_000_000), new_ulid(1_793_000_000_001));
        assert!(earlier < later);
        for id in [&earlier, &later] {
            assert_eq!(id.len(), 26);
            assert!(id.starts_with(|c: char| ('0'..='7').contains(&c)));
            assert!(
                id.chars()
                    .all(|c| "0123456789abcdefghjkmnpqrstvwxyz".contains(c))
            );
        }
    }

    #[test]
    fn a_tickets_entries_open_one_pr_when_it_is_accepted() {
        let mut f = fixture();
        let (_github, tools) = fetched(&mut f);
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        for title in ["One", "Two", "Three"] {
            log_work(&mut f, &implementer, args(title)).unwrap();
        }
        f.host.send_log_batches(now(), &tools.git).unwrap();
        assert!(
            f.host.brain_batches(Some(REPO)).unwrap().is_empty(),
            "open until accepted"
        );
        let ticket = f.ticket.clone();
        f.host
            .agent_tool(&coordinator, "accept_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        f.host.send_log_batches(now(), &tools.git).unwrap();
        f.host.send_log_batches(now(), &tools.git).unwrap();
        let batches = f.host.brain_batches(Some(REPO)).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        assert_eq!(
            (batch.kind, &batch.source),
            (
                BrainBatchKind::Log,
                &BrainSource::Project(f.coordinator.project_id.clone())
            )
        );
        assert_eq!(batch.title, "Log batch: Fix contrast (3 entries)");
        let logs = stored(&f);
        let mut paths: Vec<String> = logs.iter().map(|l| l.path.clone().unwrap()).collect();
        paths.sort();
        let mut files = batch.files.clone();
        files.sort();
        assert_eq!(files, paths);
        // The committed file is the rendered entry.
        let worktree = f.host.home.join("brain-batches").join(&batch.id);
        let log = &logs[0];
        assert_eq!(
            fs::read_to_string(worktree.join(log.path.as_ref().unwrap())).unwrap(),
            entry_file(log, "mhohlios").unwrap()
        );
    }

    #[test]
    fn a_quiet_coordinators_batch_opens_its_pr_after_24_hours() {
        let mut f = fixture();
        let (_github, tools) = fetched(&mut f);
        let coordinator = f.coordinator.id.clone();
        log_work(&mut f, &coordinator, args("Plan")).unwrap();
        let at = now();
        f.host
            .send_log_batches(at + BATCH_FLUSH_MS - 60_000, &tools.git)
            .unwrap();
        assert!(f.host.brain_batches(Some(REPO)).unwrap().is_empty());
        f.host
            .send_log_batches(at + BATCH_FLUSH_MS, &tools.git)
            .unwrap();
        let batches = f.host.brain_batches(Some(REPO)).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].title, "Log batch: Site coordinator (1 entry)");
        assert!(batches[0].files[0].starts_with("second-brain/logs/sagehaven-docs/"));
        // The next entry starts a new batch.
        log_work(&mut f, &coordinator, args("Later")).unwrap();
        let open: i64 = (f.host.db)
            .query_row(
                "SELECT count(*) FROM log_batches WHERE closed_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 1);
    }

    #[test]
    fn a_batch_holds_at_most_50_entries_and_send_now_or_an_archive_closes_one() {
        let mut f = fixture();
        let (_github, tools) = fetched(&mut f);
        let implementer = f.implementer.id.clone();
        for i in 0..51 {
            log_work(&mut f, &implementer, args(&format!("Entry {i}"))).unwrap();
        }
        let counts: Vec<(i64, bool)> = (f.host.db)
            .prepare("SELECT (SELECT count(*) FROM work_logs w WHERE w.batch_id=b.id), closed_at IS NOT NULL FROM log_batches b ORDER BY opened_at, rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(counts, [(50, true), (1, false)]);
        let files = |f: &Fixture| -> Vec<usize> {
            let batches = f.host.brain_batches(Some(REPO)).unwrap();
            batches.iter().map(|b| b.files.len()).collect()
        };
        f.host.send_log_batches(now(), &tools.git).unwrap();
        assert_eq!(files(&f), [50], "the full batch goes on its own");
        assert_eq!(
            f.host
                .send_logs_now(Some("sage-social/SAGEHAVEN-DOCS"))
                .unwrap(),
            1
        );
        f.host.send_log_batches(now(), &tools.git).unwrap();
        assert_eq!(files(&f), [50, 1], "Send now sends the open one");

        let coordinator = f.coordinator.id.clone();
        log_work(&mut f, &coordinator, args("Plan")).unwrap();
        f.host.set_archived(&coordinator, true).unwrap();
        let open: i64 = (f.host.db)
            .query_row(
                "SELECT count(*) FROM log_batches WHERE closed_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 0, "archiving the coordinator closes its batch");
        f.host.send_log_batches(now(), &tools.git).unwrap();
        assert_eq!(files(&f), [50, 1, 1]);
    }

    #[test]
    fn carried_over_rows_of_a_finished_ticket_go_at_once_in_one_batch() {
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        let (ticket, repository) = (f.ticket.clone(), f.ticket().repository_id);
        let open = (f.host)
            .create_ticket(&coordinator, &repository, "Still open", "Do")
            .unwrap();
        (f.host)
            .agent_tool(&coordinator, "close_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        // Unsent rows as the upgrade's carry-over leaves them.
        let project = f.coordinator.project_id.clone();
        for (id, task) in [("old-1", &ticket), ("old-2", &ticket), ("old-3", &open.id)] {
            let data = json!({"sequence": 0, "id": id, "project_id": project, "task_id": task,
                "repository": "app", "created_at": 1_793_000_000_000i64, "date": "2026-10-26",
                "reference": "—", "input": {"session_id": coordinator, "milestone": id,
                    "kind": "decision", "title": "Old", "changed": "c", "why": "w", "state": "s"}});
            (f.host.db)
                .execute(
                    "INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data) VALUES (?1,?2,?3,?1,'app',?4)",
                    params![id, project, coordinator, data.to_string()],
                )
                .unwrap();
        }
        f.host.atomically(Host::queue_unsent).unwrap();
        let batches: Vec<(Option<String>, i64, bool)> = (f.host.db)
            .prepare("SELECT ticket_id, (SELECT count(*) FROM work_logs w WHERE w.batch_id=b.id), closed_at IS NOT NULL FROM log_batches b ORDER BY ticket_id=?1 DESC")
            .unwrap()
            .query_map([&ticket], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            batches,
            [(Some(ticket), 2, true), (Some(open.id), 1, false)],
            "the finished ticket's rows close together; the open ticket's wait for it"
        );
    }

    #[test]
    fn closing_a_ticket_closes_its_batch() {
        let mut f = fixture();
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        log_work(&mut f, &implementer, args("Partial")).unwrap();
        let ticket = f.ticket.clone();
        f.host
            .agent_tool(&coordinator, "close_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        let open: i64 = (f.host.db)
            .query_row(
                "SELECT count(*) FROM log_batches WHERE ticket_id=?1 AND closed_at IS NULL",
                [&ticket],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 0);
    }

    #[test]
    fn a_hand_off_that_fails_halfway_queues_its_batch_once_on_the_next_try() {
        let mut f = fixture();
        let (_github, tools) = fetched(&mut f);
        let coordinator = f.coordinator.id.clone();
        log_work(&mut f, &coordinator, args("Plan")).unwrap();
        f.host.send_logs_now(None).unwrap();
        // Recording the hand-off fails after the brain batch was committed.
        (f.host.db)
            .execute_batch("CREATE TEMP TRIGGER refuse BEFORE UPDATE OF brain_batch_id ON log_batches BEGIN SELECT RAISE(ABORT,'database is busy'); END;")
            .unwrap();
        f.host.send_log_batches(now(), &tools.git).unwrap();
        assert!(f.host.brain_batches(Some(REPO)).unwrap().is_empty());
        let leftovers = fs::read_dir(f.host.home.join("brain-batches"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 0, "its worktree is removed with it");
        (f.host.db).execute_batch("DROP TRIGGER refuse;").unwrap();
        f.host.send_log_batches(now(), &tools.git).unwrap();
        f.host.send_log_batches(now(), &tools.git).unwrap();
        assert_eq!(f.host.brain_batches(Some(REPO)).unwrap().len(), 1);
    }

    #[test]
    fn a_coordinator_entry_for_a_ticket_takes_its_ref_and_stays_in_the_session_batch() {
        let mut f = fixture();
        let (_github, tools) = fetched(&mut f);
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        let ticket = f.ticket.clone();
        let repository = f.ticket().repository_id;
        let other = (f.host)
            .create_ticket(&coordinator, &repository, "Other", "Do")
            .unwrap();
        let mine = log_work(&mut f, &implementer, args("Work")).unwrap();
        let mut about = args("Reviewed");
        about["ticket_id"] = json!(ticket);
        let theirs = log_work(&mut f, &coordinator, about.clone()).unwrap();
        assert_eq!(theirs["ref"], mine["ref"]);
        let path = theirs["path"].as_str().unwrap();
        assert!(path.starts_with("second-brain/logs/app/"), "{path}");
        let mut elsewhere = args("Planned");
        elsewhere["ticket_id"] = json!(other.id);
        log_work(&mut f, &coordinator, elsewhere).unwrap();
        let batch_of = |f: &Fixture, id: &Value| -> String {
            let log = stored(f).into_iter().find(|l| l.id == *id).unwrap();
            log.batch_id.unwrap()
        };
        let session_batch = batch_of(&f, &theirs["id"]);
        let ticket_batch = batch_of(&f, &mine["id"]);
        assert_ne!(
            session_batch, ticket_batch,
            "the implementer's stays per ticket"
        );
        let entries: i64 = (f.host.db)
            .query_row(
                "SELECT count(*) FROM work_logs WHERE batch_id=?1",
                [&session_batch],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            entries, 2,
            "both tickets' coordinator entries in one session batch"
        );

        // After the accept removed the worktree, the accept entry still has the ticket's ref and
        // counts for it, in the same session batch.
        f.host
            .agent_tool(&coordinator, "accept_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        about["title"] = json!("Accepted");
        let after = log_work(&mut f, &coordinator, about).unwrap();
        assert_eq!(after["ref"], mine["ref"]);
        assert_eq!(batch_of(&f, &after["id"]), session_batch);
        let accepted = stored(&f).into_iter().find(|l| l.id == after["id"]);
        assert_eq!(accepted.unwrap().task_id.as_deref(), Some(ticket.as_str()));
        let sent = |f: &mut Fixture| -> Vec<usize> {
            f.host.send_log_batches(now(), &tools.git).unwrap();
            (f.host.brain_batches(Some(REPO)).unwrap().iter())
                .map(|b| b.files.len())
                .collect()
        };
        assert_eq!(sent(&mut f), [1], "the accept sends the ticket's batch");
        f.host.send_logs_now(None).unwrap();
        assert_eq!(
            sent(&mut f),
            [1, 3],
            "then the session's, with the accept entry"
        );
    }

    fn ready(f: &mut Fixture, message: &str) -> Value {
        let implementer = f.implementer.id.clone();
        let report = json!({"message_id": message, "kind": "ready_for_testing", "body": "Ready"});
        f.host.agent_tool(&implementer, "report", report).unwrap()
    }

    #[test]
    fn ready_for_testing_prompts_one_log_once_and_only_with_a_brain() {
        let mut f = fixture();
        assert_eq!(ready(&mut f, "r1")[PROMPT_KEY], PROMPT);
        assert!(
            ready(&mut f, "r2").get(PROMPT_KEY).is_none(),
            "each milestone once"
        );

        let mut f = fixture();
        let implementer = f.implementer.id.clone();
        log_work(&mut f, &implementer, args("Done")).unwrap();
        assert!(
            ready(&mut f, "r1").get(PROMPT_KEY).is_none(),
            "already logged"
        );

        let mut f = publish::tests::fixture();
        assert!(ready(&mut f, "r1").get(PROMPT_KEY).is_none(), "no brain");
    }

    #[test]
    fn accepting_and_publishing_prompt_the_caller_who_has_not_logged() {
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        let (status, _) = f.publish(&coordinator).unwrap();
        assert_eq!(status[PROMPT_KEY], PROMPT);
        // An entry for the ticket since that prompt, so accepting adds none.
        let ticket = f.ticket.clone();
        let mut published = args("Published");
        published["ticket_id"] = json!(ticket);
        log_work(&mut f, &coordinator, published).unwrap();
        let accepted = f
            .host
            .agent_tool(&coordinator, "accept_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        assert!(accepted.get(PROMPT_KEY).is_none());

        // A planning entry, or one for another ticket, says nothing about this one.
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        let ticket = f.ticket.clone();
        log_work(&mut f, &coordinator, args("Plan")).unwrap();
        let (status, _) = f.publish(&coordinator).unwrap();
        assert_eq!(status[PROMPT_KEY], PROMPT);
        let repository = f.ticket().repository_id;
        let other = f
            .host
            .create_ticket(&coordinator, &repository, "Other", "Do")
            .unwrap();
        let mut elsewhere = args("Other ticket");
        elsewhere["ticket_id"] = json!(other.id);
        log_work(&mut f, &coordinator, elsewhere).unwrap();
        let accepted = f
            .host
            .agent_tool(&coordinator, "accept_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        assert_eq!(accepted[PROMPT_KEY], PROMPT);
    }

    #[test]
    fn only_the_coordinator_and_implementers_are_offered_log_work() {
        for role in Role::ALL {
            let tools = mcp::tools_for(Some(role));
            let offered = (tools.as_array().unwrap().iter()).any(|t| t["name"] == "log_work");
            assert_eq!(
                offered,
                matches!(role, Role::ProjectOrchestrator | Role::Implementer),
                "{role:?}"
            );
        }
        let unnamed = mcp::tools_for(None);
        assert!(!(unnamed.as_array().unwrap().iter()).any(|t| t["name"] == "log_work"));
        let tools = mcp::tools();
        let log_work = (tools.as_array().unwrap().iter()).find(|t| t["name"] == "log_work");
        let schema = &log_work.unwrap()["inputSchema"];
        let properties: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        for key in ["ref", "author", "id", "source", "date"] {
            assert!(!properties.iter().any(|p| *p == key), "{key}");
        }
    }

    /// A store as the release before per-entry files left it: export receipts, maintenance
    /// runs, and `work_logs` rows in the old shape.
    fn before_entries(home: &Path, rows: &[(&str, &str, &str, bool)]) {
        let db = Connection::open(home.join(STORE_FILE)).unwrap();
        db.execute_batch(
            "DROP TABLE log_batches; DROP TABLE log_prompts; ALTER TABLE work_logs DROP COLUMN batch_id;
             CREATE TABLE log_exports (log_id TEXT NOT NULL REFERENCES work_logs(id), brain TEXT NOT NULL, PRIMARY KEY(log_id,brain));
             CREATE TABLE maintenance_runs (sequence INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL REFERENCES projects(id), data TEXT NOT NULL);
             CREATE INDEX runs_project ON maintenance_runs(project_id,sequence);",
        )
        .unwrap();
        for (id, session, reference, exported) in rows {
            let (project, space): (String, String) = db
                .query_row(
                    "SELECT s.project_id, json_extract(p.data,'$.space') FROM sessions s JOIN projects p ON p.id=s.project_id WHERE s.id=?1",
                    [session],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            let data = json!({"sequence": 0, "id": id, "project_id": project, "task_id": null,
                "repository": "project", "created_at": 1_793_000_000_000i64,
                "input": {"session_id": session, "milestone": id, "kind": "decision", "title": "Old",
                    "reference": reference, "changed": "c", "why": "w", "state": "s"}});
            db.execute(
                "INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data,space_id) VALUES (?1,?2,?3,?1,'project',?4,?5)",
                params![id, project, session, data.to_string(), space],
            )
            .unwrap();
            if *exported {
                db.execute("INSERT INTO log_exports VALUES (?1,'/brain')", [id])
                    .unwrap();
            }
        }
    }

    #[test]
    fn the_upgrade_keeps_every_row_and_queues_every_unexported_one_with_a_brain() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let sage = host.create_space("Sage", "", "", None).unwrap().space;
        let sage = name_brain(&mut host, &sage.id, REPO);
        let with_brain = host.create_project("With brain").unwrap();
        host.move_project_to_space(&with_brain.id, &sage).unwrap();
        let without = host.create_project("No brain").unwrap();
        let (one, two) = (
            host.coordinator_of(&with_brain.id).unwrap().unwrap(),
            host.coordinator_of(&without.id).unwrap().unwrap(),
        );
        drop(host);
        before_entries(
            home.path(),
            &[
                ("exported", &one, "—", true),
                ("fits", &one, "—", false),
                ("broken", &one, "main 1a2b3c4", false),
                ("no-brain", &two, "—", false),
            ],
        );
        let mut host = Host::open_set_up(home.path()).unwrap();
        let gone: bool = (host.db)
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('log_exports','maintenance_runs'))",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!gone);
        let logs = host.logs(&with_brain.id, None, 100).unwrap();
        assert_eq!(logs.len(), 3);
        let exported = logs.iter().find(|l| l.id == "exported").unwrap();
        assert!(exported.in_legacy_file && exported.batch_id.is_none());
        let queued: Vec<&WorkLog> = logs.iter().filter(|l| l.id != "exported").collect();
        for log in &queued {
            assert_eq!(log.id.len(), 26);
            assert!(log.batch_id.is_some());
            assert_eq!(log.brain.as_deref(), Some(REPO));
            let path = log.path.as_deref().unwrap();
            assert!(
                path.starts_with("second-brain/logs/sagehaven-docs/"),
                "{path}"
            );
        }
        // The row whose old ref breaks the contract is queued held, its text unchanged.
        let held: Vec<&&WorkLog> = queued.iter().filter(|l| l.held.is_some()).collect();
        assert_eq!(held.len(), 1);
        assert_eq!(
            (held[0].reference.as_str(), held[0].input.state.as_str()),
            ("main 1a2b3c4", "s")
        );
        let items = host.open_attention(&with_brain.id).unwrap();
        let item = items
            .iter()
            .find(|i| i.operation_id.contains("held-logs"))
            .unwrap();
        assert!(
            item.prompt.starts_with("1 log entry captured before"),
            "{}",
            item.prompt
        );
        let unsent = host.logs(&without.id, None, 100).unwrap();
        assert_eq!(unsent.len(), 1);
        assert!(unsent[0].is_unsent());
        assert_eq!(unsent[0].id, "no-brain");

        // Only the entry that fits is sent; the held one stays.
        host.legacy_pointer = home.path().join("no-pointer");
        host.record_gh_login(Some("mhohlios".into()));
        let folder = tempfile::tempdir().unwrap();
        let tools = GitHub::new(folder.path()).tools();
        assert!(run(&mut host, &tools));
        host.send_logs_now(None).unwrap();
        host.send_log_batches(now(), &tools.git).unwrap();
        let batches = host.brain_batches(Some(REPO)).unwrap();
        let fits = queued.iter().find(|l| l.held.is_none()).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].files, [fits.path.clone().unwrap()]);

        // Once the other project's Space names a brain, its waiting row is queued.
        let personal = host.project(&without.id).unwrap().space;
        name_brain(&mut host, &personal, REPO);
        host.send_log_batches(now(), &tools.git).unwrap();
        let queued = host.logs(&without.id, None, 100).unwrap().remove(0);
        assert!(queued.batch_id.is_some() && queued.held.is_none());
    }
}
