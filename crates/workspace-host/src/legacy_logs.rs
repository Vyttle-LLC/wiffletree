//! Reading a legacy log (OpenSpec change `one-space-repository`, decision 7): each `## YYYY-MM-DD
//! — Title` entry of a brain's legacy `logs/<repo>.md` file parses into one entry in the contract,
//! with `source: skill`, for Bring in a brain, which reads a brain folder on this Mac. An entry's id
//! comes from its repository, date, title and text, so bringing it in again adds nothing. Nothing
//! here logs an entry's text.
use super::*;
use folder::Root;
use memory::{
    MAX_BODY_CHARS, MAX_TITLE_CHARS, NO_REF, body_chars, entry_body, one_line, valid_login,
    valid_ref,
};
use publish::Tools;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The longest `ref` an imported entry keeps as its `ref`.
const MAX_REF_CHARS: usize = 255;
/// The entry contract's fields.
const FIELDS: [&str; 5] = ["type", "ref", "changed", "why", "state"];
/// The fields a legacy entry may carry that the entry contract has no place for: the old
/// Markdown export's own bookkeeping, which the host store already holds.
const EXPORT_KEYS: [&str; 6] = [
    "entry-id",
    "project",
    "task",
    "session",
    "milestone",
    "evidence",
];

/// `brought_compiled` holds the ids of brought-in entries the original had compiled until a
/// pass lands them in the folder's `compiled-after`.
pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS legacy_imports (brain TEXT PRIMARY KEY COLLATE NOCASE, done_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS legacy_compiled (brain TEXT NOT NULL COLLATE NOCASE, entry_id TEXT NOT NULL, PRIMARY KEY(brain,entry_id));
CREATE TABLE IF NOT EXISTS brought_compiled (repository TEXT NOT NULL, entry_id TEXT NOT NULL, PRIMARY KEY(repository,entry_id));
";

/// One entry of a legacy log as written, before it is fitted to the contract.
#[derive(Debug, Default, PartialEq)]
struct LegacyEntry {
    /// The heading's line in the file, 1-based.
    line: usize,
    date: String,
    title: String,
    /// The entry's text after its heading, exactly as the file holds it.
    text: String,
    fields: BTreeMap<String, String>,
    /// Lines that are none of the five fields, kept in `changed`.
    extra: Vec<String>,
}

/// A legacy log's entries and its `<!-- compiled-through: YYYY-MM-DD -->` date.
struct LegacyLog {
    entries: Vec<LegacyEntry>,
    compiled_through: Option<String>,
}

/// `YYYY-MM-DD — Title` after `## `, accepting the dashes the skills have written.
fn heading(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("## ")?;
    let date = rest.get(..10)?;
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let title = rest[10..].trim_start();
    let title = ["—", "–", "-"]
        .iter()
        .find_map(|dash| title.strip_prefix(dash))?
        .trim();
    (!title.is_empty()).then(|| (date.to_owned(), title.to_owned()))
}

fn compiled_marker(line: &str) -> Option<String> {
    let date = line
        .trim()
        .strip_prefix("<!-- compiled-through:")?
        .strip_suffix("-->")?
        .trim();
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(date.to_owned())
}

/// `- key: value` with a lowercase key.
fn field(line: &str) -> Option<(String, &str)> {
    let (key, value) = line.strip_prefix("- ")?.split_once(':')?;
    let named = !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    named.then(|| (key.to_owned(), value.trim()))
}

fn parse(text: &str) -> LegacyLog {
    let mut entries: Vec<LegacyEntry> = vec![];
    let mut compiled_through = None;
    // Which part of the current entry a continuation line belongs to.
    enum Into {
        Field(String),
        Extra,
        Dropped,
    }
    let mut into = Into::Extra;
    for (index, line) in text.lines().enumerate() {
        if let Some(date) = compiled_marker(line) {
            compiled_through = compiled_through.max(Some(date));
            continue;
        }
        if let Some((date, title)) = heading(line) {
            entries.push(LegacyEntry {
                line: index + 1,
                date,
                title,
                ..Default::default()
            });
            into = Into::Extra;
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        entry.text.push_str(line);
        entry.text.push('\n');
        let trimmed = line.trim();
        // The old export's entry markers and blank lines carry nothing.
        if trimmed.is_empty() || (trimmed.starts_with("<!--") && trimmed.ends_with("-->")) {
            continue;
        }
        match field(trimmed) {
            Some((key, value)) if FIELDS.contains(&key.as_str()) => {
                // A repeated field keeps both values.
                let joined = entry.fields.entry(key.clone()).or_default();
                if !joined.is_empty() {
                    joined.push_str("; ");
                }
                joined.push_str(value);
                into = Into::Field(key);
            }
            Some((key, _)) if EXPORT_KEYS.contains(&key.as_str()) => into = Into::Dropped,
            Some(_) => {
                entry.extra.push(trimmed.to_owned());
                into = Into::Extra;
            }
            None => match &into {
                Into::Field(key) => {
                    let value = entry.fields.entry(key.clone()).or_default();
                    value.push(' ');
                    value.push_str(trimmed);
                }
                Into::Extra => entry.extra.push(trimmed.to_owned()),
                Into::Dropped => {}
            },
        }
    }
    LegacyLog {
        entries,
        compiled_through,
    }
}

/// The entry's id: a ULID whose time is its date at 12:00 UTC and whose random part comes from a
/// hash of the repository, date, title and text.
fn legacy_id(repository: &str, entry: &LegacyEntry) -> String {
    let noon = chrono::NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(12, 0, 0))
        .map(|at| at.and_utc().timestamp_millis())
        .unwrap_or_default();
    let mut hash = Sha256::new();
    for part in [repository, &entry.date, &entry.title, entry.text.trim()] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    let digest = hash.finalize();
    let mut random = [0u8; 16];
    random[6..].copy_from_slice(&digest[..10]);
    brains::ulid(noon, u128::from_be_bytes(random))
}

/// `text` cut so that it is `by` characters shorter once `suffix` replaces what was cut.
fn shortened(text: &str, by: usize, suffix: &str) -> String {
    let keep = text
        .chars()
        .count()
        .saturating_sub(by + suffix.chars().count());
    let kept: String = text.chars().take(keep).collect();
    format!("{}{suffix}", kept.trim_end())
}

/// The entry fitted to the contract: one-line fields, `—` for an empty one, `observation` for a
/// type the contract does not know, and a `ref` the contract does not allow moved into `state`.
/// Over the cap, `changed` is shortened to end with a pointer to the legacy file as `version`
/// names it, such as "at 3c1e9a0", and then `state`, `why` and the title, should the rest still
/// not fit. Also answers whether it was shortened.
fn fitted(repository: &str, entry: &LegacyEntry, version: &str) -> (LogInput, String, bool) {
    let value = |key: &str| one_line(entry.fields.get(key).map_or("", String::as_str));
    let or_none = |text: String| {
        if text.is_empty() {
            NO_REF.to_owned()
        } else {
            text
        }
    };
    let mut changed = value("changed");
    for extra in &entry.extra {
        changed = format!("{changed} {}", one_line(extra)).trim().to_owned();
    }
    let mut state = value("state");
    let mut reference = value("ref");
    // A ref is never shortened, so one too long to leave room for the rest moves into `state`.
    if !valid_ref(&reference) || reference.chars().count() > MAX_REF_CHARS {
        if !reference.is_empty() {
            state = format!("{state} (legacy ref: {reference})")
                .trim()
                .to_owned();
        }
        reference = NO_REF.into();
    }
    let mut title = one_line(&entry.title);
    if title.chars().count() > MAX_TITLE_CHARS {
        title = shortened(&title, title.chars().count() - MAX_TITLE_CHARS, "…");
    }
    let mut input = LogInput {
        session_id: String::new(),
        milestone: None,
        kind: match value("type").as_str() {
            "decision" => LogKind::Decision,
            _ => LogKind::Observation,
        },
        title,
        repository: None,
        ticket_id: None,
        changed: or_none(changed),
        why: or_none(value("why")),
        state: or_none(state),
    };
    let pointer = format!("… (full text in legacy logs/{repository}.md {version})");
    let over = |input: &LogInput| {
        body_chars(&entry_body(&entry.date, input, &reference)).saturating_sub(MAX_BODY_CHARS)
    };
    let by = over(&input);
    if by > 0 {
        input.changed = shortened(&input.changed, by, &pointer);
    }
    let rest: [fn(&mut LogInput) -> &mut String; 3] =
        [|i| &mut i.state, |i| &mut i.why, |i| &mut i.title];
    for part in rest {
        let by = over(&input);
        if by == 0 {
            break;
        }
        let text = part(&mut input);
        *text = shortened(text, by, "…");
    }
    (input, reference, by > 0)
}

/// The commit that last changed each line of `path` as it stands in the working folder of the
/// original at `checkout`, by line number: a changed line has none, and a file never committed
/// none at all. Read with optional locks off, so nothing is written there.
fn blame(checkout: &Path, path: &str) -> Result<HashMap<usize, String>> {
    let file = checkout.join(path);
    let file = file.to_str().context("The file's path is not UTF-8")?;
    let porcelain = runtime::git_bytes_large(
        checkout,
        &["blame", "--porcelain", "--contents", file, "--", path],
        &[],
    )?;
    let porcelain = String::from_utf8_lossy(&porcelain);
    let mut lines = HashMap::new();
    for header in porcelain.lines().filter(|l| !l.starts_with('\t')) {
        let mut words = header.split(' ');
        let (Some(sha), Some(_), Some(line)) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        // Git names a line no commit holds by the zero SHA.
        if sha.len() == 40
            && sha.chars().all(|c| c.is_ascii_hexdigit())
            && sha.chars().any(|c| c != '0')
            && let Ok(line) = line.parse()
        {
            lines.insert(line, sha.to_owned());
        }
    }
    Ok(lines)
}

/// The GitHub login of the commit's author, or `None` when GitHub knows none.
fn commit_login(brain: &str, commit: &str, checkout: &Path, tools: &Tools) -> Option<String> {
    let endpoint = format!("repos/{brain}/commits/{commit}");
    let found = github_access::gh_json(tools, checkout, &endpoint).ok()?;
    let login = found["author"]["login"].as_str()?;
    valid_login(login).then(|| login.to_owned())
}

/// The brain's files beside `logs/` and `notes/` that Bring in copies.
const TOP_PAGES: [&str; 2] = [compile::STATUS, compile::QUEUE];
/// The old brain's config, at its repository's root, read once by Bring in.
const BRAIN_CONFIG: &str = "wiffletree-brain.json";
/// How a legacy entry's pointer names a file Git does not hold as it stands here.
const UNCOMMITTED: &str = "in this Mac's uncommitted copy";

/// What a file Bring in would write is, by its path under the brain folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BroughtKind {
    /// A log entry, compiled in the original or not.
    Entry { id: String, compiled: bool },
    /// A wiki page, `STATUS.md` or `reconcile-queue.md`.
    Page,
    /// `notes/INDEX.md`, without its old cursor, which a folder that has one keeps.
    Index,
}

/// One file Bring in would write into the Space's brain folder.
#[derive(Clone, Debug)]
pub(crate) struct Brought {
    /// Its path under the brain folder, such as `logs/calls/2026/09/2026-09-02-<id>.md`.
    pub(crate) path: String,
    pub(crate) content: Vec<u8>,
    pub(crate) kind: BroughtKind,
}

/// A brain folder on this Mac, read as its files stand, committed or not (decision 7). Nothing
/// was written there.
pub(crate) struct LegacyRead {
    pub(crate) files: Vec<Brought>,
    /// What it brings and leaves, before the Space's folder is compared.
    pub(crate) plan: BringInPlan,
    /// The old brain's `wiffletree-brain.json`, when its clone has one.
    pub(crate) config: Option<Value>,
}

/// Each path of the original Git reports as changed, untracked or ignored, relative to its
/// root, and whether it was never committed.
fn uncommitted(clone: &Path, folder: &str) -> Result<HashMap<String, bool>> {
    let listed = runtime::git_bytes_large(
        clone,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored",
            "--",
            folder,
        ],
        &[],
    )?;
    let listed = String::from_utf8_lossy(&listed);
    let mut paths = HashMap::new();
    let mut parts = listed.split('\0').filter(|part| !part.is_empty());
    while let Some(part) = parts.next() {
        let (Some(code), Some(path)) = (part.get(..2), part.get(3..)) else {
            continue;
        };
        // A rename or copy names its source next, which is not a file here.
        if code.contains(['R', 'C']) {
            parts.next();
        }
        paths.insert(
            path.to_owned(),
            matches!(code, "??" | "!!") || code.starts_with('A'),
        );
    }
    Ok(paths)
}

/// Every regular file under `at` in the brain, by its path relative to the brain: symbolic
/// links and hidden files are passed over. Folders at the top beside `logs/` and `notes/`, such
/// as `archive/`, `automation/` and `raw/`, are never read and go in `left`.
fn brain_files(brain: &Path, at: &str, left: &mut Vec<String>) -> Result<Vec<String>> {
    let mut found = vec![];
    let mut entries: Vec<_> = fs::read_dir(brain.join(at))?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type()?;
        if name.starts_with('.') || kind.is_symlink() {
            continue;
        }
        let path = match at.is_empty() {
            true => name.clone(),
            false => format!("{at}/{name}"),
        };
        if kind.is_dir() {
            if at.is_empty() && !matches!(name.as_str(), "logs" | "notes") {
                left.push(format!("{name}/"));
                continue;
            }
            found.extend(brain_files(brain, &path, left)?);
        } else if kind.is_file() {
            found.push(path);
        }
    }
    Ok(found)
}

/// The ids of the entries the original's cursor says it compiled: those its `compiled-through`
/// commit holds, in any layout, and its `compiled-after` ids. Nothing without Git.
fn compiled_ids(clone: &Path, index: &str, git: bool) -> HashSet<String> {
    let cursor = compile::read_cursor(index);
    let mut ids: HashSet<String> = cursor.after.into_iter().collect();
    let Some(through) = cursor.through.filter(|_| git) else {
        return ids;
    };
    let listed = runtime::git_bytes_large(clone, &["ls-tree", "-r", "--name-only", &through], &[]);
    for path in String::from_utf8_lossy(&listed.unwrap_or_default()).lines() {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 5 && compile::is_entry_path(&parts[parts.len() - 5..].join("/")) {
            ids.extend(compile::id_in_name(path).map(str::to_owned));
        }
    }
    ids
}

impl LegacyRead {
    /// Reads the brain folder `folder` of the clone `from` as its files stand on this disk. With
    /// `authors`, a legacy entry's author is the GitHub login of the commit that added its
    /// heading, as `gh` answers; otherwise, and for a line no commit holds, `importer`.
    pub(crate) fn read(
        from: &Path,
        folder: &str,
        importer: &str,
        authors: bool,
        tools: &Tools,
    ) -> Result<Self> {
        let folder = folder.trim().trim_matches('/');
        validate_root(folder)?;
        ensure!(
            from.is_dir(),
            "{} is not a folder on this Mac",
            from.display()
        );
        brain_sync::refuse_links(from, folder)?;
        let brain = from.join(folder);
        ensure!(brain.is_dir(), "{} has no folder {folder}/", from.display());
        let git = from.join(".git").exists();
        let changed = match git {
            true => uncommitted(from, folder)?,
            false => HashMap::new(),
        };
        // Whether the file at `path` under the brain is not on GitHub, and never committed.
        let state = |path: &str| -> (bool, bool) {
            match changed.get(&format!("{folder}/{path}")) {
                _ if !git => (true, true),
                Some(never) => (true, *never),
                None => (false, false),
            }
        };
        let origin = git
            .then(|| runtime::git_bytes_large(from, &["remote", "get-url", "origin"], &[]).ok())
            .flatten()
            .and_then(|url| pr_watch::github_repository(String::from_utf8_lossy(&url).trim()))
            .map(|(owner, name)| format!("{owner}/{name}"));
        let mut plan = BringInPlan::default();
        let paths = brain_files(&brain, "", &mut plan.left_behind)?;
        let index_text = fs::read_to_string(brain.join(compile::INDEX)).unwrap_or_default();
        let compiled = compiled_ids(from, &index_text, git);
        let mut logins: HashMap<String, Option<String>> = HashMap::new();
        let mut files = vec![];
        for path in paths {
            let in_logs = path.strip_prefix("logs/");
            let page = path.starts_with(compile::NOTES) || TOP_PAGES.contains(&path.as_str());
            if !page && in_logs.is_none() {
                continue;
            }
            let content = fs::read(brain.join(&path))?;
            let (not_on_github, never) = state(&path);
            plan.not_on_github += usize::from(not_on_github);
            plan.never_committed += usize::from(never);
            if path == compile::INDEX {
                let index = compile::without_cursor(&String::from_utf8_lossy(&content));
                files.push(Brought {
                    path,
                    content: index.into_bytes(),
                    kind: BroughtKind::Index,
                });
                continue;
            }
            if page {
                plan.pages += usize::from(path.starts_with(compile::NOTES));
                plan.status |= path == compile::STATUS;
                files.push(Brought {
                    path,
                    content,
                    kind: BroughtKind::Page,
                });
                continue;
            }
            let in_logs = in_logs.unwrap_or_default();
            let text = String::from_utf8_lossy(&content).into_owned();
            // A legacy `logs/<repo>.md` becomes one entry file per entry.
            if let Some(repository) = in_logs.strip_suffix(".md").filter(|r| !r.contains('/')) {
                if !memory::valid_folder(repository) {
                    plan.skipped.push(path);
                    continue;
                }
                let log = parse(&text);
                let version = match (not_on_github, git) {
                    (false, true) => {
                        let at = runtime::git_bytes_large(
                            from,
                            &[
                                "log",
                                "-1",
                                "--format=at %h",
                                "--",
                                &format!("{folder}/{path}"),
                            ],
                            &[],
                        )?;
                        String::from_utf8_lossy(&at).trim().to_owned()
                    }
                    _ => UNCOMMITTED.to_owned(),
                };
                let lines = match (authors, git) {
                    (true, true) => blame(from, &format!("{folder}/{path}")).unwrap_or_default(),
                    _ => HashMap::new(),
                };
                let through = log.compiled_through.clone();
                plan.entry_files += 1;
                for entry in &log.entries {
                    let (input, reference, shortened) = fitted(repository, entry, &version);
                    let id = legacy_id(repository, entry);
                    let commit = lines.get(&entry.line);
                    let login = commit.and_then(|commit| {
                        let origin = origin.as_deref()?;
                        (logins.entry(commit.clone()))
                            .or_insert_with(|| commit_login(origin, commit, from, tools))
                            .clone()
                    });
                    let author = login.unwrap_or_else(|| importer.to_owned());
                    let body = entry_body(&entry.date, &input, &reference);
                    let text = memory::entry_text(&id, repository, &author, "skill", &body);
                    plan.entries += 1;
                    plan.shortened += usize::from(shortened);
                    let compiled = through
                        .as_ref()
                        .is_some_and(|through| entry.date <= *through);
                    files.push(Brought {
                        path: memory::entry_path(".", repository, &entry.date, &id)[2..].into(),
                        content: text.into_bytes(),
                        kind: BroughtKind::Entry { id, compiled },
                    });
                }
                continue;
            }
            // Entry files that keep to the contract come byte for byte, with their ids.
            if memory::contract_problem(in_logs, &text).is_some() {
                plan.skipped.push(path);
                continue;
            }
            let Some(id) = compile::id_in_name(in_logs).map(str::to_owned) else {
                continue;
            };
            plan.entries += 1;
            plan.entry_files += 1;
            let compiled = compiled.contains(&id);
            files.push(Brought {
                path,
                content,
                kind: BroughtKind::Entry { id, compiled },
            });
        }
        let config = (fs::read(from.join(BRAIN_CONFIG)).ok())
            .and_then(|config| serde_json::from_slice(&config).ok());
        Ok(Self {
            files,
            plan,
            config,
        })
    }
}

/// `index`, or a new one, with `ids` added to its `compiled-after` and its `compiled-through`
/// left as it was: Bring in never sets it, as entries already in the folder may not be compiled.
pub(crate) fn add_compiled(index: Option<&str>, ids: &[String]) -> String {
    let index = index.unwrap_or("---\ntitle: Index\n---\n");
    let mut cursor = compile::read_cursor(index);
    for id in ids {
        if !cursor.after.contains(id) {
            cursor.after.push(id.clone());
        }
    }
    compile::write_cursor(index, &cursor)
}

impl Host {
    /// What bringing `read` in would add to the Space's folder, and the files it would write:
    /// each one the folder lacks. A page the folder has with other text stays and is listed; an
    /// entry it has, held, edited or discarded here, adds nothing; and its own INDEX stays.
    fn compare_brought<'a>(
        &self,
        space: &Space,
        read: &'a LegacyRead,
    ) -> Result<(Vec<&'a Brought>, BringInPlan)> {
        let clone = self.storage_folder(&space.storage.repository);
        let brain = brain_sync::brain_dir(&Root::new(&space.storage.root));
        let key = space.storage.repository.key();
        let mut plan = read.plan.clone();
        let mut new = vec![];
        for file in &read.files {
            let path = format!("{brain}/{}", file.path);
            match (&file.kind, fs::read(clone.join(&path)).ok()) {
                (BroughtKind::Index, Some(_)) => {}
                (BroughtKind::Page, Some(there)) if there != file.content => {
                    plan.skipped.push(file.path.clone());
                }
                (_, Some(_)) => plan.already += 1,
                (_, None) if self.entry_discarded(&key, &path)? => plan.already += 1,
                (_, None) => new.push(file),
            }
        }
        plan.compiled = (new.iter())
            .filter(|file| matches!(file.kind, BroughtKind::Entry { compiled: true, .. }))
            .count();
        Ok((new, plan))
    }

    /// Who a brought-in entry names as its author when no commit says: the human bringing it
    /// in, as their own entries name them in the Space's repository.
    pub(crate) fn bring_in_importer(&self, space_id: &SpaceId) -> Result<String> {
        let space = self.space(space_id)?.space;
        self.entry_author(&space.storage.repository)
    }

    /// Bring in's form: what it would bring into the Space's folder from `read`.
    pub(crate) fn plan_bring_in(
        &self,
        space_id: &SpaceId,
        read: &LegacyRead,
    ) -> Result<BringInPlan> {
        let space = self.space(space_id)?.space;
        Ok(self.compare_brought(&space, read)?.1)
    }

    /// Bring in a brain: writes what `read` adds into the Space's folder and hands it to the
    /// engine, which scans it and pushes at most 200 files a commit, holding any that fails with
    /// its card; a local repository commits it at once, 200 files a commit. The entries the
    /// original had compiled join the folder's `compiled-after`. Then it carries `compile`,
    /// `allowed_email_domains` and this Mac as compiler from the old brain's config, drops the
    /// Space's `brain` key, and deletes this host's old checkout of that brain unless another
    /// Space here names it.
    pub(crate) fn bring_in(&mut self, space_id: &SpaceId, read: LegacyRead) -> Result<BringInPlan> {
        let space = self.space(space_id)?.space;
        let (new, plan) = self.compare_brought(&space, &read)?;
        let clone = self.storage_folder(&space.storage.repository);
        let brain = brain_sync::brain_dir(&Root::new(&space.storage.root));
        let written: Vec<(String, &Brought)> = (new.into_iter())
            .map(|file| (format!("{brain}/{}", file.path), file))
            .collect();
        for (path, _) in &written {
            brain_sync::refuse_links(&clone, path)?;
        }
        let compiled: Vec<String> = (written.iter())
            .filter_map(|(_, file)| match &file.kind {
                BroughtKind::Entry { id, compiled: true } => Some(id.clone()),
                _ => None,
            })
            .collect();
        match Self::storage_key(&space.storage.repository) {
            None => {
                let mut paths: Vec<String> = vec![];
                for (path, file) in &written {
                    shared::write_atomically(&clone.join(path), &file.content)?;
                    paths.push(path.clone());
                }
                if !compiled.is_empty() {
                    let index = format!("{brain}/{}", compile::INDEX);
                    let base = fs::read_to_string(clone.join(&index)).ok();
                    let text = add_compiled(base.as_deref(), &compiled);
                    shared::write_atomically(&clone.join(&index), text.as_bytes())?;
                    paths.retain(|path| *path != index);
                    paths.push(index);
                }
                for chunk in paths.chunks(brain_sync::MAX_FILES_PER_COMMIT) {
                    let chunk: Vec<&str> = chunk.iter().map(String::as_str).collect();
                    let message = format!("Brought in {} files", chunk.len());
                    storage::commit_local(&clone, &chunk, &message)?;
                }
                self.brains_now = true;
            }
            Some(key) => {
                self.atomically(|host| {
                    for (path, file) in &written {
                        shared::write_atomically(&clone.join(path), &file.content)?;
                        host.wait_to_land(&key, path)?;
                    }
                    for id in &compiled {
                        host.db.execute(
                            "INSERT OR IGNORE INTO brought_compiled(repository,entry_id) VALUES (?1,?2)",
                            params![key, id],
                        )?;
                    }
                    Ok(())
                })?;
                self.issues_written.borrow_mut().insert(key);
            }
        }
        self.carry_brain_config(&space, read.config.as_ref())?;
        self.forget_old_brain(&space)?;
        Ok(plan)
    }

    /// What Bring in carries from the old brain's `wiffletree-brain.json`: its compile time and
    /// allowed email domains when the folder sets neither, and this Mac as a volunteer when the
    /// file names it as compiler. The rest stays with the old brain.
    fn carry_brain_config(&mut self, space: &Space, config: Option<&Value>) -> Result<()> {
        let Some(config) = config else {
            return Ok(());
        };
        let mut tracker = self.tracker(space)?;
        let compile = serde_json::from_value::<BrainCompile>(config["compile"].clone()).ok();
        if tracker.compile.is_none()
            && let Some(compile) = compile.filter(|compile| compile::schedule(compile).is_ok())
        {
            tracker.compile = Some(compile);
        }
        if tracker.allowed_email_domains.is_empty() {
            let domains = config["gate"]["allowed_email_domains"].as_array();
            tracker.allowed_email_domains = (domains.into_iter().flatten())
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
        }
        self.save_tracker(space, &tracker)?;
        let named_here = config["compiler"]["host_id"].as_str() == Some(self.host_id()?.as_str());
        if named_here && Self::storage_key(&space.storage.repository).is_some() {
            self.set_compile_here(&space.id, true)?;
        }
        Ok(())
    }

    /// Drops the Space's `brain` key, the record of where its old brain was, and deletes this
    /// host's own checkout of that brain, which only an earlier release wrote and marked, unless
    /// another Space here names it or it is a repository this Mac still uses.
    fn forget_old_brain(&mut self, space: &Space) -> Result<()> {
        let Some(old) = &space.brain else {
            return Ok(());
        };
        let space = Space {
            brain: None,
            ..space.clone()
        };
        self.atomically(|host| {
            host.db.execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![space.id.as_str(), encode(&space)?],
            )?;
            host.write_space_file(&space)
        })?;
        let BrainRepository::GitHub { owner, name, .. } = &old.repository else {
            return Ok(());
        };
        let identity = format!("{owner}/{name}");
        let still_used = (self.spaces()?.iter()).any(|record| {
            record.space.brain.as_ref() == Some(old)
                || record
                    .space
                    .storage
                    .repository
                    .key()
                    .eq_ignore_ascii_case(&identity)
        }) || self
            .settings_key()
            .is_some_and(|key| key == repos::key(&identity));
        let checkout = repos::clone_folder(&self.home, &identity);
        if !still_used && checkout.join(".git").join(OLD_CHECKOUT_MARKER).exists() {
            fs::remove_dir_all(&checkout)?;
        }
        Ok(())
    }
}

/// The file an earlier release put in its own checkout of a brain's `.git`.
const OLD_CHECKOUT_MARKER: &str = "wiffletree-managed";

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(date: &str, title: &str, fields: &str) -> String {
        format!("## {date} — {title}\n{fields}\n")
    }

    fn only(text: &str) -> LegacyEntry {
        parse(text).entries.remove(0)
    }

    #[test]
    fn a_fitted_entry_keeps_to_the_contract_under_its_derived_id() {
        let legacy = only(&entry(
            "2026-09-02",
            "Retry once",
            "- type: decision\n- ref: main @ 3c1e9a0\n- changed: retry\n- why: transient\n- state: shipped",
        ));
        let (input, reference, _) = fitted("calls", &legacy, "at 3c1e9a0");
        let id = legacy_id("calls", &legacy);
        let body = entry_body(&legacy.date, &input, &reference);
        let text = memory::entry_text(&id, "calls", "rb", "skill", &body);
        let path = format!("calls/2026/09/2026-09-02-{id}.md");
        assert_eq!(memory::contract_problem(&path, &text), None);
    }

    #[test]
    fn blame_names_the_commit_of_each_line_as_the_file_stands_here() {
        let repository = tempfile::tempdir().unwrap();
        let at = repository.path();
        crate::test_support::git(at, &["init", "-q", "-b", "main"]);
        fs::write(at.join("calls.md"), "one\n").unwrap();
        crate::test_support::git(at, &["add", "--all"]);
        crate::test_support::git(at, &["commit", "-q", "-m", "First"]);
        let first = crate::test_support::git(at, &["rev-parse", "HEAD"]);
        fs::write(at.join("calls.md"), "one\ntwo\n").unwrap();
        crate::test_support::git(at, &["add", "--all"]);
        crate::test_support::git(at, &["commit", "-q", "-m", "Second"]);
        let second = crate::test_support::git(at, &["rev-parse", "HEAD"]);
        // A line changed here and never committed has no commit.
        fs::write(at.join("calls.md"), "one\ntwo\nthree\n").unwrap();
        let lines = blame(at, "calls.md").unwrap();
        assert_eq!((&lines[&1], &lines[&2]), (&first, &second));
        assert_eq!(lines.get(&3), None);
    }

    #[test]
    fn a_commit_login_is_github_s_and_only_a_valid_one_is_taken() {
        let answering = |output: &'static str| Tools {
            gh: std::sync::Arc::new(move |_, _| {
                Ok(runtime::GhAnswer {
                    status: std::os::unix::process::ExitStatusExt::from_raw(0),
                    output: output.into(),
                    error: String::new(),
                })
            }),
            ..Tools::default()
        };
        let login =
            |output| commit_login("Sage/brain", "3c1e9a0", Path::new("."), &answering(output));
        assert_eq!(
            login(r#"{"author": {"login": "rb"}}"#).as_deref(),
            Some("rb")
        );
        assert_eq!(login(r#"{"author": {"login": "not a login"}}"#), None);
        assert_eq!(login(r#"{"author": null}"#), None);
    }

    #[test]
    fn a_legacy_entry_keeps_its_fields_and_drops_the_old_exports_bookkeeping() {
        let text = format!(
            "# calls — working log\n\n<!-- workspace-entry:01old -->\n{}\n- entry-id: 01old\n- project: p1\n- evidence: agent report\n{}",
            entry(
                "2026-09-02",
                "Retry once",
                "- type: decision\n- ref: main @ 3c1e9a0\n- changed: retry on\n  timeout\n- why: transient\n- state: shipped"
            ),
            entry(
                "2026-09-01",
                "Old entry",
                "Some prose before any field.\n- type: note\n- owner: rb"
            )
        );
        let log = parse(&text);
        assert_eq!(log.entries.len(), 2);
        let (input, reference, _) = fitted("calls", &log.entries[0], "at abc1234");
        assert_eq!(reference, "main @ 3c1e9a0");
        assert_eq!(
            (input.kind, input.changed.as_str()),
            (LogKind::Decision, "retry on timeout")
        );
        assert_eq!(
            (input.why.as_str(), input.state.as_str()),
            ("transient", "shipped")
        );
        assert_eq!(log.entries[0].line, 4);
        // Lines the contract has no field for go in `changed`; an unknown type is an observation.
        let (input, _, _) = fitted("calls", &log.entries[1], "at abc1234");
        assert_eq!(input.kind, LogKind::Observation);
        assert_eq!(input.changed, "Some prose before any field. - owner: rb");
        assert_eq!((input.why.as_str(), input.state.as_str()), (NO_REF, NO_REF));
    }

    #[test]
    fn a_repeated_field_keeps_both_values() {
        let legacy = only(&entry(
            "2026-09-02",
            "Twice",
            "- changed: first\n- why: w\n- changed: second\n  more",
        ));
        let (input, _, _) = fitted("calls", &legacy, "at abc1234");
        assert_eq!(input.changed, "first; second more");
    }

    #[test]
    fn a_malformed_ref_becomes_none_with_its_text_in_state() {
        let legacy = only(&entry(
            "2026-09-02",
            "Deploy",
            "- type: observation\n- ref: PR 92 on main\n- changed: x\n- why: y\n- state: live",
        ));
        let (input, reference, _) = fitted("calls", &legacy, "at abc1234");
        assert_eq!(reference, NO_REF);
        assert_eq!(input.state, "live (legacy ref: PR 92 on main)");
        // A well-formed ref too long to fit moves too, so the entry still fits the cap.
        let long = format!("{} @ 1234567", "a".repeat(2_100));
        let legacy = only(&entry(
            "2026-09-02",
            "Long ref",
            &format!("- ref: {long}\n- changed: c\n- why: w\n- state: s"),
        ));
        let (input, reference, _) = fitted("calls", &legacy, "at abc1234");
        assert_eq!(reference, NO_REF);
        assert!(body_chars(&entry_body("2026-09-02", &input, &reference)) <= MAX_BODY_CHARS);
    }

    #[test]
    fn an_oversized_transcript_fits_the_cap_and_points_to_the_legacy_file() {
        let transcript = "word ".repeat(3_600);
        let legacy = only(&entry(
            "2026-09-02",
            "Long session",
            &format!(
                "- type: observation\n- ref: —\n- changed: {transcript}\n- why: y\n- state: z"
            ),
        ));
        let (input, reference, shortened) = fitted("simpatico-sage", &legacy, "at abc1234");
        assert!(shortened);
        let body = entry_body("2026-09-02", &input, &reference);
        assert!(body_chars(&body) <= MAX_BODY_CHARS);
        assert!(
            body_chars(&body) > MAX_BODY_CHARS - 10,
            "only what must go is cut"
        );
        assert!(input.changed.starts_with("word word"));
        assert!(
            input
                .changed
                .ends_with("… (full text in legacy logs/simpatico-sage.md at abc1234)")
        );
        assert_eq!(
            (input.title.as_str(), input.why.as_str()),
            ("Long session", "y")
        );
        // Should the other fields be long too, they are cut after `changed`.
        let legacy = only(&entry(
            "2026-09-02",
            "Long everything",
            &format!("- changed: short\n- why: {transcript}\n- state: {transcript}"),
        ));
        let (input, reference, _) = fitted("simpatico-sage", &legacy, "at abc1234");
        assert!(body_chars(&entry_body("2026-09-02", &input, &reference)) <= MAX_BODY_CHARS);
        assert!(
            input
                .changed
                .contains("full text in legacy logs/simpatico-sage.md")
        );
    }

    #[test]
    fn ids_are_derived_from_the_entry_and_dated_at_its_noon() {
        let legacy = only(&entry("2026-09-02", "Retry", "- changed: x"));
        let id = legacy_id("calls", &legacy);
        assert_eq!(id, legacy_id("calls", &legacy));
        assert_ne!(id, legacy_id("sms", &legacy));
        let edited = only(&entry("2026-09-02", "Retry", "- changed: y"));
        assert_ne!(id, legacy_id("calls", &edited));
        let noon = chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();
        let noon = noon.and_hms_opt(12, 0, 0).unwrap().and_utc();
        assert_eq!(id[..10], brains::ulid(noon.timestamp_millis(), 0)[..10]);
    }

    #[test]
    fn the_compiled_through_marker_dates_the_compiled_entries() {
        let text = format!(
            "{}<!-- compiled-through: 2026-09-01 -->\n{}",
            entry("2026-09-03", "New", "- changed: x"),
            entry("2026-09-01", "Old", "- changed: y")
        );
        let log = parse(&text);
        assert_eq!(log.compiled_through.as_deref(), Some("2026-09-01"));
        assert_eq!(log.entries.len(), 2);
        // The marker is part of neither entry, so moving it changes no id.
        assert_eq!(log.entries[0].text, "- changed: x\n");
        assert_eq!(log.entries[1].text, "- changed: y\n");
    }

    mod bring_in {
        use super::*;
        use crate::brain_index::tests::{entry as entry_file, id};
        use crate::brain_sync::tests::{FOLDER, PHONE, REPO, Stored, stored_with};
        use crate::test_support::git;

        /// Every file under `folder`, `.git` included, by its relative path.
        fn snapshot(folder: &Path) -> BTreeMap<String, Vec<u8>> {
            let mut files = BTreeMap::new();
            for file in storage::files_under(folder).unwrap() {
                let path = file
                    .strip_prefix(folder)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                files.insert(path, fs::read(&file).unwrap());
            }
            files
        }

        /// `git status` as the human would see it, read without touching the index.
        fn status(folder: &Path) -> String {
            String::from_utf8(
                runtime::git_bytes_large(folder, &["status", "--porcelain=v1", "-z"], &[]).unwrap(),
            )
            .unwrap()
        }

        fn write(folder: &Path, path: &str, text: &str) {
            let path = folder.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn commit(folder: &Path, message: &str) -> String {
            git(folder, &["add", "--all"]);
            git(folder, &["commit", "-q", "-m", message]);
            git(folder, &["rev-parse", "HEAD"])
        }

        const PICA: &str = "---\ncreated: 2026-08-01\nrepos: [pica-ios, pica-api]\nsources: [logs/pica-api.md]\ntags: [product]\n---\n# Pica\n\nThe photo app.\n";

        /// A brain's clone as Michael's `vyttle-docs` is: entries and pages committed, then
        /// changed, untracked, staged and never-committed files beside them. Answers the ids of
        /// its per-entry files: compiled, committed after the cursor, untracked, and held.
        fn original(folder: &Path, host_id: &str) -> [String; 4] {
            git(folder, &["init", "-q", "-b", "main"]);
            let brain = "second-brain";
            let ids = [id(1), id(2), id(3), id(4)];
            let (compiled, text) =
                entry_file("api", &ids[0], ("mhohlios", "decision", "Retry"), "—");
            write(folder, &format!("{brain}/{compiled}"), &text);
            let calls = "# calls — working log\n\n## 2026-09-03 — Newer\n- type: observation\n- ref: —\n- changed: Moved on.\n- why: w\n- state: s\n\n<!-- compiled-through: 2026-09-01 -->\n## 2026-09-01 — Older\n- type: decision\n- ref: —\n- changed: Started.\n- why: w\n- state: s\n";
            write(folder, &format!("{brain}/logs/calls.md"), calls);
            write(folder, &format!("{brain}/notes/Pica.md"), PICA);
            write(
                folder,
                &format!("{brain}/STATUS.md"),
                "# Status\n\nAll well.\n",
            );
            write(
                folder,
                &format!("{brain}/reconcile-queue.md"),
                "# Reconcile queue\n",
            );
            write(folder, &format!("{brain}/archive/old.md"), "Old.\n");
            write(folder, &format!("{brain}/raw/dump.txt"), "Raw.\n");
            let config = json!({"version": 1, "compile": {"at": "05:30", "timezone": "America/Chicago"},
                "compiler": {"host_id": host_id, "name": "Michael's Mac"},
                "gate": {"allowed_email_domains": ["vyttle.com"]}});
            write(folder, BRAIN_CONFIG, &config.to_string());
            let through = commit(folder, "First");
            let index = format!(
                "---\ntitle: Index\ncompiled-through: {through}\ncompiled-after: []\n---\n[[Pica]]\n"
            );
            write(folder, &format!("{brain}/notes/INDEX.md"), &index);
            let (after, text) =
                entry_file("api", &ids[1], ("mhohlios", "observation", "Later"), "—");
            write(folder, &format!("{brain}/{after}"), &text);
            commit(folder, "Second");
            // Changed, untracked, staged and never committed.
            let more = "\n## 2026-09-05 — Newest\n- type: observation\n- ref: —\n- changed: Here only.\n- why: w\n- state: s\n";
            write(
                folder,
                &format!("{brain}/logs/calls.md"),
                &format!("{calls}{more}"),
            );
            let (untracked, text) = entry_file(
                "api",
                &ids[2],
                ("mhohlios", "observation", "Untracked"),
                "—",
            );
            write(folder, &format!("{brain}/{untracked}"), &text);
            let (held, text) = entry_file("api", &ids[3], ("mhohlios", "observation", "Call"), "—");
            write(
                folder,
                &format!("{brain}/{held}"),
                &text.replace("- changed: Call.", &format!("- changed: Call {PHONE}.")),
            );
            write(
                folder,
                &format!("{brain}/notes/Draft.md"),
                "# Draft\n\nOnly here.\n",
            );
            write(folder, &format!("{brain}/notes/Staged.md"), "# Staged\n");
            git(folder, &["add", &format!("{brain}/notes/Staged.md")]);
            ids
        }

        /// Sage, which an earlier release pointed at `vyttle-docs`, with an entry of its own
        /// waiting for a compile behind a cursor of its folder.
        fn sage_with_a_cursor() -> (Stored, String, String) {
            let brain = "brain:\n  repository:\n    kind: git_hub\n    owner: Vyttle-LLC\n    name: vyttle-docs\n  folder: second-brain\n";
            let mut s = stored_with(brain);
            let through = git(&s.github.bare, &["rev-parse", "main"]);
            let (pending, text) =
                entry_file("app", &id(9), ("tomas", "observation", "Pending"), "—");
            let index = format!(
                "---\ntitle: Index\ncompiled-through: {through}\ncompiled-after: []\n---\n"
            );
            s.merge(&[(pending, text), (compile::INDEX.into(), index)]);
            (s, through, id(9))
        }

        fn bring(s: &mut Stored, from: &Path) -> BringInPlan {
            let read = LegacyRead::read(from, "second-brain", "mhohlios", true, &s.tools).unwrap();
            let plan = s.f.host.bring_in(&s.sage.id, read).unwrap();
            s.sync();
            s.sync();
            s.index();
            plan
        }

        fn on_main(s: &Stored, path: &str) -> Option<String> {
            s.github.show(&format!("{FOLDER}/second-brain/{path}"))
        }

        #[test]
        fn every_entry_and_page_lands_and_the_original_is_left_byte_for_byte() {
            let (mut s, through, pending) = sage_with_a_cursor();
            let docs = tempfile::tempdir().unwrap();
            let ids = original(docs.path(), &s.f.host.host_id().unwrap());
            let (files, shown) = (snapshot(docs.path()), status(docs.path()));
            let read =
                LegacyRead::read(docs.path(), "second-brain", "mhohlios", false, &s.tools).unwrap();
            let preview = s.f.host.plan_bring_in(&s.sage.id, &read).unwrap();
            assert_eq!((preview.entries, preview.entry_files), (7, 5));
            assert_eq!(
                (preview.pages, preview.status),
                (3, true),
                "Pica, Draft and Staged"
            );
            assert_eq!(preview.left_behind, ["archive/", "raw/"]);
            assert_eq!((preview.not_on_github, preview.never_committed), (5, 4));
            assert_eq!(preview.compiled, 2);
            let plan = bring(&mut s, docs.path());
            assert_eq!(plan.entries, 7);

            // The original: its status, its index and every file, `.git` included.
            assert_eq!(snapshot(docs.path()), files);
            assert_eq!(status(docs.path()), shown);

            // Every entry lands but the one the scan holds, which stays here with its card.
            for entry_id in &ids[..3] {
                let (path, _) = entry_file("api", entry_id, ("", "", ""), "");
                assert!(on_main(&s, &path).is_some(), "{path}");
            }
            let held = s.f.host.held_entries().unwrap();
            assert_eq!(held.len(), 1);
            assert!(held[0].path.ends_with(&format!("{}.md", ids[3])));
            let logged = git(
                &s.github.bare,
                &[
                    "ls-tree",
                    "-r",
                    "--name-only",
                    "main",
                    &format!("{FOLDER}/second-brain/logs/calls/"),
                ],
            );
            assert_eq!(
                logged.lines().count(),
                3,
                "one file per legacy entry, the uncommitted one too"
            );
            // Pages come byte for byte, `repos:` and all; the old cursor does not come along.
            assert_eq!(on_main(&s, "notes/Pica.md").as_deref(), Some(PICA));
            assert!(
                on_main(&s, "notes/Draft.md").is_some() && on_main(&s, "notes/Staged.md").is_some()
            );
            assert_eq!(
                on_main(&s, "STATUS.md").as_deref(),
                Some("# Status\n\nAll well.\n")
            );
            assert!(
                on_main(&s, "archive/old.md").is_none() && on_main(&s, "raw/dump.txt").is_none()
            );

            // The original's compiled entries join `compiled-after`; the folder's cursor stays.
            let cursor = compile::read_cursor(&on_main(&s, compile::INDEX).unwrap());
            assert_eq!(cursor.through.as_deref(), Some(through.as_str()));
            assert!(
                cursor.after.contains(&ids[0]),
                "compiled by the original's cursor"
            );
            assert!(!cursor.after.contains(&ids[1]) && !cursor.after.contains(&pending));
            assert_eq!(
                cursor.after.len(),
                2,
                "and the legacy entry its marker dated"
            );
            // Every Mac reads the rest as pending: the folder's own, two api and two legacy.
            assert_eq!(s.f.host.brain_status(REPO).unwrap().pending_entries, 5);

            // What the old config carries, and the old brain's record is dropped.
            let tracker = s.f.host.tracker(&s.sage).unwrap();
            assert_eq!(tracker.compile.unwrap().at, "05:30");
            assert_eq!(tracker.allowed_email_domains, ["vyttle.com"]);
            assert!(s.f.host.compiles_here(&s.sage.storage.repository).unwrap());
            let space = s.f.host.space(&s.sage.id).unwrap().space;
            assert!(space.brain.is_none());
            let yaml = s
                .github
                .show(&format!("{FOLDER}/spaces/sage/space.yaml"))
                .unwrap();
            assert!(!yaml.contains("vyttle-docs"), "{yaml}");
        }

        #[test]
        fn bringing_in_again_adds_nothing_and_raises_no_held_entry_twice() {
            let (mut s, through, _) = sage_with_a_cursor();
            let docs = tempfile::tempdir().unwrap();
            original(docs.path(), "another-mac");
            bring(&mut s, docs.path());
            let commits = s.github.log("%H").len();
            let index = on_main(&s, compile::INDEX);
            let plan = bring(&mut s, docs.path());
            assert_eq!(plan.compiled, 0);
            assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
            // Every entry and page, STATUS.md and the queue; the folder keeps its own INDEX.
            assert_eq!(plan.already, plan.entries + plan.pages + 2);
            assert_eq!(s.github.log("%H").len(), commits, "nothing new");
            assert_eq!(on_main(&s, compile::INDEX), index);
            assert_eq!(compile::read_cursor(&index.unwrap()).through, Some(through));
            assert_eq!(s.f.host.held_entries().unwrap().len(), 1);
            // Not named as compiler, this Mac does not volunteer.
            assert!(!s.f.host.compiles_here(&s.sage.storage.repository).unwrap());
        }

        #[test]
        fn a_held_page_can_be_marked_not_sensitive_or_edited_and_then_lands() {
            let mut s = stored_with("");
            let docs = tempfile::tempdir().unwrap();
            write(
                docs.path(),
                "second-brain/notes/Pica.md",
                &format!("# Pica\n\nCall {PHONE}.\n"),
            );
            write(
                docs.path(),
                "second-brain/STATUS.md",
                &format!("# Status\n\nCall {PHONE}.\n"),
            );
            bring(&mut s, docs.path());
            let held = s.f.host.held_entries().unwrap();
            assert_eq!(held.len(), 2);
            let path = |name: &str| format!("{FOLDER}/second-brain/{name}");
            let pica = held
                .iter()
                .find(|h| h.path == path("notes/Pica.md"))
                .unwrap();
            assert!(pica.offers_not_sensitive());
            (s.f.host)
                .mark_entry_not_sensitive(REPO, &pica.path, "fixture number")
                .unwrap();
            let edited = "# Status\n\nAll well.\n";
            (s.f.host)
                .edit_held_entry(REPO, &path("STATUS.md"), edited)
                .unwrap();
            s.sync();
            assert!(s.f.host.held_entries().unwrap().is_empty());
            let marked = on_main(&s, "notes/Pica.md").unwrap();
            assert!(
                marked.contains("<!-- gate:allow phone fixture number -->"),
                "{marked}"
            );
            assert_eq!(on_main(&s, "STATUS.md").as_deref(), Some(edited));
        }

        #[test]
        fn a_page_the_folder_has_with_other_text_is_left_and_listed() {
            let (mut s, _, _) = sage_with_a_cursor();
            s.merge(&[("notes/Pica.md".into(), "# Pica\n\nOurs.\n".into())]);
            let docs = tempfile::tempdir().unwrap();
            original(docs.path(), "another-mac");
            let plan = bring(&mut s, docs.path());
            assert_eq!(plan.skipped, ["notes/Pica.md"]);
            assert_eq!(
                on_main(&s, "notes/Pica.md").as_deref(),
                Some("# Pica\n\nOurs.\n")
            );
        }

        #[test]
        fn hundreds_of_files_land_in_commits_of_at_most_two_hundred_with_no_git_needed() {
            let mut s = stored_with("");
            let docs = tempfile::tempdir().unwrap();
            for n in 0..205 {
                let (path, text) = entry_file(
                    "api",
                    &id(100 + n),
                    ("mhohlios", "observation", "Many"),
                    "—",
                );
                write(docs.path(), &format!("second-brain/{path}"), &text);
            }
            let read =
                LegacyRead::read(docs.path(), "second-brain", "mhohlios", true, &s.tools).unwrap();
            assert_eq!(
                (read.plan.not_on_github, read.plan.never_committed),
                (205, 205)
            );
            s.f.host.bring_in(&s.sage.id, read).unwrap();
            s.sync();
            let added = |s: &Stored| {
                git(
                    &s.github.bare,
                    &["show", "--format=", "--name-only", "main"],
                )
                .lines()
                .count()
            };
            assert_eq!(added(&s), brain_sync::MAX_FILES_PER_COMMIT);
            s.sync();
            assert_eq!(added(&s), 5);
            assert!(
                s.f.host
                    .unlanded_entries(&repos::key(REPO))
                    .unwrap()
                    .is_empty()
            );
        }

        #[test]
        fn a_git_ignored_file_is_not_on_github_and_was_never_committed() {
            let docs = tempfile::tempdir().unwrap();
            git(docs.path(), &["init", "-q", "-b", "main"]);
            write(docs.path(), ".gitignore", "second-brain/notes/Secret.md\n");
            write(docs.path(), "second-brain/notes/Kept.md", "# Kept\n");
            commit(docs.path(), "First");
            write(docs.path(), "second-brain/notes/Secret.md", "# Secret\n");
            let read =
                LegacyRead::read(docs.path(), "second-brain", "me", false, &Tools::default())
                    .unwrap();
            assert_eq!(read.plan.pages, 2, "Kept and Secret");
            assert_eq!((read.plan.not_on_github, read.plan.never_committed), (1, 1));
        }

        #[test]
        fn a_local_space_commits_what_it_brings_at_once() {
            let mut f = publish::tests::fixture();
            let home = f.host.create_space("Home", "", "", None).unwrap().space;
            let docs = tempfile::tempdir().unwrap();
            let ids = original(docs.path(), "another-mac");
            let read =
                LegacyRead::read(docs.path(), "second-brain", "me", false, &Tools::default())
                    .unwrap();
            f.host.bring_in(&home.id, read).unwrap();
            let clone = f.host.storage_folder(&home.storage.repository);
            assert_eq!(git(&clone, &["status", "--porcelain"]), "", "all committed");
            let (path, _) = entry_file("api", &ids[3], ("", "", ""), "");
            assert!(
                clone.join("wiffletree/second-brain").join(path).is_file(),
                "nothing is scanned or pushed locally"
            );
            let index =
                fs::read_to_string(clone.join("wiffletree/second-brain").join(compile::INDEX))
                    .unwrap();
            let cursor = compile::read_cursor(&index);
            assert_eq!((cursor.through, cursor.after.len()), (None, 2));
            assert!(
                index.contains("[[Pica]]"),
                "the original's INDEX, without its cursor"
            );
        }
    }
}
