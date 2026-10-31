//! The tracker's files (OpenSpec change `shared-issues`, phase 1): one folder per issue under
//! `issues/` of a Space's storage repository, with `issue.md`, `comments/` and `events.jsonl`,
//! and an index of them in `issues.sql`. The files are the source of truth; the index is rebuilt
//! from them when it is missing or its format changes, and an issue is read into it again when
//! its folder changed on disk, by anyone. Every write here is the human's. Issue text is
//! untrusted and may hold personal data: nothing here logs it, and commit messages name issues
//! by ID only.
use crate::*;
use repos::{Content, Record};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;
use storage::TRACKER;

pub(crate) const ISSUES: &str = "issues";
/// Raised when the index's layout changes, so it is rebuilt from the files.
const INDEX_FORMAT: i64 = 3;
const FENCE: &str = "---\n";

/// `issue.md`'s front matter, in the order it is written, so an unchanged field makes no diff.
/// Fields this release does not know follow, as they were read.
#[derive(Serialize, Deserialize)]
struct IssueFront {
    id: IssueId,
    title: String,
    state: IssueState,
    #[serde(default)]
    priority: Priority,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    repositories: Vec<String>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    related: Vec<String>,
    #[serde(default)]
    duplicate_of: Option<String>,
    author: String,
    created: String,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Serialize, Deserialize)]
struct CommentFront {
    author: String,
    created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edited: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assistant_for: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

/// The front matter and body of a Markdown file that opens with `---`.
fn split_front(content: &[u8]) -> Result<(&str, &str)> {
    let text = std::str::from_utf8(content).context("The file is not UTF-8")?;
    let rest = text
        .strip_prefix(FENCE)
        .context("The file does not open with --- front matter")?;
    let (front, body) = match rest.split_once("\n---\n") {
        Some((front, body)) => (front, body),
        None => (
            rest.strip_suffix("\n---")
                .context("The front matter has no closing ---")?,
            "",
        ),
    };
    Ok((front, body.strip_prefix('\n').unwrap_or(body)))
}

/// `---`, the front matter, `---` and, after a blank line, the body ending in one newline.
fn with_front(front: &impl Serialize, body: &str) -> Result<Vec<u8>> {
    let mut text = format!("{FENCE}{}{FENCE}", serde_yaml::to_string(front)?);
    let body = body.trim_end();
    if !body.is_empty() {
        text.push('\n');
        text.push_str(body);
        text.push('\n');
    }
    Ok(text.into_bytes())
}

pub(crate) fn issue_md(issue: &Issue) -> Result<Vec<u8>> {
    let front = IssueFront {
        id: issue.id.clone(),
        title: issue.title.clone(),
        state: issue.state,
        priority: issue.priority,
        assignee: issue.assignee.clone(),
        labels: issue.labels.clone(),
        repositories: issue.repositories.clone(),
        blocks: issue.blocks.clone(),
        blocked_by: issue.blocked_by.clone(),
        related: issue.related.clone(),
        duplicate_of: issue.duplicate_of.clone(),
        author: issue.author.clone(),
        created: issue.created.clone(),
        extra: issue.extra.clone(),
    };
    with_front(&front, &issue.description)
}

fn parse_issue(content: &[u8]) -> Result<Issue> {
    let (front, body) = split_front(content)?;
    let front: IssueFront = serde_yaml::from_str(front).context("The front matter is not valid")?;
    Ok(Issue {
        id: front.id,
        title: front.title,
        state: front.state,
        priority: front.priority,
        assignee: front.assignee,
        labels: front.labels,
        repositories: front.repositories,
        blocks: front.blocks,
        blocked_by: front.blocked_by,
        related: front.related,
        duplicate_of: front.duplicate_of,
        author: front.author,
        created: front.created,
        description: body.trim_end().to_owned(),
        extra: front.extra,
    })
}

/// `issue.md`'s front matter, each field as written, and its description: the parts sync merges
/// one at a time.
pub(crate) fn issue_parts(content: &[u8]) -> Result<(serde_yaml::Mapping, String)> {
    let (front, body) = split_front(content)?;
    let front: serde_yaml::Mapping =
        serde_yaml::from_str(front).context("The front matter is not valid")?;
    // Read as an issue too, so a part merge never writes one that would not parse.
    parse_issue(content)?;
    Ok((front, body.trim_end().to_owned()))
}

/// `issue.md` from its parts, in the fixed field order.
pub(crate) fn issue_from_parts(front: &serde_yaml::Mapping, description: &str) -> Result<Vec<u8>> {
    let front: IssueFront = serde_yaml::from_value(serde_yaml::Value::Mapping(front.clone()))?;
    with_front(&front, description)
}

pub(crate) fn comment_md(comment: &Comment) -> Result<Vec<u8>> {
    let front = CommentFront {
        author: comment.author.clone(),
        created: comment.created.clone(),
        edited: comment.edited.clone(),
        assistant_for: comment.assistant_for.clone(),
        extra: Extra::new(),
    };
    with_front(&front, &comment.body)
}

fn parse_comment(file: &str, content: &[u8]) -> Result<Comment> {
    let (front, body) = split_front(content)?;
    let front: CommentFront = serde_yaml::from_str(front)?;
    Ok(Comment {
        file: file.to_owned(),
        author: front.author,
        created: front.created,
        edited: front.edited,
        assistant_for: front.assistant_for,
        body: body.trim_end().to_owned(),
    })
}

/// The lines of `events.jsonl` that parse, in file order; others are skipped, never rewritten.
fn parse_events(content: &[u8]) -> Vec<IssueEvent> {
    String::from_utf8_lossy(content)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// `events.jsonl` as `content` holds it with `events` appended, one line each.
pub(crate) fn with_events(content: Option<Vec<u8>>, events: &[IssueEvent]) -> Result<Vec<u8>> {
    let mut content = content.unwrap_or_default();
    if !content.is_empty() && !content.ends_with(b"\n") {
        content.push(b'\n');
    }
    for event in events {
        content.extend(serde_json::to_vec(event)?);
        content.push(b'\n');
    }
    Ok(content)
}

/// An issue's folder, relative to the tracker's root.
pub(crate) fn issue_folder(id: &IssueId) -> String {
    format!("{ISSUES}/{id}")
}

pub(crate) fn issue_file(id: &IssueId) -> String {
    format!("{}/issue.md", issue_folder(id))
}

pub(crate) fn events_file(id: &IssueId) -> String {
    format!("{}/events.jsonl", issue_folder(id))
}

/// The issue ID at the start of `path`, such as `issues/CEL-151/comments/…`.
pub(crate) fn issue_of(path: &str) -> Option<IssueId> {
    let rest = path.strip_prefix(ISSUES)?.strip_prefix('/')?;
    IssueId::parse(rest.split('/').next()?).ok()
}

/// Whether an issue's folder holds any file; one a pulled deletion emptied is no issue.
fn holds_files(folder: &Path) -> bool {
    storage::files_under(folder).is_ok_and(|files| !files.is_empty())
}

/// Removes the folders under `issues/` that `path`'s deletion left empty, its issue's too.
pub(crate) fn remove_emptied(clone: &Path, path: &str) {
    let issues = clone.join(ISSUES);
    let mut folder = clone.join(path);
    while folder.pop() && folder.starts_with(&issues) && folder != issues {
        if fs::remove_dir(&folder).is_err() {
            break;
        }
    }
}

/// A comment's file name: its time with dashes for colons, then the author's login, and `-2`,
/// `-3` while one of that name exists with other content.
fn comment_file(folder: &Path, at: &str, login: &str, content: &[u8]) -> String {
    let stamp = at.replace(':', "-");
    (1..)
        .map(|n| match n {
            1 => format!("{stamp}-{login}.md"),
            n => format!("{stamp}-{login}-{n}.md"),
        })
        .find(|name| fs::read(folder.join(name)).map_or(true, |there| there == content))
        .expect("a free name")
}

/// A tracker's folder on this Mac, a storage repository's clone or local repository, read
/// through its prefix.
pub(crate) struct Store {
    pub(crate) folder: PathBuf,
    pub(crate) prefix: String,
}
impl Store {
    /// The IDs of the issue folders with this tracker's prefix that hold a file.
    pub(crate) fn ids(&self) -> Vec<IssueId> {
        let mut ids: Vec<IssueId> = (fs::read_dir(self.folder.join(ISSUES)).into_iter())
            .flatten()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                IssueId::parse(&name).ok()
            })
            .filter(|id| id.prefix() == self.prefix && self.exists(id))
            .collect();
        ids.sort();
        ids
    }
    pub(crate) fn exists(&self, id: &IssueId) -> bool {
        holds_files(&self.folder.join(issue_folder(id)))
    }
    pub(crate) fn read(&self, path: &str) -> Option<Vec<u8>> {
        fs::read(self.folder.join(path)).ok()
    }
    /// The issue's `issue.md`, or why it cannot be read.
    pub(crate) fn issue(&self, id: &IssueId) -> Result<Issue> {
        let content = self
            .read(&issue_file(id))
            .with_context(|| format!("{id} has no issue.md"))?;
        let issue = parse_issue(&content)?;
        ensure!(&issue.id == id, "Its id names another issue");
        Ok(issue)
    }
    /// The issue folder's files, relative to it, in order.
    fn files(&self, id: &IssueId) -> Vec<(String, PathBuf)> {
        let folder = self.folder.join(issue_folder(id));
        let mut files: Vec<(String, PathBuf)> = (storage::files_under(&folder).unwrap_or_default())
            .into_iter()
            .filter_map(|file| {
                let name = file.strip_prefix(&folder).ok()?.to_str()?.to_owned();
                Some((name, file))
            })
            .collect();
        files.sort();
        files
    }
    /// The names, sizes and times of the issue folder's files, which any write changes.
    pub(crate) fn stamp(&self, id: &IssueId) -> String {
        let mut stamp = String::new();
        for (name, file) in self.files(id) {
            let meta = fs::metadata(&file).ok();
            let size = meta.as_ref().map_or(0, fs::Metadata::len);
            let modified = (meta.and_then(|m| m.modified().ok()))
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_nanos());
            stamp.push_str(&format!("{name}\0{size}\0{modified}\n"));
        }
        stamp
    }
    /// What the index keeps of the issue, read from its files.
    pub(crate) fn indexed(&self, id: &IssueId, tracker: &str) -> Result<Indexed> {
        // Taken first, so a write while the files are read is found by the next look.
        let stamp = self.stamp(id);
        let mut digest = Sha256::new();
        for (name, file) in self.files(id) {
            digest.update(name.as_bytes());
            digest.update([0]);
            digest.update(fs::read(&file).unwrap_or_default());
            digest.update([0]);
        }
        let version = (digest.finalize().iter())
            .map(|b| format!("{b:02x}"))
            .collect();
        let record = self.record(id, tracker)?;
        let mut text = vec![id.to_string()];
        match &record.issue {
            Some(issue) => text.extend([issue.title.clone(), issue.description.clone()]),
            None => text.extend(record.raw.clone()),
        }
        text.extend(record.comments.iter().map(|comment| comment.body.clone()));
        let mut people: Vec<String> = (record.issue.iter())
            .flat_map(|issue| [Some(issue.author.clone()), issue.assignee.clone()])
            .flatten()
            .chain(record.comments.iter().map(|comment| comment.author.clone()))
            .chain(record.events.iter().map(|event| event.by.clone()))
            .filter(|login| !login.is_empty())
            .collect();
        people.sort();
        people.dedup();
        Ok(Indexed {
            listing: listing(&record),
            stamp,
            version,
            text: text.join("\n").to_lowercase(),
            people,
        })
    }
    /// The issue with its comments in time order and its events. A file that does not parse is
    /// shown raw with the problem, never rewritten.
    pub(crate) fn record(&self, id: &IssueId, tracker: &str) -> Result<IssueRecord> {
        let folder = self.folder.join(issue_folder(id));
        ensure!(self.exists(id), "{id} is not in this tracker");
        let (issue, raw, problem) = match self.issue(id) {
            Ok(issue) => (Some(issue), None, None),
            Err(error) => {
                let raw = self.read(&issue_file(id));
                let raw = raw.map(|raw| String::from_utf8_lossy(&raw).into_owned());
                (None, raw, Some(format!("{error:#}")))
            }
        };
        let mut comments: Vec<Comment> = (fs::read_dir(folder.join("comments")).into_iter())
            .flatten()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file = entry.file_name().into_string().ok()?;
                let content = fs::read(entry.path()).ok()?;
                file.ends_with(".md")
                    .then(|| parse_comment(&file, &content).ok())
                    .flatten()
            })
            .collect();
        comments.sort_by(|a, b| (&a.created, &a.file).cmp(&(&b.created, &b.file)));
        let events = parse_events(&self.read(&events_file(id)).unwrap_or_default());
        Ok(IssueRecord {
            tracker: tracker.to_owned(),
            id: id.clone(),
            issue,
            raw,
            problem,
            comments,
            events,
            paused: vec![],
            links: vec![],
            following: false,
        })
    }
}

/// One issue as the index keeps it, `issues.sql`'s `issue_index`.
pub(crate) struct Indexed {
    listing: IssueListing,
    stamp: String,
    version: String,
    text: String,
    people: Vec<String>,
}

/// The list's line for an issue; one whose `issue.md` does not parse has its problem and no
/// fields.
fn listing(record: &IssueRecord) -> IssueListing {
    let empty = IssueListing {
        id: record.id.clone(),
        tracker: record.tracker.clone(),
        title: String::new(),
        state: None,
        priority: Priority::None,
        assignee: None,
        labels: vec![],
        repositories: vec![],
        author: String::new(),
        created: String::new(),
        problem: record.problem.clone(),
        unread: false,
    };
    match &record.issue {
        Some(issue) => IssueListing {
            title: issue.title.clone(),
            state: Some(issue.state),
            priority: issue.priority,
            assignee: issue.assignee.clone(),
            labels: issue.labels.clone(),
            repositories: issue.repositories.clone(),
            author: issue.author.clone(),
            created: issue.created.clone(),
            ..empty
        },
        None => empty,
    }
}

/// The highest number with `prefix` that any `issues/<ID>/` path has had in the history up to
/// `commit`, so an issue deleted by an ordinary commit never gives its number away; 0 for none.
pub(crate) fn highest_number(folder: &Path, commit: &str, prefix: &str) -> Result<u64> {
    let names = runtime::git_output(
        folder,
        &["log", "--format=", "--name-only", commit, "--", ISSUES],
    )?;
    Ok(names
        .lines()
        .filter_map(issue_of)
        .filter_map(|id| match id {
            IssueId::Numbered { prefix: p, number } if p == prefix => Some(number),
            _ => None,
        })
        .max()
        .unwrap_or(0))
}

/// Four letters for a temporary ID, such as `qxvb`.
fn temporary_letters() -> String {
    let id = uuid::Uuid::new_v4();
    (id.as_bytes().iter().take(IssueId::TEMPORARY_LETTERS))
        .map(|b| char::from(b'a' + b % 26))
        .collect()
}

pub(crate) fn event(by: &str, at: &str, kind: &str, from: Value, to: Value) -> IssueEvent {
    let mut fields = serde_json::Map::new();
    fields.insert("from".into(), from);
    fields.insert("to".into(), to);
    IssueEvent {
        at: at.to_owned(),
        by: by.to_owned(),
        kind: kind.to_owned(),
        fields,
    }
}

pub(crate) fn validate_title(title: &str) -> Result<String> {
    let title = title.trim();
    ensure!(!title.is_empty(), "An issue needs a title");
    ensure!(
        title.len() <= 500 && !title.contains('\n'),
        "A title is one line of at most 500 bytes"
    );
    Ok(title.to_owned())
}

/// The files of one write, relative to the tracker's root, and what it did, such as `filed`.
pub(crate) struct Write {
    pub(crate) id: IssueId,
    pub(crate) files: Vec<(String, Vec<u8>)>,
    pub(crate) did: String,
}

impl Host {
    /// The login the human writes issues as.
    pub(crate) fn issue_author(&self) -> Result<String> {
        self.gh_login.clone().context(
            "Sign gh in to write issues: run gh auth login in Terminal, then try again. Issues can still be read.",
        )
    }
    /// The Space whose tracker holds issues with `prefix`, which is unique on this Mac, and the
    /// tracker.
    pub(crate) fn tracker_for(&self, prefix: &str) -> Result<(Space, TrackerConfig)> {
        for record in self.spaces()? {
            if let Ok(tracker) = self.tracker(&record.space)
                && tracker.prefix == prefix
            {
                return Ok((record.space, tracker));
            }
        }
        bail!("No tracker on this Mac has the prefix {prefix}")
    }
    /// Each Space with a tracker, one per storage repository, with its tracker's config.
    pub(crate) fn trackers(&self) -> Result<Vec<(Space, TrackerConfig)>> {
        let mut trackers: Vec<(Space, TrackerConfig)> = vec![];
        for record in self.spaces()? {
            let storage = &record.space.storage.repository;
            if trackers
                .iter()
                .any(|(s, _)| &s.storage.repository == storage)
            {
                continue;
            }
            if let Ok(tracker) = self.tracker(&record.space) {
                trackers.push((record.space, tracker));
            }
        }
        Ok(trackers)
    }
    /// The record keys of the GitHub trackers of the Space, or of every Space, which sync; a
    /// local tracker never does.
    pub(crate) fn issue_sync_keys(&self, space_id: Option<&SpaceId>) -> Result<Vec<String>> {
        Ok((self.trackers()?.into_iter())
            .filter(|(space, _)| space_id.is_none_or(|id| self.shares_storage(id, space)))
            .filter_map(|(space, _)| match space.storage.repository {
                StorageRepository::GitHub { .. } => {
                    Some(repos::key(&space.storage.repository.key()))
                }
                StorageRepository::Local { .. } => None,
            })
            .collect())
    }
    fn shares_storage(&self, id: &SpaceId, space: &Space) -> bool {
        (self.space(id)).is_ok_and(|r| r.space.storage.repository == space.storage.repository)
    }
    /// Runs the passes of the Space's tracker, or of every tracker, now, on this thread.
    pub(crate) fn sync_issues(
        &mut self,
        space_id: Option<&SpaceId>,
        tools: &publish::Tools,
    ) -> Result<()> {
        for key in self.issue_sync_keys(space_id)? {
            if self.settings_key().as_ref() == Some(&key) {
                self.sync_now(tools)?;
            } else if let Some(pass) = self.storage_pass(&key)? {
                let passed = storage::pass(pass, tools);
                self.finish_storage_pass(&key, passed)?;
            }
        }
        Ok(())
    }
    /// Rebuilds the index of every tracker from its files when the index is missing or of
    /// another format.
    pub(crate) fn rebuild_issue_index(&self) -> Result<()> {
        let format: Option<i64> = self
            .db
            .query_row("SELECT format FROM issue_index_format", [], |r| r.get(0))
            .optional()?;
        if format == Some(INDEX_FORMAT) {
            return Ok(());
        }
        // Another format may have other columns, so the table is made again.
        self.db
            .execute_batch("DROP TABLE IF EXISTS issue_index; DELETE FROM issue_index_format;")?;
        self.db.execute_batch(include_str!("issues.sql"))?;
        self.db
            .execute("INSERT INTO issue_index_format VALUES (?1)", [INDEX_FORMAT])?;
        for (space, tracker) in self.trackers()? {
            self.reindex_tracker(&space, &tracker.prefix)?;
        }
        Ok(())
    }
    /// Reads every issue of the Space's tracker into the index again. A tracker this Mac has
    /// never read starts with every issue read, so joining a team's backlog marks none new.
    pub(crate) fn reindex_tracker(&self, space: &Space, prefix: &str) -> Result<()> {
        let storage = space.storage.repository.key();
        let store = self.store(space, prefix);
        let tx = self.db.unchecked_transaction()?;
        let seen: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM issue_index WHERE storage=?1) OR EXISTS(SELECT 1 FROM issue_read WHERE storage=?1)",
            [&storage],
            |r| r.get(0),
        )?;
        tx.execute("DELETE FROM issue_index WHERE storage=?1", [&storage])?;
        for id in store.ids() {
            index(&tx, &store.indexed(&id, &storage)?)?;
        }
        if !seen {
            tx.execute(
                "INSERT OR REPLACE INTO issue_read SELECT storage,id,version FROM issue_index WHERE storage=?1",
                [&storage],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Reads one issue of a storage repository into the index again, or drops it when its
    /// folder is gone.
    pub(crate) fn reindex_issue(&self, storage: &StorageRepository, id: &IssueId) -> Result<()> {
        let key = storage.key();
        let store = Store {
            folder: self.storage_folder(storage),
            prefix: id.prefix().to_owned(),
        };
        match store.exists(id) {
            true => index(&self.db, &store.indexed(id, &key)?)?,
            false => {
                for table in ["issue_index", "issue_read"] {
                    self.db.execute(
                        &format!("DELETE FROM {table} WHERE storage=?1 AND id=?2"),
                        params![key, id.to_string()],
                    )?;
                }
            }
        }
        Ok(())
    }
    /// Reads into the index each issue whose folder changed on disk since it was indexed, and
    /// drops each one gone, of the tracker in storage repository `only`, or of every tracker:
    /// an issue added or edited by hand, in a local tracker or the clone, is listed as it is.
    /// Says whether it changed the index.
    pub(crate) fn refresh_issue_index(&self, only: Option<&str>) -> Result<bool> {
        let mut refreshed = false;
        for (space, tracker) in self.trackers()? {
            let storage = space.storage.repository.key();
            if only.is_some_and(|only| only != storage) {
                continue;
            }
            let store = self.store(&space, &tracker.prefix);
            let mut stamps: BTreeMap<String, String> = (self
                .db
                .prepare("SELECT id,stamp FROM issue_index WHERE storage=?1")?)
            .query_map([&storage], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
            let changed: Vec<IssueId> = (store.ids().into_iter())
                .filter(|id| stamps.remove(&id.to_string()) != Some(store.stamp(id)))
                .collect();
            if changed.is_empty() && stamps.is_empty() {
                continue;
            }
            let tx = self.db.unchecked_transaction()?;
            for id in &changed {
                index(&tx, &store.indexed(id, &storage)?)?;
            }
            for gone in stamps.keys() {
                tx.execute(
                    "DELETE FROM issue_index WHERE storage=?1 AND id=?2",
                    params![storage, gone],
                )?;
            }
            tx.commit()?;
            refreshed = true;
        }
        Ok(refreshed)
    }
    /// Refreshes the index of the Space's tracker, or of every tracker, as listing does; says
    /// whether it changed, so the snapshot's counts can follow a read.
    pub(crate) fn refresh_issues_of(&self, space_id: Option<&SpaceId>) -> Result<bool> {
        let storage = match space_id {
            Some(id) => Some(self.space(id)?.space.storage.repository.key()),
            None => None,
        };
        self.refresh_issue_index(storage.as_deref())
    }
    /// The issues of the Space's tracker, or of every tracker here, with their unread marks; with
    /// `search`, only those whose ID, title, description or comments hold each of its words.
    pub fn list_issues(
        &self,
        space_id: Option<&SpaceId>,
        search: Option<&str>,
    ) -> Result<Vec<IssueListing>> {
        self.refresh_issues_of(space_id)?;
        let storage = match space_id {
            Some(id) => Some(self.space(id)?.space.storage.repository.key()),
            None => None,
        };
        let words: Vec<String> = (search.unwrap_or_default().split_whitespace())
            .map(str::to_lowercase)
            .collect();
        let mut listed = vec![];
        let mut rows = self.db.prepare(
            "SELECT i.data, i.text, r.version IS NOT i.version FROM issue_index i LEFT JOIN issue_read r ON r.storage=i.storage AND r.id=i.id WHERE ?1 IS NULL OR i.storage=?1",
        )?;
        let rows = rows.query_map([storage], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })?;
        for row in rows {
            let (data, text, unread) = row?;
            if words.iter().all(|word| text.contains(word.as_str())) {
                let listing: IssueListing = serde_json::from_str(&data)?;
                listed.push(IssueListing { unread, ..listing });
            }
        }
        listed.sort_by(|a, b| (&a.tracker, &a.id).cmp(&(&b.tracker, &b.id)));
        Ok(listed)
    }
    /// Marks the issue read on this Mac as its files are now. Nothing is written to its tracker.
    /// Its news in the Inbox is settled too, for every project at once.
    pub fn mark_issue_read(&self, id: &IssueId) -> Result<()> {
        let (space, _) = self.tracker_for(id.prefix())?;
        let storage = &space.storage.repository;
        self.reindex_issue(storage, id)?;
        self.mark_read(&storage.key(), id)?;
        self.settle_issue_news(&storage.key(), id)
    }
    fn mark_read(&self, storage: &str, id: &IssueId) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO issue_read SELECT storage,id,version FROM issue_index WHERE storage=?1 AND id=?2",
            params![storage, id.to_string()],
        )?;
        Ok(())
    }
    fn is_read(&self, storage: &str, id: &IssueId) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM issue_index i JOIN issue_read r ON r.storage=i.storage AND r.id=i.id AND r.version=i.version WHERE i.storage=?1 AND i.id=?2)",
            params![storage, id.to_string()],
            |r| r.get(0),
        )?)
    }
    /// One issue, read from its files, with its paused files.
    pub fn read_issue(&self, id: &IssueId) -> Result<IssueRecord> {
        let (space, _) = self.tracker_for(id.prefix())?;
        let storage = &space.storage.repository;
        let store = self.store(&space, id.prefix());
        // A temporary ID this Mac gave an issue since pushed reads as its number.
        let numbered: Option<String> = match id.is_temporary() && !store.exists(id) {
            true => self
                .db
                .query_row(
                    "SELECT number FROM issue_temporary WHERE storage=?1 AND temporary=?2",
                    params![storage.key(), id.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .flatten(),
            false => None,
        };
        let id = &match numbered {
            Some(number) => IssueId::parse(&number)?,
            None => id.clone(),
        };
        let mut record = store.record(id, &storage.key())?;
        record.links = self.issue_links(&storage.key(), id)?;
        record.following = self.is_following(&storage.key(), &record)?;
        if let StorageRepository::GitHub { .. } = storage {
            let under = format!("{}/%", issue_folder(id));
            record.paused = (self.db.prepare(
                "SELECT path,why FROM sync_paused WHERE repository=?1 AND path LIKE ?2",
            )?)
            .query_map(params![repos::key(&storage.key()), under], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (path, why) = row?;
                Ok(PausedIssueFile {
                    path,
                    why: serde_json::from_str(&why)?,
                })
            })
            .collect::<Result<_>>()?;
            // Each paused part, as `issue.md#part`.
            let parts = (self.db.prepare(
                "SELECT path||'#'||part,by FROM issue_parts WHERE repository=?1 AND path LIKE ?2 ORDER BY part",
            )?)
            .query_map(params![repos::key(&storage.key()), under], |r| {
                Ok(PausedIssueFile {
                    path: r.get(0)?,
                    why: Paused::TwoMachines { machine: r.get(1)? },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
            record.paused.extend(parts);
        }
        Ok(record)
    }
    /// Files a new issue, in Triage unless the form chose another state: numbered at once in a
    /// local tracker, by the same rule as a push; with a temporary ID in a GitHub tracker until
    /// a pass pushes it.
    pub fn create_issue(&mut self, space_id: &SpaceId, new: NewIssue) -> Result<Issue> {
        let author = self.issue_author()?;
        let space = self.space(space_id)?.space;
        let tracker = self.tracker(&space)?;
        let prefix = tracker.prefix;
        let folder = self.storage_folder(&space.storage.repository);
        let id = match space.storage.repository {
            StorageRepository::Local { .. } => {
                let store = self.store(&space, &prefix);
                let committed = highest_number(&folder, "HEAD", &prefix)?;
                let here = (store.ids().into_iter())
                    .filter_map(|id| match id {
                        IssueId::Numbered { number, .. } => Some(number),
                        IssueId::Temporary { .. } => None,
                    })
                    .max()
                    .unwrap_or(0);
                IssueId::Numbered {
                    prefix: prefix.clone(),
                    number: committed.max(here) + 1,
                }
            }
            StorageRepository::GitHub { .. } => {
                self.temporary_id(&space.storage.repository, &prefix)?
            }
        };
        let issue = Issue {
            id: id.clone(),
            title: validate_title(&new.title)?,
            state: new.state,
            priority: new.priority,
            assignee: new.assignee,
            labels: new.labels,
            repositories: new.repositories,
            blocks: vec![],
            blocked_by: vec![],
            related: vec![],
            duplicate_of: None,
            author,
            created: schedules::rfc3339(now()),
            description: new.description,
            extra: Extra::new(),
        };
        let files = vec![(issue_file(&id), issue_md(&issue)?)];
        self.write_issue_files(
            &space,
            Write {
                id,
                files,
                did: "filed".into(),
            },
        )?;
        self.db.execute_batch("DELETE FROM issue_last_filed")?;
        self.db.execute(
            "INSERT INTO issue_last_filed(space) VALUES (?1)",
            [space.id.as_str()],
        )?;
        Ok(issue)
    }
    /// A temporary ID no issue folder of the GitHub tracker has, recorded as this Mac's until a
    /// pass numbers it.
    pub(crate) fn temporary_id(
        &self,
        storage: &StorageRepository,
        prefix: &str,
    ) -> Result<IssueId> {
        let folder = self.storage_folder(storage);
        loop {
            let id = IssueId::Temporary {
                prefix: prefix.to_owned(),
                letters: temporary_letters(),
            };
            if !folder.join(issue_folder(&id)).exists() {
                self.db.execute(
                    "INSERT INTO issue_temporary(storage,temporary) VALUES (?1,?2)",
                    params![storage.key(), id.to_string()],
                )?;
                return Ok(id);
            }
        }
    }
    /// The Space of the last issue filed on this Mac, while it is here.
    pub(crate) fn last_filed_space(&self) -> Result<Option<SpaceId>> {
        Ok(self
            .db
            .query_row(
                "SELECT space FROM issue_last_filed WHERE space IN (SELECT id FROM spaces)",
                [],
                |r| r.get(0).map(SpaceId),
            )
            .optional()?)
    }
    pub(crate) fn store(&self, space: &Space, prefix: &str) -> Store {
        Store {
            folder: self.storage_folder(&space.storage.repository),
            prefix: prefix.to_owned(),
        }
    }
    /// The author, Space, store and current `issue.md` of an issue the human writes to.
    fn writable(&self, id: &IssueId) -> Result<(String, Space, Store, Issue)> {
        let author = self.issue_author()?;
        let (space, _) = self.tracker_for(id.prefix())?;
        let store = self.store(&space, id.prefix());
        let current = store.issue(id)?;
        Ok((author, space, store, current))
    }
    /// Replaces the issue's fields, keeping its ID, state, author, creation and the fields this
    /// release does not know; a changed priority or assignee is also an event.
    pub fn edit_issue(&mut self, edited: Issue) -> Result<Issue> {
        let id = edited.id.clone();
        let (by, space, store, current) = self.writable(&id)?;
        let issue = Issue {
            title: validate_title(&edited.title)?,
            state: current.state,
            author: current.author.clone(),
            created: current.created.clone(),
            extra: current.extra.clone(),
            ..edited
        };
        if issue == current {
            return Ok(issue);
        }
        let at = schedules::rfc3339(now());
        let mut events = vec![];
        if issue.priority != current.priority {
            events.push(event(
                &by,
                &at,
                "priority",
                json!(current.priority),
                json!(issue.priority),
            ));
        }
        if issue.assignee != current.assignee {
            events.push(event(
                &by,
                &at,
                "assignee",
                json!(current.assignee),
                json!(issue.assignee),
            ));
        }
        let mut files = vec![(issue_file(&id), issue_md(&issue)?)];
        if !events.is_empty() {
            let content = with_events(store.read(&events_file(&id)), &events)?;
            files.push((events_file(&id), content));
        }
        let did = "edited".into();
        self.write_issue_files(&space, Write { id, files, did })?;
        Ok(issue)
    }
    /// Moves the issue to `state`, recording the move as an event, with the state its linked
    /// work derived when the move overrides it; the issue follows its work again once that
    /// changes.
    pub fn move_issue(&mut self, id: &IssueId, state: IssueState) -> Result<Issue> {
        let (by, space, store, current) = self.writable(id)?;
        if current.state == state {
            return Ok(current);
        }
        let at = schedules::rfc3339(now());
        let mut moved = event(&by, &at, "state", json!(current.state), json!(state));
        let derived = self.derived_state(&space.storage.repository.key(), id)?;
        if let Some(derived) = derived.filter(|derived| *derived != state) {
            moved.fields.insert("overrode".into(), json!(derived));
        }
        let issue = Issue { state, ..current };
        let files = vec![
            (issue_file(id), issue_md(&issue)?),
            (
                events_file(id),
                with_events(store.read(&events_file(id)), &[moved])?,
            ),
        ];
        let did = format!("moved to {}", state.label());
        let id = id.clone();
        self.write_issue_files(&space, Write { id, files, did })?;
        Ok(issue)
    }
    /// Adds the human's comment as a file of its own.
    pub fn comment_on_issue(&mut self, id: &IssueId, body: &str) -> Result<Comment> {
        let (author, space, store, _) = self.writable(id)?;
        ensure!(!body.trim().is_empty(), "A comment needs text");
        let mut comment = Comment {
            file: String::new(),
            created: schedules::rfc3339(now()),
            author,
            edited: None,
            assistant_for: None,
            body: body.trim_end().to_owned(),
        };
        let content = comment_md(&comment)?;
        let comments = store.folder.join(issue_folder(id)).join("comments");
        comment.file = comment_file(&comments, &comment.created, &comment.author, &content);
        let path = format!("{}/comments/{}", issue_folder(id), comment.file);
        let files = vec![(path, content)];
        let (id, did) = (id.clone(), "commented".into());
        self.write_issue_files(&space, Write { id, files, did })?;
        Ok(comment)
    }
    /// Writes one change's files to the Space's tracker: committed at once in a local
    /// repository, and recorded for the next pass, due two seconds later, in a GitHub one.
    /// Refuses a write over an issue file of this Mac's clone that holds neither what the host
    /// last wrote nor what it last synced, such as an agent's shell edit: building on it would
    /// record that edit as the host's, and the next pass would push it.
    pub(crate) fn write_issue_files(&mut self, space: &Space, write: Write) -> Result<()> {
        let message = format!("{}: {}", write.id, write.did);
        self.write_issues(space, vec![write], &message)
    }
    /// `write_issue_files` for several issues at once, as one commit with `message` in a local
    /// tracker; none is written while any file would be written over an outside edit.
    pub(crate) fn write_issues(
        &mut self,
        space: &Space,
        writes: Vec<Write>,
        message: &str,
    ) -> Result<()> {
        let storage = &space.storage.repository;
        for write in &writes {
            let paths: Vec<&str> = write.files.iter().map(|(p, _)| p.as_str()).collect();
            ensure!(
                !self.changed_outside(storage, &paths)?,
                "{} was changed outside Wiffletree on this Mac; settle it in the Inbox first",
                write.id
            );
        }
        let folder = self.storage_folder(storage);
        let files = || writes.iter().flat_map(|write| &write.files);
        for (path, content) in files() {
            shared::write_atomically(&folder.join(path), content)?;
        }
        match storage {
            StorageRepository::Local { .. } => {
                let paths: Vec<&str> = files().map(|(p, _)| p.as_str()).collect();
                storage::commit_local(&folder, &paths, message)?;
            }
            StorageRepository::GitHub { .. } => {
                let key = repos::key(&storage.key());
                for (path, content) in files() {
                    Record::wrote(&self.db, &key, path, Some(content))?;
                }
                self.issues_written.borrow_mut().insert(key);
            }
        }
        // The human's own change is no news to them.
        for write in &writes {
            self.reindex_issue(storage, &write.id)?;
            self.mark_read(&storage.key(), &write.id)?;
        }
        Ok(())
    }
    /// Whether any of these files of a GitHub tracker's clone holds neither what the host last
    /// wrote nor what it last synced, which a pass pauses as changed outside Wiffletree. A file
    /// paused because two Macs changed it still takes the human's edits. A local tracker, never
    /// pushed, has no such files.
    pub(crate) fn changed_outside(
        &self,
        storage: &StorageRepository,
        paths: &[&str],
    ) -> Result<bool> {
        if let StorageRepository::Local { .. } = storage {
            return Ok(false);
        }
        let key = repos::key(&storage.key());
        let folder = self.storage_folder(storage);
        for path in paths {
            let (synced, written): (Content, Content) = self
                .db
                .query_row(
                    "SELECT synced,written FROM sync_record WHERE repository=?1 AND path=?2",
                    params![key, path],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .unwrap_or_default();
            let here = fs::read(folder.join(path)).ok();
            if here != synced && here != written {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// After a pass pushed new issues as `numbered`, renames each one's folder in the clone
    /// from its temporary ID and rewrites the temporary ID in the files under `issues/`, as the
    /// pushed commit already has them. Each file moved or rewritten is noted as the host wrote
    /// it, so one the human changed meanwhile is pushed by the next pass and none is paused.
    pub(crate) fn apply_numbers(
        &self,
        storage: &StorageRepository,
        numbered: &[(IssueId, IssueId)],
    ) -> Result<()> {
        let key = repos::key(&storage.key());
        let clone = self.storage_folder(storage);
        for (temporary, number) in numbered {
            let (from, to) = (
                clone.join(issue_folder(temporary)),
                clone.join(issue_folder(number)),
            );
            let relative = |file: &Path| -> Result<String> {
                let path = file.strip_prefix(&clone)?.to_str();
                Ok(path.context("A path is not UTF-8")?.to_owned())
            };
            // Each file moved or rewritten: its path and content before, and where it is now.
            let mut touched: Vec<(String, Vec<u8>, PathBuf)> = vec![];
            if from.exists() {
                for file in storage::files_under(&from)? {
                    let moved = to.join(file.strip_prefix(&from)?);
                    if !moved.exists() {
                        let before = fs::read(&file)?;
                        fs::create_dir_all(moved.parent().context("A file is in a folder")?)?;
                        fs::rename(&file, &moved)?;
                        touched.push((relative(&file)?, before, moved));
                    }
                }
                fs::remove_dir_all(&from)?;
            }
            let (old, new) = (temporary.to_string(), number.to_string());
            for file in storage::files_under(&clone.join(ISSUES))? {
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                let rewritten = issues_sync::replace_id(&text, &old, &new);
                if rewritten != text {
                    shared::write_atomically(&file, rewritten.as_bytes())?;
                    if !touched.iter().any(|(_, _, now)| now == &file) {
                        touched.push((relative(&file)?, text.into_bytes(), file));
                    }
                }
            }
            // Each issue the renaming touched, as it was and is now, so one read stays read.
            let mut issues = vec![(temporary.clone(), number.clone())];
            for (was, _, now) in &touched {
                let pair = (issue_of(was), issue_of(&relative(now)?));
                if let (Some(was), Some(now)) = pair
                    && !issues.contains(&(was.clone(), now.clone()))
                {
                    issues.push((was, now));
                }
            }
            // Only a file that held what the host wrote or synced is the host's write now; one
            // changed outside Wiffletree stays unrecorded and is paused again.
            for (was, before, now) in touched {
                let recorded: Option<(Content, Content)> = self
                    .db
                    .query_row(
                        "SELECT synced,written FROM sync_record WHERE repository=?1 AND path=?2",
                        params![key, was],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let (synced, written) = recorded.unwrap_or_default();
                if [synced, written].contains(&Some(before)) {
                    Record::wrote(&self.db, &key, &relative(&now)?, Some(&fs::read(&now)?))?;
                }
            }
            self.db.execute(
                "DELETE FROM sync_record WHERE repository=?1 AND path LIKE ?2",
                params![key, format!("{}/%", issue_folder(temporary))],
            )?;
            for table in [
                "issue_links",
                "issue_derived",
                "issue_pending",
                "issue_follows",
            ] {
                self.db.execute(
                    &format!("UPDATE {table} SET id=?3 WHERE storage=?1 AND id=?2"),
                    params![storage.key(), old, new],
                )?;
            }
            self.db.execute(
                "UPDATE issue_temporary SET number=?3 WHERE storage=?1 AND temporary=?2",
                params![storage.key(), old, new],
            )?;
            for (was, now) in issues {
                let read = self.is_read(&storage.key(), &was)?;
                self.reindex_issue(storage, &was)?;
                self.reindex_issue(storage, &now)?;
                if read {
                    self.mark_read(&storage.key(), &now)?;
                }
            }
        }
        Ok(())
    }
    /// The snapshot's summary of each Space's tracker. Reads the index and sync's record only.
    pub(crate) fn issue_trackers(&self) -> Result<Vec<IssueTracker>> {
        let mut trackers = vec![];
        for record in self.spaces()? {
            let space = record.space;
            let storage = space.storage.repository.key();
            let prefix = self.tracker(&space).ok().map(|tracker| tracker.prefix);
            let (issues, open, unread): (usize, usize, usize) = self.db.query_row(
                "SELECT count(*), coalesce(sum(i.state NOT IN ('done','canceled')),0), coalesce(sum(r.version IS NOT i.version),0) FROM issue_index i LEFT JOIN issue_read r ON r.storage=i.storage AND r.id=i.id WHERE i.storage=?1",
                [&storage],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let local = matches!(space.storage.repository, StorageRepository::Local { .. });
            let key = repos::key(&storage);
            // The tracker's files in a table of sync, which `also` narrows.
            let count = |table: &str, also: &str| -> Result<usize> {
                let sql = format!(
                    "SELECT count(*) FROM {table} WHERE repository=?1 AND (path='{TRACKER}' OR path LIKE '{ISSUES}/%'){also}"
                );
                Ok(self.db.query_row(&sql, [&key], |r| r.get(0))?)
            };
            let (waiting, paused) = match local {
                true => (0, 0),
                false => (
                    count("sync_record", " AND written IS NOT synced")?,
                    count("sync_paused", "")? + count("issue_parts", "")?,
                ),
            };
            let (synced_at, error) = match local {
                true => (None, None),
                false => self.storage_sync_state(&key)?,
            };
            trackers.push(IssueTracker {
                space_id: space.id,
                tracker: storage,
                prefix,
                local,
                login: self.gh_login.clone(),
                issues,
                open,
                unread,
                waiting,
                paused,
                synced_at,
                error,
            });
        }
        Ok(trackers)
    }
}

fn index(db: &Connection, indexed: &Indexed) -> Result<()> {
    let listing = &indexed.listing;
    let state = listing
        .state
        .map(|state| json!(state).as_str().map(str::to_owned));
    db.execute(
        "INSERT OR REPLACE INTO issue_index(storage,id,state,data,stamp,version,text,people) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            listing.tracker,
            listing.id.to_string(),
            state.flatten(),
            encode(listing)?,
            indexed.stamp,
            indexed.version,
            indexed.text,
            encode(&indexed.people)?,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn id(text: &str) -> IssueId {
        IssueId::parse(text).unwrap()
    }

    /// An issue in the Celadon sample project, with invented people.
    pub(crate) fn blurry(text: &str) -> Issue {
        Issue {
            id: id(text),
            title: "Theme previews look blurry on Retina displays".into(),
            state: IssueState::Triage,
            priority: Priority::High,
            assignee: Some("pshah".into()),
            labels: vec!["bug".into()],
            repositories: vec!["celadon-theme/website".into()],
            blocks: vec![],
            blocked_by: vec![],
            related: vec!["CEL-148".into()],
            duplicate_of: None,
            author: "treyes".into(),
            created: "2026-10-29T14:02:11Z".into(),
            description: "Screenshots are scaled at 1x.\n\nSee the gallery page.".into(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn an_issue_a_comment_and_events_survive_a_round_trip() {
        let issue = blurry("CEL-152");
        let written = issue_md(&issue).unwrap();
        let text = String::from_utf8(written.clone()).unwrap();
        assert!(text.starts_with("---\nid: CEL-152\ntitle: "), "{text}");
        assert!(text.contains("\nstate: triage\npriority: high\nassignee: pshah\n"));
        assert!(text.ends_with("---\n\nScreenshots are scaled at 1x.\n\nSee the gallery page.\n"));
        assert_eq!(parse_issue(&written).unwrap(), issue);
        let comment = Comment {
            file: "2026-10-29T14-02-11Z-pshah.md".into(),
            author: "pshah".into(),
            created: "2026-10-29T14:02:11Z".into(),
            edited: None,
            assistant_for: None,
            body: "It is the 1x export.".into(),
        };
        let content = comment_md(&comment).unwrap();
        assert_eq!(
            String::from_utf8(content.clone()).unwrap(),
            "---\nauthor: pshah\ncreated: 2026-10-29T14:02:11Z\n---\n\nIt is the 1x export.\n"
        );
        assert_eq!(parse_comment(&comment.file, &content).unwrap(), comment);
        let moved = event(
            "pshah",
            "2026-10-29T14:03:00Z",
            "state",
            json!("triage"),
            json!("todo"),
        );
        let lines = with_events(Some(b"not json".to_vec()), std::slice::from_ref(&moved)).unwrap();
        assert_eq!(
            parse_events(&lines),
            [moved],
            "a line that does not parse is skipped"
        );
        assert!(lines.starts_with(b"not json\n"), "and kept");
    }

    #[test]
    fn a_field_from_a_newer_release_is_kept_and_only_the_changed_line_differs() {
        let issue = blurry("CEL-152");
        let mut text = String::from_utf8(issue_md(&issue).unwrap()).unwrap();
        text = text.replacen(
            "created: 2026-10-29T14:02:11Z\n",
            "created: 2026-10-29T14:02:11Z\nestimate: 3\n",
            1,
        );
        let read = parse_issue(text.as_bytes()).unwrap();
        assert_eq!(read.extra["estimate"], json!(3));
        let edited = Issue {
            priority: Priority::Low,
            ..read
        };
        let written = String::from_utf8(issue_md(&edited).unwrap()).unwrap();
        let changed: Vec<(&str, &str)> = (text.lines().zip(written.lines()))
            .filter(|(before, after)| before != after)
            .collect();
        assert_eq!(changed, [("priority: high", "priority: low")]);
        assert_eq!(text.lines().count(), written.lines().count());
    }

    #[test]
    fn an_issue_that_does_not_parse_is_listed_by_its_folder_and_shown_raw() {
        let folder = tempfile::tempdir().unwrap();
        let raw = "---\ntitle: [unclosed\n---\nBody\n";
        let at = folder.path().join("issues/CEL-160");
        fs::create_dir_all(&at).unwrap();
        fs::write(at.join("issue.md"), raw).unwrap();
        fs::create_dir_all(folder.path().join("issues/notes")).unwrap();
        let store = Store {
            folder: folder.path().into(),
            prefix: "CEL".into(),
        };
        assert_eq!(
            store.ids(),
            [id("CEL-160")],
            "a folder named by no ID is no issue"
        );
        let record = store
            .record(&id("CEL-160"), "celadon-theme/storage")
            .unwrap();
        let listing = listing(&record);
        assert_eq!(listing.title, "");
        assert!(listing.problem.unwrap().contains("front matter"));
        assert_eq!(record.raw.as_deref(), Some(raw));
        assert!(record.issue.is_none() && record.problem.is_some());
        assert_eq!(
            fs::read_to_string(at.join("issue.md")).unwrap(),
            raw,
            "never rewritten"
        );
    }

    #[test]
    fn a_tracker_with_other_states_is_refused_and_one_without_states_has_the_fixed_ones() {
        let other = "format: 1\nprefix: CEL\nstates: [triage, done]\n";
        let error = storage::check(TRACKER, other.as_bytes()).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "This tracker uses states this release does not know"
        );
        storage::check(TRACKER, b"format: 1\nprefix: CEL\n").unwrap();
        let written = serde_yaml::to_string(&TrackerConfig::new("CEL".into())).unwrap();
        assert_eq!(
            written,
            "format: 1\nprefix: CEL\nstates:\n- triage\n- backlog\n- todo\n- in_progress\n- in_review\n- done\n- canceled\nlabels: []\n"
        );
    }

    #[test]
    fn a_second_comment_in_the_same_second_gets_its_own_file() {
        let folder = tempfile::tempdir().unwrap();
        let first = comment_file(folder.path(), "2026-10-29T14:02:11Z", "jlin", b"one");
        assert_eq!(first, "2026-10-29T14-02-11Z-jlin.md");
        fs::write(folder.path().join(&first), "one").unwrap();
        let again = comment_file(folder.path(), "2026-10-29T14:02:11Z", "jlin", b"one");
        assert_eq!(again, first, "the same content is the same comment");
        let second = comment_file(folder.path(), "2026-10-29T14:02:11Z", "jlin", b"two");
        assert_eq!(second, "2026-10-29T14-02-11Z-jlin-2.md");
    }
}
