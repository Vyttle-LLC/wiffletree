//! Work logs (OpenSpec change `one-space-repository`): `log_work` and the human's capture form
//! store one entry each, which the host renders as one file in the entry contract and hands to
//! the repository's sync engine (`brain_sync`), so it lands straight on `main`. Rows an earlier
//! release left waiting land the same way. Nothing here logs an entry's text; errors and events
//! name entry ids only.
use super::*;
use brains::new_ulid;
use chrono::{Datelike, Local, TimeZone};
use folder::Root;

/// The cap on an entry's body.
pub(crate) const MAX_BODY_CHARS: usize = 2000;
pub(crate) const MAX_TITLE_CHARS: usize = 80;
pub(crate) const NO_REF: &str = "—";
/// The held-entries Inbox item's id after `BRAIN_NOTICE_PREFIX`, followed by how many it holds.
const HELD_ITEM: &str = "held-logs:";
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

/// Whether the brain batch `b` an earlier release left stopped before GitHub had its entries on
/// a pull request: still queued, or failed at its push.
const NEVER_OPENED: &str = "json_extract(b.data,'$.stage') IN ('queued','failed') AND json_extract(b.data,'$.pull_request') IS NULL";

/// Which agents may log: the project coordinator and implementers. Assistant threads log too,
/// in their Space's brain, through their own tools; a job's run does not.
pub(crate) fn logs_work(role: Role) -> bool {
    matches!(role, Role::ProjectOrchestrator | Role::Implementer)
}

/// The log folder of assistant threads' entries, which name no repository.
pub(crate) const ASSISTANT_FOLDER: &str = "assistant";
/// The log folder of a coordinator's entries for no repository.
pub(crate) const PLANNING_FOLDER: &str = "planning";

/// Adds the tables an earlier release wrote and the entries' batch column and, once, carries the
/// rows captured before per-entry files over: an exported row is marked as in its legacy file,
/// and the export receipts and the maintenance runs are dropped.
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
        host.db.execute_batch(legacy_logs::SCHEMA)?;
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
        Ok(())
    })
}

/// The host's local date of `at`, `YYYY-MM-DD`.
pub(crate) fn local_date(at: i64) -> String {
    let at = Local.timestamp_millis_opt(at).single().unwrap_or_default();
    format!("{:04}-{:02}-{:02}", at.year(), at.month(), at.day())
}

/// One line: whitespace runs, newlines included, become one space, and HTML comments are
/// escaped so that no entry carries the gate's `gate:allow` suppression marker.
pub(crate) fn one_line(value: &str) -> String {
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
    // Only the human's ruling, which `rule_conflict` writes, may say it is one.
    ensure!(
        !(input.state.to_lowercase()).starts_with(&brain_index::RULING_PREFIX.to_lowercase()),
        "state may not begin \"{}\": only the human rules on a conflict, in Wiffletree's Inbox",
        brain_index::RULING_PREFIX.trim_end()
    );
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
pub(crate) fn valid_folder(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A GitHub login as the entry contract has it.
pub(crate) fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && !login.starts_with('-')
        && !login.ends_with('-')
        && !login.contains("--")
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A `ref` the entry contract allows: `—`, `<branch> @ —`, or `<branch> @ <sha>[…<sha>]` with an
/// optional ` · PR #NN`, where each SHA is 7–40 lowercase hex characters.
pub(crate) fn valid_ref(reference: &str) -> bool {
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

pub(crate) fn entry_path(folder: &str, repository: &str, date: &str, id: &str) -> String {
    let (year, month) = (&date[..4], &date[5..7]);
    format!("{folder}/logs/{repository}/{year}/{month}/{date}-{id}.md")
}

/// An entry's body in the contract: its heading and the five fields.
pub(crate) fn entry_body(date: &str, input: &LogInput, reference: &str) -> String {
    let kind = match input.kind {
        LogKind::Decision => "decision",
        LogKind::Observation => "observation",
    };
    format!(
        "## {date} — {}\n- type: {kind}\n- ref: {reference}\n- changed: {}\n- why: {}\n- state: {}\n",
        input.title, input.changed, input.why, input.state
    )
}

/// The body's length as the cap counts it.
pub(crate) fn body_chars(body: &str) -> usize {
    body.trim().chars().count()
}

/// The entry file: the four frontmatter keys, then `body`.
pub(crate) fn entry_text(
    id: &str,
    repository: &str,
    author: &str,
    source: &str,
    body: &str,
) -> String {
    format!("---\nid: {id}\nrepo: {repository}\nauthor: {author}\nsource: {source}\n---\n{body}")
}

/// The entry's file in the contract, refused when its body is over the cap.
fn entry_file(log: &WorkLog, author: &str) -> Result<String> {
    let body = entry_body(&log.date, &log.input, &log.reference);
    let length = body_chars(&body);
    ensure!(
        length <= MAX_BODY_CHARS,
        "The entry is {length} characters and the cap is {MAX_BODY_CHARS}: cut at least {} characters. Nothing was stored.",
        length - MAX_BODY_CHARS
    );
    Ok(entry_text(
        &log.id,
        &log.repository,
        author,
        "wiffletree",
        &body,
    ))
}

/// Why `text` breaks the entry contract as the file `path`, relative to the brain's `logs/`, or
/// `None` when it keeps to it. Names the rule and never the text, which may be sensitive.
pub(crate) fn contract_problem(path: &str, text: &str) -> Option<String> {
    let bad_path = || Some("its path is not <repo>/YYYY/MM/YYYY-MM-DD-<id>.md".to_owned());
    let [repo, year, month, name] = path.split('/').collect::<Vec<_>>()[..] else {
        return bad_path();
    };
    let dated_id = name
        .strip_suffix(".md")
        .and_then(|stem| Some((stem.get(..10)?, stem.get(11..)?)));
    let Some((date, id)) = dated_id else {
        return bad_path();
    };
    let dated = date.starts_with(&format!("{year}-{month}-"))
        && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok();
    if !valid_folder(repo) || !dated || !valid_ulid(id) {
        return bad_path();
    }
    let Some((frontmatter, body)) =
        (text.strip_prefix("---\n")).and_then(|t| t.split_once("\n---\n"))
    else {
        return Some("it has no frontmatter closed by ---".into());
    };
    let mut keys = vec![];
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(": ") else {
            return Some("a frontmatter line is not key: value".into());
        };
        keys.push((key, value.trim()));
    }
    let names: Vec<&str> = keys.iter().map(|(key, _)| *key).collect();
    if names != ["id", "repo", "author", "source"] {
        return Some("its frontmatter is not exactly id, repo, author and source".into());
    }
    if keys[0].1 != id || keys[1].1 != repo {
        return Some("its frontmatter does not match its file name".into());
    }
    if !valid_login(keys[2].1) || !["wiffletree", "skill"].contains(&keys[3].1) {
        return Some("its author or source is not one the contract allows".into());
    }
    if body_chars(body) > MAX_BODY_CHARS {
        return Some(format!("it is over the {MAX_BODY_CHARS}-character cap"));
    }
    body_problem(date, body)
}

/// Whether `id` is a lowercase ULID.
fn valid_ulid(id: &str) -> bool {
    id.len() == 26
        && id.starts_with(|c: char| ('0'..='7').contains(&c))
        && (id.chars()).all(|c| {
            c.is_ascii_digit() || ("a"..="z").contains(&&*c.to_string()) && !"ilou".contains(c)
        })
}

/// Why an entry's body breaks the contract: its heading and exactly the five fields, each with a
/// value and optionally continued on indented lines.
fn body_problem(date: &str, body: &str) -> Option<String> {
    let mut lines = body.lines().filter(|line| !line.trim().is_empty());
    let heading = lines.next().and_then(|line| line.strip_prefix("## "));
    let Some((dated, title)) = heading.and_then(|line| line.split_once(" — ")) else {
        return Some("its first line is not ## YYYY-MM-DD — Title".into());
    };
    if dated != date || title.trim().is_empty() {
        return Some("its heading's date or title does not fit".into());
    }
    let mut fields: Vec<(&str, &str)> = vec![];
    for line in lines {
        match line
            .strip_prefix("- ")
            .and_then(|field| field.split_once(": "))
        {
            Some((name, value)) if !value.trim().is_empty() => fields.push((name, value.trim())),
            _ if line.starts_with(char::is_whitespace) && !fields.is_empty() => {}
            _ => return Some("a line is not a - field: value line or its continuation".into()),
        }
    }
    let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
    if names != ["type", "ref", "changed", "why", "state"] {
        return Some("its fields are not exactly type, ref, changed, why and state".into());
    }
    if !["decision", "observation"].contains(&fields[0].1) {
        return Some("its type is neither decision nor observation".into());
    }
    (!valid_ref(fields[1].1)).then(|| "its ref is not one the contract allows".into())
}

/// This Mac's account name, as a local repository's entries name their author with no `gh`
/// login: the characters a folder name allows.
fn account_name() -> String {
    let account = (std::env::var("USER").or_else(|_| std::env::var("LOGNAME"))).unwrap_or_default();
    let name: String = (account.chars())
        .map(
            |c| match c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                true => c,
                false => '-',
            },
        )
        .collect();
    match name.is_empty() {
        true => "local".into(),
        false => name,
    }
}

/// The name a workspace repository's entries are logged under: its GitHub name, or its
/// workspace name.
pub(crate) fn repository_log_name(repository: &Repository) -> String {
    (repository.github.as_ref())
        .map(|github| github.name.clone())
        .unwrap_or_else(|| repository.name.clone())
}

/// The entry's folder for a workspace repository, refused when the contract forbids its name.
fn repository_folder(repository: &Repository) -> Result<String> {
    let name = repository_log_name(repository);
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
    /// computes `ref`, takes the author from gh's login or, in a local repository, this Mac's
    /// account, and records the provider, model and machine. A milestone repeated with the same
    /// content returns its first entry. Refused, storing nothing, for a GitHub repository with
    /// no login, or without room under the cap.
    fn capture(&mut self, session: &Session, input: LogInput) -> Result<WorkLog> {
        ensure!(
            logs_work(session.role) || session.role == Role::Assistant,
            "log_work is for the project coordinator and implementers, and for assistant threads"
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
        // A thread logs in its own Space's brain, for no project, repository or ticket.
        let (project, space_id) = match session.role {
            Role::Assistant => (None, self.thread(&session.id)?.space_id),
            _ => {
                let project = self.project(&session.project_id)?;
                (Some(project.clone()), project.space)
            }
        };
        let space = self.space(&space_id)?.space;
        let author = self.entry_author(&space.storage.repository)?;
        let project_id = project.as_ref().map(|p| p.id.clone()).unwrap_or_default();
        let ticket = match session.role {
            Role::Assistant => {
                ensure!(
                    input.repository.is_none() && input.ticket_id.is_none(),
                    "A thread's entry names no repository or ticket; leave them out"
                );
                None
            }
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
                                .is_ok_and(|c| c.project_id == project_id)
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
            None if project.is_none() => (None, ASSISTANT_FOLDER.to_owned(), NO_REF.to_owned()),
            None => match &input.repository {
                Some(named) => {
                    let repositories = self.project_repositories(&project_id)?;
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
                None => (None, PLANNING_FOLDER.to_owned(), NO_REF.to_owned()),
            },
        };
        let folder = brain_sync::brain_dir(&Root::new(&space.storage.root));
        let at = now();
        let id = new_ulid(at);
        let date = local_date(at);
        let mut log = WorkLog {
            sequence: 0,
            path: Some(entry_path(&folder, &repository, &date, &id)),
            id,
            project_id: project_id.clone(),
            task_id,
            repository,
            created_at: at,
            date,
            input,
            reference,
            space_id: Some(space_id),
            brain: Some(space.storage.repository.key()),
            batch_id: None,
            written: true,
            author: Some(author.clone()),
            provider: Some(session.provider),
            model: (self.session_runtime(&session.id)?.profile).map(|p| p.model),
            machine: Some(sync::machine_name()),
            in_legacy_file: false,
            held: None,
            released: false,
        };
        let text = entry_file(&log, &author)?;
        self.atomically(|host| {
            host.store_log(&mut log)?;
            Self::event(
                &host.db,
                &log.project_id,
                Some(&log.input.session_id),
                "work_log",
                &log.id,
            )?;
            let path = log.path.clone().context("A captured entry has a path")?;
            let project = project.as_ref().map(|project| project.id.as_str());
            host.land_entry(&space, &path, &text, Some((&log.id, project)))
        })?;
        Ok(log)
    }

    /// Who an entry in `repository` is by: gh's login, or with none, in a local repository, this
    /// Mac's account name.
    pub(crate) fn entry_author(&self, repository: &StorageRepository) -> Result<String> {
        let login = self.gh_login.clone().filter(|login| valid_login(login));
        match (login, repository) {
            (Some(login), _) => Ok(login),
            (None, StorageRepository::Local { .. }) => Ok(account_name()),
            (None, StorageRepository::GitHub { .. }) => bail!(
                "Wiffletree has not read your GitHub login, so nothing was stored. Sign gh in (gh auth login), then Sync now on the Brains page."
            ),
        }
    }

    /// Inserts the entry; one without a milestone takes its id as its milestone column, which
    /// is unique per session.
    fn store_log(&mut self, log: &mut WorkLog) -> Result<()> {
        self.db.execute(
            "INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data,space_id) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                log.id,
                assistant::project_column(&log.project_id),
                log.input.session_id,
                log.input.milestone.as_deref().unwrap_or(&log.id),
                log.repository,
                encode(&*log)?,
                log.space_id.as_ref().map(SpaceId::as_str),
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

    /// The prompt for one `log_work` call at `milestone` of the ticket, or `None`: when this
    /// milestone was prompted already, or when the session has logged an entry for the ticket
    /// since its previous prompted milestone of it.
    pub(crate) fn milestone_prompt(
        &mut self,
        session: &Session,
        ticket: &Ticket,
        milestone: &str,
    ) -> Result<Option<&'static str>> {
        if !logs_work(session.role) {
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
        Ok(self.db.prepare("SELECT sequence,data FROM work_logs WHERE project_id=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3")?.query_map(params![project, before.unwrap_or(i64::MAX), page_limit(limit)?], read_log)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The `logs` command: a page of the project's entries, newest first. The first page also
    /// ends with every held entry older than it, so the human can always fix or release one.
    pub fn logs_page(
        &self,
        project: &str,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Vec<WorkLog>> {
        let mut logs = self.logs(project, before, limit)?;
        if before.is_none()
            && let Some(oldest) = logs.last().map(|log| log.sequence)
        {
            let held = self.db.prepare("SELECT sequence,data FROM work_logs WHERE project_id=?1 AND sequence<?2 AND json_extract(data,'$.held') IS NOT NULL ORDER BY sequence DESC")?.query_map(params![project, oldest], read_log)?.collect::<rusqlite::Result<Vec<_>>>()?;
            logs.extend(held);
        }
        Ok(logs)
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

    /// `land_carried_over`, until a call finds nothing left.
    pub(crate) fn land_what_waits(&mut self) -> Result<()> {
        if !self.carried_over {
            self.carried_over = self.land_carried_over()?;
        }
        Ok(())
    }

    /// Lands what an earlier release left waiting on this Mac and nothing else: each row never
    /// exported whose batch never reached GitHub, and each such log batch's other files, such as
    /// a ruling, in the folder of a Space naming its brain. A row that breaks the entry
    /// contract or the cap as it stands is held, its text unchanged, and is never sent; the
    /// project's coordinator gets one Inbox item saying how many it holds. A row in a batch that
    /// opened a pull request or merged stays there, and one the human discarded stays discarded. `true` when nothing is left for a later call, which a row with
    /// no author until gh's login is read leaves.
    fn land_carried_over(&mut self) -> Result<bool> {
        let rows: Vec<(String, WorkLog)> = (self.db.prepare(
            "SELECT id,data FROM work_logs WHERE json_extract(data,'$.written') IS NOT 1 AND json_extract(data,'$.in_legacy_file') IS NOT 1 AND json_extract(data,'$.released') IS NOT 1 AND json_extract(data,'$.held') IS NULL ORDER BY sequence",
        )?)
        .query_map([], |r| Ok((r.get(0)?, decode(r, 1)?)))?
        .collect::<rusqlite::Result<_>>()?;
        let mut done = true;
        let mut newly_held = std::collections::BTreeSet::new();
        for (id, mut entry) in rows {
            if self.batch_settled(&entry)? {
                entry.written = true;
                self.db.execute(
                    "UPDATE work_logs SET data=?2 WHERE id=?1",
                    params![id, encode(&entry)?],
                )?;
                continue;
            }
            let space = match self.project(&entry.project_id) {
                Ok(project) => Some(project.space),
                Err(_) => entry.space_id.clone(),
            };
            let Some(Ok(space)) = space.map(|space| self.space(&space)) else {
                continue;
            };
            let author = entry
                .author
                .clone()
                .map_or_else(|| self.entry_author(&space.space.storage.repository), Ok);
            let Ok(author) = author else {
                done = false;
                continue;
            };
            // What cannot be written now is tried again at the next start.
            if let Err(error) = self.land_row(&id, &mut entry, &space.space, &author) {
                eprintln!("Carrying log entry {id} over: {error:#}");
            }
            if entry.held.is_some() {
                newly_held.insert(entry.project_id.clone());
            }
        }
        for project in &newly_held {
            self.raise_held_item(project)?;
        }
        self.land_queued_batches()?;
        Ok(done)
    }

    /// Whether the batch an earlier release put `entry` in is past landing here: it opened a pull
    /// request, or it merged or was discarded, or forgetting its brain deleted it after it was
    /// handed over. One that stopped before any pull request still waits.
    fn batch_settled(&self, entry: &WorkLog) -> Result<bool> {
        let settled: Option<bool> = self
            .db
            .query_row(
                &format!("SELECT l.brain_batch_id IS NOT NULL AND (b.id IS NULL OR NOT ({NEVER_OPENED})) FROM log_batches l LEFT JOIN brain_batches b ON b.id=l.brain_batch_id WHERE l.id=?1"),
                [&entry.batch_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(settled.unwrap_or(false))
    }

    /// Writes an unsent row to its Space's folder, giving it an entry id, its brain and its
    /// path, or holds it when it breaks the contract as it stands.
    fn land_row(
        &mut self,
        id: &str,
        entry: &mut WorkLog,
        space: &Space,
        author: &str,
    ) -> Result<()> {
        // The coordinator's rows were logged for the project as a whole.
        if entry.repository == "project" {
            entry.repository = PLANNING_FOLDER.into();
        }
        if entry.path.is_none() {
            entry.id = new_ulid(entry.created_at);
        }
        let folder = brain_sync::brain_dir(&Root::new(&space.storage.root));
        let path = entry_path(&folder, &entry.repository, &entry.date, &entry.id);
        entry.path = Some(path.clone());
        entry.space_id = Some(space.id.clone());
        entry.brain = Some(space.storage.repository.key());
        entry.held = unfit(entry).map(str::to_owned);
        entry.written = entry.held.is_none();
        self.atomically(|host| {
            host.db.execute(
                "UPDATE work_logs SET id=?2,data=?3,repository=?4,space_id=?5 WHERE id=?1",
                params![
                    id,
                    entry.id,
                    encode(&*entry)?,
                    entry.repository,
                    space.id.as_str()
                ],
            )?;
            if !entry.written {
                return Ok(());
            }
            let text = entry_file(entry, author)?;
            let project = (!entry.project_id.is_empty()).then_some(entry.project_id.as_str());
            host.land_entry(space, &path, &text, Some((&entry.id, project)))
        })
    }

    /// Lands the files of each log batch that never opened a pull request and no row wrote, as they stand in its worktree, in
    /// the folder of a Space naming its brain. Nothing marks a batch done, as its rows stay
    /// unwritten: a file already in the folder, or discarded here, is left.
    fn land_queued_batches(&mut self) -> Result<()> {
        let batches: Vec<(String, String, Value)> = (self.db.prepare(&format!(
            "SELECT b.id,b.repository,b.data FROM brain_batches b WHERE {NEVER_OPENED} AND json_extract(b.data,'$.kind')='log'",
        ))?)
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, decode(r, 2)?)))?
        .collect::<rusqlite::Result<_>>()?;
        for (id, repository, batch) in batches {
            let names_it = |space: &Space| {
                matches!(
                    space.brain.as_ref().map(|brain| &brain.repository),
                    Some(BrainRepository::GitHub { owner, name, .. })
                        if format!("{owner}/{name}").eq_ignore_ascii_case(&repository)
                )
            };
            let Some(record) = self.spaces()?.into_iter().find(|r| names_it(&r.space)) else {
                continue;
            };
            let space = record.space;
            let clone = self.storage_folder(&space.storage.repository);
            let brain = brain_sync::brain_dir(&Root::new(&space.storage.root));
            let worktree = self.home.join("brain-batches").join(&id);
            let files = (batch["files"].as_array().into_iter().flatten()).filter_map(Value::as_str);
            for file in files.map(str::to_owned).collect::<Vec<_>>() {
                let Some((_, in_logs)) = file.split_once("logs/") else {
                    continue;
                };
                let target = format!("{brain}/logs/{in_logs}");
                let key = space.storage.repository.key();
                if clone.join(&target).exists() || self.entry_discarded(&key, &target)? {
                    continue;
                }
                match fs::read_to_string(worktree.join(&file)) {
                    Ok(text) => self.land_entry(&space, &target, &text, None)?,
                    Err(error) => eprintln!("Batch {id}: {file} cannot be read: {error}"),
                }
            }
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
        let operation = format!("{BRAIN_NOTICE_PREFIX}{HELD_ITEM}{held}");
        self.request_attention(&coordinator, "local", &operation, &prompt, &[])?;
        Ok(())
    }

    fn held_log(&self, id: &str) -> Result<WorkLog> {
        let log = (self.db)
            .query_row(
                "SELECT sequence,data FROM work_logs WHERE id=?1",
                [id],
                |r| {
                    let mut log: WorkLog = decode(r, 1)?;
                    log.sequence = r.get(0)?;
                    Ok(log)
                },
            )
            .optional()?
            .context("Log entry not found")?;
        ensure!(log.held.is_some(), "This log entry is not held");
        Ok(log)
    }

    /// Fix: the held entry takes the human's text, folder and `ref`. Refused, changing nothing,
    /// while it still breaks the entry contract or the cap. Once it fits it lands.
    pub fn fix_held_log(&mut self, id: &str, fix: LogFix) -> Result<WorkLog> {
        let mut log = self.held_log(id)?;
        log.input = folded(LogInput {
            kind: fix.kind,
            title: fix.title,
            changed: fix.changed,
            why: fix.why,
            state: fix.state,
            ..log.input
        })?;
        log.repository = fix.repository.trim().to_owned();
        log.reference = fix.reference.trim().to_owned();
        if let Some(rule) = unfit(&log) {
            bail!("This entry still cannot be sent: {rule}. Nothing was changed.");
        }
        let space = (log.space_id.clone()).context("A held entry has a Space")?;
        let space = self.space(&space)?.space;
        // From the Space's folder as it is now, which a move may have changed since it was held.
        let folder = brain_sync::brain_dir(&Root::new(&space.storage.root));
        let path = entry_path(&folder, &log.repository, &log.date, &log.id);
        log.path = Some(path.clone());
        log.brain = Some(space.storage.repository.key());
        log.held = None;
        log.written = true;
        let author = match log.author.clone() {
            Some(author) => author,
            None => self.entry_author(&space.storage.repository)?,
        };
        let text = entry_file(&log, &author)?;
        self.atomically(|host| {
            host.db.execute(
                "UPDATE work_logs SET data=?2,repository=?3 WHERE id=?1",
                params![log.id, encode(&log)?, log.repository],
            )?;
            let project = (!log.project_id.is_empty()).then_some(log.project_id.as_str());
            host.land_entry(&space, &path, &text, Some((&log.id, project)))?;
            host.clear_held_item(&log.project_id)
        })?;
        Ok(log)
    }

    /// Release: the held entry stays in the project's memory and is never sent.
    pub fn release_held_log(&mut self, id: &str) -> Result<WorkLog> {
        let mut log = self.held_log(id)?;
        log.held = None;
        log.released = true;
        self.atomically(|host| {
            host.db.execute(
                "UPDATE work_logs SET data=?2 WHERE id=?1",
                params![log.id, encode(&log)?],
            )?;
            host.clear_held_item(&log.project_id)
        })?;
        Ok(log)
    }

    /// Clears the project's held-entries Inbox item once none of its entries is held.
    fn clear_held_item(&mut self, project: &str) -> Result<()> {
        let held: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM work_logs WHERE project_id=?1 AND json_extract(data,'$.held') IS NOT NULL)",
            [project],
            |r| r.get(0),
        )?;
        if held {
            return Ok(());
        }
        let open: Vec<String> = self
            .db
            .prepare(
                "SELECT id FROM attention WHERE project_id=?1 AND operation_id LIKE ?2 AND json_extract(data,'$.answer') IS NULL",
            )?
            .query_map(
                params![project, format!("{BRAIN_NOTICE_PREFIX}{HELD_ITEM}%")],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        for id in open {
            self.resolve_attention(&id, "Cleared: no log entry is held")?;
        }
        Ok(())
    }
}

/// A `work_logs` row read as `sequence,data`.
fn read_log(r: &rusqlite::Row) -> rusqlite::Result<WorkLog> {
    let mut log: WorkLog = decode(r, 1)?;
    log.sequence = r.get(0)?;
    Ok(log)
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
    use crate::brain_sync::tests::{self, FOLDER, REPO};
    use crate::publish::tests::Fixture;
    use crate::test_support::git;

    /// The contract's entry fixtures, read where `gate/` keeps them.
    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../gate/tests/fixtures/entries"
    );
    /// Where each fixture entry is checked (`gate/tests/fixtures/README.md`).
    const FIXTURE_PATH: &str =
        "second-brain/logs/sms-service/2026/10/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md";

    /// The publish fixture's project, in a Sage Space on this Mac, with gh signed in: a ticket in
    /// `app` with two commits since its base and a passed review.
    fn fixture() -> Fixture {
        let mut f = publish::tests::fixture();
        let sage = f.host.create_space("Sage", "", "", None).unwrap().space;
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &sage.id))
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
            "wiffletree/second-brain/logs/app/{}/{}/{date}-{id}.md",
            &date[..4],
            &date[5..7]
        );
        assert_eq!(logged["path"], path);
        assert_eq!(logged["id"], id);
        let space = f.host.project(&log.project_id).unwrap().space;
        let repository = f.host.space(&space).unwrap().space.storage.repository.key();
        assert_eq!(
            (&log.brain, log.author.as_deref(), log.provider),
            (&Some(repository), Some("mhohlios"), Some(Provider::Claude))
        );
        assert_eq!(log.task_id.as_deref(), Some(f.ticket.as_str()));
        assert_eq!(log.machine, Some(sync::machine_name()));
        assert_eq!(log.space_id, Some(space));
        assert!(log.written && !log.is_unsent());
    }

    #[test]
    fn a_capture_is_refused_and_nothing_stored_without_room_or_a_place() {
        let mut f = fixture();
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
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
            .set_added_repositories(&other.id, vec![repository.clone()])
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
                .starts_with("wiffletree/second-brain/logs/planning/")
        );
        let mut named = args("Repo");
        named["repository"] = json!("allowed-org/app");
        let named = log_work(&mut f, &coordinator, named).unwrap();
        assert!(
            named["path"]
                .as_str()
                .unwrap()
                .starts_with("wiffletree/second-brain/logs/app/")
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
            written: false,
            author: None,
            provider: None,
            model: None,
            machine: None,
            in_legacy_file: false,
            held: None,
            released: false,
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
    fn the_contract_passes_every_good_fixture_and_refuses_every_bad_one() {
        let in_logs = FIXTURE_PATH.strip_prefix("second-brain/logs/").unwrap();
        for (name, text) in fixture_entries("good") {
            assert_eq!(contract_problem(in_logs, &text), None, "{name}");
        }
        for (name, text) in fixture_entries("bad") {
            assert!(contract_problem(in_logs, &text).is_some(), "{name}");
        }
        let (good, text) = fixture_entries("good").remove(0);
        for path in [
            "sms-service/2026/10/not-an-entry.md",
            "sms-service/2026/11/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md",
            "sms-service/2026/02/2026-02-30-01jbx4m2k8q6v9z3n5r7t0w2yb.md",
            "-sms/2026/10/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md",
            "sms-service/2026/10/2026-10-29-01JBX4M2K8Q6V9Z3N5R7T0W2YB.md",
            "2026/10/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md",
        ] {
            assert!(contract_problem(path, &text).is_some(), "{good}: {path}");
        }
    }

    #[test]
    fn the_cap_is_part_of_the_contract_and_a_problem_never_quotes_the_entry() {
        let in_logs = FIXTURE_PATH.strip_prefix("second-brain/logs/").unwrap();
        let (_, text) = fixture_entries("good").remove(0);
        let body = text.split_once("\n---\n").unwrap().1.trim().chars().count();
        let room = "x".repeat(MAX_BODY_CHARS - body);
        let line = "- state: ";
        let fits = text.replace(line, &format!("{line}{room}"));
        assert_eq!(contract_problem(in_logs, &fits), None);
        let over = text.replace(line, &format!("{line}{room}x"));
        assert!(contract_problem(in_logs, &over).unwrap().contains("cap"));
        for malformed in [
            "app/2026/10/2026-10-01-x.txt",
            "app/2026/10/short.md",
            "app/2026/10/.md",
        ] {
            assert!(contract_problem(malformed, &fits).unwrap().contains("path"));
        }
        let leaks = text.replace("- type: decision", &format!("- type: {}", tests::PHONE));
        let problem = contract_problem(in_logs, &leaks).unwrap();
        assert!(!problem.contains("555-0134"), "{problem}");
    }

    #[test]
    fn the_renderer_writes_the_good_fixture_entries_and_none_of_the_bad_ones() {
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
    fn a_threads_entry_is_the_assistants_in_its_spaces_brain_and_outlives_its_thread() {
        let mut f = fixture();
        let sage = f.host.project(&f.coordinator.project_id).unwrap().space;
        let thread = f
            .host
            .start_thread("t1", &sage, "Remember", &[], None)
            .unwrap();
        let session = f.host.session(&thread.id).unwrap();
        for (key, value) in [("repository", "app"), ("ticket_id", f.ticket.as_str())] {
            let mut named = args("Retry once");
            named[key] = json!(value);
            let refused = f
                .host
                .thread_tool(&session, "log_work", &named)
                .unwrap_err();
            assert!(
                refused
                    .to_string()
                    .contains("names no repository or ticket"),
                "{key}"
            );
        }
        let logged = (f
            .host
            .thread_tool(&session, "log_work", &args("Retry once")))
        .unwrap();
        let path = logged["path"].as_str().unwrap();
        let wiki = "wiffletree/second-brain/logs/assistant/";
        assert!(path.starts_with(wiki), "{path}");
        assert_eq!(logged["ref"], NO_REF);
        let log: WorkLog = (f.host.db)
            .query_row(
                "SELECT data FROM work_logs WHERE id=?1",
                [logged["id"].as_str()],
                |r| decode(r, 0),
            )
            .unwrap();
        assert_eq!(
            (log.project_id.as_str(), log.space_id.as_ref()),
            ("", Some(&sage))
        );
        // The file follows the entry contract its fixtures hold.
        let text = entry_file(&log, "mhohlios").unwrap();
        let (rendered_path, rendered) = rendered_like(&text).unwrap();
        assert_eq!(rendered, text);
        assert!(rendered_path.starts_with("second-brain/logs/assistant/"));
        assert!(text.contains("repo: assistant\n") && text.contains("- ref: —\n"));
        let repository = f.host.space(&sage).unwrap().space.storage.repository;
        let clone = f.host.storage_folder(&repository);
        f.host.delete_thread(&thread.id).unwrap();
        assert!(clone.join(path).is_file(), "the entry outlives its thread");
        assert!(f.host.thread(&thread.id).is_err());
    }

    #[test]
    fn a_coordinator_entry_for_a_ticket_takes_its_ref_even_after_the_accept() {
        let mut f = fixture();
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        let ticket = f.ticket.clone();
        let mine = log_work(&mut f, &implementer, args("Work")).unwrap();
        let mut about = args("Reviewed");
        about["ticket_id"] = json!(ticket);
        let theirs = log_work(&mut f, &coordinator, about.clone()).unwrap();
        assert_eq!(theirs["ref"], mine["ref"]);
        let path = theirs["path"].as_str().unwrap();
        assert!(
            path.starts_with("wiffletree/second-brain/logs/app/"),
            "{path}"
        );
        // After the accept removed the worktree, the accept entry still has the ticket's ref and
        // counts for it.
        f.host
            .agent_tool(&coordinator, "accept_ticket", json!({"ticket_id": ticket}))
            .unwrap();
        about["title"] = json!("Accepted");
        let after = log_work(&mut f, &coordinator, about).unwrap();
        assert_eq!(after["ref"], mine["ref"]);
        let accepted = stored(&f).into_iter().find(|l| l.id == after["id"]);
        assert_eq!(accepted.unwrap().task_id.as_deref(), Some(ticket.as_str()));
    }

    fn ready(f: &mut Fixture, message: &str) -> Value {
        let implementer = f.implementer.id.clone();
        let report = json!({"message_id": message, "kind": "ready_for_testing", "body": "Ready"});
        f.host.agent_tool(&implementer, "report", report).unwrap()
    }

    #[test]
    fn ready_for_testing_prompts_one_log_once_in_every_space() {
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
        assert_eq!(
            ready(&mut f, "r1")[PROMPT_KEY],
            PROMPT,
            "any Space's project"
        );
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
    fn the_upgrade_keeps_every_row_lands_every_unexported_one_and_holds_what_breaks_the_contract() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let sage = host.create_space("Sage", "", "", None).unwrap().space;
        let project = host.create_project("With rows").unwrap();
        host.move_project_to_space(&project.id, &sage.id).unwrap();
        let one = host.coordinator_of(&project.id).unwrap().unwrap();
        drop(host);
        before_entries(
            home.path(),
            &[
                ("exported", &one, "—", true),
                ("fits", &one, "—", false),
                ("broken", &one, "main 1a2b3c4", false),
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
        host.record_gh_login(Some("mhohlios".into()));
        host.land_what_waits().unwrap();
        let logs = host.logs(&project.id, None, 100).unwrap();
        assert_eq!(logs.len(), 3, "every row stays");
        let exported = logs.iter().find(|l| l.id == "exported").unwrap();
        assert!(exported.in_legacy_file && exported.path.is_none());
        let clone = host.storage_folder(&sage.storage.repository);
        let held: Vec<&WorkLog> = logs.iter().filter(|l| l.held.is_some()).collect();
        assert_eq!(held.len(), 1);
        assert_eq!(
            (held[0].reference.as_str(), held[0].input.state.as_str()),
            ("main 1a2b3c4", "s")
        );
        assert!(!clone.join(held[0].path.as_ref().unwrap()).exists());
        let fits = (logs.iter())
            .find(|l| !l.in_legacy_file && l.held.is_none())
            .unwrap();
        assert_eq!(fits.id.len(), 26);
        let path = fits.path.as_deref().unwrap();
        assert!(
            path.starts_with("wiffletree/second-brain/logs/planning/"),
            "{path}"
        );
        assert!(clone.join(path).is_file() && !fits.is_unsent());
        let items = host.open_attention(&project.id).unwrap();
        let item = items
            .iter()
            .find(|i| i.operation_id.contains("held-logs"))
            .unwrap();
        assert!(
            item.prompt.starts_with("1 log entry captured before"),
            "{}",
            item.prompt
        );
        // A second start lands nothing more.
        let commits = git(&clone, &["rev-list", "--count", "HEAD"]);
        drop(host);
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.record_gh_login(Some("mhohlios".into()));
        host.land_what_waits().unwrap();
        assert_eq!(git(&clone, &["rev-list", "--count", "HEAD"]), commits);
    }

    #[test]
    fn the_upgrade_keeps_every_earlier_table_and_changes_no_user_version() {
        let home = tempfile::tempdir().unwrap();
        drop(Host::open_set_up(home.path()).unwrap());
        let db = Connection::open(home.path().join(STORE_FILE)).unwrap();
        let version = |db: &Connection| -> i64 {
            db.query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap()
        };
        let before = version(&db);
        assert!(before <= 10, "an earlier build refuses anything newer");
        drop(db);
        drop(Host::open_set_up(home.path()).unwrap());
        let db = Connection::open(home.path().join(STORE_FILE)).unwrap();
        assert_eq!(version(&db), before);
        for table in [
            "brains",
            "brain_batches",
            "brain_batch_checks",
            "log_batches",
            "legacy_imports",
            "legacy_compiled",
            "ruling_triggers",
            "entry_files",
        ] {
            let held: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(held, "{table}");
        }
    }

    #[test]
    fn a_store_from_before_held_files_named_their_line_gains_the_column() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE sync_held (repository TEXT NOT NULL, path TEXT NOT NULL, check_name TEXT NOT NULL, rule TEXT, reason TEXT, PRIMARY KEY(repository,path));",
        )
        .unwrap();
        brain_sync::migrate(&db).unwrap();
        brain_sync::migrate(&db).unwrap();
        db.execute(
            "INSERT INTO sync_held(repository,path,check_name,line) VALUES ('a','b','c',4)",
            [],
        )
        .unwrap();
    }

    /// Puts a row an earlier release captured in a log batch, whose brain batch stands at `stage`
    /// with `pull_request`, for the brain `brain`.
    fn in_a_batch(
        s: &crate::brain_sync::tests::Stored,
        id: &str,
        stage: &str,
        pull_request: Option<u64>,
    ) {
        let db = &s.f.host.db;
        let coordinator = &s.f.coordinator;
        let (project, space) = (&coordinator.project_id, s.sage.id.as_str());
        let data = json!({"sequence": 0, "id": id, "project_id": project, "task_id": null,
            "repository": "app", "created_at": 1_793_000_000_000i64, "date": "2026-10-20",
            "reference": "—", "author": "mhohlios", "batch_id": format!("logs-{id}"),
            "path": format!("second-brain/logs/app/2026/10/2026-10-20-{id}.md"),
            "input": {"session_id": coordinator.id, "kind": "decision", "title": "Queued",
                "changed": "c", "why": "w", "state": "s"}});
        db.execute(
            "INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data,space_id) VALUES (?1,?2,?3,?1,'app',?4,?5)",
            params![id, project, coordinator.id, data.to_string(), space],
        )
        .unwrap();
        db.execute(
            "INSERT INTO log_batches(id,brain,project_id,opened_at,closed_at,brain_batch_id) VALUES (?1,'Sage-Social/sagehaven-docs',?2,1,2,?3)",
            params![format!("logs-{id}"), project, format!("batch-{id}")],
        )
        .unwrap();
        let batch = json!({"id": format!("batch-{id}"), "repository": "Sage-Social/sagehaven-docs",
            "kind": "log", "source": {"kind": "project", "id": project}, "title": "Log batch",
            "files": [format!("second-brain/logs/app/2026/10/2026-10-20-{id}.md")],
            "stage": stage, "created_at": 1, "updated_at": 1,
            "pull_request": pull_request.map(|number| json!({"number": number, "url": "u"}))});
        db.execute(
            "INSERT INTO brain_batches(id,repository,data) VALUES (?1,'Sage-Social/sagehaven-docs',?2)",
            params![format!("batch-{id}"), batch.to_string()],
        )
        .unwrap();
    }

    /// A Space naming the separate brain `Sage-Social/sagehaven-docs`.
    const NAMES_A_BRAIN: &str = "brain:\n  repository:\n    kind: git_hub\n    owner: Sage-Social\n    name: sagehaven-docs\n  folder: second-brain\n";

    #[test]
    fn a_space_with_a_separate_brain_keeps_its_key_logs_to_its_folder_and_never_writes_the_old_brain()
     {
        let mut s = tests::stored_with(NAMES_A_BRAIN);
        let space_file = format!("{FOLDER}/spaces/sage/space.yaml");
        let before = s.github.show(&space_file).unwrap();
        let implementer = s.f.implementer.id.clone();
        let logged = log_work(&mut s.f, &implementer, args("Mine")).unwrap();
        s.sync();
        let path = logged["path"].as_str().unwrap();
        assert!(
            path.starts_with("wiffletree/second-brain/logs/app/"),
            "{path}"
        );
        assert!(s.github.show(path).is_some());
        assert_eq!(s.github.show(&space_file).unwrap(), before, "the key stays");
        // Saving the Space keeps the key, as no screen edits it.
        let mut edited = s.f.host.space(&s.sage.id).unwrap().space;
        edited.description = "Changed here".into();
        edited.brain = None;
        s.f.host.update_space(edited).unwrap();
        let brain = s.f.host.space(&s.sage.id).unwrap().space.brain.unwrap();
        assert_eq!(brain.folder, "second-brain");
        // A read resolves to the Space's own repository, and never to the old brain.
        assert_eq!(
            s.f.host
                .owner_brain(&SessionOwner::Space(s.sage.id.clone()))
                .unwrap(),
            REPO
        );
        assert!(s.f.host.brain("Sage-Social/sagehaven-docs").is_err());
    }

    #[test]
    fn a_queued_batch_lands_in_the_folder_and_an_open_pull_request_is_named_once() {
        let mut s = tests::stored_with(NAMES_A_BRAIN);
        let (queued, open, merged) = (
            "01jbx4m2k8q6v9z3n5r7t0w2ya",
            "01jbx4m2k8q6v9z3n5r7t0w2yb",
            "01jbx4m2k8q6v9z3n5r7t0w2yc",
        );
        in_a_batch(&s, queued, "queued", None);
        in_a_batch(&s, open, "checks", Some(214));
        in_a_batch(&s, merged, "merged", Some(213));
        let (failed, refused, discarded) = (
            "01jbx4m2k8q6v9z3n5r7t0w2yf",
            "01jbx4m2k8q6v9z3n5r7t0w2yg",
            "01jbx4m2k8q6v9z3n5r7t0w2yh",
        );
        in_a_batch(&s, failed, "failed", None);
        in_a_batch(&s, refused, "failed", Some(215));
        in_a_batch(&s, discarded, "discarded", None);
        // Two rows of one pull request name it once.
        s.f.host
            .db
            .execute(
                "INSERT INTO brain_batches(id,repository,data) SELECT 'again',repository,data FROM brain_batches WHERE id=?1",
                [format!("batch-{open}")],
            )
            .unwrap();
        s.f.host.land_what_waits().unwrap();
        s.sync();
        let landed =
            |id: &str| format!("{FOLDER}/second-brain/logs/app/2026/10/2026-10-20-{id}.md");
        assert!(
            s.github.show(&landed(queued)).is_some(),
            "the queued entry lands"
        );
        assert!(
            s.github.show(&landed(failed)).is_some(),
            "it never reached GitHub"
        );
        for left in [open, merged, refused, discarded] {
            assert!(
                s.github.show(&landed(left)).is_none(),
                "{left} stays where it went"
            );
        }
        let page = s.f.host.brains_page().unwrap();
        assert_eq!(
            page.left_pull_requests,
            [214, 215].map(|number| LeftPullRequest {
                repository: "Sage-Social/sagehaven-docs".into(),
                number,
            })
        );
        // The rows are done, so a later start lands nothing more.
        let commits = s.github.log("%s").len();
        s.f.host.carried_over = false;
        s.f.host.land_what_waits().unwrap();
        s.sync();
        assert_eq!(s.github.log("%s").len(), commits);
    }

    #[test]
    fn a_ruling_in_a_batch_that_failed_before_any_pull_request_lands_from_its_worktree() {
        let mut s = tests::stored_with(NAMES_A_BRAIN);
        let id = "01jbx4m2k8q6v9z3n5r7t0w2yd";
        let file = format!("second-brain/logs/app/2026/10/2026-10-20-{id}.md");
        let worktree = s.f.host.home.join("brain-batches").join("ruling");
        fs::create_dir_all(worktree.join("second-brain/logs/app/2026/10")).unwrap();
        fs::write(worktree.join(&file), "A ruling's file.\n").unwrap();
        let batch = json!({"id": "ruling", "repository": "Sage-Social/sagehaven-docs",
            "kind": "log", "source": {"kind": "brains_page"}, "title": "Ruling",
            "files": [file], "stage": "failed", "created_at": 1, "updated_at": 1});
        s.f.host
            .db
            .execute(
                "INSERT INTO brain_batches(id,repository,data) VALUES ('ruling','Sage-Social/sagehaven-docs',?1)",
                [batch.to_string()],
            )
            .unwrap();
        s.f.host.land_what_waits().unwrap();
        s.sync();
        let landed = format!("{FOLDER}/second-brain/logs/app/2026/10/2026-10-20-{id}.md");
        assert_eq!(s.github.show(&landed).unwrap(), "A ruling's file.\n");
    }

    #[test]
    fn the_upgrade_writes_nothing_to_the_tracker_or_compilers_and_pushes_nothing_with_nothing_waiting()
     {
        let mut s = tests::stored_with(NAMES_A_BRAIN);
        let (commits, pushes) = (s.github.log("%s").len(), s.github.pushes());
        s.f.host.land_what_waits().unwrap();
        s.sync();
        assert_eq!(s.github.log("%s").len(), commits);
        assert_eq!(s.github.pushes(), pushes);
        assert!(
            s.github
                .show(&format!("{FOLDER}/compilers/x.yaml"))
                .is_none()
        );
    }

    #[test]
    fn a_row_of_a_forgotten_brain_stays_there_and_one_never_handed_over_lands() {
        let mut s = tests::stored();
        let (forgotten, unsent) = ("01jbx4m2k8q6v9z3n5r7t0w2yj", "01jbx4m2k8q6v9z3n5r7t0w2yk");
        in_a_batch(&s, forgotten, "merged", Some(213));
        in_a_batch(&s, unsent, "queued", None);
        let db = &s.f.host.db;
        // Forget deleted the brain's batches, and left the log batches naming them.
        db.execute(
            "DELETE FROM brain_batches WHERE id=?1",
            [format!("batch-{forgotten}")],
        )
        .unwrap();
        db.execute(
            "DELETE FROM brain_batches WHERE id=?1",
            [format!("batch-{unsent}")],
        )
        .unwrap();
        db.execute(
            "UPDATE log_batches SET brain_batch_id=NULL WHERE id=?1",
            [format!("logs-{unsent}")],
        )
        .unwrap();
        s.f.host.land_what_waits().unwrap();
        s.sync();
        let landed =
            |id: &str| format!("{FOLDER}/second-brain/logs/app/2026/10/2026-10-20-{id}.md");
        assert!(
            s.github.show(&landed(forgotten)).is_none(),
            "already on its brain"
        );
        assert!(
            s.github.show(&landed(unsent)).is_some(),
            "it never left this Mac"
        );
        let project = s.f.coordinator.project_id.clone();
        let rows = s.f.host.logs(&project, None, 10).unwrap();
        assert!(rows.iter().all(|row| !row.is_unsent()));
    }

    #[test]
    fn a_row_waits_for_a_login_on_github_and_lands_once_one_is_read() {
        let mut s = tests::stored();
        let id = "01jbx4m2k8q6v9z3n5r7t0w2ye";
        in_a_batch(&s, id, "queued", None);
        s.f.host
            .db
            .execute(
                "UPDATE work_logs SET data=json_remove(data,'$.author') WHERE id=?1",
                [id],
            )
            .unwrap();
        s.f.host.record_gh_login(None);
        s.f.host.land_what_waits().unwrap();
        assert!(!s.f.host.carried_over);
        assert!(
            s.f.host
                .unlanded_entries(&repos::key(REPO))
                .unwrap()
                .is_empty()
        );
        s.f.host.record_gh_login(Some("mhohlios".into()));
        s.f.host.land_what_waits().unwrap();
        assert!(s.f.host.carried_over);
        assert_eq!(
            s.f.host.unlanded_entries(&repos::key(REPO)).unwrap().len(),
            1
        );
    }

    #[test]
    fn a_held_entry_is_fixed_or_released_and_the_inbox_item_clears_once_none_is_held() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let sage = host.create_space("Sage", "", "", None).unwrap().space;
        let project = host.create_project("Held").unwrap();
        host.move_project_to_space(&project.id, &sage.id).unwrap();
        let coordinator = host.coordinator_of(&project.id).unwrap().unwrap();
        drop(host);
        before_entries(
            home.path(),
            &[
                ("fix-me", &coordinator, "main 1a2b3c4", false),
                ("release-me", &coordinator, "see PR 92", false),
            ],
        );
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.record_gh_login(Some("mhohlios".into()));
        host.land_what_waits().unwrap();
        let item_open = |host: &Host| {
            (host.open_attention(&project.id).unwrap().iter())
                .any(|i| i.operation_id.contains("held-logs"))
        };
        assert!(item_open(&host));
        let logs = host.logs(&project.id, None, 100).unwrap();
        let held = |title_ref: &str| {
            (logs.iter())
                .find(|l| l.reference == title_ref)
                .unwrap()
                .clone()
        };
        let (to_fix, to_release) = (held("main 1a2b3c4"), held("see PR 92"));
        // The panel's first page always reaches every held entry, however old.
        let page = host.logs_page(&project.id, None, 1).unwrap();
        assert_eq!(page.len(), 2);
        assert!(page.iter().all(|l| l.held.is_some()));
        assert_eq!(host.logs(&project.id, None, 1).unwrap().len(), 1);
        let clone = host.storage_folder(&sage.storage.repository);
        let path = |log: &WorkLog| clone.join(log.path.as_ref().unwrap());
        assert!(!path(&to_fix).exists() && !path(&to_release).exists());

        let fix = |reference: &str| LogFix {
            kind: LogKind::Decision,
            title: "Old".into(),
            repository: PLANNING_FOLDER.into(),
            reference: reference.into(),
            changed: "c".into(),
            why: "w".into(),
            state: "s (was main 1a2b3c4)".into(),
        };
        let error = host
            .fix_held_log(&to_fix.id, fix("main 1a2b3c4"))
            .unwrap_err();
        assert!(format!("{error:#}").contains("its ref"), "{error:#}");
        assert_eq!(
            host.held_log(&to_fix.id).unwrap(),
            to_fix,
            "nothing changed"
        );
        // Held under the brain the entry was written for, it lands in the Space's folder now.
        (host.db.execute(
            "UPDATE work_logs SET data=json_set(data,'$.brain','Sage-Social/sagehaven-docs') WHERE id=?1",
            [&to_fix.id],
        ))
        .unwrap();
        let fixed = host.fix_held_log(&to_fix.id, fix(NO_REF)).unwrap();
        assert_eq!(
            (fixed.held.as_deref(), fixed.reference.as_str()),
            (None, NO_REF)
        );
        assert_eq!(fixed.brain, Some(sage.storage.repository.key()));
        assert!(item_open(&host), "another entry is still held");

        let released = host.release_held_log(&to_release.id).unwrap();
        assert!(released.released && released.held.is_none());
        assert!(!item_open(&host));
        assert!(
            host.release_held_log(&to_release.id).is_err(),
            "no longer held"
        );
        let kept = host.logs(&project.id, None, 100).unwrap();
        assert_eq!(kept.len(), 2, "a released entry stays in the store");

        // The fixed entry lands; the released one never.
        assert!(path(&fixed).is_file() && !path(&to_release).exists());
    }
}
