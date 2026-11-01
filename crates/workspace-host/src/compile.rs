//! Brain compile (OpenSpec change `brain-compile`): one built-in job per brain this host
//! compiles, on the scheduler of `jobs.rs`. A run finds the entries pending since the cursor in
//! `notes/INDEX.md`, verifies their refs off the host thread, and with any pending starts one
//! session owned by the brain, on the Brain compiler row, in a worktree of the brain checkout.
//! The session edits only the wiki and has only `queue_conflict` and `finish_compile`. The host
//! checks its changes, writes the cursor and `reconcile-queue.md`, and hands them to the brain's
//! batch flow as a compile batch: nothing reaches `main` but through its PR and the brain gate.
//! Nothing here logs an entry's or a note's text; records and errors name ids and paths only.
use crate::*;
use brain_index::{
    RULING_PREFIX, entry_fields, footnote_targets, footnotes, frontmatter, link_key, list,
    wikilinks,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use verify_refs::{Checked, Clones};
use workspace_core::job::{JobKey, Schedule};
use workspace_core::{
    BrainCompileState, BrainConflict, CompileOutcome, CompileRun, CompileRunPage, CompileTrigger,
    ConflictCard, ConflictState, Ruling,
};

/// The bundled compile procedure, the compile session's whole instructions, and its version.
pub(crate) const PROCEDURE: &str = include_str!("../skills/procedures/compile.md");
pub(crate) const PROCEDURE_VERSION: u32 = 1;
/// A run compiles at most this many pending entries, oldest id first.
const MAX_ENTRIES: usize = 200;
/// The notes a run's input carries, in characters.
const MAX_NOTE_CHARS: usize = 60_000;
/// A compile session's turn is stopped at this many minutes.
pub(crate) const TIMEOUT_MINUTES: i64 = 30;
/// What a compile session's stopped turn is told, which marks its run timed out.
pub(crate) const TIMED_OUT: &str = "The compile ran for 30 minutes and was stopped.";
/// A built-in compile job's name before its brain.
const JOB: &str = "brain-compile:";
const INDEX: &str = "notes/INDEX.md";
const STATUS: &str = "STATUS.md";
const QUEUE: &str = "reconcile-queue.md";
const NOTES: &str = "notes/";
const CONFLICT_MARKER: &str = "<!-- conflict:";
/// Under the data folder: compile worktrees and the input file of each run.
const WORKTREES: &str = "compile-worktrees";
const INPUTS: &str = "compile-inputs";
const RULES: [&str; 3] = [
    "decision-contradiction",
    "agent-disagreement",
    "destructive",
];
const RULINGS: [&str; 4] = ["Uphold existing", "Keep new", "Keep both", "Defer"];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS compile_runs (id TEXT PRIMARY KEY, repository TEXT NOT NULL COLLATE NOCASE, started_at INTEGER NOT NULL, session_id TEXT, data TEXT NOT NULL, work TEXT);
CREATE INDEX IF NOT EXISTS compile_runs_by_brain ON compile_runs(repository, started_at);
CREATE TABLE IF NOT EXISTS compile_conflicts (run_id TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(run_id, id));
CREATE TABLE IF NOT EXISTS conflict_rulings (repository TEXT NOT NULL COLLATE NOCASE, conflict_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(repository, conflict_id));
CREATE TABLE IF NOT EXISTS ruling_triggers (repository TEXT NOT NULL COLLATE NOCASE, entry_id TEXT NOT NULL, PRIMARY KEY(repository, entry_id));
";

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(SCHEMA)?;
    Ok(())
}

/// The cursor in `notes/INDEX.md`'s frontmatter: the commit on `main` compiled through, and the
/// ids of entries added after it that are already compiled.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Cursor {
    pub(crate) through: Option<String>,
    pub(crate) after: Vec<String>,
}

pub(crate) fn read_cursor(index: &str) -> Cursor {
    let (keys, _) = frontmatter(index);
    Cursor {
        through: (keys.get("compiled-through"))
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty()),
        after: keys
            .get("compiled-after")
            .map(|a| list(a))
            .unwrap_or_default(),
    }
}

/// `index` with the cursor's two keys in its frontmatter, replacing any it had; every other
/// line is kept as it was.
pub(crate) fn write_cursor(index: &str, cursor: &Cursor) -> String {
    let lines = [
        format!(
            "compiled-through: {}",
            cursor.through.as_deref().unwrap_or_default()
        ),
        format!("compiled-after: [{}]", cursor.after.join(", ")),
    ];
    let cursor_key =
        |line: &str| line.starts_with("compiled-through:") || line.starts_with("compiled-after:");
    let (head, body) = match index.strip_prefix("---\n").and_then(|rest| {
        let end = rest
            .find("\n---\n")
            .map(|at| at + 1)
            .or_else(|| rest.ends_with("\n---").then(|| rest.len() - 3))?;
        Some((
            &rest[..end],
            rest[end..]
                .trim_start_matches("---")
                .trim_start_matches('\n'),
        ))
    }) {
        Some((head, body)) => (head.to_owned(), body.to_owned()),
        None => (String::new(), index.to_owned()),
    };
    // A list continued on `- item` lines belongs to the key above it.
    let mut kept = vec![];
    let mut dropping = false;
    for line in head.lines() {
        if cursor_key(line) {
            dropping = true;
            continue;
        }
        if dropping && line.trim_start().starts_with("- ") {
            continue;
        }
        dropping = false;
        kept.push(line.to_owned());
    }
    kept.extend(lines);
    format!("---\n{}\n---\n{body}", kept.join("\n"))
}

/// A pending entry as a run reads it on `main`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingEntry {
    pub(crate) id: String,
    /// Relative to the brain folder.
    pub(crate) path: String,
    /// Its repository folder under `logs/`.
    pub(crate) folder: String,
    pub(crate) author: String,
    pub(crate) reference: String,
    pub(crate) kind: String,
    pub(crate) state: String,
    pub(crate) text: String,
}

/// An entry file's path relative to the brain folder: `logs/<repo>/YYYY/MM/<file>.md`.
fn is_entry_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    matches!(parts.as_slice(), ["logs", _, year, month, file]
        if year.len() == 4 && month.len() == 2 && file.ends_with(".md")
            && year.bytes().chain(month.bytes()).all(|b| b.is_ascii_digit()))
}

fn entry(path: &str, text: &str) -> PendingEntry {
    let (keys, body) = frontmatter(text);
    let fields = entry_fields(body);
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    let field = |key: &str| fields.get(key).cloned().unwrap_or_default();
    PendingEntry {
        id: (keys.get("id").cloned()).unwrap_or_else(|| stem.get(11..).unwrap_or(stem).to_owned()),
        path: path.to_owned(),
        folder: path.split('/').nth(1).unwrap_or_default().to_owned(),
        author: keys.get("author").cloned().unwrap_or_default(),
        reference: field("ref"),
        kind: field("type"),
        state: field("state"),
        text: text.to_owned(),
    }
}

/// The text of each `paths` at `commit`, relative to `prefix`, in one `git cat-file --batch`.
fn read_at(checkout: &Path, commit: &str, prefix: &str, paths: &[String]) -> Result<Vec<String>> {
    if paths.is_empty() {
        return Ok(vec![]);
    }
    let asked: String = (paths.iter())
        .map(|path| format!("{commit}:{prefix}{path}\n"))
        .collect();
    let sent = runtime::git_bytes_large(checkout, &["cat-file", "--batch"], asked.as_bytes())?;
    let contents = brain_index::blobs(&sent)?;
    ensure!(
        contents.len() == paths.len(),
        "Git sent {} of {} files",
        contents.len(),
        paths.len()
    );
    Ok((contents.into_iter())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .collect())
}

/// One file at `commit`, or empty when it is not there.
fn file_at(checkout: &Path, commit: &str, path: &str) -> String {
    runtime::git_bytes(
        checkout,
        &["cat-file", "blob", &format!("{commit}:{path}")],
        b"",
    )
    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    .unwrap_or_default()
}

/// The entry files added between the cursor's commit and `head`, or every entry file at
/// `head` without one, relative to the brain folder.
fn entry_paths(
    checkout: &Path,
    head: &str,
    prefix: &str,
    through: Option<&str>,
) -> Result<Vec<String>> {
    let listed = match through {
        Some(through) => {
            ensure!(
                publish::is_ancestor(checkout, through, head),
                "The cursor's commit {through} is not on main"
            );
            runtime::git_bytes_large(
                checkout,
                &[
                    "-c",
                    "core.quotePath=false",
                    "log",
                    "--diff-filter=A",
                    "--no-renames",
                    "--name-only",
                    "--format=",
                    &format!("{through}..{head}"),
                    "--",
                    &format!("{prefix}logs"),
                ],
                b"",
            )?
        }
        None => runtime::git_bytes_large(
            checkout,
            &[
                "-c",
                "core.quotePath=false",
                "ls-tree",
                "-r",
                "--name-only",
                head,
                "--",
                &format!("{prefix}logs"),
            ],
            b"",
        )?,
    };
    let listed = String::from_utf8_lossy(&listed);
    let mut paths: Vec<String> = (listed.lines())
        .filter_map(|line| line.strip_prefix(prefix))
        .filter(|path| is_entry_path(path))
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// What a run gathers off the host thread before any model call.
pub(crate) struct Prepare {
    pub(crate) run_id: String,
    pub(crate) checkout: PathBuf,
    pub(crate) head: String,
    pub(crate) brain_dir: String,
    /// The entries the legacy import recorded as compiled, for a brain with no cursor yet.
    pub(crate) legacy_compiled: HashSet<String>,
    pub(crate) clones: Clones,
}

pub(crate) struct Prepared {
    pub(crate) run_id: String,
    pub(crate) gathered: Result<Gathered, String>,
}

pub(crate) struct Gathered {
    /// The commit of `main` the run read, which its worktree starts from.
    pub(crate) head: String,
    /// Every pending entry, though the run takes at most `MAX_ENTRIES`.
    pub(crate) pending: usize,
    pub(crate) entries: Vec<PendingEntry>,
    pub(crate) checked: Vec<Checked>,
    /// The cursor the run's compile PR writes.
    pub(crate) cursor: Cursor,
    pub(crate) status: String,
    pub(crate) index: String,
    /// The notes the entries name or link to, within `MAX_NOTE_CHARS`.
    pub(crate) notes: Vec<(String, String)>,
}

impl Prepare {
    pub(crate) fn run(&self, tools: &publish::Tools) -> Prepared {
        let run_id = self.run_id.clone();
        Prepared {
            run_id,
            gathered: self.gather(tools).map_err(|error| format!("{error:#}")),
        }
    }
    fn gather(&self, tools: &publish::Tools) -> Result<Gathered> {
        let prefix = format!("{}/", self.brain_dir);
        let (checkout, head) = (&self.checkout, self.head.as_str());
        let index = file_at(checkout, head, &format!("{prefix}{INDEX}"));
        let cursor = read_cursor(&index);
        let paths = entry_paths(checkout, head, &prefix, cursor.through.as_deref())?;
        let texts = read_at(checkout, head, &prefix, &paths)?;
        let compiled: HashSet<&String> = match &cursor.through {
            Some(_) => cursor.after.iter().collect(),
            None => cursor.after.iter().chain(&self.legacy_compiled).collect(),
        };
        let mut pending: Vec<PendingEntry> = (paths.iter().zip(&texts))
            .map(|(path, text)| entry(path, text))
            .filter(|entry| !compiled.contains(&entry.id))
            .collect();
        pending.sort_by(|a, b| a.id.cmp(&b.id));
        let total = pending.len();
        pending.truncate(MAX_ENTRIES);
        let cursor = match total > MAX_ENTRIES {
            true => Cursor {
                through: cursor.through.clone(),
                after: (cursor.after.iter().cloned())
                    .chain(pending.iter().map(|e| e.id.clone()))
                    .collect(),
            },
            false => Cursor {
                through: Some(self.head.clone()),
                after: vec![],
            },
        };
        if pending.is_empty() {
            return Ok(Gathered {
                head: self.head.clone(),
                pending: 0,
                entries: vec![],
                checked: vec![],
                cursor,
                status: String::new(),
                index,
                notes: vec![],
            });
        }
        let refs: Vec<(&str, &str)> = (pending.iter())
            .map(|e| (e.folder.as_str(), e.reference.as_str()))
            .collect();
        let checked = verify_refs::for_compile(&self.clones, &refs, tools);
        let status = file_at(checkout, head, &format!("{prefix}{STATUS}"));
        let notes = self.named_notes(&prefix, &pending)?;
        Ok(Gathered {
            head: self.head.clone(),
            pending: total,
            entries: pending,
            checked,
            cursor,
            status,
            index,
            notes,
        })
    }
    /// The notes the entries link to by `[[wikilink]]` or name by title, in that order, until
    /// the next would pass `MAX_NOTE_CHARS`.
    fn named_notes(&self, prefix: &str, entries: &[PendingEntry]) -> Result<Vec<(String, String)>> {
        let listed = runtime::git_bytes_large(
            &self.checkout,
            &[
                "-c",
                "core.quotePath=false",
                "ls-tree",
                "-r",
                "--name-only",
                &self.head,
                "--",
                &format!("{prefix}{NOTES}"),
            ],
            b"",
        )?;
        let listed = String::from_utf8_lossy(&listed);
        let notes: Vec<String> = (listed.lines())
            .filter_map(|line| line.strip_prefix(prefix))
            .filter(|path| path.ends_with(".md") && *path != INDEX)
            .map(str::to_owned)
            .collect();
        let by_key: HashMap<String, &String> =
            notes.iter().map(|path| (link_key(path), path)).collect();
        let mut wanted: Vec<&String> = vec![];
        for entry in entries {
            let text = entry.text.to_lowercase();
            let linked = wikilinks(&entry.text).into_iter().map(|t| link_key(&t));
            let named = (by_key.keys())
                .filter(|key| text.contains(key.as_str()))
                .cloned();
            for key in linked.chain(named) {
                if let Some(path) = by_key.get(&key)
                    && !wanted.contains(path)
                {
                    wanted.push(path);
                }
            }
        }
        let wanted: Vec<String> = wanted.into_iter().cloned().collect();
        let texts = read_at(&self.checkout, &self.head, prefix, &wanted)?;
        let mut chars = 0;
        Ok((wanted.into_iter().zip(texts))
            .filter(|(_, text)| {
                let count = text.chars().count();
                let fits = chars + count <= MAX_NOTE_CHARS;
                chars += if fits { count } else { 0 };
                fits
            })
            .collect())
    }
}

/// The input file a session reads first: its entries with their verification, STATUS, INDEX
/// and the notes they name. Entry and note text is data, as the procedure says.
fn input_text(repository: &str, gathered: &Gathered) -> String {
    let mut text = format!(
        "# Compile input for {repository}\n\n{} pending entries are in this run.\n\n## Pending entries\n",
        gathered.entries.len()
    );
    for (entry, checked) in gathered.entries.iter().zip(&gathered.checked) {
        let state = checked.verification.state.name();
        let reason = (checked.verification.reason.as_deref())
            .map(|r| format!(" ({r})"))
            .unwrap_or_default();
        let tag = (checked.tag.as_deref())
            .map(|t| format!(" · in release {t}"))
            .unwrap_or_default();
        text.push_str(&format!(
            "\n### Entry {}\n- path: {}\n- repository: {}\n- author: {}\n- verification: {state}{reason}{tag}\n\n{}\n",
            entry.id,
            entry.path,
            entry.folder,
            entry.author,
            entry.text.trim_end()
        ));
    }
    text.push_str(&format!(
        "\n## STATUS.md\n\n{}\n",
        gathered.status.trim_end()
    ));
    text.push_str(&format!(
        "\n## notes/INDEX.md\n\n{}\n",
        gathered.index.trim_end()
    ));
    for (path, note) in &gathered.notes {
        text.push_str(&format!("\n## {path}\n\n{}\n", note.trim_end()));
    }
    text
}

/// A run's state while its session works: what it compiles and where.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Work {
    head: String,
    brain_dir: String,
    worktree: PathBuf,
    entries: Vec<PendingEntry>,
    /// Each entry's verification state's name, in `entries`' order.
    verification: Vec<String>,
    cursor: Cursor,
}

/// A conflict the session queued.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Queued {
    id: String,
    rule: String,
    title: String,
    claim: String,
    conflicts_with: String,
    evidence: Vec<String>,
    recommend: String,
    reason: String,
}

/// A stable conflict id from what it is about, so the same conflict queued again keeps its id.
fn conflict_id(rule: &str, claim: &str, conflicts_with: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{rule}\0{claim}\0{conflicts_with}").as_bytes());
    format!("c-{}", &workspace_core::pairing::hex(&digest)[..8])
}

/// One conflict's block in `reconcile-queue.md`, as the brain browser reads it.
fn conflict_block(conflict: &Queued, entry: Option<(&PendingEntry, &str)>) -> String {
    let claim = match entry {
        Some((entry, state)) => format!("[{}]({}), {state}", entry.id, entry.path),
        None => conflict.claim.clone(),
    };
    let field = |value: &str| memory::one_line(value);
    format!(
        "{CONFLICT_MARKER}{} -->\n## {} · {}\n- claim: {claim}\n- conflicts-with: {}\n- evidence: {}\n- recommend: {}: {}\n",
        conflict.id,
        field(&conflict.title),
        conflict.rule,
        field(&conflict.conflicts_with),
        field(&conflict.evidence.join("; ")),
        conflict.recommend,
        field(&conflict.reason)
    )
}

/// The open conflicts' blocks of a `reconcile-queue.md`, by id, in order.
fn queue_blocks(queue: &str) -> Vec<(String, String)> {
    (queue.split(CONFLICT_MARKER).skip(1))
        .filter_map(|block| {
            let (id, _) = block.split_once("-->")?;
            let text = format!("{CONFLICT_MARKER}{}", block.trim_end());
            Some((id.trim().to_owned(), format!("{text}\n")))
        })
        .collect()
}

/// `reconcile-queue.md` from the open conflicts: those still open on `main`, less the ones a
/// compiled ruling settled, then the run's own, with no id twice.
fn queue_text(existing: &str, ruled: &HashSet<String>, queued: &[String]) -> String {
    let mut seen: HashSet<String> = HashSet::new();
    let kept = (queue_blocks(existing).into_iter()).filter(|(id, _)| !ruled.contains(id));
    let new = queued.iter().map(|block| {
        let id = queue_blocks(block)
            .first()
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        (id, block.clone())
    });
    let blocks: Vec<String> = (kept.chain(new))
        .filter(|(id, _)| seen.insert(id.clone()))
        .map(|(_, block)| block)
        .collect();
    let mut text = String::from(
        "# Reconcile queue\n\nOpen conflicts, written by Wiffletree's brain compile. Rule on them in Wiffletree's Inbox.\n",
    );
    for block in blocks {
        text.push('\n');
        text.push_str(&block);
    }
    text
}

/// Why a compile session's changes may not be pushed, checked in its worktree: only the wiki
/// changed, every changed note has its frontmatter, every wikilink resolves, and every source
/// and footnote names an entry file. Answers the changed files relative to the brain folder.
fn check_changes(worktree: &Path, brain_dir: &str) -> Result<Vec<String>> {
    let status = runtime::git_bytes(
        worktree,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
        b"",
    )?;
    let status = String::from_utf8_lossy(&status);
    let prefix = format!("{brain_dir}/");
    let mut changed = vec![];
    for record in status.split('\0').filter(|r| r.len() > 3) {
        let (code, path) = record.split_at(3);
        let relative = path.strip_prefix(&prefix);
        let wiki =
            relative.is_some_and(|p| (p.starts_with(NOTES) && p.ends_with(".md")) || p == STATUS);
        ensure!(wiki, "The compile changed {path}, outside the wiki");
        ensure!(
            !code.contains('D'),
            "The compile deleted {path}; notes are never deleted"
        );
        let file = worktree.join(path);
        ensure!(
            fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file()),
            "The compile left {path} as something other than a file"
        );
        changed.push(relative.unwrap_or_default().to_owned());
    }
    let brain = worktree.join(brain_dir);
    let notes: HashSet<String> = fs::read_dir(brain.join(NOTES))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(link_key))
        .collect();
    let entries = entry_ids(&brain.join("logs"));
    for path in &changed {
        let text = fs::read_to_string(brain.join(path))?;
        let (keys, body) = frontmatter(&text);
        if path.starts_with(NOTES) && path != INDEX {
            for key in ["title", "kind", "updated", "sources"] {
                ensure!(
                    keys.get(key).is_some_and(|v| !v.trim().is_empty()),
                    "{path} has no {key} in its frontmatter"
                );
            }
            ensure!(
                matches!(keys["kind"].trim(), "decision" | "observation"),
                "{path}'s kind is neither decision nor observation"
            );
            for source in list(&keys["sources"]) {
                ensure!(
                    entries.contains(&source),
                    "{path} names the source {source}, which is no entry file"
                );
            }
        }
        for link in wikilinks(body) {
            ensure!(
                notes.contains(&link_key(&link)),
                "{path} links [[{link}]], which is no note"
            );
        }
        let folder = Path::new(path)
            .parent()
            .map(|p| brain.join(p))
            .unwrap_or(brain.clone());
        for (label, definition) in footnotes(body) {
            let targets = footnote_targets(&definition);
            ensure!(
                !targets.is_empty(),
                "{path}'s footnote [^{label}] names no entry"
            );
            for target in targets {
                let file = target.split('#').next().unwrap_or_default();
                let exists = match file.ends_with(".md") {
                    true => folder.join(file).is_file(),
                    false => entries.contains(file),
                };
                ensure!(
                    exists,
                    "{path}'s footnote [^{label}] names {file}, which is no entry file"
                );
            }
        }
    }
    Ok(changed)
}

/// The id of every entry file under `logs`, from its file name `<date>-<id>.md`.
fn entry_ids(logs: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut folders = vec![logs.to_owned()];
    while let Some(folder) = folders.pop() {
        for item in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = item.path();
            if path.is_dir() {
                folders.push(path);
            } else if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Some(id) = stem.get(11..)
            {
                ids.insert(id.to_owned());
            }
        }
    }
    ids
}

fn compile_key(repository: &str) -> JobKey {
    JobKey::built_in(&format!("{JOB}{}", repository.to_ascii_lowercase()))
}

/// The scheduler's `run` for a brain's compile job.
fn run_built_in(host: &mut Host, repository: &str, start: jobs::BuiltInStart) -> Result<String> {
    let trigger = match start {
        jobs::BuiltInStart::Due { catch_up: 0 } => CompileTrigger::Scheduled,
        jobs::BuiltInStart::Due { .. } => CompileTrigger::CatchUp,
        jobs::BuiltInStart::RunNow => CompileTrigger::RunNow,
        jobs::BuiltInStart::Triggered => CompileTrigger::Ruling,
    };
    // Rulings pending now are this run's to compile, whatever starts it.
    for ruling in host.new_rulings(repository)? {
        host.db.execute(
            "INSERT OR IGNORE INTO ruling_triggers VALUES (?1,?2)",
            params![repository, ruling],
        )?;
    }
    match host.start_compile(repository, trigger)? {
        Some(run) => Ok(match run.reason.is_empty() {
            true => format!("compile run {} started", run.id),
            false => run.reason,
        }),
        None => Ok("a compile run is in progress".into()),
    }
}

/// The scheduler's trigger: a fetched `main` added a ruling entry no run has yet started for.
fn ruling_merged(host: &Host, repository: &str) -> bool {
    host.new_rulings(repository)
        .is_ok_and(|rulings| !rulings.is_empty())
}

impl Host {
    /// Registers one built-in compile job per brain whose config names this host as compiler,
    /// due at the config's time in its timezone, and drops the others'. A changed time starts
    /// its due times afresh.
    pub(crate) fn refresh_compile_jobs(&mut self) -> Result<()> {
        let mut wanted: Vec<jobs::BuiltIn> = vec![];
        for brain in self.brains()? {
            if !self.compiles_here(&brain)? {
                continue;
            }
            let schedule =
                (brain.config.config().and_then(|c| c.compile.as_ref())).and_then(|compile| {
                    let (hour, minute) = compile.at.split_once(':')?;
                    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
                    (hour < 24 && minute < 60 && jobs::timezone(&compile.timezone).is_ok()).then(
                        || Schedule {
                            cron: format!("{minute} {hour} * * *"),
                            timezone: compile.timezone.clone(),
                        },
                    )
                });
            wanted.push(jobs::BuiltIn {
                name: format!("{JOB}{}", brain.repository.to_ascii_lowercase()),
                subject: brain.repository.clone(),
                owner: format!("the {} brain", brain.repository),
                schedule,
                triggered: ruling_merged,
                run: run_built_in,
            });
        }
        let compile_job = |b: &jobs::BuiltIn| b.name.starts_with(JOB);
        for old in self.built_ins.iter().filter(|b| compile_job(b)) {
            let kept = wanted.iter().find(|w| w.name == old.name);
            if kept.is_none_or(|w| w.schedule != old.schedule) {
                self.db
                    .execute("DELETE FROM job_due WHERE key=?1", [old.key().as_str()])?;
            }
        }
        self.built_ins.retain(|b| !compile_job(b));
        self.built_ins.extend(wanted);
        Ok(())
    }

    /// Whether this host is the brain's compiler: its config on `main` names this host's id.
    pub(crate) fn compiles_here(&self, brain: &Brain) -> Result<bool> {
        let named = (brain.config.config())
            .and_then(|config| config.compiler.as_ref())
            .map(|compiler| compiler.host_id.clone());
        Ok(!brain.spaces.is_empty() && named == Some(self.host_id()?))
    }

    /// The ruling entries pending on `main` that no run has started for.
    fn new_rulings(&self, repository: &str) -> Result<Vec<String>> {
        let started: HashSet<String> = (self
            .db
            .prepare("SELECT entry_id FROM ruling_triggers WHERE repository=?1")?)
        .query_map([repository], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        Ok((self.brain_rulings(repository)?.into_iter())
            .filter(|ruling| ruling.pending && !started.contains(&ruling.entry_id))
            .map(|ruling| ruling.entry_id)
            .collect())
    }

    fn compile_run(&self, id: &str) -> Result<CompileRun> {
        self.db
            .query_row("SELECT data FROM compile_runs WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?
            .with_context(|| format!("No compile run {id}"))
    }

    fn running_compile(&self, repository: &str) -> Result<Option<CompileRun>> {
        Ok(self
            .db
            .query_row(
                "SELECT data FROM compile_runs WHERE repository=?1 AND json_extract(data,'$.outcome')='running' ORDER BY started_at DESC LIMIT 1",
                [repository],
                |r| decode(r, 0),
            )
            .optional()?)
    }

    fn save_compile_run(&self, run: &CompileRun) -> Result<()> {
        self.db.execute(
            "INSERT INTO compile_runs(id,repository,started_at,data) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![run.id, run.repository, run.started_at, encode(run)?],
        )?;
        Ok(())
    }

    fn compile_work(&self, run: &str) -> Result<Work> {
        let work: Option<String> =
            self.db
                .query_row("SELECT work FROM compile_runs WHERE id=?1", [run], |r| {
                    r.get(0)
                })?;
        Ok(serde_json::from_str(
            &work.context("The compile run has no work")?,
        )?)
    }

    /// The open compile batch of the brain, which holds every new run back until it merges or
    /// the human discards it.
    fn open_compile_batch(&self, repository: &str) -> Result<Option<BrainBatch>> {
        Ok((self.brain_batches(Some(repository))?.into_iter())
            .find(|b| b.kind == BrainBatchKind::Compile && !b.is_finished()))
    }

    /// Why a run cannot start now: one is running, or a compile PR is open.
    fn compile_waits(&self, repository: &str) -> Result<Option<String>> {
        if self.running_compile(repository)?.is_some() {
            return Ok(Some("A compile run is in progress".into()));
        }
        Ok(self
            .open_compile_batch(repository)?
            .map(|batch| match &batch.pull_request {
                Some(pr) => format!("Waiting for PR #{}", pr.number),
                None => "Waiting for the compile batch to open its PR".into(),
            }))
    }

    /// Run now: starts a run through the scheduler, or says why it waits.
    pub fn run_brain_compile(&mut self, repository: &str) -> Result<CompileRun> {
        let brain = self.brain(repository)?;
        ensure!(
            self.compiles_here(&brain)?,
            "This Mac does not compile {}: its config names another compiler",
            brain.repository
        );
        if let Some(why) = self.compile_waits(&brain.repository)? {
            bail!("{why}");
        }
        self.refresh_compile_jobs()?;
        self.run_job_now(&compile_key(&brain.repository))?;
        (self
            .brain_compile_runs(&brain.repository, 0, 1)?
            .runs
            .into_iter()
            .next())
        .context("The compile run was not recorded")
    }

    /// Records a run of the brain started by `trigger`: waiting while a compile PR is open,
    /// else running while its preparation is queued. `None` while another run is in progress.
    pub(crate) fn start_compile(
        &mut self,
        repository: &str,
        trigger: CompileTrigger,
    ) -> Result<Option<CompileRun>> {
        let brain = self.brain(repository)?;
        ensure!(
            self.compiles_here(&brain)?,
            "This Mac does not compile {repository}"
        );
        if self.running_compile(&brain.repository)?.is_some() {
            return Ok(None);
        }
        let at = now();
        let mut run = CompileRun {
            id: brains::new_ulid(at),
            repository: brain.repository.clone(),
            trigger,
            started_at: at,
            ended_at: None,
            pending: 0,
            processed: 0,
            model_calls: 0,
            profile: None,
            procedure: None,
            outcome: CompileOutcome::Running,
            reason: String::new(),
            conflicts: 0,
            batch_id: None,
            pull_request: None,
            merged: false,
            tokens: 0,
            cost: None,
        };
        let waits = self.compile_waits(&brain.repository)?;
        let head = brain.head.clone();
        match (waits, head) {
            (Some(why), _) => {
                run.outcome = CompileOutcome::Waiting;
                run.reason = why;
                run.ended_at = Some(at);
            }
            (None, None) => {
                run.outcome = CompileOutcome::Failed;
                run.reason = "The brain has not been fetched yet".into();
                run.ended_at = Some(at);
            }
            (None, Some(head)) => {
                let brain_dir = (brain.config.config().map(|c| c.brain_dir.clone()))
                    .unwrap_or_else(|| SpaceBrain::DEFAULT_FOLDER.into());
                let legacy_compiled = (self
                    .db
                    .prepare("SELECT entry_id FROM legacy_compiled WHERE brain=?1")?)
                .query_map([&brain.repository], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
                self.compile_preparations.push(Prepare {
                    run_id: run.id.clone(),
                    checkout: PathBuf::from(&brain.checkout),
                    head,
                    brain_dir,
                    legacy_compiled,
                    clones: Clones::of(&self.repositories()?),
                });
            }
        }
        self.save_compile_run(&run)?;
        Ok(Some(run))
    }

    pub(crate) fn take_compile_preparations(&mut self) -> Vec<Prepare> {
        std::mem::take(&mut self.compile_preparations)
    }

    fn end_compile(
        &mut self,
        run: &mut CompileRun,
        outcome: CompileOutcome,
        reason: String,
    ) -> Result<()> {
        run.outcome = outcome;
        run.reason = reason;
        run.ended_at = Some(now());
        self.save_compile_run(run)?;
        if let Ok(work) = self.compile_work(&run.id) {
            let checkout = PathBuf::from(self.brain(&run.repository)?.checkout);
            let _ = fs::remove_dir_all(&work.worktree);
            let _ = runtime::git_output(&checkout, &["worktree", "prune"]);
        }
        let _ = fs::remove_file(self.home.join(INPUTS).join(format!("{}.md", run.id)));
        Ok(())
    }

    /// What the preparation found: nothing pending ends the run with no model call; otherwise
    /// the Brain compiler row is resolved and the run's session starts in its own worktree.
    pub(crate) fn compile_prepared(&mut self, prepared: Prepared) -> Result<()> {
        let mut run = self.compile_run(&prepared.run_id)?;
        if run.outcome != CompileOutcome::Running {
            return Ok(());
        }
        let gathered = match prepared.gathered {
            Ok(gathered) => gathered,
            Err(error) => return self.end_compile(&mut run, CompileOutcome::Failed, error),
        };
        run.pending = gathered.pending;
        if gathered.entries.is_empty() {
            let reason = "No pending entries · no model call".to_owned();
            return self.end_compile(&mut run, CompileOutcome::Skipped, reason);
        }
        if self.model_selection()?.row(&Job::Compiler).is_none() {
            let reason = "Waiting for a Brain compiler model: choose one in Models".to_owned();
            return self.end_compile(&mut run, CompileOutcome::Waiting, reason);
        }
        let pick = match self.resolve_job(&Job::Compiler, false, false) {
            Ok(pick) if !pick.waits => pick,
            Ok(pick) => return self.end_compile(&mut run, CompileOutcome::Waiting, pick.reason),
            Err(error) => {
                return self.end_compile(&mut run, CompileOutcome::Waiting, format!("{error:#}"));
            }
        };
        let brain = self.brain(&run.repository)?;
        let checkout = PathBuf::from(&brain.checkout);
        let worktree = self.home.join(WORKTREES).join(&run.id);
        fs::create_dir_all(self.home.join(WORKTREES))?;
        let head = gathered.head.clone();
        let into = worktree
            .to_str()
            .context("The data folder's path is not UTF-8")?;
        if let Err(error) = runtime::git_output(
            &checkout,
            &["worktree", "add", "--quiet", "--detach", into, &head],
        ) {
            return self.end_compile(&mut run, CompileOutcome::Failed, format!("{error:#}"));
        }
        let brain_dir = (brain.config.config().map(|c| c.brain_dir.clone()))
            .unwrap_or_else(|| SpaceBrain::DEFAULT_FOLDER.into());
        let input = self.home.join(INPUTS).join(format!("{}.md", run.id));
        fs::create_dir_all(self.home.join(INPUTS))?;
        fs::write(&input, input_text(&run.repository, &gathered))?;
        let work = Work {
            head: head.clone(),
            brain_dir: brain_dir.clone(),
            worktree: worktree.clone(),
            verification: (gathered.checked.iter())
                .map(|c| c.verification.state.name().to_owned())
                .collect(),
            entries: gathered.entries,
            cursor: gathered.cursor,
        };
        let session = Session {
            id: new_id(),
            project_id: String::new(),
            parent_id: None,
            repository_id: None,
            name: format!("Brain compile · {}", run.repository),
            role: Role::Compiler,
            provider: pick.profile.provider,
            status: Status::Ready,
            archived: false,
            owner: Some(SessionOwner::Brain(run.repository.clone())),
        };
        let first = format!(
            "Compile run {} of the brain {}. Your working folder is a worktree of the brain at {head}, and the brain folder is `{brain_dir}/`. First read your input, {}, whole: it holds the {} pending entries with their verification, STATUS, INDEX and the notes they name. Then follow the compile procedure and end with finish_compile.",
            run.id,
            run.repository,
            input.display(),
            work.entries.len()
        );
        run.processed = work.entries.len();
        run.model_calls = 1;
        run.profile = Some(pick.profile.clone());
        run.procedure = Some(PROCEDURE_VERSION);
        self.atomically(|host| {
            host.db.execute(
                "INSERT INTO sessions VALUES (?1,NULL,NULL,?2,?3)",
                params![session.id, tag(&session.role)?, encode(&session)?],
            )?;
            let mut runtime = host.pin(
                &session.id,
                pick.profile.clone(),
                Chooser::Wiffletree,
                &pick.reason,
            )?;
            runtime.workdir = Some(worktree.to_string_lossy().into_owned());
            host.save_runtime(&runtime)?;
            host.db.execute(
                "UPDATE compile_runs SET session_id=?2,work=?3,data=?4 WHERE id=?1",
                params![
                    run.id,
                    session.id,
                    serde_json::to_string(&work)?,
                    encode(&run)?
                ],
            )?;
            host.send(&format!("compile:{}", run.id), None, &session.id, &first)?;
            Ok(())
        })
    }

    fn session_compile(&self, session: &str) -> Result<CompileRun> {
        let run: Option<CompileRun> = self
            .db
            .query_row(
                "SELECT data FROM compile_runs WHERE session_id=?1",
                [session],
                |r| decode(r, 0),
            )
            .optional()?;
        let run = run.context("This session is no compile run")?;
        ensure!(
            run.outcome == CompileOutcome::Running,
            "This compile run has ended"
        );
        Ok(run)
    }

    /// The launch of a compile session: its worktree, where it writes only the brain's wiki.
    pub(crate) fn compile_launch(&self, session: &Session) -> Result<provider::ThreadLaunch> {
        let run = self.session_compile(&session.id)?;
        let work = self.compile_work(&run.id)?;
        let worktree = fs::canonicalize(&work.worktree)?;
        Ok(provider::ThreadLaunch {
            compile: Some(worktree.join(&work.brain_dir)),
            scratch: worktree,
            ..Default::default()
        })
    }

    /// `queue_conflict` and `finish_compile`, the compile session's only workspace tools.
    pub(crate) fn compile_tool(
        &mut self,
        session: &Session,
        name: &str,
        args: &Value,
        git: &publish::GitTool,
    ) -> Result<Value> {
        ensure!(
            session.role == Role::Compiler,
            "{name} is for brain compile runs"
        );
        let mut run = self.session_compile(&session.id)?;
        let work = self.compile_work(&run.id)?;
        let string = |key: &str, limit: usize| -> Result<String> {
            let value = args[key].as_str().map(str::trim).unwrap_or_default();
            ensure!(!value.is_empty(), "Give {key}");
            text(value, limit)?;
            Ok(value.to_owned())
        };
        match name {
            "queue_conflict" => {
                service::only_arguments(
                    name,
                    args,
                    &[
                        "rule",
                        "title",
                        "claim",
                        "conflicts_with",
                        "evidence",
                        "recommend",
                        "reason",
                    ],
                )?;
                let rule = string("rule", 64)?;
                ensure!(
                    RULES.contains(&rule.as_str()),
                    "rule is one of {}",
                    RULES.join(", ")
                );
                let recommend = string("recommend", 64)?;
                ensure!(
                    RULINGS.contains(&recommend.as_str()),
                    "recommend is one of {}",
                    RULINGS.join(", ")
                );
                let claim = string("claim", 128)?;
                ensure!(
                    work.entries.iter().any(|e| e.id == claim),
                    "claim is the id of one of this run's pending entries"
                );
                let evidence: Vec<String> = (args["evidence"].as_array().into_iter().flatten())
                    .filter_map(Value::as_str)
                    .map(memory::one_line)
                    .filter(|line| !line.is_empty())
                    .collect();
                ensure!(
                    !evidence.is_empty() && evidence.len() <= 20,
                    "Give one to 20 evidence lines"
                );
                for line in &evidence {
                    text(line, 1000)?;
                }
                let conflicts_with = string("conflicts_with", 512)?;
                let queued = Queued {
                    id: conflict_id(&rule, &claim, &conflicts_with),
                    rule,
                    title: string("title", 200)?,
                    claim,
                    conflicts_with,
                    evidence,
                    recommend,
                    reason: string("reason", 1000)?,
                };
                self.db.execute(
                    "INSERT OR IGNORE INTO compile_conflicts VALUES (?1,?2,?3)",
                    params![run.id, queued.id, encode(&queued)?],
                )?;
                Ok(json!({"id": queued.id}))
            }
            "finish_compile" => {
                service::only_arguments(name, args, &["summary"])?;
                string("summary", 1000)?;
                match self.finish_compile(&mut run, &work, git) {
                    Ok(()) => Ok(json!({"finished": true, "conflicts": run.conflicts})),
                    Err(error) => {
                        let reason = format!("{error:#}");
                        self.end_compile(&mut run, CompileOutcome::Failed, reason.clone())?;
                        bail!("The run failed and nothing was pushed: {reason}")
                    }
                }
            }
            other => bail!("{other} is not one of a compile run's tools"),
        }
    }

    /// Checks the session's changes, writes the cursor and the conflict queue, and queues the
    /// compile batch, which the brain's batch flow pushes and opens a PR for.
    fn finish_compile(
        &mut self,
        run: &mut CompileRun,
        work: &Work,
        git: &publish::GitTool,
    ) -> Result<()> {
        let changed = check_changes(&work.worktree, &work.brain_dir)?;
        let brain = work.worktree.join(&work.brain_dir);
        let queued: Vec<Queued> = (self
            .db
            .prepare("SELECT data FROM compile_conflicts WHERE run_id=?1 ORDER BY rowid")?)
        .query_map([&run.id], |r| decode(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
        let ruled: HashSet<String> = (work.entries.iter())
            .filter_map(|e| e.state.strip_prefix(RULING_PREFIX)?.split_once(':'))
            .map(|(id, _)| id.trim().to_owned())
            .collect();
        let blocks: Vec<String> = (queued.iter())
            .map(|conflict| {
                let at = work.entries.iter().position(|e| e.id == conflict.claim);
                let entry = at.map(|at| (&work.entries[at], work.verification[at].as_str()));
                conflict_block(conflict, entry)
            })
            .collect();
        let existing = fs::read_to_string(brain.join(QUEUE)).unwrap_or_default();
        let queue = queue_text(&existing, &ruled, &blocks);
        let index = fs::read_to_string(brain.join(INDEX)).unwrap_or_default();
        let index = write_cursor(&index, &work.cursor);
        let mut files: Vec<(String, Vec<u8>)> = (changed.iter())
            .filter(|path| path.as_str() != INDEX)
            .map(|path| {
                Ok((
                    format!("{}/{path}", work.brain_dir),
                    fs::read(brain.join(path))?,
                ))
            })
            .collect::<Result<_>>()?;
        files.push((format!("{}/{INDEX}", work.brain_dir), index.into_bytes()));
        if queue != existing {
            files.push((format!("{}/{QUEUE}", work.brain_dir), queue.into_bytes()));
        }
        let title = format!("Compile: {} entries", work.entries.len());
        let batch = self.queue_brain_batch(
            &run.repository,
            BrainBatchKind::Compile,
            BrainSource::BrainsPage,
            &title,
            &files,
            git,
        )?;
        run.conflicts = queued.len();
        run.batch_id = Some(batch.id);
        let reason = format!(
            "{} entries compiled · {} conflicts queued",
            work.entries.len(),
            queued.len()
        );
        self.end_compile(run, CompileOutcome::PrOpened, reason)
    }

    /// A compile session's turn ended: a run it did not finish fails, or times out when its
    /// turn was stopped at 30 minutes.
    pub(crate) fn compile_turn_ended(
        &mut self,
        session: &str,
        stopped: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let Ok(mut run) = self.session_compile(session) else {
            return Ok(());
        };
        let (outcome, reason) = match (stopped, error) {
            (Some(TIMED_OUT), _) => (CompileOutcome::TimedOut, TIMED_OUT.to_owned()),
            (Some(reason), _) => (CompileOutcome::Failed, reason.to_owned()),
            (None, Some(error)) => (CompileOutcome::Failed, error.to_owned()),
            (None, None) => (
                CompileOutcome::Failed,
                "The compile ended without finish_compile".to_owned(),
            ),
        };
        self.end_compile(&mut run, outcome, reason)
    }

    /// Ends the runs a stopped host cut off: their sessions' turns cannot finish any longer.
    pub(crate) fn end_interrupted_compiles(&mut self) -> Result<()> {
        let running: Vec<String> = (self.db.prepare(
            "SELECT id FROM compile_runs WHERE json_extract(data,'$.outcome')='running'",
        )?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        for id in running {
            let mut run = self.compile_run(&id)?;
            let reason = "Wiffletree stopped while the run was in progress".to_owned();
            self.end_compile(&mut run, CompileOutcome::Failed, reason)?;
        }
        Ok(())
    }

    /// A page of the brain's runs, newest first, each with its PR and its session's usage.
    pub fn brain_compile_runs(
        &self,
        repository: &str,
        offset: usize,
        limit: usize,
    ) -> Result<CompileRunPage> {
        let limit = limit.clamp(1, MAX_PAGE);
        let total: i64 = self.db.query_row(
            "SELECT count(*) FROM compile_runs WHERE repository=?1",
            [repository],
            |r| r.get(0),
        )?;
        let rows: Vec<(CompileRun, Option<String>)> = (self.db.prepare(
            "SELECT data,session_id FROM compile_runs WHERE repository=?1 ORDER BY started_at DESC, id DESC LIMIT ?2 OFFSET ?3",
        )?)
        .query_map(params![repository, limit as i64, offset as i64], |r| Ok((decode(r, 0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
        let mut runs = vec![];
        for (mut run, session) in rows {
            if let Some(batch) = run
                .batch_id
                .as_deref()
                .and_then(|id| self.brain_batch(id).ok())
            {
                run.pull_request = batch.pull_request.map(|pr| pr.number);
                run.merged = batch.stage == BrainStage::Merged;
            }
            if let Some(session) = session {
                (run.tokens, run.cost) = self.session_usage(&session)?;
            }
            runs.push(run);
        }
        Ok(CompileRunPage {
            runs,
            total: total as usize,
        })
    }

    /// The tokens a session's requests used, and their estimated cost when priced.
    fn session_usage(&self, session: &str) -> Result<(u64, Option<f64>)> {
        type Row = (String, i64, i64, i64, i64);
        let rows: Vec<Row> = (self.db.prepare(
            "SELECT model,input,output,cache_read,cache_write FROM usage_requests WHERE session_id=?1",
        )?)
        .query_map([session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
        let mut tokens = 0u64;
        let mut cost: Option<f64> = None;
        for (model, input, output, cache_read, cache_write) in rows {
            let (input, output) = (input as u64, output as u64);
            let (cache_read, cache_write) = (cache_read as u64, cache_write as u64);
            tokens += input + output;
            let billed = workspace_core::pricing::BilledTokens {
                uncached_input: input.saturating_sub(cache_read + cache_write),
                cache_read,
                cache_write_5m: cache_write,
                cache_write_1h: 0,
                output,
                fast: false,
                long_prompt: false,
            };
            if let Some(price) = workspace_core::pricing::estimate(&model, &billed) {
                *cost.get_or_insert(0.) += price.total();
            }
        }
        Ok((tokens, cost))
    }

    /// The compile of each brain this host compiles, for the Brains page.
    pub(crate) fn brain_compile_states(&self) -> Result<Vec<BrainCompileState>> {
        let needs_model = self.model_selection()?.row(&Job::Compiler).is_none();
        let mut states = vec![];
        for brain in self.brains()? {
            if !self.compiles_here(&brain)? {
                continue;
            }
            let key = compile_key(&brain.repository);
            let next_at: Option<i64> = self
                .db
                .query_row(
                    "SELECT next_at FROM job_due WHERE key=?1",
                    [key.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let pending = (self.brain_status(&brain.repository).ok())
                .map_or(0, |status| status.pending_entries);
            let waits = self.compile_waits(&brain.repository)?;
            states.push(BrainCompileState {
                last_run: self
                    .brain_compile_runs(&brain.repository, 0, 1)?
                    .runs
                    .into_iter()
                    .next(),
                repository: brain.repository,
                next_at,
                pending,
                run_now_waits: waits,
                needs_model,
            });
        }
        Ok(states)
    }

    /// Every open conflict of the brains this Mac's Spaces name, as cards for the Inbox of each
    /// project whose Space names the brain and whose repositories include the claim's, or for
    /// the Brains page when none does. A deferred card hides until the next compile.
    pub(crate) fn conflict_cards(&self) -> Result<Vec<ConflictCard>> {
        let mut cards = vec![];
        let projects = self.projects()?;
        for brain in self.brains()? {
            if brain.spaces.is_empty() {
                continue;
            }
            let Ok(conflicts) = self.brain_conflicts(&brain.repository, None) else {
                continue;
            };
            if conflicts.is_empty() {
                continue;
            }
            let last_compile = self.last_compile_commit(&brain.repository)?;
            let rulings = self.brain_rulings(&brain.repository)?;
            let mut folders: Vec<(String, Vec<String>)> = vec![];
            for project in projects.iter().filter(|p| brain.spaces.contains(&p.space)) {
                let names = (self.project_repositories(&project.id)?.iter())
                    .map(|r| memory::repository_log_name(r).to_lowercase())
                    .collect();
                folders.push((project.id.clone(), names));
            }
            for conflict in conflicts {
                let local = self.local_ruling(&brain.repository, &conflict.id)?;
                let state = match (
                    &local,
                    rulings.iter().find(|r| r.conflict_id == conflict.id),
                ) {
                    (Some(local), _) if local.ruling == Ruling::Defer => {
                        if local.last_compile == last_compile {
                            continue;
                        }
                        ConflictState::Open
                    }
                    (_, Some(entry)) => ConflictState::Ruled {
                        ruling: local
                            .as_ref()
                            .map_or("Ruled".into(), |l| l.ruling.label().into()),
                        by: entry.author.clone(),
                        pull_request: None,
                        merged: true,
                    },
                    (Some(local), None) => {
                        let batch = local
                            .batch_id
                            .as_deref()
                            .and_then(|id| self.brain_batch(id).ok());
                        ConflictState::Ruled {
                            ruling: local.ruling.label().into(),
                            by: local.by.clone(),
                            pull_request: batch
                                .as_ref()
                                .and_then(|b| b.pull_request.as_ref())
                                .map(|pr| pr.number),
                            merged: batch.is_some_and(|b| b.stage == BrainStage::Merged),
                        }
                    }
                    (None, None) => ConflictState::Open,
                };
                let repository = conflict.repository.as_deref().map(str::to_lowercase);
                let projects = (folders.iter())
                    .filter(|(_, names)| repository.as_ref().is_some_and(|r| names.contains(r)))
                    .map(|(id, _)| id.clone())
                    .collect();
                let (existing, claim) = self.conflict_sides(&brain.repository, &conflict)?;
                cards.push(ConflictCard {
                    repository: brain.repository.clone(),
                    recommended: ConflictCard::recommended_in(&conflict.recommend),
                    existing,
                    claim,
                    conflict,
                    projects,
                    state,
                });
            }
        }
        Ok(cards)
    }

    fn local_ruling(&self, repository: &str, conflict: &str) -> Result<Option<LocalRuling>> {
        Ok(self
            .db
            .query_row(
                "SELECT data FROM conflict_rulings WHERE repository=?1 AND conflict_id=?2",
                params![repository, conflict],
                |r| decode(r, 0),
            )
            .optional()?)
    }

    /// The human's ruling on a conflict. Uphold existing, Keep new and Keep both write one
    /// `decision` entry by the human, in the claim's repository folder, whose `state` begins
    /// `Ruling on conflict <id>:`, in a batch queued at once; Defer writes nothing and hides
    /// the card until the next compile. Only the human's client sends this command.
    pub fn rule_conflict(
        &mut self,
        repository: &str,
        conflict_id: &str,
        ruling: Ruling,
        git: &publish::GitTool,
    ) -> Result<Option<ConflictCard>> {
        let brain = self.brain(repository)?;
        let conflict = (self.brain_conflicts(&brain.repository, None)?.into_iter())
            .find(|c| c.id == conflict_id)
            .with_context(|| {
                format!(
                    "{conflict_id} is not an open conflict of {}",
                    brain.repository
                )
            })?;
        if ruling == Ruling::Defer {
            let local = LocalRuling {
                ruling,
                by: self.gh_login.clone().unwrap_or_default(),
                batch_id: None,
                last_compile: self.last_compile_commit(&brain.repository)?,
            };
            self.save_local_ruling(&brain.repository, conflict_id, &local)?;
            return Ok(None);
        }
        ensure!(
            self.local_ruling(&brain.repository, conflict_id)?
                .is_none_or(|l| l.ruling == Ruling::Defer),
            "{conflict_id} is already ruled on"
        );
        let author = (self.gh_login.clone())
            .filter(|login| memory::valid_login(login))
            .context("Wiffletree has not read your GitHub login. Sign gh in, then Sync now on the Brains page.")?;
        let folder = (conflict.repository.clone())
            .filter(|folder| memory::valid_folder(folder))
            .context(
                "The conflict names no entry, so the ruling has no repository to be logged for",
            )?;
        let (changed, state) = ruling_text(&conflict, &ruling)?;
        let at = now();
        let id = brains::new_ulid(at);
        let date = memory::local_date(at);
        let input = LogInput {
            session_id: String::new(),
            milestone: None,
            kind: LogKind::Decision,
            title: one_line_capped(
                &format!("Ruling: {}", conflict.title),
                memory::MAX_TITLE_CHARS,
            ),
            repository: None,
            ticket_id: None,
            changed,
            why: format!("The human ruled on conflict {conflict_id}."),
            state,
        };
        let body = memory::entry_body(&date, &input, memory::NO_REF);
        let length = memory::body_chars(&body);
        ensure!(
            length <= memory::MAX_BODY_CHARS,
            "The ruling is {length} characters and the cap is {}; shorten Keep both's line",
            memory::MAX_BODY_CHARS
        );
        let brain_dir = (brain.config.config().map(|c| c.brain_dir.clone()))
            .unwrap_or_else(|| SpaceBrain::DEFAULT_FOLDER.into());
        let path = memory::entry_path(&brain_dir, &folder, &date, &id);
        let file = memory::entry_text(&id, &folder, &author, "wiffletree", &body);
        let batch = self.queue_brain_batch(
            &brain.repository,
            BrainBatchKind::Log,
            BrainSource::BrainsPage,
            &format!("Ruling on conflict {conflict_id}"),
            &[(path, file.into_bytes())],
            git,
        )?;
        let local = LocalRuling {
            ruling,
            by: author,
            batch_id: Some(batch.id),
            last_compile: None,
        };
        self.save_local_ruling(&brain.repository, conflict_id, &local)?;
        Ok((self.conflict_cards()?.into_iter())
            .find(|card| card.repository == brain.repository && card.conflict.id == conflict_id))
    }

    fn save_local_ruling(
        &self,
        repository: &str,
        conflict: &str,
        ruling: &LocalRuling,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO conflict_rulings VALUES (?1,?2,?3) ON CONFLICT(repository,conflict_id) DO UPDATE SET data=excluded.data",
            params![repository, conflict, encode(ruling)?],
        )?;
        Ok(())
    }
}

/// A ruling made on this Mac.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LocalRuling {
    ruling: Ruling,
    /// The human's GitHub login.
    by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    batch_id: Option<String>,
    /// For Defer: the commit of the brain's last compile when the human deferred; the card
    /// shows again once another compile merges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_compile: Option<String>,
}

fn one_line_capped(value: &str, limit: usize) -> String {
    memory::one_line(value).chars().take(limit).collect()
}

/// A ruling entry's `changed` and `state`.
fn ruling_text(conflict: &BrainConflict, ruling: &Ruling) -> Result<(String, String)> {
    let claim = conflict
        .entry_id
        .clone()
        .unwrap_or_else(|| memory::one_line(&conflict.claim));
    let with = memory::one_line(&conflict.conflicts_with);
    let id = &conflict.id;
    Ok(match ruling {
        Ruling::UpholdExisting => (
            format!("Upheld {with}; the claim {claim} is not adopted."),
            format!("{RULING_PREFIX}{id}: Uphold existing."),
        ),
        Ruling::KeepNew => (
            format!("Adopted the claim {claim} over {with}, which is superseded."),
            format!("{RULING_PREFIX}{id}: Keep new."),
        ),
        Ruling::KeepBoth { scope } => {
            let scope = memory::one_line(scope);
            ensure!(
                !scope.is_empty(),
                "Keep both needs one line that scopes both"
            );
            (
                scope.clone(),
                format!("{RULING_PREFIX}{id}: Keep both. {scope}"),
            )
        }
        Ruling::Defer => bail!("Defer writes no entry"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain_index::tests::{entry as entry_file, id, note};
    use crate::brains::tests::{GitHub, REPO, name_brain, passing_checks, run};
    use crate::publish::tests::Fixture;
    use std::sync::Arc;
    use workspace_core::{Line, ModelProfile, Then};

    /// The publish fixture's project, whose repository is `app`, in a Sage Space whose brain is
    /// `REPO` on a fake GitHub, with a config naming this host as compiler at 02:00 in New York.
    struct Brained {
        f: Fixture,
        github: Arc<GitHub>,
        tools: publish::Tools,
        _github_folder: tempfile::TempDir,
    }

    fn brained() -> Brained {
        brained_on("02:00", None)
    }

    /// `brained`, compiling at `at`, on another host when `compiler` names one.
    fn brained_on(at: &str, compiler: Option<&str>) -> Brained {
        let mut f = publish::tests::fixture();
        f.host.legacy_pointer = f.home.path().join("no-pointer");
        let sage = f.host.create_space("Sage", "", "", None).unwrap().space;
        let sage = name_brain(&mut f.host, &sage.id, REPO);
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &sage))
        .unwrap();
        f.host.record_gh_login(Some("mhohlios".into()));
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let host_id = compiler.map_or_else(|| f.host.host_id().unwrap(), str::to_owned);
        let config = json!({"version": 1, "brain_dir": "second-brain",
            "compiler": {"host_id": host_id, "name": "Michael's Mac"},
            "compile": {"at": at, "timezone": "America/New_York"}});
        github.commit_file_on_main(BrainConfig::FILE, &config.to_string());
        assert!(run(&mut f.host, &tools));
        f.host.confirm_brain(REPO).unwrap();
        Brained {
            f,
            github,
            tools,
            _github_folder: folder,
        }
    }

    impl Brained {
        /// Commits files of the brain folder to `main` and runs the brain's pass.
        fn merge(&mut self, files: &[(String, String)]) -> String {
            let files: Vec<(String, &str)> = (files.iter())
                .map(|(path, text)| (format!("second-brain/{path}"), text.as_str()))
                .collect();
            let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), *t)).collect();
            let commit = self.github.commit_files_on_main(&files, "A batch (#7)");
            assert!(run(&mut self.f.host, &self.tools));
            commit
        }
        /// What a run's preparation finds now, as the service gathers it.
        fn gather(&self) -> Gathered {
            let brain = self.f.host.brain(REPO).unwrap();
            let prepare = Prepare {
                run_id: "r".into(),
                checkout: PathBuf::from(&brain.checkout),
                head: brain.head.unwrap(),
                brain_dir: "second-brain".into(),
                legacy_compiled: HashSet::new(),
                clones: Clones::default(),
            };
            prepare.run(&self.tools).gathered.unwrap()
        }
        /// Prepares every queued run and hands each to the host, as the service does.
        fn prepare(&mut self) {
            for prepare in self.f.host.take_compile_preparations() {
                let prepared = prepare.run(&self.tools);
                self.f.host.compile_prepared(prepared).unwrap();
            }
        }
        fn runs(&self) -> Vec<CompileRun> {
            self.f.host.brain_compile_runs(REPO, 0, 100).unwrap().runs
        }
        fn with_compiler_row(&mut self) {
            let mut selection = self.f.host.model_selection().unwrap();
            selection.set_compiler_row(Some(Line {
                first: ModelProfile {
                    provider: Provider::Claude,
                    model: "sonnet".into(),
                    effort: "medium".into(),
                },
                then: Then::Wait,
            }));
            self.f.host.set_model_selection(&selection).unwrap();
        }
        /// The run's session, after a Run now with entries pending and a Brain compiler row.
        fn started(&mut self) -> (CompileRun, Session, Work) {
            self.with_compiler_row();
            self.f.host.run_brain_compile(REPO).unwrap();
            self.prepare();
            let run = self.runs().remove(0);
            assert_eq!(run.outcome, CompileOutcome::Running, "{}", run.reason);
            let session: String = (self.f.host.db)
                .query_row(
                    "SELECT session_id FROM compile_runs WHERE id=?1",
                    [&run.id],
                    |r| r.get(0),
                )
                .unwrap();
            let work = self.f.host.compile_work(&run.id).unwrap();
            (run, self.f.host.session(&session).unwrap(), work)
        }
        fn tool(&mut self, session: &Session, name: &str, args: Value) -> Result<Value> {
            let git = self.tools.git.clone();
            self.f.host.compile_tool(session, name, &args, &git)
        }
        fn compile_batches(&self) -> Vec<BrainBatch> {
            (self.f.host.brain_batches(Some(REPO)).unwrap().into_iter())
                .filter(|b| b.kind == BrainBatchKind::Compile)
                .collect()
        }
        fn cursor_on_main(&self) -> Cursor {
            let brain = self.f.host.brain(REPO).unwrap();
            let head = brain.head.unwrap();
            read_cursor(&file_at(
                Path::new(&brain.checkout),
                &head,
                "second-brain/notes/INDEX.md",
            ))
        }
    }

    fn ny(text: &str) -> i64 {
        let zone: chrono_tz::Tz = "America/New_York".parse().unwrap();
        let at = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap();
        use chrono::TimeZone;
        zone.from_local_datetime(&at)
            .earliest()
            .unwrap()
            .timestamp_millis()
    }

    fn relay(n: u128) -> (String, String) {
        entry_file(
            "app",
            &id(n),
            ("mhohlios", "observation", "Relay health"),
            "—",
        )
    }

    #[test]
    fn the_cursor_is_two_frontmatter_keys_that_replace_their_old_values() {
        let index = "---\ntitle: Index\ncompiled-through: abc\ncompiled-after:\n  - one\n  - two\n---\n[[Call Relay]]\n";
        assert_eq!(
            read_cursor(index),
            Cursor {
                through: Some("abc".into()),
                after: vec!["one".into(), "two".into()],
            }
        );
        let cursor = Cursor {
            through: Some("def".into()),
            after: vec!["three".into()],
        };
        let written = write_cursor(index, &cursor);
        assert_eq!(
            written,
            "---\ntitle: Index\ncompiled-through: def\ncompiled-after: [three]\n---\n[[Call Relay]]\n"
        );
        assert_eq!(read_cursor(&written), cursor);
        let fresh = write_cursor("[[Call Relay]]\n", &cursor);
        assert_eq!(read_cursor(&fresh), cursor);
        assert!(fresh.ends_with("---\n[[Call Relay]]\n"));
        assert_eq!(read_cursor("# INDEX\n"), Cursor::default());
    }

    #[test]
    fn same_day_and_late_entries_are_pending_and_none_is_skipped_or_taken_twice() {
        let mut b = brained();
        // Two entries of one day: the first is compiled by a run, the second by the next.
        let first = b.merge(&[relay(10)]);
        let gathered = b.gather();
        let ids: Vec<&str> = gathered.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, [id(10)]);
        assert_eq!(gathered.cursor.through.as_deref(), Some(first.as_str()));
        let compiled = write_cursor("", &gathered.cursor);
        b.merge(&[("notes/INDEX.md".into(), compiled)]);
        b.merge(&[relay(12)]);
        // A batch opened before the compile, with an older id, merges after it.
        b.merge(&[relay(11)]);
        let ids: Vec<String> = b.gather().entries.into_iter().map(|e| e.id).collect();
        assert_eq!(ids, [id(11), id(12)]);
        // A failed run leaves the cursor, so the next finds the same entries.
        assert_eq!(b.gather().entries.len(), 2);
    }

    #[test]
    fn a_run_takes_200_entries_oldest_first_and_the_cursor_lists_them_until_the_rest_are_done() {
        let mut b = brained();
        let entries: Vec<(String, String)> = (1..=203).map(relay).collect();
        let head = b.merge(&entries);
        let gathered = b.gather();
        assert_eq!((gathered.pending, gathered.entries.len()), (203, 200));
        assert_eq!(gathered.entries[0].id, id(1));
        assert_eq!(gathered.cursor.through, None);
        assert_eq!(gathered.cursor.after.len(), 200);
        b.merge(&[("notes/INDEX.md".into(), write_cursor("", &gathered.cursor))]);
        let rest = b.gather();
        let ids: Vec<String> = rest.entries.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, [id(201), id(202), id(203)]);
        assert_ne!(rest.cursor.through.as_deref(), Some(head.as_str()));
        assert!(rest.cursor.through.is_some() && rest.cursor.after.is_empty());
    }

    #[test]
    fn the_first_compile_leaves_out_what_the_legacy_import_recorded_as_compiled() {
        let mut b = brained();
        b.merge(&[relay(1), relay(2)]);
        let brain = b.f.host.brain(REPO).unwrap();
        let prepare = Prepare {
            run_id: "r".into(),
            checkout: PathBuf::from(&brain.checkout),
            head: brain.head.unwrap(),
            brain_dir: "second-brain".into(),
            legacy_compiled: HashSet::from([id(1)]),
            clones: Clones::default(),
        };
        let gathered = prepare.run(&b.tools).gathered.unwrap();
        let ids: Vec<String> = gathered.entries.into_iter().map(|e| e.id).collect();
        assert_eq!(ids, [id(2)]);
    }

    #[test]
    fn nothing_pending_makes_no_model_call_and_writes_nothing_without_resolving_the_row() {
        let mut b = brained();
        let first = b.merge(&[relay(1)]);
        b.merge(&[(
            "notes/INDEX.md".into(),
            write_cursor(
                "",
                &Cursor {
                    through: Some(first),
                    after: vec![],
                },
            ),
        )]);
        // No Brain compiler row: an idle run never asks for one.
        assert!(
            b.f.host
                .model_selection()
                .unwrap()
                .row(&Job::Compiler)
                .is_none()
        );
        let sessions = b.f.host.sessions().unwrap().len();
        b.f.host.run_brain_compile(REPO).unwrap();
        b.prepare();
        let run = b.runs().remove(0);
        assert_eq!(
            (run.outcome, run.reason.as_str(), run.model_calls),
            (
                CompileOutcome::Skipped,
                "No pending entries · no model call",
                0
            )
        );
        assert_eq!(b.f.host.sessions().unwrap().len(), sessions);
        assert!(b.compile_batches().is_empty());
        assert!(!b.f.host.home.join(WORKTREES).join(&run.id).exists());
    }

    #[test]
    fn pending_entries_without_a_brain_compiler_row_wait_for_one() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.f.host.run_brain_compile(REPO).unwrap();
        b.prepare();
        let run = b.runs().remove(0);
        assert_eq!(run.outcome, CompileOutcome::Waiting);
        assert!(
            run.reason.contains("Brain compiler model"),
            "{}",
            run.reason
        );
        let page = b.f.host.brains_page().unwrap();
        assert!(page.compiles[0].needs_model);
    }

    #[test]
    fn asleep_over_two_oclock_runs_once_on_wake_as_a_catch_up() {
        let mut b = brained();
        b.f.host.fire_and_check(ny("2026-10-30 01:00")).unwrap();
        assert!(b.runs().is_empty());
        let pass = b.f.host.fire_and_check(ny("2026-10-30 07:00")).unwrap();
        assert_eq!(pass.next, Some(ny("2026-10-31 02:00")));
        b.f.host.fire_and_check(ny("2026-10-30 07:05")).unwrap();
        let runs = b.runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].trigger, CompileTrigger::CatchUp);
        // No Space lists it.
        assert!(b.f.host.job_states().unwrap().is_empty());
    }

    #[test]
    fn a_time_repeated_by_daylight_saving_runs_once() {
        // 01:30 happens twice in New York on November 1, 2026.
        let mut b = brained_on("01:30", None);
        b.f.host.fire_and_check(ny("2026-11-01 00:00")).unwrap();
        let fall_back = ny("2026-11-01 00:00") + 3 * 3_600_000;
        for minutes in [91, 95, 151, 155] {
            b.f.host
                .fire_and_check(ny("2026-11-01 00:00") + minutes * 60_000)
                .unwrap();
            b.prepare();
        }
        b.f.host.fire_and_check(fall_back).unwrap();
        b.prepare();
        assert_eq!(b.runs().len(), 1);
    }

    #[test]
    fn another_hosts_brain_is_never_compiled_here() {
        let mut b = brained_on("02:00", Some("another-host"));
        b.merge(&[relay(1)]);
        b.f.host.fire_and_check(ny("2026-10-30 01:00")).unwrap();
        b.f.host.fire_and_check(ny("2026-10-30 03:00")).unwrap();
        assert!(b.runs().is_empty());
        let refused = b.f.host.run_brain_compile(REPO).unwrap_err();
        assert!(
            refused.to_string().contains("does not compile"),
            "{refused:#}"
        );
        assert!(b.f.host.brains_page().unwrap().compiles.is_empty());
    }

    #[test]
    fn an_open_compile_pr_holds_every_run_and_run_now_says_why() {
        let mut b = brained();
        let files = [("second-brain/STATUS.md".to_owned(), b"# Status\n".to_vec())];
        let git = b.tools.git.clone();
        (b.f.host.queue_brain_batch(
            REPO,
            BrainBatchKind::Compile,
            BrainSource::BrainsPage,
            "Compile: 1 entries",
            &files,
            &git,
        ))
        .unwrap();
        passing_checks(&b.github);
        assert!(run(&mut b.f.host, &b.tools));
        let refused = b.f.host.run_brain_compile(REPO).unwrap_err();
        assert_eq!(refused.to_string(), "Waiting for PR #214");
        b.f.host.fire_and_check(ny("2026-10-30 01:00")).unwrap();
        b.f.host.fire_and_check(ny("2026-10-30 02:00")).unwrap();
        let run = b.runs().remove(0);
        assert_eq!(
            (run.outcome, run.reason.as_str()),
            (CompileOutcome::Waiting, "Waiting for PR #214")
        );
        let page = b.f.host.brains_page().unwrap();
        assert_eq!(
            page.compiles[0].run_now_waits.as_deref(),
            Some("Waiting for PR #214")
        );
    }

    #[test]
    fn a_compile_session_belongs_to_the_brain_works_in_its_worktree_and_has_two_tools() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, session, work) = b.started();
        assert_eq!(session.role, Role::Compiler);
        assert_eq!(session.owner, Some(SessionOwner::Brain(REPO.into())));
        assert_eq!(
            (session.project_id.as_str(), session.parent_id.as_deref()),
            ("", None)
        );
        assert!(!session.owner.as_ref().unwrap().offers_coordination_tools());
        assert_eq!(run.procedure, Some(PROCEDURE_VERSION));
        assert_eq!(run.profile.as_ref().unwrap().model, "sonnet");
        let runtime = b.f.host.session_runtime(&session.id).unwrap();
        assert_eq!(
            runtime.workdir.map(PathBuf::from),
            Some(work.worktree.clone())
        );
        assert!(work.worktree.join("second-brain/logs").is_dir());
        let launch = b.f.host.compile_launch(&session).unwrap();
        assert!(launch.compile.unwrap().ends_with("second-brain"));
        // The project's agents are not offered either tool, and the compile no other.
        assert!(b.tool(&session, "log_work", json!({})).is_err());
        let implementer = b.f.implementer.id.clone();
        for tool in ["queue_conflict", "finish_compile"] {
            assert!(b.f.host.agent_tool(&implementer, tool, json!({})).is_err());
            assert!(!mcp::tools().to_string().contains(tool));
        }
        // It is listed with no project's sessions.
        assert!(
            b.f.host
                .live_sessions()
                .unwrap()
                .iter()
                .all(|s| s.id != session.id)
        );
    }

    /// A note citing the entry, as the procedure asks.
    fn relay_note(entry: &str) -> String {
        format!(
            "---\ntitle: Call Relay\nkind: observation\nupdated: 2026-10-31\nsources: [{entry}]\n---\nThe relay's health check is `/healthz`.[^1]\n\n[^1]: [2026-10-20](../logs/app/2026/10/2026-10-20-{entry}.md)\n"
        )
    }

    #[test]
    fn a_finished_compile_queues_one_compile_batch_with_the_cursor_and_the_conflict_queue() {
        let mut b = brained();
        let head = b.merge(&[relay(1)]);
        let (run, session, work) = b.started();
        let brain = work.worktree.join("second-brain");
        fs::create_dir_all(brain.join("notes")).unwrap();
        fs::write(brain.join("notes/Call Relay.md"), relay_note(&id(1))).unwrap();
        fs::write(brain.join("STATUS.md"), "# Status\n\nSee [[Call Relay]].\n").unwrap();
        let queued = b
            .tool(
                &session,
                "queue_conflict",
                json!({"rule": "decision-contradiction",
                "title": "Retry cap", "claim": id(1), "conflicts_with": "notes/Call Relay.md",
                "evidence": ["claimed: PR #92 is open"], "recommend": "Uphold existing",
                "reason": "PR #92 is still open"}),
            )
            .unwrap();
        let conflict = queued["id"].as_str().unwrap().to_owned();
        b.tool(
            &session,
            "finish_compile",
            json!({"summary": "Folded one entry"}),
        )
        .unwrap();
        let run = b.f.host.compile_run(&run.id).unwrap();
        assert_eq!((run.outcome, run.conflicts), (CompileOutcome::PrOpened, 1));
        let batch = b.compile_batches().remove(0);
        assert_eq!(batch.title, "Compile: 1 entries");
        let mut files = batch.files.clone();
        files.sort();
        assert_eq!(
            files,
            [
                "second-brain/STATUS.md",
                "second-brain/notes/Call Relay.md",
                "second-brain/notes/INDEX.md",
                "second-brain/reconcile-queue.md",
            ]
        );
        // The batch's commit holds the cursor and the conflict's block.
        let written = b.f.host.home.join("brain-batches").join(&batch.id);
        let index = fs::read_to_string(written.join("second-brain/notes/INDEX.md")).unwrap();
        assert_eq!(read_cursor(&index).through.as_deref(), Some(head.as_str()));
        let queue = fs::read_to_string(written.join("second-brain/reconcile-queue.md")).unwrap();
        assert!(queue.contains(&format!("<!-- conflict:{conflict} -->")));
        assert!(queue.contains("## Retry cap · decision-contradiction"));
        assert!(queue.contains("- recommend: Uphold existing: PR #92 is still open"));
        // The worktree is gone, and the turn ending changes nothing more.
        assert!(!work.worktree.exists());
        b.f.host
            .compile_turn_ended(&session.id, None, None)
            .unwrap();
        assert_eq!(
            b.f.host.compile_run(&run.id).unwrap().outcome,
            CompileOutcome::PrOpened
        );
        // Main is pushed only as the batch's branch; it merges only through its PR.
        passing_checks(&b.github);
        assert!(run_pass(&mut b));
        assert_eq!(b.cursor_on_main(), Cursor::default());
        assert!(
            b.f.host
                .run_brain_compile(REPO)
                .unwrap_err()
                .to_string()
                .starts_with("Waiting for PR #")
        );
    }

    fn run_pass(b: &mut Brained) -> bool {
        run(&mut b.f.host, &b.tools)
    }

    #[test]
    fn a_change_outside_the_wiki_or_a_broken_link_fails_the_run_and_leaves_the_cursor() {
        type Change = fn(&Path);
        let cases: [(&str, Change); 3] = [
            ("outside the wiki", |brain| {
                fs::write(brain.join("logs/app/2026/10/extra.md"), "x\n").unwrap()
            }),
            ("which is no note", |brain| {
                fs::write(brain.join("STATUS.md"), "See [[Nowhere]].\n").unwrap()
            }),
            ("has no kind", |brain| {
                fs::create_dir_all(brain.join("notes")).unwrap();
                fs::write(
                    brain.join("notes/Bare.md"),
                    "---\ntitle: Bare\n---\nText.\n",
                )
                .unwrap()
            }),
        ];
        for (why, change) in cases {
            let mut b = brained();
            b.merge(&[relay(1)]);
            let (run, session, work) = b.started();
            change(&work.worktree.join("second-brain"));
            let failed = b
                .tool(&session, "finish_compile", json!({"summary": "Done"}))
                .unwrap_err();
            assert!(failed.to_string().contains(why), "{why}: {failed:#}");
            let run = b.f.host.compile_run(&run.id).unwrap();
            assert_eq!(run.outcome, CompileOutcome::Failed, "{why}");
            assert!(run.reason.contains(why), "{why}: {}", run.reason);
            assert!(b.compile_batches().is_empty(), "{why}");
            assert_eq!(b.cursor_on_main(), Cursor::default(), "{why}");
        }
    }

    #[test]
    fn a_session_stopped_at_30_minutes_times_its_run_out_with_no_pr() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, session, _) = b.started();
        b.f.host
            .compile_turn_ended(&session.id, Some(TIMED_OUT), None)
            .unwrap();
        let run = b.f.host.compile_run(&run.id).unwrap();
        assert_eq!(
            (run.outcome, run.reason.as_str()),
            (CompileOutcome::TimedOut, TIMED_OUT)
        );
        assert!(b.compile_batches().is_empty());
        // A restart ends a run its host cut off.
        b.merge(&[relay(2)]);
        let (cut, _, _) = b.started();
        b.f.host.end_interrupted_compiles().unwrap();
        assert_eq!(
            b.f.host.compile_run(&cut.id).unwrap().outcome,
            CompileOutcome::Failed
        );
    }

    /// The brain with an entry of `app` that a queued conflict claims.
    fn with_conflict(b: &mut Brained) -> String {
        let (path, _) = relay(1);
        let queue = format!(
            "# Reconcile queue\n\n<!-- conflict:c-1 -->\n## Retry cap · decision-contradiction\n- claim: [{}]({path}), claimed\n- conflicts-with: [[Call Relay]]\n- evidence: PR #92 is open\n- recommend: Uphold existing: PR #92 is still open\n",
            id(1)
        );
        let decision = note("Call Relay", "decision", &[id(1)], "Retries cap at 5.");
        b.merge(&[relay(1), decision, ("reconcile-queue.md".into(), queue)]);
        "c-1".into()
    }

    fn conflict_notes(b: &Brained, project: &str) -> Vec<ConflictCard> {
        (b.f.host.workspace_notes(&[]).unwrap().into_iter())
            .filter(|note| note.is_for(project))
            .filter_map(|note| match note {
                WorkspaceNote::Conflict(card) => Some(*card),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_conflict_is_a_card_in_each_project_with_its_repository_and_a_ruling_clears_it() {
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        let project = b.f.coordinator.project_id.clone();
        let cards = conflict_notes(&b, &project);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].conflict.id, conflict);
        assert_eq!(cards[0].recommended.as_deref(), Some("Uphold existing"));
        assert_eq!(cards[0].state, ConflictState::Open);
        // What each side says.
        let existing = cards[0].existing.as_ref().unwrap();
        assert_eq!(
            (existing.label.as_str(), existing.text.as_str()),
            ("The decision note says", "Retries cap at 5.")
        );
        assert!(
            existing
                .detail
                .starts_with("notes/Call Relay.md · kind: decision")
        );
        let claim = cards[0].claim.as_ref().unwrap();
        assert_eq!(claim.label, "The new entry says · 2026-10-20");
        assert_eq!(claim.text, "Relay health.");
        assert!(
            claim.detail.starts_with("app · mhohlios · "),
            "{}",
            claim.detail
        );
        // A project of the Space that does not use `app` shows none; nor does the Brains page.
        let sage = b.f.host.project(&project).unwrap().space;
        let other = b.f.host.create_project("Watch onboarding").unwrap();
        b.f.host.move_project_to_space(&other.id, &sage).unwrap();
        b.f.host
            .set_project_repositories(&other.id, Some(vec![]))
            .unwrap();
        assert!(conflict_notes(&b, &other.id).is_empty());
        assert!(b.f.host.brains_page().unwrap().conflicts.is_empty());
        let git = b.tools.git.clone();
        let card =
            (b.f.host
                .rule_conflict(REPO, &conflict, Ruling::KeepNew, &git))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&card.state, ConflictState::Ruled { ruling, by, merged: false, .. }
            if ruling == "Keep new" && by == "mhohlios")
        );
        // The decision entry is the human's, in the claim's folder, and its batch opens at once.
        let batch = (b.f.host.brain_batches(Some(REPO)).unwrap().into_iter())
            .find(|batch| batch.title == format!("Ruling on conflict {conflict}"))
            .unwrap();
        assert_eq!(batch.kind, BrainBatchKind::Log);
        let path = &batch.files[0];
        assert!(path.starts_with("second-brain/logs/app/"), "{path}");
        let text = fs::read_to_string(
            b.f.host
                .home
                .join("brain-batches")
                .join(&batch.id)
                .join(path),
        )
        .unwrap();
        assert!(text.contains("author: mhohlios"));
        assert!(text.contains("- type: decision"));
        assert!(text.contains(&format!(
            "- state: Ruling on conflict {conflict}: Keep new."
        )));
        let again =
            b.f.host
                .rule_conflict(REPO, &conflict, Ruling::UpholdExisting, &git);
        assert!(again.unwrap_err().to_string().contains("already ruled"));
        // Only the human's client rules: no agent tool does, and the command needs the client.
        let ruling = Command::RuleConflict {
            repository: REPO.into(),
            conflict_id: conflict.clone(),
            ruling: Ruling::Defer,
        };
        assert!(ruling.is_human_only());
        let implementer = b.f.implementer.id.clone();
        assert!(
            b.f.host
                .agent_tool(&implementer, "rule_conflict", json!({}))
                .is_err()
        );
        assert!(!mcp::tools().to_string().contains("rule_conflict"));
    }

    #[test]
    fn uphold_and_keep_both_write_their_rulings_and_defer_writes_nothing() {
        let rulings = [
            (Ruling::UpholdExisting, "Uphold existing."),
            (
                Ruling::KeepBoth {
                    scope: "/healthz is liveness; /ready is readiness.".into(),
                },
                "Keep both. /healthz is liveness; /ready is readiness.",
            ),
        ];
        for (ruling, state) in rulings {
            let mut b = brained();
            let conflict = with_conflict(&mut b);
            let git = b.tools.git.clone();
            b.f.host
                .rule_conflict(REPO, &conflict, ruling, &git)
                .unwrap();
            let batch = (b.f.host.brain_batches(Some(REPO)).unwrap().into_iter())
                .find(|batch| batch.kind == BrainBatchKind::Log)
                .unwrap();
            let text = fs::read_to_string(
                b.f.host
                    .home
                    .join("brain-batches")
                    .join(&batch.id)
                    .join(&batch.files[0]),
            )
            .unwrap();
            assert!(
                text.contains(&format!("- state: Ruling on conflict {conflict}: {state}")),
                "{text}"
            );
        }
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        let project = b.f.coordinator.project_id.clone();
        let git = b.tools.git.clone();
        let batches = b.f.host.brain_batches(Some(REPO)).unwrap().len();
        assert_eq!(
            b.f.host
                .rule_conflict(REPO, &conflict, Ruling::Defer, &git)
                .unwrap(),
            None
        );
        assert_eq!(b.f.host.brain_batches(Some(REPO)).unwrap().len(), batches);
        assert!(conflict_notes(&b, &project).is_empty());
        // The next compile raises it again while it still holds.
        b.merge(&[("STATUS.md".into(), "# Status\n".into())]);
        assert_eq!(conflict_notes(&b, &project).len(), 1);
    }

    #[test]
    fn a_ruling_merged_from_any_install_starts_a_compile_once() {
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        b.f.host.fire_and_check(ny("2026-10-30 09:00")).unwrap();
        // The legacy-free first pass: compile everything there, then mark it compiled.
        let head = b.f.host.brain(REPO).unwrap().head.unwrap();
        let index = write_cursor(
            "",
            &Cursor {
                through: Some(head),
                after: vec![],
            },
        );
        b.merge(&[
            ("notes/INDEX.md".into(), index),
            ("STATUS.md".into(), "# Status\n".into()),
        ]);
        let before = b.runs().len();
        // Tomás's ruling merges on main.
        let ruling = format!(
            "---\nid: {}\nrepo: app\nauthor: tomas\nsource: wiffletree\n---\n## 2026-10-31 — Ruling\n- type: decision\n- ref: —\n- changed: Kept new.\n- why: Ruled.\n- state: Ruling on conflict {conflict}: Keep new.\n",
            id(40)
        );
        b.merge(&[(format!("logs/app/2026/10/2026-10-31-{}.md", id(40)), ruling)]);
        b.f.host.fire_and_check(ny("2026-10-30 09:01")).unwrap();
        b.f.host.fire_and_check(ny("2026-10-30 09:02")).unwrap();
        let runs = b.runs();
        assert_eq!(runs.len(), before + 1);
        assert_eq!(runs[0].trigger, CompileTrigger::Ruling);
        // Every install shows the card as ruled once the ruling is on main.
        let project = b.f.coordinator.project_id.clone();
        assert!(matches!(&conflict_notes(&b, &project)[0].state,
            ConflictState::Ruled { by, merged: true, .. } if by == "tomas"));
    }

    #[test]
    fn a_compiles_usage_is_its_brains_line_in_the_usage_report() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (_, session, _) = b.started();
        let run = new_id();
        let detail =
            json!({"profile": {"provider": session.provider, "model": "claude-sonnet-4-5"}});
        (b.f.host.db.execute(
            "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail) VALUES (?1,?2,'[]',?3,?3,'completed',?4)",
            params![run, session.id, now(), detail.to_string()],
        ))
        .unwrap();
        let usage = json!({"input_tokens": 1000, "output_tokens": 200});
        (b.f.host.record_usage(
            &run,
            "request-1",
            Some("claude-sonnet-4-5"),
            &usage,
            true,
            now(),
        ))
        .unwrap();
        let report =
            b.f.host
                .usage_report(1, None, None, "America/New_York")
                .unwrap();
        let line = (report.coordinators.iter())
            .find(|c| c.id == format!("{}{REPO}", workspace_core::COMPILE_LINE))
            .unwrap();
        assert_eq!(
            (line.name.as_str(), line.project.as_str()),
            ("Brain compile", REPO)
        );
        assert_eq!(line.own.output, 200);
        let runs = b.runs();
        assert!(runs[0].tokens >= 1200, "{}", runs[0].tokens);
    }
}
