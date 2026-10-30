//! Issues and the human on this Mac (OpenSpec change `shared-issues`, phase 5): who an issue can
//! be assigned to and their profiles' names, the issues the human follows, the Inbox's issue
//! notes, and settling a part of `issue.md` sync paused. Follows, notes and names stay in
//! `issues.sql`. Of all this only settling a part writes to a tracker, and only as the human
//! chose. Issue text is untrusted and may hold personal data: notes carry it to the app only,
//! and nothing here logs it.
use crate::*;
use repos::Record;
use std::collections::BTreeSet;
use tracker::{issue_file, issue_of};

/// How long a tracker's collaborators, and a login's profile name, are kept before an ask for
/// the assignee picker reads them again.
const PEOPLE_FRESH_MS: i64 = 3_600_000;

/// What a read of a tracker's people asks GitHub, and then what it found.
#[derive(Clone, Debug, Default)]
pub(crate) struct PeopleRead {
    storage: String,
    /// `owner/name` of a GitHub tracker; a local one has no collaborators.
    repository: Option<String>,
    /// The logins whose profile names are read.
    logins: Vec<String>,
    /// `None` where GitHub did not let this Mac read them.
    collaborators: Option<Vec<String>>,
    names: Vec<(String, Option<String>)>,
}

/// Whether `login` is one GitHub could have given, so it is safe in an API path.
fn is_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && !login.starts_with('-')
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Reads `read`'s repository's collaborators, where GitHub allows, and the profile name of each
/// of its logins and collaborators, through `gh`, off the host thread.
pub(crate) fn read_people(read: &PeopleRead, tools: &publish::Tools, home: &Path) -> PeopleRead {
    let mut found = read.clone();
    if let Some(repository) = &read.repository {
        let endpoint = format!("repos/{repository}/collaborators?per_page=100");
        found.collaborators =
            (github_access::gh_json(tools, home, &endpoint).ok()).and_then(|list| {
                let list = list.as_array()?.iter();
                Some(
                    list.filter_map(|c| c["login"].as_str().map(str::to_owned))
                        .collect(),
                )
            });
    }
    let mut logins: Vec<String> = (read.logins.iter())
        .chain(found.collaborators.iter().flatten())
        .filter(|login| is_login(login))
        .cloned()
        .collect();
    logins.sort();
    logins.dedup();
    found.names = (logins.into_iter())
        .filter_map(|login| {
            let user = github_access::gh_json(tools, home, &format!("users/{login}")).ok()?;
            let name = user["name"].as_str().filter(|name| !name.trim().is_empty());
            Some((login, name.map(str::to_owned)))
        })
        .collect();
    found
}

/// Whether `text` mentions `@login`, in any case, as a whole word.
pub(crate) fn mentions(text: &str, login: &str) -> bool {
    let (text, wanted) = (text.to_lowercase(), format!("@{}", login.to_lowercase()));
    let part_of_login = |c: char| c.is_ascii_alphanumeric() || c == '-';
    text.match_indices(&wanted).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + wanted.len()..].chars().next();
        !before.is_some_and(|c| part_of_login(c) || c == '@') && !after.is_some_and(part_of_login)
    })
}

/// A comment's first line that has text, as a note quotes it.
fn first_line(body: &str) -> String {
    let line = body.lines().find(|line| !line.trim().is_empty());
    let line = line.unwrap_or_default().trim();
    match line.chars().count() > 120 {
        true => format!("{}…", line.chars().take(119).collect::<String>()),
        false => line.to_owned(),
    }
}

/// A part or file of an issue as a note names it.
fn named_part(path: &str) -> String {
    match path.rsplit_once('#') {
        Some((_, issues_sync::DESCRIPTION)) => "the description".into(),
        Some((_, field)) => format!("the {}", field.replace('_', " ")),
        None if path.ends_with("/issue.md") => "the issue".into(),
        None if path.ends_with("/events.jsonl") => "its timeline".into(),
        None if path.contains("/comments/") => "a comment".into(),
        None => path.to_owned(),
    }
}

/// "The description" from "the description".
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A PR's number, the last part of its URL.
fn pr_number(url: &str) -> &str {
    url.trim_end_matches('/').rsplit('/').next().unwrap_or(url)
}

impl Host {
    /// The human's GitHub login, compared without case.
    fn is_me(&self, login: &str) -> bool {
        (self.gh_login.as_deref()).is_some_and(|me| me.eq_ignore_ascii_case(login))
    }
    /// The name on `login`'s GitHub profile when read, else the login.
    pub(crate) fn display_name(&self, login: &str) -> String {
        let name: Option<String> = (self.db)
            .query_row(
                "SELECT name FROM issue_profiles WHERE login=?1",
                [login],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten();
        name.unwrap_or_else(|| login.to_owned())
    }
    /// Whether the human follows the issue on this Mac: as they chose, or else when they filed
    /// it, are assigned it or commented on it.
    pub(crate) fn is_following(&self, storage: &str, record: &IssueRecord) -> Result<bool> {
        let chosen: Option<bool> = (self.db)
            .query_row(
                "SELECT follow FROM issue_follows WHERE storage=?1 AND id=?2",
                params![storage, record.id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(chosen.unwrap_or_else(|| {
            let issue = record.issue.as_ref();
            issue.is_some_and(|i| self.is_me(&i.author))
                || (issue.and_then(|i| i.assignee.as_deref())).is_some_and(|a| self.is_me(a))
                || record.comments.iter().any(|c| self.is_me(&c.author))
        }))
    }
    /// Follows the issue on this Mac, or stops following it. Nothing reaches its tracker.
    pub fn follow_issue(&self, id: &IssueId, follow: bool) -> Result<()> {
        let (space, _) = self.tracker_for(id.prefix())?;
        ensure!(
            self.store(&space, id.prefix()).exists(id),
            "{id} is not in this tracker"
        );
        self.db.execute(
            "INSERT OR REPLACE INTO issue_follows(storage,id,follow) VALUES (?1,?2,?3)",
            params![space.storage.repository.key(), id.to_string(), follow],
        )?;
        Ok(())
    }

    /// Who an issue in the Space's tracker can be assigned to: the logins its issues name, the
    /// repository's collaborators where read, and the human, each with their profile's name.
    pub fn issue_people(&self, space_id: &SpaceId) -> Result<Vec<IssuePerson>> {
        let storage = self.space(space_id)?.space.storage.repository.key();
        let mut logins: BTreeSet<String> = self.tracker_logins(&storage)?;
        let collaborators = (self.db)
            .prepare("SELECT login FROM issue_collaborators WHERE storage=?1")?
            .query_map([&storage], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        logins.extend(collaborators);
        logins.extend(self.gh_login.clone());
        let mut people: Vec<IssuePerson> = (logins.into_iter())
            .map(|login| {
                let name = Some(self.display_name(&login)).filter(|name| *name != login);
                IssuePerson { login, name }
            })
            .collect();
        people.sort_by_key(|person| person.login.to_lowercase());
        people.dedup_by(|a, b| a.login.eq_ignore_ascii_case(&b.login));
        Ok(people)
    }
    /// The logins the tracker's issues name.
    fn tracker_logins(&self, storage: &str) -> Result<BTreeSet<String>> {
        let mut logins = BTreeSet::new();
        let mut rows = (self.db).prepare("SELECT people FROM issue_index WHERE storage=?1")?;
        for people in rows.query_map([storage], |r| r.get::<_, String>(0))? {
            logins.extend(serde_json::from_str::<Vec<String>>(&people?)?);
        }
        Ok(logins)
    }
    /// What the assignee picker's people need read from GitHub, once an hour per tracker: its
    /// collaborators and the names of logins not read within the hour. Claims the read, so
    /// asks meanwhile start none.
    pub(crate) fn people_to_read(&self, space_id: &SpaceId, at: i64) -> Result<Option<PeopleRead>> {
        let space = self.space(space_id)?.space;
        let storage = space.storage.repository.key();
        let read_at: Option<i64> = (self.db)
            .query_row(
                "SELECT at FROM issue_collaborators_read WHERE storage=?1",
                [&storage],
                |r| r.get(0),
            )
            .optional()?;
        if read_at.is_some_and(|read| at - read < PEOPLE_FRESH_MS) {
            return Ok(None);
        }
        self.db.execute(
            "INSERT OR REPLACE INTO issue_collaborators_read(storage,at) VALUES (?1,?2)",
            params![storage, at],
        )?;
        let mut logins = vec![];
        for login in self.tracker_logins(&storage)? {
            let named: Option<i64> = (self.db)
                .query_row(
                    "SELECT at FROM issue_profiles WHERE login=?1",
                    [&login],
                    |r| r.get(0),
                )
                .optional()?;
            if named.is_none_or(|named| at - named >= PEOPLE_FRESH_MS) {
                logins.push(login);
            }
        }
        let repository = match &space.storage.repository {
            StorageRepository::GitHub { .. } => Some(storage.clone()),
            StorageRepository::Local { .. } => None,
        };
        Ok(Some(PeopleRead {
            storage,
            repository,
            logins,
            ..PeopleRead::default()
        }))
    }
    /// Keeps what a read of a tracker's people found.
    pub(crate) fn record_issue_people(&self, read: PeopleRead) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        if let Some(collaborators) = &read.collaborators {
            tx.execute(
                "DELETE FROM issue_collaborators WHERE storage=?1",
                [&read.storage],
            )?;
            for login in collaborators {
                tx.execute(
                    "INSERT OR IGNORE INTO issue_collaborators(storage,login) VALUES (?1,?2)",
                    params![read.storage, login],
                )?;
            }
        }
        for (login, name) in &read.names {
            tx.execute(
                "INSERT OR REPLACE INTO issue_profiles(login,name,at) VALUES (?1,?2,?3)",
                params![login, name, now()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The issues whose files `paths` name, as they are before a pass writes them: `None` for
    /// one not here yet.
    pub(crate) fn issues_before<'a>(
        &self,
        storage: &StorageRepository,
        paths: impl Iterator<Item = &'a String>,
    ) -> Vec<(IssueId, Option<IssueRecord>)> {
        let mut ids: Vec<IssueId> = paths.filter_map(|path| issue_of(path)).collect();
        ids.sort();
        ids.dedup();
        let key = storage.key();
        (ids.into_iter())
            .map(|id| {
                let store = tracker::Store {
                    folder: self.storage_folder(storage),
                    prefix: id.prefix().to_owned(),
                };
                let record = store.record(&id, &key).ok();
                (id, record)
            })
            .collect()
    }
    /// Notes what a pass brought each issue in `before` that the human should hear of: a new
    /// Urgent issue in a repository on this Mac, an assignment to them, a mention, and a new
    /// comment on an issue they follow. Nothing is noted of the human's own changes. A failure
    /// is logged with the issue's ID only.
    pub(crate) fn note_issue_news(
        &self,
        storage: &StorageRepository,
        before: Vec<(IssueId, Option<IssueRecord>)>,
    ) {
        if self.gh_login.is_none() {
            return;
        }
        for (id, old) in before {
            if let Err(error) = self.note_news_of(storage, &id, old.as_ref()) {
                // Only the outermost context: an inner error may quote the issue's files.
                let error = error.context(format!("{id}’s news was not noted"));
                eprintln!("Issues: {error}");
            }
        }
    }
    fn note_news_of(
        &self,
        storage: &StorageRepository,
        id: &IssueId,
        old: Option<&IssueRecord>,
    ) -> Result<()> {
        let key = storage.key();
        let store = tracker::Store {
            folder: self.storage_folder(storage),
            prefix: id.prefix().to_owned(),
        };
        if !store.exists(id) {
            return Ok(());
        }
        let record = store.record(id, &key)?;
        let Some(issue) = &record.issue else {
            return Ok(());
        };
        let me = self.gh_login.clone().unwrap_or_default();
        let was = old.and_then(|old| old.issue.as_ref());
        let at = schedules::rfc3339(now());
        let note = |kind, text: String, detail: String| IssueNote {
            tracker: key.clone(),
            id: id.clone(),
            kind,
            text,
            detail,
            at: at.clone(),
            paused: None,
        };
        let mut notes = vec![];
        if old.is_none() && !self.is_me(&issue.author) && issue.priority == Priority::Urgent {
            let mine: Vec<&str> = (issue.repositories.iter())
                .filter(|named| {
                    (self.repositories().unwrap_or_default().iter()).any(|r| r.is_github(named))
                })
                .map(|named| named.rsplit('/').next().unwrap_or(named))
                .collect();
            if !mine.is_empty() {
                let who = self.display_name(&issue.author);
                let text = format!("{who} filed an urgent bug in a repository you work in");
                let detail = format!("{} · {}", mine.join(", "), issue.title);
                notes.push((
                    "urgent".to_owned(),
                    note(IssueNoteKind::Urgent, text, detail),
                ));
            }
        }
        let assigned = |issue: &Issue| issue.assignee.as_deref().is_some_and(|a| self.is_me(a));
        if assigned(issue) && !was.is_some_and(assigned) {
            let by = (record.events.iter().rev())
                .find(|e| {
                    e.kind == "assignee" && e.fields["to"].as_str().is_some_and(|to| self.is_me(to))
                })
                .map_or(issue.author.as_str(), |event| event.by.as_str());
            if !self.is_me(by) {
                let text = format!("{} assigned you", self.display_name(by));
                let key = format!("assigned:{at}");
                notes.push((
                    key,
                    note(IssueNoteKind::Assigned, text, issue.title.clone()),
                ));
            }
        }
        let mentioned = |issue: &Issue| mentions(&issue.description, &me);
        if mentioned(issue) && !was.is_some_and(mentioned) {
            let text = "You were mentioned in the description".to_owned();
            let detail = issue.title.clone();
            notes.push((
                "mention:description".into(),
                note(IssueNoteKind::Mentioned, text, detail),
            ));
        }
        let seen: BTreeSet<&str> = (old.iter())
            .flat_map(|old| old.comments.iter().map(|c| c.file.as_str()))
            .collect();
        let following = self.is_following(&key, &record)?;
        for comment in &record.comments {
            if seen.contains(comment.file.as_str()) || self.is_me(&comment.author) {
                continue;
            }
            let who = self.display_name(&comment.author);
            let (kind, text) = if mentions(&comment.body, &me) {
                (
                    IssueNoteKind::Mentioned,
                    format!("{who} mentioned you in a comment"),
                )
            } else if following {
                let whose = match self.is_me(&issue.author) {
                    true => "your issue",
                    false => "an issue you follow",
                };
                (
                    IssueNoteKind::Commented,
                    format!("{who} commented on {whose}"),
                )
            } else {
                continue;
            };
            let key = format!("comment:{}", comment.file);
            notes.push((key, note(kind, text, first_line(&comment.body))));
        }
        for (note_key, note) in notes {
            self.add_issue_note(&note_key, &note)?;
        }
        Ok(())
    }
    /// Notes that a linked issue moved with its work on this Mac, naming the PR whose change
    /// moved it, from `events`.
    pub(crate) fn note_moved(
        &self,
        storage: &str,
        issue: &Issue,
        events: &[IssueEvent],
        at: &str,
    ) -> Result<()> {
        let mut text = format!("Moved to {}", issue.state.label());
        if let Some(pr) = events.iter().rev().find(|event| event.kind == "pr") {
            let url = pr.fields["url"].as_str().unwrap_or_default();
            let did = match pr.fields["status"].as_str() {
                Some("open") => "opened",
                Some("merged") => "merged",
                _ => "closed",
            };
            text.push_str(&format!(": PR #{} {did}", pr_number(url)));
        }
        let note = IssueNote {
            tracker: storage.to_owned(),
            id: issue.id.clone(),
            kind: IssueNoteKind::Moved,
            text,
            detail: issue.title.clone(),
            at: at.to_owned(),
            paused: None,
        };
        self.add_issue_note(&format!("moved:{at}"), &note)
    }
    fn add_issue_note(&self, key: &str, note: &IssueNote) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO issue_notes(storage,id,key,data) VALUES (?1,?2,?3,?4)",
            params![note.tracker, note.id.to_string(), key, encode(note)?],
        )?;
        Ok(())
    }
    /// Settles the issue's news, for every project at once, as the human opened it.
    pub(crate) fn settle_issue_news(&self, storage: &str, id: &IssueId) -> Result<()> {
        self.db.execute(
            "DELETE FROM issue_notes WHERE storage=?1 AND id=?2",
            params![storage, id.to_string()],
        )?;
        Ok(())
    }

    /// The Inbox's issue notes: each paused part, then each paused issue file, then the news,
    /// newest first. Those of a tracker no longer here are left out.
    pub(crate) fn issue_notes(&self) -> Result<Vec<WorkspaceNote>> {
        let mut paused = vec![];
        let mut news = vec![];
        for (space, _) in self.trackers()? {
            let storage = space.storage.repository.key();
            if let StorageRepository::GitHub { .. } = space.storage.repository {
                let key = repos::key(&storage);
                paused.extend(self.paused_issue_notes(&key, &storage)?);
            }
            let mut rows = (self.db).prepare("SELECT data FROM issue_notes WHERE storage=?1")?;
            for data in rows.query_map([&storage], |r| r.get::<_, String>(0))? {
                news.push(serde_json::from_str::<IssueNote>(&data?)?);
            }
        }
        news.sort_by(|a, b| b.at.cmp(&a.at));
        Ok((paused.into_iter().chain(news))
            .map(WorkspaceNote::Issue)
            .collect())
    }
    fn paused_issue_notes(&self, key: &str, storage: &str) -> Result<Vec<IssueNote>> {
        let repository = self.stored_repository_name(key)?;
        let mut rows: Vec<(String, Paused)> = vec![];
        let mut parts = (self.db).prepare(
            "SELECT path,part,by FROM issue_parts WHERE repository=?1 ORDER BY path,part",
        )?;
        for row in parts.query_map([key], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })? {
            let (path, part, by) = row?;
            rows.push((
                format!("{path}#{part}"),
                Paused::TwoMachines { machine: by },
            ));
        }
        let mut files = (self.db).prepare(
            "SELECT path,why FROM sync_paused WHERE repository=?1 AND path LIKE 'issues/%' ORDER BY path",
        )?;
        for row in files.query_map([key], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (path, why) = row?;
            rows.push((path, serde_json::from_str(&why)?));
        }
        let mut notes = vec![];
        for (path, why) in rows {
            let Some(id) = issue_of(&path) else {
                continue;
            };
            let what = named_part(&path);
            let part = path.contains('#');
            let (text, detail) = match &why {
                Paused::TwoMachines { machine } if part => (
                    format!("Paused: you and {machine} both changed {what}"),
                    format!("Your version is kept on this Mac until you choose. The rest of {id} keeps syncing."),
                ),
                Paused::TwoMachines { .. } => (
                    format!("Paused: {what} changed here and on another Mac"),
                    format!("This Mac keeps its version until you choose. The rest of {id} keeps syncing."),
                ),
                Paused::OutsideWiffletree => (
                    format!("{} was changed outside Wiffletree on this Mac", capitalized(&what)),
                    "By an editor, a script or an agent’s shell. It isn’t pushed until you choose; this Mac uses it as it is now.".into(),
                ),
                Paused::CannotBeRead { machine, reason } => (
                    format!("{} from {machine} can’t be used here", capitalized(&what)),
                    format!("{reason}. This Mac keeps its own version."),
                ),
            };
            let setting = PausedSetting {
                path: path.clone(),
                setting: format!("{id} · {what}"),
                paused: why,
                text: format!("{text}. {detail}"),
                repository: Some(repository.clone()),
            };
            notes.push(IssueNote {
                tracker: storage.to_owned(),
                id,
                kind: IssueNoteKind::Paused,
                text,
                detail,
                at: String::new(),
                paused: Some(setting),
            });
        }
        Ok(notes)
    }

    /// The paused part `part` of the `issue.md` at `file` in GitHub storage repository `name`:
    /// the record key, the clone, and the other side's value.
    fn paused_part(
        &self,
        name: &str,
        file: &str,
        part: &str,
    ) -> Result<(String, PathBuf, Option<String>)> {
        let key = repos::key(name);
        let theirs: Option<Option<String>> = (self.db)
            .query_row(
                "SELECT theirs FROM issue_parts WHERE repository=?1 AND path=?2 AND part=?3",
                params![key, file, part],
                |r| r.get(0),
            )
            .optional()?;
        let theirs = theirs.context("That part is not paused")?;
        Ok((key, repos::clone_folder(&self.home, name), theirs))
    }
    /// This Mac's value of a paused part beside the other side's, line by line, as Compare
    /// shows them.
    pub(crate) fn compare_part(&self, name: &str, path: &str) -> Result<Vec<skills::Line>> {
        let (file, part) = path.split_once('#').context("Not a part")?;
        let (_, clone, theirs) = self.paused_part(name, file, part)?;
        let here = fs::read(clone.join(file)).context("The issue is gone")?;
        let mine = issues_sync::issue_part(&here, part)?;
        Ok(skills::line_diff(
            mine.as_deref().unwrap_or_default(),
            theirs.as_deref().unwrap_or_default(),
        ))
    }
    /// Settles a paused part: Keep this Mac's leaves its value, which the next pass pushes, and
    /// Use theirs writes the other side's here. The pass, due two seconds later, does the rest.
    pub(crate) fn settle_part(&mut self, name: &str, path: &str, settle: Settle) -> Result<()> {
        let (file, part) = path.split_once('#').context("Not a part")?;
        let (key, clone, theirs) = self.paused_part(name, file, part)?;
        ensure!(
            matches!(settle, Settle::KeepThisMachines | Settle::UseTheOthers),
            "That note does not offer this choice"
        );
        let storage = storage::github(name);
        let id = issue_of(file).context("Not an issue's file")?;
        ensure!(
            file == issue_file(&id) && !self.changed_outside(&storage, &[file])?,
            "{id} was changed outside Wiffletree on this Mac; settle that first"
        );
        if settle == Settle::UseTheOthers {
            let here = fs::read(clone.join(file)).context("The issue is gone")?;
            let content = issues_sync::with_part(&here, part, theirs.as_deref())?;
            shared::write_atomically(&clone.join(file), &content)?;
            Record::wrote(&self.db, &key, file, Some(&content))?;
            self.reindex_issue(&storage, &id)?;
        }
        self.db.execute(
            "DELETE FROM issue_parts WHERE repository=?1 AND path=?2 AND part=?3",
            params![key, file, part],
        )?;
        self.issues_written.borrow_mut().insert(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue_work::tests::clone_here;
    use crate::storage::tests::{host, join_sage, shared_storage};
    use crate::sync::tests::GitHub;
    use crate::tracker::tests::{blurry, id};
    use std::sync::Arc;
    use tracker::issue_md;

    /// This Mac, signed in as `jlin`, with Sage joined and SAGE-1 pulled: filed by treyes,
    /// assigned to pshah.
    fn sage() -> (tempfile::TempDir, Arc<GitHub>, tempfile::TempDir, Host) {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit(
            "issues/SAGE-1/issue.md",
            issue_md(&blurry("SAGE-1")).unwrap(),
        );
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        (folder, github, home, host)
    }

    fn sync(host: &mut Host, github: &Arc<GitHub>) {
        host.sync_issues(None, &github.tools()).unwrap();
    }

    fn news(host: &Host) -> Vec<(String, IssueNoteKind, String, String)> {
        (host.issue_notes().unwrap().into_iter())
            .filter_map(|note| match note {
                WorkspaceNote::Issue(n) => Some((n.id.to_string(), n.kind, n.text, n.detail)),
                _ => None,
            })
            .collect()
    }

    fn comment(github: &Arc<GitHub>, issue: &str, at: &str, by: &str, body: &str) {
        let file = format!("issues/{issue}/comments/{}-{by}.md", at.replace(':', "-"));
        github.commit(
            &file,
            format!("---\nauthor: {by}\ncreated: {at}\n---\n{body}\n"),
        );
    }

    #[test]
    fn a_teammates_assignment_mention_and_urgent_bug_reach_the_inbox_until_opened() {
        let (_folder, github, home, mut host) = sage();
        clone_here(
            &mut host,
            &home.path().join("chat"),
            "celadon-theme/celadon-chat",
        );
        let text = github.show("issues/SAGE-1/issue.md").unwrap();
        github.commit(
            "issues/SAGE-1/issue.md",
            text.replace("assignee: pshah", "assignee: jlin"),
        );
        let assigned = r#"{"at":"2026-10-30T09:00:00Z","by":"pshah","kind":"assignee","from":"pshah","to":"jlin"}"#;
        github.commit("issues/SAGE-1/events.jsonl", format!("{assigned}\n"));
        comment(
            &github,
            "SAGE-1",
            "2026-10-30T09:01:00Z",
            "pshah",
            "Over to @JLin.\nMore.",
        );
        let mut urgent = blurry("SAGE-2");
        urgent.priority = Priority::Urgent;
        urgent.repositories = vec!["celadon-theme/celadon-chat".into()];
        github.commit("issues/SAGE-2/issue.md", issue_md(&urgent).unwrap());
        let mut elsewhere = blurry("SAGE-3");
        elsewhere.priority = Priority::Urgent;
        github.commit("issues/SAGE-3/issue.md", issue_md(&elsewhere).unwrap());
        sync(&mut host, &github);
        let title = "Theme previews look blurry on Retina displays";
        assert_eq!(
            news(&host),
            [
                (
                    "SAGE-1".into(),
                    IssueNoteKind::Assigned,
                    "pshah assigned you".into(),
                    title.into()
                ),
                (
                    "SAGE-1".into(),
                    IssueNoteKind::Mentioned,
                    "pshah mentioned you in a comment".into(),
                    "Over to @JLin.".into()
                ),
                (
                    "SAGE-2".into(),
                    IssueNoteKind::Urgent,
                    "treyes filed an urgent bug in a repository you work in".into(),
                    format!("celadon-chat · {title}")
                ),
            ]
        );
        // Opening an issue settles its news, for every project at once.
        host.mark_issue_read(&id("SAGE-1")).unwrap();
        assert_eq!(news(&host).len(), 1);
        // A profile's name, once read, names the teammate.
        let read = PeopleRead {
            storage: NAME_KEY.into(),
            names: vec![("treyes".into(), Some("Tomás Reyes".into()))],
            ..PeopleRead::default()
        };
        host.record_issue_people(read).unwrap();
        comment(
            &github,
            "SAGE-2",
            "2026-10-30T09:02:00Z",
            "treyes",
            "@jlin can you look?",
        );
        sync(&mut host, &github);
        assert!(
            news(&host)
                .iter()
                .any(|n| n.2 == "Tomás Reyes mentioned you in a comment")
        );
    }

    const NAME_KEY: &str = crate::sync::tests::NAME;

    #[test]
    fn comments_reach_the_inbox_for_issues_followed_and_never_for_the_humans_own() {
        let (_folder, github, _home, mut host) = sage();
        comment(
            &github,
            "SAGE-1",
            "2026-10-30T09:00:00Z",
            "pshah",
            "Not followed yet.",
        );
        sync(&mut host, &github);
        assert!(
            news(&host).is_empty(),
            "not filed, assigned or commented on"
        );
        assert!(!host.read_issue(&id("SAGE-1")).unwrap().following);
        host.follow_issue(&id("SAGE-1"), true).unwrap();
        assert!(host.read_issue(&id("SAGE-1")).unwrap().following);
        comment(
            &github,
            "SAGE-1",
            "2026-10-30T09:01:00Z",
            "pshah",
            "Repro'd on macOS 15 too.",
        );
        host.comment_on_issue(&id("SAGE-1"), "Mine.").unwrap();
        sync(&mut host, &github);
        let expected = (
            "SAGE-1".to_owned(),
            IssueNoteKind::Commented,
            "pshah commented on an issue you follow".to_owned(),
            "Repro'd on macOS 15 too.".to_owned(),
        );
        assert_eq!(news(&host), [expected]);
        // Commenting follows, until the human chooses not to.
        host.follow_issue(&id("SAGE-1"), false).unwrap();
        assert!(!host.read_issue(&id("SAGE-1")).unwrap().following);
        comment(
            &github,
            "SAGE-1",
            "2026-10-30T09:02:00Z",
            "pshah",
            "Still there.",
        );
        sync(&mut host, &github);
        assert_eq!(news(&host).len(), 1);
        let issue = &tracker::issue_file(&id("SAGE-1"));
        assert!(
            !github.show(issue).unwrap().contains("follow"),
            "follows stay on this Mac"
        );
    }

    #[test]
    fn the_assignee_picker_offers_the_trackers_logins_collaborators_and_the_human_with_names() {
        let (_folder, github, _home, mut host) = sage();
        comment(&github, "SAGE-1", "2026-10-30T09:00:00Z", "mchen", "Hi");
        sync(&mut host, &github);
        let space = crate::storage::tests::space_named(&host, "Sage");
        let read = host.people_to_read(&space.id, 0).unwrap().unwrap();
        assert_eq!(read.repository.as_deref(), Some(NAME_KEY));
        assert_eq!(read.logins, ["mchen", "pshah", "treyes"]);
        assert!(
            host.people_to_read(&space.id, 1).unwrap().is_none(),
            "claimed"
        );
        let found = PeopleRead {
            collaborators: Some(vec!["okim".into()]),
            names: vec![("pshah".into(), Some("Priya Shah".into()))],
            ..read
        };
        host.record_issue_people(found).unwrap();
        let people: Vec<(String, Option<String>)> = (host.issue_people(&space.id).unwrap())
            .into_iter()
            .map(|p| (p.login, p.name))
            .collect();
        assert_eq!(
            people,
            [
                ("jlin".into(), None),
                ("mchen".into(), None),
                ("okim".into(), None),
                ("pshah".into(), Some("Priya Shah".into())),
                ("treyes".into(), None),
            ]
        );
    }

    #[test]
    fn people_are_read_through_gh_and_a_login_that_is_no_login_is_never_asked_for() {
        let asked = Arc::new(std::sync::Mutex::new(vec![]));
        let tools = publish::Tools {
            gh: Arc::new({
                let asked = asked.clone();
                move |_, args| {
                    let endpoint = args.last().unwrap().to_string();
                    asked.lock().unwrap().push(endpoint.clone());
                    let output = match endpoint.as_str() {
                        e if e.starts_with("repos/") => json!([{"login": "okim"}]),
                        "users/okim" => json!({"login": "okim", "name": "Olu Kim"}),
                        _ => json!({"login": "x", "name": null}),
                    };
                    Ok(runtime::GhAnswer {
                        status: std::os::unix::process::ExitStatusExt::from_raw(0),
                        output: output.to_string(),
                        error: String::new(),
                    })
                }
            }),
            ..publish::Tools::default()
        };
        let read = PeopleRead {
            storage: "o/s".into(),
            repository: Some("o/s".into()),
            logins: vec!["pshah".into(), "../admin".into()],
            ..PeopleRead::default()
        };
        let found = read_people(&read, &tools, Path::new("/"));
        assert_eq!(found.collaborators, Some(vec!["okim".into()]));
        assert_eq!(
            found.names,
            [
                ("okim".into(), Some("Olu Kim".into())),
                ("pshah".into(), None)
            ]
        );
        let asked = asked.lock().unwrap();
        assert!(!asked.iter().any(|e| e.contains("..")), "{asked:?}");
    }

    #[test]
    fn a_mention_is_the_whole_login_in_any_case() {
        assert!(mentions("Thanks @JLin, see above", "jlin"));
        assert!(mentions("@jlin", "jlin"));
        assert!(!mentions("@jlinx and @@jlin", "jlin"));
        assert!(!mentions("mail jlin@example.com", "jlin"));
        assert!(!mentions("a-@jlin", "jlin"));
    }

    #[test]
    fn only_a_login_github_could_give_reaches_an_api_path() {
        assert!(is_login("pshah") && is_login("t-reyes2"));
        assert!(!is_login("../repos") && !is_login("-x") && !is_login(""));
    }
}
