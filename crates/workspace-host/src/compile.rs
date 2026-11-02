//! Brain compile (OpenSpec change `one-space-repository`, decisions 1 and 2): one built-in job per
//! wiki this Mac compiles, on the scheduler of `jobs.rs`. A due run takes a pass first, so `main`
//! is fresh, and then, off the host thread, reads the claim branch's tip, finds the entries
//! pending by the cursor in `notes/INDEX.md`, and with any pending and a Brain compiler model
//! claims the run (`repos::claim`). A Mac that lost, or found a live claim, stays owed and reads
//! the tip after each pull until the run is done or the claim frees. The run that won verifies
//! its refs and starts one session owned by the wiki, in a worktree of the repository's clone at
//! the commit it read. The session edits only the wiki and has only `queue_conflict` and
//! `finish_compile`. The host checks its changes, writes the cursor and `reconcile-queue.md`, and
//! hands the run's exact files to the wiki rule set (`brain_sync`), which lands them on the
//! current head, scanned, all or nothing. A run's end pushes `landed` or `released`, tried once.
//! A local repository takes no claim. Nothing here logs an entry's or a note's text; records and
//! errors name ids and paths only.
use crate::*;
use brain_index::{
    RULING_PREFIX, entry_fields, footnote_targets, footnotes, frontmatter, link_key, list,
    wikilinks,
};
use repos::{Claim, ClaimState, Claiming, Tip};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use verify_refs::{Checked, Clones};
use workspace_core::job::{JobKey, Schedule};
use workspace_core::{
    BrainCompileState, BrainConflict, CompileOutcome, CompileRun, CompileRunPage, CompileTrigger,
    ConflictCard, ConflictState, Ruling,
};

/// The bundled compile procedure, the compile session's whole instructions, and its version.
pub(crate) const PROCEDURE: &str = include_str!("../skills/procedures/compile.md");
pub(crate) const PROCEDURE_VERSION: u32 = 2;
/// A run compiles at most this many pending entries, oldest id first.
const MAX_ENTRIES: usize = 200;
/// The notes a run's input carries, in characters.
const MAX_NOTE_CHARS: usize = 60_000;
/// A compile session's turn is stopped at this many minutes.
pub(crate) const TIMEOUT_MINUTES: i64 = 30;
/// What a compile session's stopped turn is told, which marks its run timed out.
pub(crate) const TIMED_OUT: &str = "The compile ran for 30 minutes and was stopped.";
/// A wiki is falling behind after this many scheduled times with entries waiting.
const MISSED_TIMES_BEHIND: u32 = 2;
/// A built-in compile job's name before its brain.
const JOB: &str = "brain-compile:";
const INDEX: &str = "notes/INDEX.md";
pub(crate) const STATUS: &str = "STATUS.md";
pub(crate) const QUEUE: &str = "reconcile-queue.md";
pub(crate) const NOTES: &str = "notes/";
const CONFLICT_MARKER: &str = "<!-- conflict:";
/// Under the data folder: compile worktrees and the input file of each run.
const WORKTREES: &str = "compile-worktrees";
const INPUTS: &str = "compile-inputs";
const RULES: [&str; 3] = [
    "decision-contradiction",
    "agent-disagreement",
    "destructive",
];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS compile_runs (id TEXT PRIMARY KEY, repository TEXT NOT NULL COLLATE NOCASE, started_at INTEGER NOT NULL, session_id TEXT, data TEXT NOT NULL, work TEXT);
CREATE INDEX IF NOT EXISTS compile_runs_by_brain ON compile_runs(repository, started_at);
CREATE TABLE IF NOT EXISTS compile_conflicts (run_id TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(run_id, id));
CREATE TABLE IF NOT EXISTS conflict_rulings (repository TEXT NOT NULL COLLATE NOCASE, conflict_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(repository, conflict_id));
CREATE TABLE IF NOT EXISTS ruling_triggers (repository TEXT NOT NULL COLLATE NOCASE, entry_id TEXT NOT NULL, PRIMARY KEY(repository, entry_id));
CREATE TABLE IF NOT EXISTS compile_owed (repository TEXT PRIMARY KEY COLLATE NOCASE, due TEXT NOT NULL, catch_up INTEGER NOT NULL, lost_run TEXT);
CREATE TABLE IF NOT EXISTS compile_claims (run TEXT PRIMARY KEY, commit_id TEXT NOT NULL, due TEXT NOT NULL);
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

/// Whether `main`'s cursor has compiled everything `run`'s did: the same `compiled-through`
/// with at least its `compiled-after` ids, or a later one, as another Mac may have moved it on
/// since.
pub(crate) fn at_or_past(checkout: &Path, main: &Cursor, run: &Cursor) -> bool {
    match (&run.through, &main.through) {
        (Some(run_through), Some(main_through)) if run_through == main_through => {
            run.after.iter().all(|id| main.after.contains(id))
        }
        (Some(run_through), Some(main_through)) => {
            publish::is_ancestor(checkout, run_through, main_through)
        }
        (Some(_), None) => false,
        (None, Some(_)) => true,
        (None, None) => run.after.iter().all(|id| main.after.contains(id)),
    }
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

/// The id in an entry file's name, `<date>-<id>.md`.
fn id_in_name(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?.strip_suffix(".md")?;
    name.get(11..).filter(|id| !id.is_empty())
}

/// The ids of the entries `commit`'s tree holds, in whatever layout it holds them, so a cursor
/// from before a move into the folder finds its entries by id.
pub(crate) fn entry_ids_at(checkout: &Path, commit: &str) -> Result<HashSet<String>> {
    let listed = runtime::git_bytes_large(
        checkout,
        &[
            "-c",
            "core.quotePath=false",
            "ls-tree",
            "-r",
            "--name-only",
            commit,
        ],
        b"",
    )?;
    let listed = String::from_utf8_lossy(&listed);
    // `<layout>/logs/<repo>/YYYY/MM/<file>.md`: the last five parts, in any layout.
    let in_logs = |path: &&str| {
        let parts: Vec<&str> = path.split('/').collect();
        parts.len() >= 5 && is_entry_path(&parts[parts.len() - 5..].join("/"))
    };
    Ok((listed.lines())
        .filter(in_logs)
        .filter_map(id_in_name)
        .map(str::to_owned)
        .collect())
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
pub(crate) fn file_at(checkout: &Path, commit: &str, path: &str) -> String {
    runtime::git_bytes(
        checkout,
        &["cat-file", "blob", &format!("{commit}:{path}")],
        b"",
    )
    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    .unwrap_or_default()
}

/// The entry files at `head` not at the cursor's commit, by id, or every entry file at `head`
/// without one, relative to the brain folder.
fn entry_paths(
    checkout: &Path,
    head: &str,
    prefix: &str,
    through: Option<&str>,
) -> Result<Vec<String>> {
    let compiled = match through {
        Some(through) => {
            ensure!(
                publish::is_ancestor(checkout, through, head),
                "The cursor's commit {through} is not on main"
            );
            entry_ids_at(checkout, through)?
        }
        None => HashSet::new(),
    };
    let listed = runtime::git_bytes_large(
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
    )?;
    let listed = String::from_utf8_lossy(&listed);
    let mut paths: Vec<String> = (listed.lines())
        .filter_map(|line| line.strip_prefix(prefix))
        .filter(|path| is_entry_path(path))
        .filter(|path| !id_in_name(path).is_some_and(|id| compiled.contains(id)))
        .map(str::to_owned)
        .collect();
    paths.sort();
    Ok(paths)
}

/// The schedule of a compile time, or why the schedule could not read it.
pub(crate) fn schedule(compile: &BrainCompile) -> Result<Schedule> {
    let (hour, minute) = (compile.at.split_once(':'))
        .filter(|(hour, minute)| hour.len() == 2 && minute.len() == 2)
        .and_then(|(hour, minute)| Some((hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?)))
        .filter(|(hour, minute)| *hour < 24 && *minute < 60)
        .with_context(|| format!("{:?} is not a time; use HH:MM", compile.at))?;
    jobs::timezone(&compile.timezone)?;
    Ok(Schedule {
        cron: format!("{minute} {hour} * * *"),
        timezone: compile.timezone.clone(),
    })
}

fn entries_waiting(count: usize) -> String {
    match count {
        1 => "1 entry is waiting".to_owned(),
        count => format!("{count} entries are waiting"),
    }
}

/// What a run compiled elsewhere says, naming the Mac when `main` does.
pub(crate) fn compiled_elsewhere(machine: Option<&str>) -> String {
    match machine {
        Some(machine) => format!("Compiled elsewhere, on {machine}, so nothing landed"),
        None => "Compiled elsewhere, so nothing landed".to_owned(),
    }
}

/// What a run needs to claim on GitHub. The pass lock keeps a claim's Git commands apart from a
/// pass's, which reads the `FETCH_HEAD` its own fetch wrote.
#[derive(Clone)]
pub(crate) struct Claimer {
    url: String,
    host: String,
    machine: String,
    login: String,
    due: String,
    running: Arc<Mutex<()>>,
}
impl Claimer {
    fn read_tip(&self, checkout: &Path, git: &publish::GitTool) -> Result<Option<Tip>> {
        let _running = (self.running.lock()).unwrap_or_else(PoisonError::into_inner);
        repos::read_tip(checkout, &self.url, git)
    }
    fn claim(
        &self,
        checkout: &Path,
        run: &str,
        tip: Option<&Tip>,
        git: &publish::GitTool,
    ) -> Result<Claiming> {
        let claim = Claim {
            host: self.host.clone(),
            machine: self.machine.clone(),
            login: self.login.clone(),
            run: run.to_owned(),
            due: self.due.clone(),
            state: ClaimState::Claimed,
            at: now(),
        };
        let _running = (self.running.lock()).unwrap_or_else(PoisonError::into_inner);
        repos::claim(checkout, &self.url, tip, &claim, git)
    }
}

/// A run's claim to end on GitHub, tried once.
pub(crate) struct ClaimEnd {
    clone: PathBuf,
    url: String,
    parent: String,
    claim: Claim,
    running: Arc<Mutex<()>>,
}
impl ClaimEnd {
    pub(crate) fn push(&self, git: &publish::GitTool) {
        let _running = (self.running.lock()).unwrap_or_else(PoisonError::into_inner);
        let ended = repos::end_claim(&self.clone, &self.url, &self.parent, &self.claim, git);
        if let Err(error) = ended {
            eprintln!("Ending compile claim {}: {error:#}", self.claim.run);
        }
    }
}

/// An owed run's read of the tip after a pull, off the host thread.
pub(crate) struct TipRead {
    repository: String,
    checkout: PathBuf,
    head: String,
    brain_dir: String,
    claimer: Claimer,
}
impl TipRead {
    pub(crate) fn run(&self, git: &publish::GitTool) -> TipReading {
        let prefix = format!("{}/", self.brain_dir);
        let pending = pending_entries(&self.checkout, &self.head, &prefix);
        TipReading {
            repository: self.repository.clone(),
            tip: (self.claimer.read_tip(&self.checkout, git)).map_err(|e| format!("{e:#}")),
            nothing_pending: pending.is_ok_and(|(_, _, pending)| pending.is_empty()),
        }
    }
    pub(crate) fn failed(&self) -> TipReading {
        TipReading {
            repository: self.repository.clone(),
            tip: Err("The read of the claim stopped unexpectedly".into()),
            nothing_pending: false,
        }
    }
}

pub(crate) struct TipReading {
    repository: String,
    tip: Result<Option<Tip>, String>,
    nothing_pending: bool,
}

/// What a run gathers off the host thread before any model call.
pub(crate) struct Prepare {
    pub(crate) run_id: String,
    pub(crate) checkout: PathBuf,
    pub(crate) head: String,
    pub(crate) brain_dir: String,
    pub(crate) clones: Clones,
    /// How it claims; `None` for a local repository, which takes no claim.
    pub(crate) claimer: Option<Claimer>,
    /// Why the Brain compiler row waits, which ends a run with entries pending before its claim.
    pub(crate) waits: Option<String>,
}

pub(crate) struct Prepared {
    pub(crate) run_id: String,
    /// The commit of the claim it won, and the due time that claim is for.
    pub(crate) claim: Option<(String, String)>,
    pub(crate) gathered: Result<Gathered, Stopped>,
}

/// Why a run stopped before its model.
#[derive(Debug)]
pub(crate) enum Stopped {
    Failed(String),
    /// GitHub could not be reached, so it claimed nothing.
    Unreachable,
    /// The tip says that Mac landed this due time.
    CompiledElsewhere(String),
    /// Its Brain compiler row waits, or it has none.
    Waits(String),
    /// That Mac holds a live claim.
    Live(String),
    /// Another Mac's claim beat its own.
    Lost,
    /// GitHub turned its claim down.
    Refused(String),
}

pub(crate) struct Gathered {
    /// The commit of `main` the run read, which its worktree starts from.
    pub(crate) head: String,
    /// Every pending entry, though the run takes at most `MAX_ENTRIES`.
    pub(crate) pending: usize,
    pub(crate) entries: Vec<PendingEntry>,
    pub(crate) checked: Vec<Checked>,
    /// The cursor the run's commit writes.
    pub(crate) cursor: Cursor,
    pub(crate) status: String,
    pub(crate) index: String,
    /// The notes the entries name or link to, within `MAX_NOTE_CHARS`.
    pub(crate) notes: Vec<(String, String)>,
}

/// `notes/INDEX.md` at `head`, its cursor, and the entries pending there, oldest id first.
fn pending_entries(
    checkout: &Path,
    head: &str,
    prefix: &str,
) -> Result<(String, Cursor, Vec<PendingEntry>)> {
    let index = file_at(checkout, head, &format!("{prefix}{INDEX}"));
    let cursor = read_cursor(&index);
    let paths = entry_paths(checkout, head, prefix, cursor.through.as_deref())?;
    let texts = read_at(checkout, head, prefix, &paths)?;
    let mut pending: Vec<PendingEntry> = (paths.iter().zip(&texts))
        .map(|(path, text)| entry(path, text))
        .filter(|entry| !cursor.after.contains(&entry.id))
        .collect();
    pending.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((index, cursor, pending))
}

impl Prepare {
    pub(crate) fn run(&self, tools: &publish::Tools) -> Prepared {
        let mut claim = None;
        let gathered = self.gather(tools, &mut claim);
        Prepared {
            run_id: self.run_id.clone(),
            claim,
            gathered,
        }
    }
    /// The tip first, then what is pending, then the model, and only then the claim, so a Mac
    /// with nothing to do writes nothing.
    fn gather(
        &self,
        tools: &publish::Tools,
        claim: &mut Option<(String, String)>,
    ) -> Result<Gathered, Stopped> {
        let failed = |error: anyhow::Error| Stopped::Failed(format!("{error:#}"));
        let prefix = format!("{}/", self.brain_dir);
        let (checkout, head) = (&self.checkout, self.head.as_str());
        let tip = match &self.claimer {
            Some(claimer) => {
                let tip = claimer.read_tip(checkout, &tools.git);
                let tip = tip.map_err(|_| Stopped::Unreachable)?;
                let landed = tip.as_ref().and_then(|tip| tip.landed(&claimer.due));
                if let Some(machine) = landed {
                    return Err(Stopped::CompiledElsewhere(machine.to_owned()));
                }
                tip
            }
            None => None,
        };
        let (index, cursor, mut pending) =
            pending_entries(checkout, head, &prefix).map_err(failed)?;
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
        if let Some(waits) = &self.waits {
            return Err(Stopped::Waits(waits.clone()));
        }
        if let Some(claimer) = &self.claimer {
            if let Some(tip) = tip.as_ref().filter(|tip| !tip.free(now())) {
                let holder = tip.claim.as_ref().map(|claim| claim.machine.clone());
                return Err(Stopped::Live(holder.unwrap_or_default()));
            }
            let claiming = claimer.claim(checkout, &self.run_id, tip.as_ref(), &tools.git);
            match claiming.map_err(|_| Stopped::Unreachable)? {
                Claiming::Won(commit) => *claim = Some((commit, claimer.due.clone())),
                Claiming::Lost => return Err(Stopped::Lost),
                Claiming::Refused(why) => return Err(Stopped::Refused(why)),
            }
        }
        let refs: Vec<(&str, &str)> = (pending.iter())
            .map(|e| (e.folder.as_str(), e.reference.as_str()))
            .collect();
        let checked = verify_refs::for_compile(&self.clones, &refs, tools);
        let status = file_at(checkout, head, &format!("{prefix}{STATUS}"));
        let notes = self.named_notes(&prefix, &pending).map_err(failed)?;
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
    /// The product pages whose `repos:` list the entries' folders, then the notes the entries
    /// link to by `[[wikilink]]` or name by title, until the next would pass `MAX_NOTE_CHARS`.
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
        let texts = read_at(&self.checkout, &self.head, prefix, &notes)?;
        let by_key: HashMap<String, &String> =
            notes.iter().map(|path| (link_key(path), path)).collect();
        let mut wanted: Vec<&String> = (notes.iter().zip(&texts))
            .filter(|(_, text)| {
                let (keys, _) = frontmatter(text);
                let repos = keys.get("repos").map(|names| list(names));
                repos.is_some_and(|repos| {
                    (repos.iter()).any(|name| {
                        (entries.iter()).any(|entry| entry.folder.eq_ignore_ascii_case(name))
                    })
                })
            })
            .map(|(path, _)| path)
            .collect();
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
        let texts: HashMap<&String, &String> = notes.iter().zip(&texts).collect();
        let mut chars = 0;
        Ok((wanted.into_iter())
            .map(|path| (path.clone(), texts[path].clone()))
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
    /// The files its session finished, waiting to land.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    landing: Option<Landing>,
}

/// A finished run's exact files, which wait in its worktree for the wiki rule set to land them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Landing {
    pub(crate) run: String,
    pub(crate) worktree: PathBuf,
    /// The commit it read, which its cursor is computed from.
    pub(crate) head: String,
    pub(crate) cursor: Cursor,
    /// Its files' paths in the repository.
    pub(crate) files: Vec<String>,
    /// `notes/INDEX.md`'s path in the repository.
    pub(crate) index: String,
    /// How many entries it compiled.
    pub(crate) entries: usize,
    /// The commit of its claim, which must still be the tip when it lands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claim: Option<String>,
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

/// Whether `source` has the shape of an entry's id, a ULID. A note from before this procedure
/// may list file paths, which stay until a page is rewritten.
fn is_entry_id(source: &str) -> bool {
    brain_index::is_ulid(source)
}

/// Why a compile session's changes may not be pushed, checked in its worktree: only the wiki
/// changed, every changed note has its frontmatter and keeps each name its `repos:` had, every
/// wikilink resolves, and every entry id in its sources and footnotes names an entry file.
/// Answers the changed files relative to the brain folder.
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
        // A scan skips a hidden path, and nothing lands unscanned.
        ensure!(
            !path.split('/').any(|part| part.starts_with('.')),
            "The compile wrote {path}, a hidden path the scan does not read"
        );
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
                    !is_entry_id(&source) || entries.contains(&source),
                    "{path} names the source {source}, which is no entry file"
                );
            }
            let before = file_at(worktree, "HEAD", &format!("{prefix}{path}"));
            let (had, _) = frontmatter(&before);
            let now = keys
                .get("repos")
                .map(|names| list(names))
                .unwrap_or_default();
            for name in had
                .get("repos")
                .map(|names| list(names))
                .unwrap_or_default()
            {
                ensure!(now.contains(&name), "{path} dropped {name} from its repos");
            }
        }
        for link in wikilinks(body) {
            ensure!(
                notes.contains(&link_key(&link)),
                "{path} links [[{link}]], which is no note"
            );
        }
        let defined = footnotes(body);
        for label in footnote_marks(body) {
            ensure!(
                defined.iter().any(|(l, _)| *l == label),
                "{path}'s footnote [^{label}] has no definition"
            );
        }
        for (label, definition) in defined {
            let targets = footnote_targets(&definition);
            ensure!(
                !targets.is_empty(),
                "{path}'s footnote [^{label}] names no entry"
            );
            for target in targets {
                let file = target.split('#').next().unwrap_or_default();
                let exists = match file.ends_with(".md") {
                    true => resolved(path, file)
                        .is_some_and(|entry| is_entry_path(&entry) && brain.join(entry).is_file()),
                    false => !is_entry_id(file) || entries.contains(file),
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

/// `link`, relative to the brain-folder file `from`, as a path in the brain folder; `None` when
/// it leaves the folder.
fn resolved(from: &str, link: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in link.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The labels of the footnote marks in `body`'s text, leaving out their definitions.
fn footnote_marks(body: &str) -> Vec<String> {
    let mut marks: Vec<String> = vec![];
    for line in body.lines() {
        let mut rest = line;
        if let Some(definition) = rest.strip_prefix("[^") {
            rest = definition.split_once("]:").map_or(rest, |(_, text)| text);
        }
        while let Some(at) = rest.find("[^") {
            let Some((label, after)) = rest[at + 2..].split_once(']') else {
                break;
            };
            if !marks.iter().any(|m| m == label) {
                marks.push(label.to_owned());
            }
            rest = after;
        }
    }
    marks
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

/// The scheduler's `run` for a wiki's compile job. A run due in a GitHub repository is owed until
/// a pass made `main` fresh; the pass asked for here starts it.
fn run_built_in(host: &mut Host, repository: &str, start: jobs::BuiltInStart) -> Result<String> {
    let (trigger, due) = match start {
        jobs::BuiltInStart::Due { catch_up, due } => (
            match catch_up {
                0 => CompileTrigger::Scheduled,
                _ => CompileTrigger::CatchUp,
            },
            Some(due),
        ),
        jobs::BuiltInStart::RunNow => (CompileTrigger::RunNow, None),
    };
    let brain = host.brain(repository)?;
    let (storage, _) = host.brain_home(&brain)?;
    if let (Some(due), Some(key)) = (&due, Host::storage_key(&storage)) {
        host.owe_compile(&brain.repository, due, trigger)?;
        host.issues_written.borrow_mut().insert(key);
        return Ok("a pull goes first".into());
    }
    let Some(run) = host.start_compile(repository, trigger, due)? else {
        return Ok("a compile run is in progress".into());
    };
    Ok(match run.reason.is_empty() {
        true => format!("compile run {} started", run.id),
        false => run.reason,
    })
}

/// A run this Mac still owes: due and waiting for a pull, or, with `lost_run`, lost to another
/// Mac's claim and reading the tip after each pull.
struct Owed {
    due: String,
    catch_up: bool,
    lost_run: Option<String>,
}

impl Host {
    /// Registers one built-in compile job per wiki this Mac compiles, due at the folder's compile
    /// time, or only on Run now without one.
    pub(crate) fn refresh_compile_jobs(&mut self) -> Result<()> {
        let mut wanted = vec![];
        for (storage, spaces) in self.wiki_spaces()? {
            if !self.compiles_here(&storage)? {
                continue;
            }
            let repository = storage.key();
            let tracker = (spaces.first()).and_then(|space| self.tracker(space).ok());
            let compile = tracker.and_then(|tracker| tracker.compile);
            let schedule = compile.and_then(|compile| match schedule(&compile) {
                Ok(schedule) => Some(schedule),
                Err(error) => {
                    eprintln!("The compile time of {repository}: {error:#}");
                    None
                }
            });
            wanted.push(jobs::BuiltIn {
                name: format!("{JOB}{}", repository.to_ascii_lowercase()),
                owner: format!("the wiki in {repository}"),
                subject: repository,
                schedule,
                run: run_built_in,
            });
        }
        self.built_ins.retain(|b| !b.name.starts_with(JOB));
        for job in &wanted {
            self.forget_stale_due(job)?;
        }
        self.built_ins.extend(wanted);
        Ok(())
    }

    /// Forgets a job's stored due time when its schedule no longer names it, as after the compile
    /// time changed. A due time the schedule still names stays, so a missed run catches up.
    fn forget_stale_due(&self, job: &jobs::BuiltIn) -> Result<()> {
        let key = job.key();
        let stored: Option<i64> = self
            .db
            .query_row(
                "SELECT next_at FROM job_due WHERE key=?1",
                [key.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let named = match (stored, &job.schedule) {
            (Some(due), Some(schedule)) => jobs::names_time(schedule, due)?,
            _ => stored.is_none(),
        };
        if !named {
            (self.db).execute("DELETE FROM job_due WHERE key=?1", [key.as_str()])?;
        }
        Ok(())
    }

    /// Forgets the wiki's job's due time, as this Mac stopped compiling it.
    pub(crate) fn forget_compile_due(&self, repository: &str) -> Result<()> {
        let key = compile_key(repository);
        (self.db).execute("DELETE FROM job_due WHERE key=?1", [key.as_str()])?;
        Ok(())
    }

    fn owed(&self, repository: &str) -> Result<Option<Owed>> {
        Ok(self
            .db
            .query_row(
                "SELECT due,catch_up,lost_run FROM compile_owed WHERE repository=?1",
                [repository],
                |r| {
                    Ok(Owed {
                        due: r.get(0)?,
                        catch_up: r.get(1)?,
                        lost_run: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    fn owe_compile(&self, repository: &str, due: &str, trigger: CompileTrigger) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO compile_owed(repository,due,catch_up) VALUES (?1,?2,?3)",
            params![repository, due, trigger == CompileTrigger::CatchUp],
        )?;
        Ok(())
    }

    fn clear_owed(&self, repository: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM compile_owed WHERE repository=?1", [repository])?;
        Ok(())
    }

    /// A pull of the repository `key` ended: a run owed starts, or reads the tip when it lost.
    pub(crate) fn compile_after_pull(&mut self, key: &str) -> Result<()> {
        let Some(repository) = self.brain_keys()?.remove(key) else {
            return Ok(());
        };
        let Some(owed) = self.owed(&repository)? else {
            return Ok(());
        };
        let brain = self.brain(&repository)?;
        let (storage, root) = self.brain_home(&brain)?;
        if !self.compiles_here(&storage)? {
            return self.clear_owed(&repository);
        }
        if owed.lost_run.is_none() {
            let trigger = match owed.catch_up {
                true => CompileTrigger::CatchUp,
                false => CompileTrigger::Scheduled,
            };
            self.start_compile(&repository, trigger, Some(owed.due))?;
            return Ok(());
        }
        let Some(head) = self.brain_head(&storage)? else {
            return Ok(());
        };
        self.compile_tip_reads.push(TipRead {
            claimer: self.claimer(key, &owed.due)?,
            checkout: self.storage_folder(&storage),
            brain_dir: brain_sync::brain_dir(&root),
            repository,
            head,
        });
        Ok(())
    }

    /// An owed run's tip: done when it landed this due time or nothing is pending, and a claim
    /// that freed starts the run. A live claim, or no answer, leaves it owed.
    pub(crate) fn compile_tip_read(&mut self, reading: TipReading) -> Result<()> {
        let repository = reading.repository;
        let (Some(owed), Ok(tip)) = (self.owed(&repository)?, reading.tip) else {
            return Ok(());
        };
        if let Some(tip) = &tip {
            self.name_winner(&owed, tip)?;
            if tip.landed(&owed.due).is_some() {
                return self.clear_owed(&repository);
            }
        }
        if reading.nothing_pending {
            return self.clear_owed(&repository);
        }
        if tip.is_none_or(|tip| tip.free(now())) {
            let trigger = match owed.catch_up {
                true => CompileTrigger::CatchUp,
                false => CompileTrigger::Scheduled,
            };
            self.start_compile(&repository, trigger, Some(owed.due))?;
        }
        Ok(())
    }

    /// Names the Mac a run lost to, which it could not read at the moment it lost, as a refused
    /// claim fetches nothing.
    fn name_winner(&mut self, owed: &Owed, tip: &Tip) -> Result<()> {
        let (Some(lost), Some(claim)) = (&owed.lost_run, &tip.claim) else {
            return Ok(());
        };
        let mut run = self.compile_run(lost)?;
        let reason = format!("Lost to {}", claim.machine);
        if run.reason != reason {
            run.reason = reason;
            self.save_compile_run(&run)?;
        }
        Ok(())
    }

    fn claimer(&self, key: &str, due: &str) -> Result<Claimer> {
        Ok(Claimer {
            url: self.stored_repository(key)?.push_url,
            host: self.host_id()?,
            machine: sync::machine_name(),
            login: self.gh_login.clone().unwrap_or_default(),
            due: due.to_owned(),
            running: self.sync_passes.clone(),
        })
    }

    /// The Spaces of each wiki on this Mac, by the repository that holds it.
    fn wiki_spaces(&self) -> Result<Vec<(StorageRepository, Vec<Space>)>> {
        let mut wikis: Vec<(StorageRepository, Vec<Space>)> = vec![];
        for record in self.spaces()? {
            let storage = record.space.storage.repository.clone();
            match wikis
                .iter_mut()
                .find(|(repository, _)| *repository == storage)
            {
                Some((_, spaces)) => spaces.push(record.space),
                None => wikis.push((storage, vec![record.space])),
            }
        }
        Ok(wikis)
    }

    /// The names of the Spaces in the repository, as usage names a wiki.
    pub(crate) fn wiki_name(&self, repository: &str) -> Result<String> {
        let names: Vec<String> = (self.wiki_spaces()?.into_iter())
            .filter(|(storage, _)| storage.key() == repository)
            .flat_map(|(_, spaces)| spaces)
            .map(|space| space.name)
            .collect();
        Ok(match names.is_empty() {
            true => repository.to_owned(),
            false => names.join(", "),
        })
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

    /// The files of the repository's run waiting to land.
    pub(crate) fn landing_of(&self, repository: &str) -> Result<Option<Landing>> {
        Ok(match self.running_compile(repository)? {
            Some(run) => self.compile_work(&run.id).ok().and_then(|w| w.landing),
            None => None,
        })
    }

    /// Run now: starts a run through the scheduler, or says why it waits. Its run reads the tip
    /// fresh, so a live claim on another Mac ends it with no model call, saying so, and a claim
    /// decides between Macs that press it at once.
    pub fn run_brain_compile(&mut self, repository: &str) -> Result<CompileRun> {
        let brain = self.brain(repository)?;
        let (storage, _) = self.brain_home(&brain)?;
        ensure!(
            self.compiles_here(&storage)?,
            "Compiling is off on this Mac. Turn on Compile on this Mac first."
        );
        ensure!(
            self.running_compile(&brain.repository)?.is_none(),
            "A compile run is in progress"
        );
        self.refresh_compile_jobs()?;
        self.run_job_now(&compile_key(&brain.repository))?;
        (self
            .brain_compile_runs(&brain.repository, 0, 1)?
            .runs
            .into_iter()
            .next())
        .context("The compile run was not recorded")
    }

    /// Why this Mac's Brain compiler row cannot run a compile now, when it cannot.
    fn compiler_waits(&self) -> Result<Option<String>> {
        if self.model_selection()?.row(&Job::Compiler).is_none() {
            return Ok(Some(
                "Waiting for a Brain compiler model: choose one in Models".into(),
            ));
        }
        Ok(match self.resolve_job(None, &Job::Compiler, false, false) {
            Ok(pick) if !pick.waits => None,
            Ok(pick) => Some(pick.reason),
            Err(error) => Some(format!("{error:#}")),
        })
    }

    /// Records a run of the wiki started by `trigger`, for the due time `due` or else as a Run
    /// now of its own, running while its preparation is queued. `None` while another run is in
    /// progress.
    pub(crate) fn start_compile(
        &mut self,
        repository: &str,
        trigger: CompileTrigger,
        due: Option<String>,
    ) -> Result<Option<CompileRun>> {
        let brain = self.brain(repository)?;
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
            unasked: 0,
            model_calls: 0,
            profile: None,
            procedure: None,
            outcome: CompileOutcome::Running,
            reason: String::new(),
            conflicts: 0,
            commit: None,
            tokens: 0,
            cost: None,
        };
        let (storage, root) = self.brain_home(&brain)?;
        match self.brain_head(&storage)? {
            None => {
                run.outcome = CompileOutcome::Failed;
                run.reason = "The repository has not been synced yet".into();
                run.ended_at = Some(at);
            }
            Some(head) => {
                let due = due.unwrap_or_else(|| format!("run-now:{}", run.id));
                let claimer = (Self::storage_key(&storage))
                    .map(|key| self.claimer(&key, &due))
                    .transpose()?;
                self.compile_preparations.push(Prepare {
                    run_id: run.id.clone(),
                    checkout: self.storage_folder(&storage),
                    head,
                    brain_dir: brain_sync::brain_dir(&root),
                    clones: Clones::of(&self.repositories()?)
                        .with_listed(&self.listed_repositories(&storage)?, &self.home),
                    claimer,
                    waits: self.compiler_waits()?,
                });
            }
        }
        self.save_compile_run(&run)?;
        Ok(Some(run))
    }

    pub(crate) fn take_compile_preparations(&mut self) -> Vec<Prepare> {
        std::mem::take(&mut self.compile_preparations)
    }

    /// The code repositories the Spaces in `storage` list, as `owner/name`.
    pub(crate) fn listed_repositories(&self, storage: &StorageRepository) -> Result<Vec<String>> {
        Ok((self.wiki_spaces()?.into_iter())
            .filter(|(repository, _)| repository == storage)
            .flat_map(|(_, spaces)| spaces)
            .flat_map(|space| space.repositories)
            .collect())
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
            let _ = fs::remove_dir_all(&work.worktree);
            if let Ok(clone) = (self.brain(&run.repository)).and_then(|b| self.brain_clone(&b)) {
                let _ = runtime::git_output(&clone, &["worktree", "prune"]);
            }
        }
        let _ = fs::remove_file(self.home.join(INPUTS).join(format!("{}.md", run.id)));
        self.end_claim(run)
    }

    /// Queues the end of the run's claim on GitHub, `landed` for one that landed and otherwise
    /// `released`, as a child of the claim. One another Mac took over ends with nothing pushed.
    fn end_claim(&mut self, run: &CompileRun) -> Result<()> {
        let held: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT commit_id,due FROM compile_claims WHERE run=?1",
                [&run.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((commit, due)) = held else {
            return Ok(());
        };
        self.db
            .execute("DELETE FROM compile_claims WHERE run=?1", [&run.id])?;
        let state = match run.outcome {
            CompileOutcome::Lost => return Ok(()),
            CompileOutcome::Landed => ClaimState::Landed,
            _ => ClaimState::Released,
        };
        let brain = self.brain(&run.repository)?;
        let (storage, _) = self.brain_home(&brain)?;
        let key = Self::storage_key(&storage).context("A local repository takes no claim")?;
        let claimer = self.claimer(&key, &due)?;
        self.claim_ends.push(ClaimEnd {
            clone: self.storage_folder(&storage),
            parent: commit,
            claim: Claim {
                host: claimer.host,
                machine: claimer.machine,
                login: claimer.login,
                run: run.id.clone(),
                due,
                state,
                at: now(),
            },
            url: claimer.url,
            running: claimer.running,
        });
        Ok(())
    }

    pub(crate) fn take_claim_ends(&mut self) -> Vec<ClaimEnd> {
        std::mem::take(&mut self.claim_ends)
    }

    pub(crate) fn take_compile_tip_reads(&mut self) -> Vec<TipRead> {
        std::mem::take(&mut self.compile_tip_reads)
    }

    /// A run that stopped before its model. A Mac stays owed when it lost or found a live claim,
    /// reading the tip from then on, and when it could not reach GitHub; otherwise it is done.
    fn compile_stopped(&mut self, run: &mut CompileRun, stopped: Stopped) -> Result<()> {
        let repository = run.repository.clone();
        // A Run now owes nothing, and leaves a due run as it stood.
        let lost = |host: &Self, run: &CompileRun| -> Result<()> {
            if run.trigger != CompileTrigger::RunNow {
                host.db.execute(
                    "UPDATE compile_owed SET lost_run=?2 WHERE repository=?1",
                    params![repository, run.id],
                )?;
            }
            Ok(())
        };
        match stopped {
            Stopped::Unreachable => {
                let reason = "GitHub could not be reached, so nothing was claimed".to_owned();
                self.end_compile(run, CompileOutcome::Waiting, reason)
            }
            Stopped::Live(machine) => {
                let reason = match run.trigger {
                    CompileTrigger::RunNow => format!("Compiling on {machine}, so no run started"),
                    _ => format!("Lost to {machine}"),
                };
                lost(self, run)?;
                self.end_compile(run, CompileOutcome::Lost, reason)
            }
            Stopped::Lost => {
                lost(self, run)?;
                let reason = "Another Mac claimed this run first".to_owned();
                self.end_compile(run, CompileOutcome::Lost, reason)
            }
            Stopped::CompiledElsewhere(machine) => {
                self.clear_owed(&repository)?;
                let reason = compiled_elsewhere(Some(&machine));
                self.end_compile(run, CompileOutcome::CompiledElsewhere, reason)
            }
            Stopped::Waits(reason) => {
                self.clear_owed(&repository)?;
                self.end_compile(run, CompileOutcome::Waiting, reason)
            }
            Stopped::Refused(why) => {
                self.clear_owed(&repository)?;
                let reason = format!("GitHub refused the claim: {why}");
                self.end_compile(run, CompileOutcome::Failed, reason)
            }
            Stopped::Failed(error) => {
                self.clear_owed(&repository)?;
                self.end_compile(run, CompileOutcome::Failed, error)
            }
        }
    }

    /// What the preparation found: a run that stopped ends; nothing pending ends it with no
    /// model call; otherwise it holds its claim, the Brain compiler row is resolved again and
    /// the run's session starts in its own worktree. A run that ended meanwhile gives its claim
    /// back.
    pub(crate) fn compile_prepared(&mut self, prepared: Prepared) -> Result<()> {
        let mut run = self.compile_run(&prepared.run_id)?;
        if let Some((commit, due)) = &prepared.claim {
            self.db.execute(
                "INSERT INTO compile_claims(run,commit_id,due) VALUES (?1,?2,?3)",
                params![run.id, commit, due],
            )?;
            self.clear_owed(&run.repository)?;
        }
        if run.outcome != CompileOutcome::Running {
            return self.end_claim(&run);
        }
        let gathered = match prepared.gathered {
            Ok(gathered) => gathered,
            Err(stopped) => return self.compile_stopped(&mut run, stopped),
        };
        run.pending = gathered.pending;
        if gathered.entries.is_empty() {
            self.clear_owed(&run.repository)?;
            let reason = "No pending entries · no model call".to_owned();
            return self.end_compile(&mut run, CompileOutcome::Skipped, reason);
        }
        run.unasked = (gathered.checked.iter())
            .filter(|checked| {
                checked.verification.reason.as_deref() == Some(verify_refs::NOT_ASKED)
            })
            .count();
        if let Some(reason) = self.compiler_waits()? {
            return self.end_compile(&mut run, CompileOutcome::Waiting, reason);
        }
        let pick = match self.resolve_job(None, &Job::Compiler, false, false) {
            Ok(pick) if !pick.waits => pick,
            Ok(pick) => return self.end_compile(&mut run, CompileOutcome::Waiting, pick.reason),
            Err(error) => {
                return self.end_compile(&mut run, CompileOutcome::Waiting, format!("{error:#}"));
            }
        };
        let brain = self.brain(&run.repository)?;
        let (storage, root) = self.brain_home(&brain)?;
        let checkout = self.storage_folder(&storage);
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
        let brain_dir = brain_sync::brain_dir(&root);
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
            landing: None,
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
            "Compile run {} of the wiki in {}. Your working folder is a worktree of the repository at {head}, and the brain folder is `{brain_dir}/`. First read your input, {}, whole: it holds the {} pending entries with their verification, STATUS, INDEX and the notes they name. Then follow the compile procedure and end with finish_compile.",
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
                    Ruling::NAMES.contains(&recommend.as_str()),
                    "recommend is one of {}",
                    Ruling::NAMES.join(", ")
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
                // A run that already finished waits to land, and stays as it is.
                if work.landing.is_some() {
                    return Ok(json!({"finished": true, "conflicts": run.conflicts}));
                }
                match self.finish_compile(&mut run, work) {
                    Ok(()) => Ok(json!({"finished": true, "conflicts": run.conflicts})),
                    Err(error) => {
                        let reason = format!("{error:#}");
                        self.end_compile(&mut run, CompileOutcome::Failed, reason.clone())?;
                        bail!("The run failed and nothing landed: {reason}")
                    }
                }
            }
            other => bail!("{other} is not one of a compile run's tools"),
        }
    }

    /// Checks the session's changes, writes the cursor and the conflict queue into its worktree,
    /// and hands the run's exact files to the wiki rule set; a local repository commits them at
    /// once.
    fn finish_compile(&mut self, run: &mut CompileRun, mut work: Work) -> Result<()> {
        let changed = check_changes(&work.worktree, &work.brain_dir)?;
        for path in changed.iter().map(String::as_str).chain([INDEX, QUEUE]) {
            let path = format!("{}/{path}", work.brain_dir);
            brain_sync::refuse_links(&work.worktree, &path)?;
        }
        let brain = work.worktree.join(&work.brain_dir);
        let queued: Vec<Queued> = (self
            .db
            .prepare("SELECT data FROM compile_conflicts WHERE run_id=?1 ORDER BY rowid")?)
        .query_map([&run.id], |r| decode(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
        // Only a decision entry is a ruling, as `brain_rulings` reads them.
        let ruled: HashSet<String> = (work.entries.iter())
            .filter(|e| e.kind == "decision")
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
        shared::write_atomically(&brain.join(INDEX), index.as_bytes())?;
        let in_repository = |path: &str| format!("{}/{path}", work.brain_dir);
        let mut files: Vec<String> = (changed.iter())
            .filter(|path| path.as_str() != INDEX)
            .map(|path| in_repository(path))
            .collect();
        files.push(in_repository(INDEX));
        if queue != existing {
            shared::write_atomically(&brain.join(QUEUE), queue.as_bytes())?;
            files.push(in_repository(QUEUE));
        }
        let claim = self
            .db
            .query_row(
                "SELECT commit_id FROM compile_claims WHERE run=?1",
                [&run.id],
                |r| r.get(0),
            )
            .optional()?;
        let landing = Landing {
            claim,
            run: run.id.clone(),
            worktree: work.worktree.clone(),
            head: work.head.clone(),
            cursor: work.cursor.clone(),
            index: in_repository(INDEX),
            entries: work.entries.len(),
            files,
        };
        work.landing = Some(landing.clone());
        run.conflicts = queued.len();
        run.reason = format!(
            "{} entries compiled · {} conflicts queued",
            work.entries.len(),
            queued.len()
        );
        self.db.execute(
            "UPDATE compile_runs SET work=?2,data=?3 WHERE id=?1",
            params![run.id, serde_json::to_string(&work)?, encode(&*run)?],
        )?;
        let brain = self.brain(&run.repository)?;
        let (storage, _) = self.brain_home(&brain)?;
        if let Some(key) = Self::storage_key(&storage) {
            self.issues_written.borrow_mut().insert(key);
            return Ok(());
        }
        self.land_locally(&landing, &self.storage_folder(&storage))
    }

    /// Lands a run in a local repository, which has no pass to go through.
    fn land_locally(&mut self, landing: &Landing, clone: &Path) -> Result<()> {
        let head = runtime::git_output(clone, &["rev-parse", "HEAD"])?;
        match landing.verdict(clone, head.trim())? {
            brain_sync::Verdict::AlreadyOnMain(commit) => {
                self.end_landing(landing, CompileOutcome::Landed, None, Some(&commit))
            }
            brain_sync::Verdict::CompiledElsewhere(machine) => {
                let reason = compiled_elsewhere(machine.as_deref());
                let outcome = CompileOutcome::CompiledElsewhere;
                self.end_landing(landing, outcome, Some(reason), None)
            }
            brain_sync::Verdict::Land(files) => {
                for (path, content) in &files {
                    shared::write_atomically(&clone.join(path), content)?;
                }
                let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
                let message = format!("Compiled {} entries", landing.entries);
                storage::commit_local(clone, &paths, &message)?;
                let commit = runtime::git_output(clone, &["rev-parse", "HEAD"])?;
                self.brains_now = true;
                let outcome = CompileOutcome::Landed;
                self.end_landing(landing, outcome, None, Some(commit.trim()))
            }
        }
    }

    /// Ends a run whose files reached their landing, naming its commit when it made one.
    pub(crate) fn end_landing(
        &mut self,
        landing: &Landing,
        outcome: CompileOutcome,
        reason: Option<String>,
        commit: Option<&str>,
    ) -> Result<()> {
        let mut run = self.compile_run(&landing.run)?;
        if run.outcome != CompileOutcome::Running {
            return Ok(());
        }
        run.commit = commit.map(str::to_owned);
        let reason = reason.unwrap_or_else(|| run.reason.clone());
        self.end_compile(&mut run, outcome, reason)
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
        // A finished run's turn ends before its files land.
        if self
            .compile_work(&run.id)
            .is_ok_and(|w| w.landing.is_some())
        {
            return Ok(());
        }
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

    /// Ends the runs a stopped host cut off, as their sessions' turns cannot finish any longer. A
    /// run that finished keeps its output: a local one lands now, and a GitHub one waits for the
    /// repository's next pass.
    pub(crate) fn end_interrupted_compiles(&mut self) -> Result<()> {
        let running: Vec<String> = (self.db.prepare(
            "SELECT id FROM compile_runs WHERE json_extract(data,'$.outcome')='running'",
        )?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        for id in running {
            let mut run = self.compile_run(&id)?;
            let landing = (self.compile_work(&id).ok()).and_then(|work| work.landing);
            let waiting = landing.filter(|landing| landing.worktree.exists());
            let storage = (self.brain(&run.repository))
                .and_then(|brain| self.brain_home(&brain))
                .map(|(storage, _)| storage);
            match (waiting, storage) {
                (Some(_), Ok(storage)) if Self::storage_key(&storage).is_some() => {}
                (Some(landing), Ok(storage)) => {
                    let clone = self.storage_folder(&storage);
                    if let Err(error) = self.land_locally(&landing, &clone) {
                        let reason = format!("{error:#}");
                        self.end_compile(&mut run, CompileOutcome::Failed, reason)?;
                    }
                }
                _ => {
                    let reason = "Wiffletree stopped while the run was in progress".to_owned();
                    self.end_compile(&mut run, CompileOutcome::Failed, reason)?;
                }
            }
        }
        Ok(())
    }

    /// A page of the wiki's runs, newest first, each with its session's usage.
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

    /// The compile of each wiki on this Mac, for the Brains page.
    pub(crate) fn brain_compile_states(&self) -> Result<Vec<BrainCompileState>> {
        let needs_model = self.model_selection()?.row(&Job::Compiler).is_none();
        let mut states = vec![];
        for brain in self.brains()? {
            let status = self.brain_status(&brain.repository).ok();
            let pending = status.as_ref().map_or(0, |status| status.pending_entries);
            let (storage, _) = self.brain_home(&brain)?;
            let compilers = self.compilers(&storage)?;
            let last_run = (self.brain_compile_runs(&brain.repository, 0, 1)?.runs)
                .into_iter()
                .next();
            // Scheduled times count from when entries began to wait since the last compile.
            let waiting_since = status.and_then(|status| {
                let oldest = status.oldest_pending?;
                Some(status.last_compile.map_or(oldest, |at| at.max(oldest)))
            });
            states.push(BrainCompileState {
                behind: self.falling_behind(&brain, &compilers, pending, waiting_since)?,
                run_now_waits: (self.running_compile(&brain.repository)?)
                    .map(|_| "A compile run is in progress".into()),
                compiles_here: self.compiles_here(&storage)?,
                last_run,
                compilers,
                repository: brain.repository,
                pending,
                needs_model,
            });
        }
        Ok(states)
    }

    /// Why the wiki is falling behind: no Mac compiles it and entries wait, or two scheduled
    /// times passed with entries waiting since `waiting_since`. A local repository's one Mac
    /// always compiles it.
    fn falling_behind(
        &self,
        brain: &Brain,
        compilers: &[Compiler],
        pending: usize,
        waiting_since: Option<i64>,
    ) -> Result<Option<String>> {
        if pending == 0 {
            return Ok(None);
        }
        let (storage, _) = self.brain_home(brain)?;
        let name = self.wiki_name(&brain.repository)?;
        if compilers.is_empty() && Self::storage_key(&storage).is_some() {
            return Ok(Some(format!(
                "No Mac compiles {name}'s wiki. {}.",
                entries_waiting(pending)
            )));
        }
        let space = (self.wiki_spaces()?.into_iter())
            .find(|(repository, _)| *repository == storage)
            .and_then(|(_, mut spaces)| spaces.pop());
        let compile = space.and_then(|space| self.tracker(&space).ok()?.compile);
        let (Some(compile), Some(waiting_since)) = (compile, waiting_since) else {
            return Ok(None);
        };
        let last_run = (self
            .brain_compile_runs(&brain.repository, 0, 1)?
            .runs
            .first())
        .map_or("no compile yet".to_owned(), |run| {
            format!("last run {}", memory::local_date(run.started_at))
        });
        // A shared file may hold a time the schedule cannot read, as `refresh_compile_jobs` logs.
        let missed = schedule(&compile).and_then(|s| jobs::times_since(&s, waiting_since, now()));
        Ok(
            (missed.is_ok_and(|missed| missed >= MISSED_TIMES_BEHIND)).then(|| {
                format!(
                    "{name}'s wiki is falling behind: {}, {last_run}.",
                    entries_waiting(pending).to_lowercase()
                )
            }),
        )
    }

    /// Every open conflict of this Mac's wikis, as cards for the Inbox of each project in a Space
    /// using the folder whose repositories include the claim's, or for the Brains page when none
    /// does. A deferred card hides until the next compile.
    pub(crate) fn conflict_cards(&self) -> Result<Vec<ConflictCard>> {
        let mut cards = vec![];
        let projects = self.projects()?;
        for brain in self.brains()? {
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
                        landed: true,
                    },
                    (Some(local), None) => ConflictState::Ruled {
                        ruling: local.ruling.label().into(),
                        by: local.by.clone(),
                        landed: false,
                    },
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

    /// The ruling made here on a conflict, unless its entry was discarded before it reached
    /// `main`, which leaves the conflict open to rule on again.
    fn local_ruling(&self, repository: &str, conflict: &str) -> Result<Option<LocalRuling>> {
        let local: Option<LocalRuling> = self
            .db
            .query_row(
                "SELECT data FROM conflict_rulings WHERE repository=?1 AND conflict_id=?2",
                params![repository, conflict],
                |r| decode(r, 0),
            )
            .optional()?;
        let discarded = |entry: &str| self.entry_discarded(repository, entry).unwrap_or(false);
        Ok(local.filter(|l| !l.entry.as_deref().is_some_and(discarded)))
    }

    /// The human's ruling on a conflict. Uphold existing, Keep new and Keep both write one
    /// `decision` entry by the human, in the claim's repository folder, whose `state` begins
    /// `Ruling on conflict <id>:`, which lands as any log entry does; Defer writes nothing and
    /// hides the card until the next compile, and the next compile picks a ruling up. Only the
    /// human's client sends this command.
    pub fn rule_conflict(
        &mut self,
        repository: &str,
        conflict_id: &str,
        ruling: Ruling,
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
        // A conflict is ruled once, here or on any install whose ruling reached `main`.
        let ruled_here = (self.local_ruling(&brain.repository, conflict_id)?)
            .is_some_and(|l| l.ruling != Ruling::Defer);
        let ruled_on_main =
            (self.brain_rulings(&brain.repository)?.iter()).any(|r| r.conflict_id == conflict_id);
        ensure!(
            !ruled_here && !ruled_on_main,
            "{conflict_id} is already ruled on"
        );
        if ruling == Ruling::Defer {
            let local = LocalRuling {
                ruling,
                by: self.gh_login.clone().unwrap_or_default(),
                entry: None,
                last_compile: self.last_compile_commit(&brain.repository)?,
            };
            self.save_local_ruling(&brain.repository, conflict_id, &local)?;
            return Ok(None);
        }
        let (storage, root) = self.brain_home(&brain)?;
        let author = self.entry_author(&storage)?;
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
        let path = memory::entry_path(&brain_sync::brain_dir(&root), &folder, &date, &id);
        let file = memory::entry_text(&id, &folder, &author, "wiffletree", &body);
        let space = (self.spaces()?.into_iter())
            .find(|record| record.space.storage.repository == storage)
            .context("No Space here is in this repository")?
            .space;
        self.land_entry(&space, &path, &file, None)?;
        let local = LocalRuling {
            ruling,
            by: author,
            entry: Some(path),
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
    /// Who ruled, as the entry's author.
    by: String,
    /// The ruling entry's file in the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry: Option<String>,
    /// For Defer: the commit of the wiki's last compile when the human deferred; the card
    /// shows again once another compile lands.
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
    use crate::brain_sync::tests::{FOLDER, PHONE, REPO, Stored, stored};
    use workspace_core::{Line, ModelProfile, Then};

    /// The brain folder as a path of the repository.
    const BRAIN: &str = "wiffletree/second-brain";

    /// The stored Space's wiki, compiled on this Mac.
    fn brained() -> Stored {
        let mut b = stored();
        let space = b.sage.id.clone();
        b.f.host.set_compile_here(&space, true).unwrap();
        b.sync();
        b
    }

    /// Makes GitHub refuse every push, with the branch left where it is; the hook that does.
    fn refusing_pushes(b: &Stored) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let hook = b.github.bare.join("hooks/pre-receive");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(
            &hook,
            "#!/bin/sh\necho main needs a pull request >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        hook
    }

    impl Stored {
        /// What a run's preparation finds now, as the service gathers it.
        fn gather(&self) -> Gathered {
            let prepare = Prepare {
                run_id: "r".into(),
                checkout: self.clone_folder(),
                head: self.head(),
                brain_dir: BRAIN.into(),
                clones: Clones::default(),
                claimer: None,
                waits: None,
            };
            prepare.run(&self.tools).gathered.unwrap()
        }
        fn head(&self) -> String {
            let brain = self.f.host.brain(REPO).unwrap();
            let (storage, _) = self.f.host.brain_home(&brain).unwrap();
            self.f.host.brain_head(&storage).unwrap().unwrap()
        }
        /// Prepares every queued run, reads every owed tip and ends every claim, handing each to
        /// the host, as the service does.
        fn prepare(&mut self) {
            self.end_claims();
            for read in self.f.host.take_compile_tip_reads() {
                let reading = read.run(&self.tools.git);
                self.f.host.compile_tip_read(reading).unwrap();
            }
            for prepare in self.f.host.take_compile_preparations() {
                let prepared = prepare.run(&self.tools);
                self.f.host.compile_prepared(prepared).unwrap();
            }
            self.end_claims();
        }
        fn end_claims(&mut self) {
            for end in self.f.host.take_claim_ends() {
                end.push(&self.tools.git);
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
            self.f.host.compile_tool(session, name, &args)
        }
        /// A run that wrote `note` and finished, whose files wait to land.
        fn finished(&mut self, note: &str) -> (CompileRun, Work) {
            let (run, session, work) = self.started();
            let brain = work.worktree.join(BRAIN);
            fs::create_dir_all(brain.join("notes")).unwrap();
            fs::write(brain.join("notes/Call Relay.md"), note).unwrap();
            self.tool(&session, "finish_compile", json!({"summary": "Done"}))
                .unwrap();
            (run, work)
        }
        fn cursor_on_main(&self) -> Cursor {
            let index = self.github.show(&format!("{BRAIN}/notes/INDEX.md"));
            read_cursor(&index.unwrap_or_default())
        }
        fn run_named(&self, run: &CompileRun) -> CompileRun {
            self.f.host.compile_run(&run.id).unwrap()
        }
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
    fn with_no_compiled_through_every_entry_is_pending_but_those_in_compiled_after() {
        let mut b = brained();
        let index = write_cursor(
            "",
            &Cursor {
                through: None,
                after: vec![id(1)],
            },
        );
        b.merge(&[relay(1), relay(2), ("notes/INDEX.md".into(), index)]);
        let ids: Vec<String> = b.gather().entries.into_iter().map(|e| e.id).collect();
        assert_eq!(ids, [id(2)]);
    }

    #[test]
    fn a_cursor_from_before_the_move_makes_only_newer_entries_pending() {
        // The repository as it stood before the move: entries at its root.
        let clone = tempfile::tempdir().unwrap();
        let at = clone.path();
        let commit = |path: &str, text: &str| {
            let file = at.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
            crate::test_support::git(at, &["add", "--all"]);
            crate::test_support::git(at, &["commit", "-q", "-m", path]);
            crate::test_support::git(at, &["rev-parse", "HEAD"])
        };
        crate::test_support::git(at, &["init", "-q", "-b", "main"]);
        let (before, text) = relay(1);
        let through = commit(&format!("second-brain/{before}"), &text);
        // The move renames the entry.
        crate::test_support::git(at, &["mv", "second-brain", "wiffletree"]);
        fs::create_dir(at.join("moved")).unwrap();
        fs::rename(at.join("wiffletree"), at.join("moved/second-brain")).unwrap();
        fs::rename(at.join("moved"), at.join("wiffletree")).unwrap();
        crate::test_support::git(at, &["add", "--all"]);
        crate::test_support::git(at, &["commit", "-q", "-m", "Move into wiffletree/"]);
        let (after, text) = relay(2);
        let head = commit(&format!("wiffletree/second-brain/{after}"), &text);
        let paths = entry_paths(at, &head, "wiffletree/second-brain/", Some(&through)).unwrap();
        assert_eq!(paths, [after]);
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
        assert!(page.compiles.iter().all(|compile| compile.needs_model));
    }

    #[test]
    fn no_job_is_scheduled_so_a_compile_runs_only_on_run_now() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.f.host.refresh_compile_jobs().unwrap();
        assert!(b.f.host.built_ins.iter().all(|job| job.schedule.is_none()));
        let later = now() + 2 * 86_400_000;
        b.f.host.fire_and_check(later).unwrap();
        b.prepare();
        assert!(b.runs().is_empty());
        // No Space lists it.
        assert!(b.f.host.job_states().unwrap().is_empty());
    }

    #[test]
    fn run_now_says_why_while_a_run_is_in_progress() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.started();
        let refused = b.f.host.run_brain_compile(REPO).unwrap_err();
        assert_eq!(refused.to_string(), "A compile run is in progress");
        let page = b.f.host.brains_page().unwrap();
        let compile = page.compiles.iter().find(|c| c.repository == REPO);
        assert_eq!(
            compile.unwrap().run_now_waits.as_deref(),
            Some("A compile run is in progress")
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
        assert!(work.worktree.join(BRAIN).join("logs").is_dir());
        let launch = b.f.host.compile_launch(&session).unwrap();
        assert!(launch.compile.unwrap().ends_with(BRAIN));
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
    fn a_finished_compile_lands_its_files_the_cursor_and_the_conflict_queue_in_one_commit() {
        let mut b = brained();
        let head = b.merge(&[relay(1)]);
        let (run, session, work) = b.started();
        let brain = work.worktree.join(BRAIN);
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
        // Nothing has landed until the pass, and the turn ending changes nothing.
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Running);
        b.f.host
            .compile_turn_ended(&session.id, None, None)
            .unwrap();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Running);
        assert_eq!(b.cursor_on_main(), Cursor::default());
        b.tool(&session, "finish_compile", json!({"summary": "Again"}))
            .unwrap();
        assert_eq!(
            b.run_named(&run).outcome,
            CompileOutcome::Running,
            "it finishes once"
        );
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Landed, "{}", run.reason);
        assert_eq!(run.conflicts, 1);
        assert_eq!(run.commit.as_deref(), Some(b.github.log("%H")[0].as_str()));
        assert_eq!(
            b.github.log("%s")[0],
            format!("Compiled 1 entries · {}", sync::machine_name())
        );
        let shown = crate::test_support::git(
            &b.github.bare,
            &["show", "--format=", "--name-only", "main"],
        );
        let mut files: Vec<&str> = shown.lines().collect();
        files.sort();
        assert_eq!(
            files,
            [
                "wiffletree/second-brain/STATUS.md",
                "wiffletree/second-brain/notes/Call Relay.md",
                "wiffletree/second-brain/notes/INDEX.md",
                "wiffletree/second-brain/reconcile-queue.md",
            ]
        );
        assert_eq!(b.cursor_on_main().through.as_deref(), Some(head.as_str()));
        let queue = b
            .github
            .show(&format!("{BRAIN}/reconcile-queue.md"))
            .unwrap();
        assert!(queue.contains(&format!("<!-- conflict:{conflict} -->")));
        assert!(queue.contains("## Retry cap · decision-contradiction"));
        assert!(queue.contains("- recommend: Uphold existing: PR #92 is still open"));
        // The clone holds what landed, and the worktree is gone.
        let written = b.clone_folder().join(BRAIN).join("notes/Call Relay.md");
        assert_eq!(fs::read_to_string(written).unwrap(), relay_note(&id(1)));
        assert!(!work.worktree.exists());
    }

    #[test]
    fn a_local_repository_commits_a_finished_compile_at_once() {
        let mut f = publish::tests::fixture();
        let home = f.host.create_space("Home", "", "", None).unwrap().space;
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &home.id))
        .unwrap();
        let repository = home.storage.repository.key();
        let clone = f.host.storage_folder(&home.storage.repository);
        let (path, text) = relay(1);
        let file = clone.join(BRAIN).join(&path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        storage::commit_local(&clone, &[&format!("{BRAIN}/{path}")], "An entry").unwrap();
        let mut selection = f.host.model_selection().unwrap();
        selection.set_compiler_row(Some(Line {
            first: ModelProfile {
                provider: Provider::Claude,
                model: "sonnet".into(),
                effort: "medium".into(),
            },
            then: Then::Wait,
        }));
        f.host.set_model_selection(&selection).unwrap();
        f.host.run_brain_compile(&repository).unwrap();
        for prepare in f.host.take_compile_preparations() {
            let prepared = prepare.run(&publish::Tools::default());
            f.host.compile_prepared(prepared).unwrap();
        }
        let run = (f.host.brain_compile_runs(&repository, 0, 1).unwrap().runs).remove(0);
        assert_eq!(run.outcome, CompileOutcome::Running, "{}", run.reason);
        let session: String = (f.host.db)
            .query_row(
                "SELECT session_id FROM compile_runs WHERE id=?1",
                [&run.id],
                |r| r.get(0),
            )
            .unwrap();
        let session = f.host.session(&session).unwrap();
        let work = f.host.compile_work(&run.id).unwrap();
        let notes = work.worktree.join(BRAIN).join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("Call Relay.md"), relay_note(&id(1))).unwrap();
        let args = json!({"summary": "Done"});
        f.host
            .compile_tool(&session, "finish_compile", &args)
            .unwrap();
        let run = f.host.compile_run(&run.id).unwrap();
        assert_eq!(run.outcome, CompileOutcome::Landed, "{}", run.reason);
        let head = crate::test_support::git(&clone, &["rev-parse", "HEAD"]);
        assert_eq!(run.commit, Some(head));
        assert!(clone.join(BRAIN).join("notes/Call Relay.md").is_file());
    }

    #[test]
    fn a_push_refused_because_logs_landed_goes_again_with_no_model_call_on_the_new_head() {
        let mut b = brained();
        let head = b.merge(&[relay(1)]);
        let (run, work) = b.finished(&relay_note(&id(1)));
        let sessions = b.f.host.sessions().unwrap().len();
        // Three entries land after the run read main, and one just before its push.
        let later: Vec<(String, String)> = vec![relay(2), relay(3)];
        b.commit(&later);
        let (path, text) = relay(4);
        let github = b.github.clone();
        let file = format!("{BRAIN}/{path}");
        *b.github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || github.commit(&file, text.as_str())),
        ));
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Landed, "{}", run.reason);
        assert_eq!(run.model_calls, 1, "no model is involved");
        assert_eq!(b.f.host.sessions().unwrap().len(), sessions);
        // The commit is on the new head, and its cursor still names the run's commit.
        let parent = b.github.log("%H")[1].clone();
        assert_eq!(run.commit.as_deref(), Some(b.github.log("%H")[0].as_str()));
        assert_ne!(parent, head);
        assert!(
            b.github.log("%s")[1].ends_with(".md"),
            "{:?}",
            b.github.log("%s")
        );
        assert_eq!(b.cursor_on_main().through.as_deref(), Some(head.as_str()));
        assert_eq!(work.cursor.through.as_deref(), Some(head.as_str()));
        // Those logs are pending in the next run.
        b.index();
        let ids: Vec<String> = b.gather().entries.into_iter().map(|e| e.id).collect();
        assert_eq!(ids, [id(2), id(3), id(4)]);
    }

    #[test]
    fn a_landing_github_refuses_for_good_ends_its_run_and_run_now_works_again() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        let hook = refusing_pushes(&b);
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert!(run.reason.contains("nothing landed"), "{}", run.reason);
        assert_eq!(b.cursor_on_main(), Cursor::default());
        fs::remove_file(&hook).unwrap();
        b.started();
    }

    #[test]
    fn a_landing_whose_push_cannot_reach_github_waits_and_lands_when_it_can() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        // The network drops under the push, with GitHub back in reach as the pass looks at why.
        let real = b.tools.git.clone();
        let dropping = real.clone();
        b.tools.git = std::sync::Arc::new(move |path: &Path, args: &[&str]| {
            match publish::subcommand(args) {
                "push" => Ok(runtime::GitAnswer {
                    status: publish::tests::exit(128),
                    output: String::new(),
                    error: "fatal: unable to access: Connection reset by peer".into(),
                }),
                _ => dropping(path, args),
            }
        });
        let _ = b.sync_answering();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Running);
        b.tools.git = real;
        b.sync_answering().unwrap();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Landed);
    }

    #[test]
    fn a_link_on_main_fails_the_run_and_no_file_is_written_through_it() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let outside = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let at = work.path();
        let git = crate::test_support::git;
        git(at, &["clone", "-q", b.github.bare.to_str().unwrap(), "."]);
        fs::create_dir_all(at.join(BRAIN)).unwrap();
        std::os::unix::fs::symlink(outside.path(), at.join(BRAIN).join("notes")).unwrap();
        git(at, &["add", "--all"]);
        git(at, &["commit", "-q", "-m", "A link"]);
        git(at, &["push", "-q", "origin", "HEAD:main"]);
        b.sync();
        let (run, session, _) = b.started();
        let failed = b
            .tool(&session, "finish_compile", json!({"summary": "Done"}))
            .unwrap_err();
        assert!(failed.to_string().contains("symbolic link"), "{failed:#}");
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert!(run.reason.contains("notes/INDEX.md"), "{}", run.reason);
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert_eq!(b.cursor_on_main(), Cursor::default());
    }

    #[test]
    fn a_link_in_the_clone_fails_the_landing_and_no_file_is_written_through_it() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        let outside = tempfile::tempdir().unwrap();
        let clone = b.clone_folder().join(BRAIN);
        fs::create_dir_all(&clone).unwrap();
        std::os::unix::fs::symlink(outside.path(), clone.join("notes")).unwrap();
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert!(run.reason.contains("symbolic link"), "{}", run.reason);
        assert!(run.reason.contains("/notes/"), "{}", run.reason);
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert_eq!(b.cursor_on_main(), Cursor::default());
    }

    #[test]
    fn a_cursor_already_past_lands_nothing_and_each_entry_compiles_once() {
        let mut b = brained();
        let head = b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        // A takeover's run landed the same cursor first.
        let landed = write_cursor(
            "",
            &Cursor {
                through: Some(head.clone()),
                after: vec![],
            },
        );
        b.commit(&[("notes/INDEX.md".into(), landed)]);
        let commits = b.github.log("%s").len();
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::CompiledElsewhere);
        assert_eq!(run.outcome.label(), "Compiled elsewhere");
        assert_eq!(run.commit, None);
        assert_eq!(b.github.log("%s").len(), commits, "nothing landed");
        assert_eq!(b.cursor_on_main().through, Some(head));
    }

    #[test]
    fn a_hand_edit_fails_the_run_and_stands_with_the_cursor_unchanged() {
        let mut b = brained();
        b.merge(&[
            relay(1),
            note("Call Relay", "observation", &[id(1)], "Health is /status."),
        ]);
        let (run, work) = b.finished(&relay_note(&id(1)));
        assert!(work.worktree.exists());
        let edited = note("Call Relay", "observation", &[id(1)], "Health is /live.");
        b.commit(std::slice::from_ref(&edited));
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert!(
            run.reason
                .contains("main changed wiffletree/second-brain/notes/Call Relay.md"),
            "{}",
            run.reason
        );
        assert_eq!(
            b.github.show(&format!("{BRAIN}/{}", edited.0)),
            Some(edited.1)
        );
        assert_eq!(b.cursor_on_main(), Cursor::default());
        assert!(!work.worktree.exists());
    }

    #[test]
    fn a_wiki_file_that_fails_the_scan_lands_nothing_and_names_the_file_and_rule() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, _) = b.finished(&format!("{}Call {PHONE}.\n", relay_note(&id(1))));
        let commits = b.github.log("%s").len();
        b.sync();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert!(run.reason.contains("notes/Call Relay.md"), "{}", run.reason);
        assert!(
            run.reason.contains("sensitive-info rule phone"),
            "{}",
            run.reason
        );
        assert!(!run.reason.contains("555"), "{}", run.reason);
        assert_eq!(
            b.github.log("%s").len(),
            commits,
            "nothing of the run lands"
        );
        assert_eq!(
            b.cursor_on_main(),
            Cursor::default(),
            "the cursor is unchanged"
        );
        // Nothing of the run lingers as a held file, and the next run finds the same entries.
        assert!(b.f.host.held_notes().unwrap().is_empty());
        assert_eq!(b.gather().entries.len(), 1);
    }

    #[test]
    fn an_older_run_record_with_a_pull_request_still_reads() {
        let old = json!({"id": "r", "repository": "Sage/brain", "trigger": "ruling",
            "started_at": 1, "pending": 1, "processed": 1, "model_calls": 1,
            "outcome": "pr_opened", "batch_id": "b", "pull_request": 214, "merged": true});
        let run: CompileRun = serde_json::from_value(old).unwrap();
        assert_eq!(run.outcome, CompileOutcome::Landed);
    }

    #[test]
    fn a_change_outside_the_wiki_or_a_broken_link_fails_the_run_and_leaves_the_cursor() {
        type Change = fn(&Path);
        let cases: [(&str, Change); 6] = [
            // A footnote out of `logs/` to a Markdown file that is no entry.
            ("which is no entry file", |brain| {
                fs::create_dir_all(brain.join("notes")).unwrap();
                let note = relay_note(&id(1)).replace(
                    &format!("../logs/app/2026/10/2026-10-20-{}.md", id(1)),
                    "../logs/../../README.md",
                );
                fs::write(brain.join("notes/Call Relay.md"), note).unwrap()
            }),
            ("has no definition", |brain| {
                fs::create_dir_all(brain.join("notes")).unwrap();
                let note = relay_note(&id(1)).replace("`/healthz`.[^1]", "`/healthz`.[^1][^2]");
                fs::write(brain.join("notes/Call Relay.md"), note).unwrap()
            }),
            ("outside the wiki", |brain| {
                fs::write(brain.join("logs/app/2026/10/extra.md"), "x\n").unwrap()
            }),
            ("a hidden path", |brain| {
                fs::create_dir_all(brain.join("notes")).unwrap();
                fs::write(brain.join("notes/.draft.md"), relay_note(&id(1))).unwrap()
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
            change(&work.worktree.join(BRAIN));
            let failed = b
                .tool(&session, "finish_compile", json!({"summary": "Done"}))
                .unwrap_err();
            assert!(failed.to_string().contains(why), "{why}: {failed:#}");
            let run = b.f.host.compile_run(&run.id).unwrap();
            assert_eq!(run.outcome, CompileOutcome::Failed, "{why}");
            assert!(run.reason.contains(why), "{why}: {}", run.reason);
            assert_eq!(b.cursor_on_main(), Cursor::default(), "{why}");
        }
    }

    #[test]
    fn a_session_stopped_at_30_minutes_times_its_run_out_and_lands_nothing() {
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
    fn with_conflict(b: &mut Stored) -> String {
        let (path, _) = relay(1);
        let queue = format!(
            "# Reconcile queue\n\n<!-- conflict:c-1 -->\n## Retry cap · decision-contradiction\n- claim: [{}]({path}), claimed\n- conflicts-with: [[Call Relay]]\n- evidence: PR #92 is open\n- recommend: Uphold existing: PR #92 is still open\n",
            id(1)
        );
        let decision = note("Call Relay", "decision", &[id(1)], "Retries cap at 5.");
        b.merge(&[relay(1), decision, ("reconcile-queue.md".into(), queue)]);
        "c-1".into()
    }

    fn conflict_notes(b: &Stored, project: &str) -> Vec<ConflictCard> {
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
        assert!(conflict_notes(&b, &other.id).is_empty());
        assert!(b.f.host.brains_page().unwrap().conflicts.is_empty());
        // Once the Space lists `app`, every project of it inherits it, and shows the card.
        let app = (b.f.host.repositories().unwrap().into_iter())
            .find_map(|r| r.github.map(|g| format!("{}/{}", g.owner, g.name)))
            .unwrap();
        b.f.host.set_repository_spaces(&app, &[sage]).unwrap();
        assert_eq!(conflict_notes(&b, &other.id).len(), 1);
        let card = (b.f.host.rule_conflict(REPO, &conflict, Ruling::KeepNew))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&card.state, ConflictState::Ruled { ruling, by, landed: false }
            if ruling == "Keep new" && by == "mhohlios")
        );
        // The decision entry is the human's, in the claim's folder, and lands as any entry.
        b.sync();
        let shown = crate::test_support::git(
            &b.github.bare,
            &["show", "--format=", "--name-only", "main"],
        );
        let path = shown.trim();
        assert!(path.starts_with(&format!("{BRAIN}/logs/app/")), "{path}");
        let text = b.github.show(path).unwrap();
        assert!(text.contains("author: mhohlios"));
        assert!(text.contains("- type: decision"));
        assert!(text.contains(&format!(
            "- state: Ruling on conflict {conflict}: Keep new."
        )));
        // A ruling starts no run: the next compile picks it up.
        assert!(b.runs().is_empty());
        let again =
            b.f.host
                .rule_conflict(REPO, &conflict, Ruling::UpholdExisting);
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
        // Nor is a ruled conflict deferred.
        let deferred = b.f.host.rule_conflict(REPO, &conflict, Ruling::Defer);
        assert!(deferred.unwrap_err().to_string().contains("already ruled"));
        // A ruled card no longer counts as needing the human.
        assert!(
            (b.f.host.workspace_notes(&[]).unwrap().iter())
                .all(|note| !matches!(note, WorkspaceNote::Conflict(_)) || !note.needs_you())
        );
    }

    #[test]
    fn a_ruling_whose_entry_is_discarded_leaves_the_conflict_open_to_rule_again() {
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        let project = b.f.coordinator.project_id.clone();
        let phone = Ruling::KeepBoth {
            scope: format!("Call {PHONE} first."),
        };
        (b.f.host.rule_conflict(REPO, &conflict, phone)).unwrap();
        b.sync();
        let held = b.f.host.held_entries().unwrap().remove(0);
        assert_eq!(held.project_id, None, "its card is on the Brains page");
        let page = b.f.host.brains_page().unwrap();
        assert_eq!(page.held, std::slice::from_ref(&held));
        b.f.host.discard_held_entry(REPO, &held.path).unwrap();
        let cards = conflict_notes(&b, &project);
        assert_eq!(cards[0].state, ConflictState::Open);
        assert!(WorkspaceNote::Conflict(Box::new(cards[0].clone())).needs_you());
        let card =
            (b.f.host
                .rule_conflict(REPO, &conflict, Ruling::UpholdExisting))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&card.state, ConflictState::Ruled { ruling, .. } if ruling == "Uphold existing")
        );
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
            b.f.host.rule_conflict(REPO, &conflict, ruling).unwrap();
            b.sync();
            let shown = crate::test_support::git(
                &b.github.bare,
                &["show", "--format=", "--name-only", "main"],
            );
            let text = b.github.show(shown.trim()).unwrap();
            assert!(
                text.contains(&format!("- state: Ruling on conflict {conflict}: {state}")),
                "{text}"
            );
        }
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        let project = b.f.coordinator.project_id.clone();
        assert_eq!(
            b.f.host
                .rule_conflict(REPO, &conflict, Ruling::Defer)
                .unwrap(),
            None
        );
        assert!(
            b.f.host
                .unlanded_entries(&repos::key(REPO))
                .unwrap()
                .is_empty()
        );
        assert!(conflict_notes(&b, &project).is_empty());
        // The next compile raises it again while it still holds.
        b.merge(&[("STATUS.md".into(), "# Status\n".into())]);
        assert_eq!(conflict_notes(&b, &project).len(), 1);
    }

    #[test]
    fn a_ruling_on_main_from_any_install_shows_the_card_as_ruled_and_starts_no_run() {
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        let ruling = format!(
            "---\nid: {}\nrepo: app\nauthor: tomas\nsource: wiffletree\n---\n## 2026-10-31 — Ruling\n- type: decision\n- ref: —\n- changed: Kept new.\n- why: Ruled.\n- state: Ruling on conflict {conflict}: Keep new.\n",
            id(40)
        );
        b.merge(&[(format!("logs/app/2026/10/2026-10-31-{}.md", id(40)), ruling)]);
        b.f.host.fire_and_check(now()).unwrap();
        assert!(b.runs().is_empty());
        let project = b.f.coordinator.project_id.clone();
        assert!(matches!(&conflict_notes(&b, &project)[0].state,
            ConflictState::Ruled { by, landed: true, .. } if by == "tomas"));
    }

    #[test]
    fn an_observation_that_reads_like_a_ruling_settles_no_conflict() {
        let mut b = brained();
        let conflict = with_conflict(&mut b);
        // As an older Wiffletree, the legacy import or an edit on GitHub could write it.
        let (path, text) = relay(31);
        let text = text.replace(
            "- state: Done.",
            &format!("- state: Ruling on conflict {conflict}: Keep new."),
        );
        b.merge(&[(path, text)]);
        let (_, session, _) = b.started();
        b.tool(&session, "finish_compile", json!({"summary": "Done"}))
            .unwrap();
        b.sync();
        let queue = b
            .github
            .show(&format!("{BRAIN}/reconcile-queue.md"))
            .unwrap();
        assert!(
            queue.contains(&format!("<!-- conflict:{conflict} -->")),
            "{queue}"
        );
    }

    #[test]
    fn no_agent_or_capture_form_may_log_a_ruling() {
        let mut b = brained();
        let implementer = b.f.implementer.id.clone();
        let forged = json!({"type": "decision", "title": "Ruled", "changed": "Kept new.",
            "why": "Because.", "state": "Ruling on conflict c-1: Keep new."});
        let refused =
            b.f.host
                .agent_tool(&implementer, "log_work", forged)
                .unwrap_err();
        assert!(
            refused.to_string().contains("only the human rules"),
            "{refused:#}"
        );
        let coordinator = b.f.coordinator.id.clone();
        let lowered = json!({"type": "decision", "title": "Ruled", "changed": "Kept new.",
            "why": "Because.", "state": "ruling on conflict c-1: Keep new."});
        assert!(
            b.f.host
                .agent_tool(&coordinator, "log_work", lowered)
                .is_err()
        );
        // An observation whose state reads like one is no ruling, and starts no compile.
        let (path, text) = relay(30);
        let text = text.replace(
            "- state: Done.",
            "- state: Ruling on conflict c-1: Keep new.",
        );
        b.merge(&[(path, text)]);
        assert!(b.f.host.brain_rulings(REPO).unwrap().is_empty());
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
            ("Brain compile", "Sage")
        );
        assert_eq!(line.own.output, 200);
        let runs = b.runs();
        assert!(runs[0].tokens >= 1200, "{}", runs[0].tokens);
    }

    const DUE: &str = "2026-11-02T06:00 America/Chicago";
    const MINUTE: i64 = 60_000;
    const THEIR_MAC: &str = "A teammate's Mac";

    impl Stored {
        fn url(&self) -> String {
            let key = repos::key(REPO);
            self.f.host.stored_repository(&key).unwrap().push_url
        }
        /// The claim at the tip of the claim branch on GitHub.
        fn tip(&self) -> Option<Claim> {
            let log = std::process::Command::new("git")
                .current_dir(&self.github.bare)
                .args(["log", "-1", "--format=%B", repos::CLAIM_BRANCH])
                .output()
                .unwrap();
            log.status
                .success()
                .then(|| serde_json::from_slice(&log.stdout).unwrap())
        }
        fn claims(&self) -> usize {
            let log = std::process::Command::new("git")
                .current_dir(&self.github.bare)
                .args(["rev-list", "--count", repos::CLAIM_BRANCH])
                .output()
                .unwrap();
            String::from_utf8_lossy(&log.stdout)
                .trim()
                .parse()
                .unwrap_or(0)
        }
        /// A teammate's Mac puts `state` at the tip of the claim branch, written at `at`, from a
        /// clone of its own.
        fn teammate(&self, state: ClaimState, at: i64, due: &str) -> String {
            let folder = tempfile::tempdir().unwrap();
            let clone = folder.path().join("teammate");
            crate::test_support::git(
                folder.path(),
                &[
                    "clone",
                    "-q",
                    "--single-branch",
                    self.github.bare.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
            let (url, git) = (self.url(), &self.tools.git);
            let tip = repos::read_tip(&clone, &url, git).unwrap();
            let claim = Claim {
                host: "their-host".into(),
                machine: THEIR_MAC.into(),
                login: "jlin".into(),
                run: brains::new_ulid(now()),
                due: due.into(),
                state,
                at,
            };
            match repos::claim(&clone, &url, tip.as_ref(), &claim, git).unwrap() {
                Claiming::Won(commit) => commit,
                other => panic!("{other:?}"),
            }
        }
        /// The scheduler finds the run due, and the pull it asks for ends.
        fn due(&mut self) {
            let due = jobs::BuiltInStart::Due {
                catch_up: 0,
                due: DUE.into(),
            };
            run_built_in(&mut self.f.host, REPO, due).unwrap();
            self.pull();
        }
        fn pull(&mut self) {
            self.sync();
            self.prepare();
        }
        fn owed(&self) -> Option<Owed> {
            self.f.host.owed(REPO).unwrap()
        }
        fn state(&self) -> BrainCompileState {
            let states = self.f.host.brain_compile_states().unwrap();
            (states.into_iter())
                .find(|state| state.repository == REPO)
                .unwrap()
        }
        fn exhaust_quota(&mut self) {
            let window = QuotaWindow {
                key: "weekly".into(),
                label: "Weekly".into(),
                used_percent: Some(100.0),
                observed_at: now(),
                duration_mins: None,
                resets_at: Some(now() + 60 * MINUTE),
                status: None,
            };
            let reading = QuotaReading {
                provider: Provider::Claude,
                account: None,
                observed_at: now(),
                source: "test".into(),
                windows: vec![window],
                error: None,
            };
            self.f.host.record_quota(reading).unwrap();
        }
    }

    #[test]
    fn a_mac_turns_compiling_on_and_off_and_its_job_follows_the_folders_compile_time() {
        let mut b = stored();
        let (space, key) = (b.sage.id.clone(), compile_key(REPO));
        b.f.host.refresh_compile_jobs().unwrap();
        assert!(b.f.host.built_in(&key).is_none(), "compiling is off");
        let refused = b.f.host.run_brain_compile(REPO).unwrap_err();
        assert!(
            format!("{refused:#}").contains("Compile on this Mac"),
            "{refused:#}"
        );
        b.f.host.set_compile_here(&space, true).unwrap();
        b.sync();
        let file = format!("wiffletree/compilers/{}.yaml", b.f.host.host_id().unwrap());
        assert!(b.github.show(&file).is_some());
        b.f.host.refresh_compile_jobs().unwrap();
        let job = b.f.host.built_in(&key).unwrap();
        assert_eq!(job.schedule, None, "no compile time, only Run now");
        let at = |at: &str, timezone: &str| BrainCompile {
            at: at.into(),
            timezone: timezone.into(),
            extra: Extra::new(),
        };
        // A time the schedule cannot read is refused with nothing written.
        for bad in [at("25:00", "America/Chicago"), at("06:00", "Mars/Olympus")] {
            assert!(b.f.host.set_compile_time(&space, Some(bad)).is_err());
        }
        assert_eq!(b.f.host.tracker(&b.sage).unwrap().compile, None);
        let commits = b.github.log("%s").len();
        (b.f.host
            .set_compile_time(&space, Some(at("06:30", "America/Chicago"))))
        .unwrap();
        b.sync();
        assert_eq!(b.github.log("%s").len(), commits + 1);
        let shown = git_show_names(&b);
        assert_eq!(shown, ["wiffletree/wiffletree-issues.yaml"]);
        b.f.host.refresh_compile_jobs().unwrap();
        let job = b.f.host.built_in(&key).unwrap();
        let schedule = job.schedule.clone().unwrap();
        assert_eq!(schedule.cron, "30 6 * * *");
        // Due at that time, by the scheduler, on this Mac.
        b.f.host.fire_and_check(now()).unwrap();
        let next: i64 = (b.f.host.db)
            .query_row(
                "SELECT next_at FROM job_due WHERE key=?1",
                [key.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        b.f.host.fire_and_check(next + 1).unwrap();
        assert!(b.owed().is_some(), "a due run waits for its pull");
        assert!(b.f.host.issues_written.borrow().contains(&repos::key(REPO)));
        assert_eq!(b.tip(), None, "nothing is claimed before the pull");
        b.pull();
        let claim = b.tip().unwrap();
        assert!(
            claim.due.ends_with("T06:30 America/Chicago"),
            "{}",
            claim.due
        );
        assert_eq!(claim.state, ClaimState::Claimed);
        assert_eq!(b.runs()[0].trigger, CompileTrigger::Scheduled);
        // A restart keeps a due time the schedule still names, so a missed run catches up.
        let kept = |b: &Stored| -> Option<i64> {
            (b.f.host.db)
                .query_row(
                    "SELECT next_at FROM job_due WHERE key=?1",
                    [key.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .unwrap()
        };
        let stored = kept(&b).unwrap();
        b.f.host.built_ins.clear();
        b.f.host.refresh_compile_jobs().unwrap();
        assert_eq!(kept(&b), Some(stored));
        // A changed time forgets the old due time; compiling off drops the job.
        (b.f.host
            .set_compile_time(&space, Some(at("02:00", "America/Chicago"))))
        .unwrap();
        b.f.host.refresh_compile_jobs().unwrap();
        assert_eq!(kept(&b), None);
        b.f.host.set_compile_here(&space, false).unwrap();
        b.f.host.refresh_compile_jobs().unwrap();
        assert!(b.f.host.built_in(&key).is_none());
        for command in [
            Command::SetCompileHere {
                space_id: space.clone(),
                compiles: true,
            },
            Command::SetCompileTime {
                space_id: space,
                compile: None,
            },
        ] {
            assert!(command.is_human_only());
        }
    }

    fn git_show_names(b: &Stored) -> Vec<String> {
        let shown = crate::test_support::git(
            &b.github.bare,
            &["show", "--format=", "--name-only", "main"],
        );
        shown.lines().map(str::to_owned).collect()
    }

    #[test]
    fn nothing_pending_pushes_nothing_not_even_a_claim_and_makes_no_model_call() {
        let mut b = brained();
        b.with_compiler_row();
        let (sessions, pushes) = (b.f.host.sessions().unwrap().len(), b.github.pushes());
        b.due();
        let run = b.runs().remove(0);
        assert_eq!((run.outcome, run.model_calls), (CompileOutcome::Skipped, 0));
        assert_eq!(b.github.pushes(), pushes);
        assert_eq!(b.tip(), None);
        assert_eq!(b.f.host.sessions().unwrap().len(), sessions);
        assert!(b.owed().is_none());
    }

    #[test]
    fn a_tip_landed_for_this_due_time_is_recorded_as_compiled_on_that_mac_whatever_is_pending() {
        for pending in [false, true] {
            let mut b = brained();
            b.with_compiler_row();
            if pending {
                b.merge(&[relay(1)]);
            }
            b.teammate(ClaimState::Landed, now(), DUE);
            let (sessions, claims) = (b.f.host.sessions().unwrap().len(), b.claims());
            b.due();
            let run = b.runs().remove(0);
            assert_eq!(run.outcome, CompileOutcome::CompiledElsewhere);
            assert_eq!(run.reason, compiled_elsewhere(Some(THEIR_MAC)));
            assert_eq!((run.model_calls, run.tokens), (0, 0));
            assert_eq!(b.claims(), claims, "it claims nothing");
            assert_eq!(b.f.host.sessions().unwrap().len(), sessions);
            assert!(b.owed().is_none());
        }
    }

    #[test]
    fn with_no_row_or_out_of_quota_a_mac_waits_says_why_and_claims_nothing() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.due();
        let run = b.runs().remove(0);
        assert_eq!(run.outcome, CompileOutcome::Waiting);
        assert!(run.reason.contains("Brain compiler"), "{}", run.reason);
        assert_eq!(b.tip(), None);
        assert!(b.owed().is_none());
        b.with_compiler_row();
        b.exhaust_quota();
        b.due();
        let run = b.runs().remove(0);
        assert_eq!((run.outcome, run.model_calls), (CompileOutcome::Waiting, 0));
        assert_eq!(b.tip(), None, "the next volunteer takes the run");
    }

    #[test]
    fn a_run_that_claims_lands_and_pushes_landed_as_a_child_of_its_claim() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        b.due();
        let (run, session, work) = {
            let run = b.runs().remove(0);
            assert_eq!(run.outcome, CompileOutcome::Running, "{}", run.reason);
            let id: String = (b.f.host.db)
                .query_row(
                    "SELECT session_id FROM compile_runs WHERE id=?1",
                    [&run.id],
                    |r| r.get(0),
                )
                .unwrap();
            let work = b.f.host.compile_work(&run.id).unwrap();
            (run, b.f.host.session(&id).unwrap(), work)
        };
        let claimed = b.tip().unwrap();
        assert_eq!(
            (claimed.run.as_str(), claimed.due.as_str()),
            (run.id.as_str(), DUE)
        );
        assert_eq!(claimed.state, ClaimState::Claimed);
        assert!(b.owed().is_none(), "the winner owes nothing");
        let refused = b.f.host.run_brain_compile(REPO).unwrap_err();
        assert!(format!("{refused:#}").contains("in progress"));
        let brain = work.worktree.join(BRAIN);
        fs::create_dir_all(brain.join("notes")).unwrap();
        fs::write(brain.join("notes/Call Relay.md"), relay_note(&id(1))).unwrap();
        b.tool(&session, "finish_compile", json!({"summary": "Done"}))
            .unwrap();
        b.pull();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Landed);
        let landed = b.tip().unwrap();
        assert_eq!(landed.state, ClaimState::Landed);
        assert_eq!(
            (landed.run.as_str(), landed.due.as_str()),
            (run.id.as_str(), DUE)
        );
        assert_eq!(b.claims(), 2);
    }

    #[test]
    fn a_failed_run_releases_its_claim_for_the_next_mac_and_leaves_the_cursor() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        b.due();
        let session: String = (b.f.host.db)
            .query_row(
                "SELECT session_id FROM compile_runs WHERE id=?1",
                [&b.runs()[0].id],
                |r| r.get(0),
            )
            .unwrap();
        // The session stops as the provider's quota runs out.
        let quota = Some("You have hit your limit");
        b.f.host.compile_turn_ended(&session, None, quota).unwrap();
        b.prepare();
        let run = b.runs().remove(0);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert_eq!(b.tip().unwrap().state, ClaimState::Released);
        assert_eq!(b.cursor_on_main(), Cursor::default());
    }

    #[test]
    fn a_live_claim_leaves_a_mac_owed_until_it_is_released_then_it_runs_as_its_child() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        b.teammate(ClaimState::Claimed, now() - 59 * MINUTE, DUE);
        let sessions = b.f.host.sessions().unwrap().len();
        b.due();
        let lost = b.runs().remove(0);
        assert_eq!(lost.outcome, CompileOutcome::Lost);
        assert_eq!(lost.reason, format!("Lost to {THEIR_MAC}"));
        assert!(b.owed().unwrap().lost_run.is_some());
        assert_eq!(b.f.host.sessions().unwrap().len(), sessions);
        // A pull with the claim still live changes nothing and adds no row.
        b.pull();
        assert_eq!(b.runs().len(), 1);
        // Run now starts no run, and the page says the wiki is compiling on that Mac.
        b.f.host.run_brain_compile(REPO).unwrap();
        b.prepare();
        let refused = b.state().last_run.unwrap();
        assert_eq!(refused.outcome, CompileOutcome::Lost);
        assert_eq!(
            refused.reason,
            format!("Compiling on {THEIR_MAC}, so no run started")
        );
        assert_eq!(b.f.host.sessions().unwrap().len(), sessions);
        assert_eq!(
            b.owed().unwrap().lost_run,
            Some(lost.id.clone()),
            "the due run stands"
        );
        b.teammate(ClaimState::Released, now(), DUE);
        b.pull();
        b.prepare();
        let run = b.runs().remove(0);
        assert_eq!(run.outcome, CompileOutcome::Running, "{}", run.reason);
        assert_eq!(b.tip().unwrap().run, run.id);
        assert_eq!(b.claims(), 3, "as a child of the release");
        assert!(b.owed().is_none());
    }

    #[test]
    fn run_now_starts_as_soon_as_a_run_on_another_mac_has_ended() {
        for ended in [ClaimState::Landed, ClaimState::Released] {
            let mut b = brained();
            b.merge(&[relay(1)]);
            b.with_compiler_row();
            b.teammate(ClaimState::Claimed, now(), DUE);
            b.f.host.run_brain_compile(REPO).unwrap();
            b.prepare();
            assert_eq!(b.runs()[0].outcome, CompileOutcome::Lost);
            b.teammate(ended, now(), DUE);
            b.f.host.run_brain_compile(REPO).unwrap();
            b.prepare();
            let run = b.runs().remove(0);
            assert_eq!(run.outcome, CompileOutcome::Running, "{}", run.reason);
            assert_eq!(b.tip().unwrap().run, run.id);
        }
    }

    #[test]
    fn an_owed_mac_takes_over_a_claim_older_than_an_hour_at_its_next_pull() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        b.teammate(ClaimState::Claimed, now() - 58 * MINUTE, DUE);
        b.due();
        assert_eq!(b.runs()[0].outcome, CompileOutcome::Lost);
        // A crashed Mac's claim, an hour old by the time of this pull.
        let expired = Tip {
            commit: "c".into(),
            claim: Some(Claim {
                host: "h".into(),
                machine: "m".into(),
                login: "l".into(),
                run: "r".into(),
                due: DUE.into(),
                state: ClaimState::Claimed,
                at: now() - 61 * MINUTE,
            }),
        };
        assert!(expired.free(now()));
        b.teammate(ClaimState::Claimed, now() - 61 * MINUTE, DUE);
        b.pull();
        b.prepare();
        let run = b.runs().remove(0);
        assert_eq!(run.outcome, CompileOutcome::Running, "{}", run.reason);
        let tip = b.tip().unwrap();
        assert_eq!(
            (tip.run.as_str(), tip.state),
            (run.id.as_str(), ClaimState::Claimed)
        );
    }

    #[test]
    fn a_lost_claim_is_not_pushed_again_and_its_winner_is_named_at_the_next_pull() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        let teammate_tip = std::sync::Arc::new(std::sync::Mutex::new(None));
        // A teammate's claim lands between this Mac's read of the tip and its push.
        let bare = b.github.bare.clone();
        // The fake's own tool is locked while its hook runs, so this Mac's rival goes straight
        // to the bare repository.
        let rival: publish::GitTool = std::sync::Arc::new(|path: &Path, args: &[&str]| {
            let identity = ["-c", "user.name=T", "-c", "user.email=t@example.invalid"];
            let args = [&identity[..], args].concat();
            match publish::subcommand(&args) {
                "push" => runtime::git_push(path, &args),
                _ => runtime::git_answer(path, &args),
            }
        });
        let folder = tempfile::tempdir().unwrap();
        let clone = folder.path().join("teammate");
        crate::test_support::git(
            folder.path(),
            &[
                "clone",
                "-q",
                "--single-branch",
                bare.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        let (won, bare_url) = (teammate_tip.clone(), bare.clone());
        *b.github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || {
                let claim = Claim {
                    host: "their-host".into(),
                    machine: THEIR_MAC.into(),
                    login: "jlin".into(),
                    run: "theirs".into(),
                    due: DUE.into(),
                    state: ClaimState::Claimed,
                    at: now(),
                };
                let url = bare_url.to_str().unwrap();
                let answer = repos::claim(&clone, url, None, &claim, &rival).unwrap();
                *won.lock().unwrap() = Some(answer);
            }),
        ));
        b.due();
        assert!(matches!(
            teammate_tip.lock().unwrap().as_ref(),
            Some(Claiming::Won(_))
        ));
        let lost = b.runs().remove(0);
        assert_eq!(lost.outcome, CompileOutcome::Lost);
        assert_eq!(lost.reason, "Another Mac claimed this run first");
        // This Mac's one push and the teammate's: nothing goes again.
        assert_eq!(b.claims(), 1);
        assert_eq!(b.tip().unwrap().machine, THEIR_MAC);
        b.pull();
        assert_eq!(
            b.run_named(&lost).reason,
            format!("Lost to {THEIR_MAC}"),
            "the next read names the Mac"
        );
        // That Mac lands: the run is done, with no second row.
        b.teammate(ClaimState::Landed, now(), DUE);
        b.pull();
        assert!(b.owed().is_none());
        assert_eq!(b.runs().len(), 1);
    }

    #[test]
    fn a_login_that_can_read_but_not_push_ends_the_attempt_with_its_reason_and_stops_retrying() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        let inner = b.tools.git.clone();
        b.tools.git = Arc::new(move |path: &Path, args: &[&str]| {
            if publish::subcommand(args) != "push" {
                return inner(path, args);
            }
            Ok(runtime::GitAnswer {
                status: crate::publish::tests::exit(128),
                output: String::new(),
                error: "fatal: unable to access: The requested URL returned error: 403".into(),
            })
        });
        b.due();
        b.prepare();
        let runs = b.runs();
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0].outcome, CompileOutcome::Failed);
        assert!(runs[0].reason.contains("403"), "{}", runs[0].reason);
        assert!(b.owed().is_none(), "this due time is over");
        b.pull();
        b.prepare();
        assert_eq!(b.runs().len(), 1, "no row for each pull");
    }

    #[test]
    fn an_owed_mac_stops_when_nothing_is_pending_any_more() {
        let mut b = brained();
        let head = b.merge(&[relay(1)]);
        b.with_compiler_row();
        b.teammate(ClaimState::Claimed, now(), DUE);
        b.due();
        assert!(b.owed().is_some());
        let index = write_cursor(
            "",
            &Cursor {
                through: Some(head),
                after: vec![],
            },
        );
        b.merge(&[("notes/INDEX.md".into(), index)]);
        b.pull();
        assert!(b.owed().is_none());
    }

    #[test]
    fn offline_at_the_due_time_a_mac_claims_nothing_and_stays_due() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        let due = jobs::BuiltInStart::Due {
            catch_up: 1,
            due: DUE.into(),
        };
        run_built_in(&mut b.f.host, REPO, due).unwrap();
        b.github
            .offline
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(b.sync_answering().is_err());
        b.prepare();
        assert!(b.runs().is_empty());
        assert!(b.owed().is_some());
        b.github
            .offline
            .store(false, std::sync::atomic::Ordering::Relaxed);
        b.pull();
        let run = b.runs().remove(0);
        assert_eq!(
            (run.outcome, run.trigger),
            (CompileOutcome::Running, CompileTrigger::CatchUp),
            "{}",
            run.reason
        );
    }

    #[test]
    fn a_mac_that_slept_through_its_claim_lands_nothing_and_pushes_nothing() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        let (run, _) = b.finished(&relay_note(&id(1)));
        // A takeover while this Mac slept.
        b.teammate(ClaimState::Claimed, now(), DUE);
        let (commits, claims) = (b.github.log("%s").len(), b.claims());
        b.pull();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Lost);
        assert_eq!(run.commit, None);
        assert_eq!((b.github.log("%s").len(), b.claims()), (commits, claims));
        assert_eq!(b.cursor_on_main(), Cursor::default());
    }

    #[test]
    fn a_run_compiled_elsewhere_names_the_mac_and_releases_its_claim() {
        let mut b = brained();
        let head = b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        let index = format!("{FOLDER}/second-brain/notes/INDEX.md");
        let cursor = Cursor {
            through: Some(head),
            after: vec![],
        };
        b.github.commit_files(
            &[(index, write_cursor("", &cursor))],
            &format!("Compiled 1 entries · {THEIR_MAC}"),
        );
        b.pull();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::CompiledElsewhere);
        assert!(run.reason.contains(THEIR_MAC), "{}", run.reason);
        assert_eq!(b.tip().unwrap().state, ClaimState::Released);
    }

    #[test]
    fn a_claim_github_refuses_fails_the_run_with_its_reason_and_stops_there() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        b.with_compiler_row();
        let hook = refusing_pushes(&b);
        b.due();
        fs::remove_file(hook).unwrap();
        let run = b.runs().remove(0);
        assert_eq!(run.outcome, CompileOutcome::Failed);
        assert!(
            run.reason.contains("main needs a pull request"),
            "{}",
            run.reason
        );
        assert!(b.owed().is_none());
    }

    #[test]
    fn a_wiki_says_when_it_is_falling_behind() {
        let mut b = stored();
        let old = entry_file("app", &id(1), ("mhohlios", "observation", "Old"), "—");
        b.merge(&[old]);
        let behind = |b: &Stored| b.state().behind;
        assert_eq!(
            behind(&b).as_deref(),
            Some("No Mac compiles Sage's wiki. 1 entry is waiting.")
        );
        let space = b.sage.id.clone();
        b.f.host.set_compile_here(&space, true).unwrap();
        b.sync();
        assert_eq!(behind(&b), None, "no compile time, and a volunteer");
        let compile = BrainCompile {
            at: "06:00".into(),
            timezone: "America/Chicago".into(),
            extra: Extra::new(),
        };
        b.f.host.set_compile_time(&space, Some(compile)).unwrap();
        let shown = behind(&b).unwrap();
        assert!(
            shown.contains("is falling behind: 1 entry is waiting"),
            "{shown}"
        );
        // An entry logged now has seen no scheduled time.
        let mut fresh = brained();
        let space = fresh.sage.id.clone();
        let compile = BrainCompile {
            at: "06:00".into(),
            timezone: "America/Chicago".into(),
            extra: Extra::new(),
        };
        fresh
            .f
            .host
            .set_compile_time(&space, Some(compile))
            .unwrap();
        let new = brains::new_ulid(now());
        fresh.merge(&[entry_file(
            "app",
            &new,
            ("mhohlios", "observation", "New"),
            "—",
        )]);
        assert_eq!(behind(&fresh), None);
    }

    #[test]
    fn entries_fold_by_repos_and_a_changed_note_keeps_its_repos_and_its_keys() {
        let mut b = brained();
        let pica = "---\ncreated: 2026-01-02\nrepos: [pica-ios, pica-api]\nsources: [vyttle-overview.md]\ntags: [mobile]\n---\nPica ships weekly.\n";
        let entry = entry_file(
            "pica-api",
            &id(1),
            ("mhohlios", "observation", "Relay"),
            "—",
        );
        let elsewhere = "---\ntitle: Reviso\nkind: observation\nupdated: 2026-10-01\nsources: []\nrepos: [reviso-api]\n---\nReviso.\n";
        b.merge(&[
            entry,
            ("notes/Pica.md".into(), pica.into()),
            ("notes/Reviso.md".into(), elsewhere.into()),
        ]);
        let gathered = b.gather();
        let named: Vec<&str> = gathered
            .notes
            .iter()
            .map(|(path, _)| path.as_str())
            .collect();
        assert_eq!(
            named,
            ["notes/Pica.md"],
            "the page whose repos list its folder"
        );
        // A note that drops a name fails the run, naming the note and the repository.
        let (run, session, work) = b.started();
        let page = work.worktree.join(BRAIN).join("notes/Pica.md");
        let id_one = id(1);
        let rewritten = |repos: &str| {
            format!(
                "---\ntitle: Pica\nkind: observation\nupdated: 2026-11-01\ncreated: 2026-01-02\nrepos: [{repos}]\nsources: [vyttle-overview.md, {id_one}]\ntags: [mobile]\n---\nPica ships weekly.\n"
            )
        };
        fs::write(&page, rewritten("pica-ios")).unwrap();
        let failed = b.tool(&session, "finish_compile", json!({"summary": "Done"}));
        let failed = format!("{:#}", failed.unwrap_err());
        assert!(
            failed.contains("notes/Pica.md") && failed.contains("pica-api"),
            "{failed}"
        );
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Failed);
        // One that keeps them gains `title` and `kind`, and a path among its sources stays valid.
        let (run, session, work) = b.started();
        let page = work.worktree.join(BRAIN).join("notes/Pica.md");
        fs::write(&page, rewritten("pica-ios, pica-api")).unwrap();
        b.tool(&session, "finish_compile", json!({"summary": "Done"}))
            .unwrap();
        b.pull();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Landed);
        let landed = b.github.show(&format!("{BRAIN}/notes/Pica.md")).unwrap();
        for kept in [
            "title: Pica",
            "kind: observation",
            "created: 2026-01-02",
            "tags: [mobile]",
        ] {
            assert!(landed.contains(kept), "{kept}");
        }
    }

    #[test]
    fn a_changed_note_still_needs_title_and_kind_and_an_entry_id_must_name_an_entry() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, session, work) = b.started();
        let page = work.worktree.join(BRAIN).join("notes/Old.md");
        fs::create_dir_all(page.parent().unwrap()).unwrap();
        let missing = id(99);
        fs::write(
            &page,
            format!("---\ntitle: Old\nkind: observation\nupdated: 2026-11-01\nsources: [{missing}]\n---\nOld.\n"),
        )
        .unwrap();
        let failed = b.tool(&session, "finish_compile", json!({"summary": "Done"}));
        assert!(format!("{:#}", failed.unwrap_err()).contains("which is no entry file"));
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Failed);
    }

    #[test]
    fn a_compile_time_the_schedule_cannot_read_stops_only_that_wikis_schedule() {
        let mut b = brained();
        let tracker = format!("{FOLDER}/wiffletree-issues.yaml");
        let edited =
            "format: 1\nprefix: SAGE\ncompile:\n  at: \"6:00\"\n  timezone: America/Chicago\n";
        b.github.commit(&tracker, edited);
        b.merge(&[relay(1)]);
        assert_eq!(b.state().behind, None);
        assert_eq!(b.state().pending, 1);
        b.f.host.brains_page().unwrap();
        let space = b.sage.id.clone();
        b.f.host.storage_status(&space).unwrap();
        b.f.host.refresh_compile_jobs().unwrap();
        let job = b.f.host.built_in(&compile_key(REPO)).unwrap();
        assert_eq!(job.schedule, None, "only Run now until the file is fixed");
    }

    #[test]
    fn only_a_whole_ulid_is_an_entry_id_and_a_path_shaped_source_is_left_alone() {
        assert!(is_entry_id(&id(1)));
        let path = "strategymemo-2026-06-01.md";
        assert_eq!(path.len(), 26);
        for source in [path, "notes/pri\u{e9}ces-du-marche.md"] {
            assert!(!is_entry_id(source), "{source}");
            assert_eq!(brains::ulid_time(source), None);
        }
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, session, work) = b.started();
        let page = work.worktree.join(BRAIN).join("notes/Old.md");
        fs::create_dir_all(page.parent().unwrap()).unwrap();
        fs::write(
            &page,
            format!("---\ntitle: Old\nkind: observation\nupdated: 2026-11-01\nsources: [{path}, {}]\n---\nOld.\n", id(1)),
        )
        .unwrap();
        b.tool(&session, "finish_compile", json!({"summary": "Done"}))
            .unwrap();
        b.pull();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Landed);
    }

    #[test]
    fn a_finished_run_keeps_its_output_through_a_restart_and_lands_at_the_next_pass() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        b.f.host.end_interrupted_compiles().unwrap();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Running);
        b.pull();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Landed);
        assert_eq!(run.commit.as_deref(), Some(&*b.github.log("%H")[0]));
        assert_eq!(b.tip().unwrap().state, ClaimState::Landed);
    }

    #[test]
    fn a_finished_run_whose_output_is_gone_fails_at_a_restart_and_releases_its_claim() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, work) = b.finished(&relay_note(&id(1)));
        fs::remove_dir_all(&work.worktree).unwrap();
        b.f.host.end_interrupted_compiles().unwrap();
        b.prepare();
        assert_eq!(b.run_named(&run).outcome, CompileOutcome::Failed);
        assert_eq!(b.tip().unwrap().state, ClaimState::Released);
    }

    #[test]
    fn a_landing_whose_push_landed_with_its_answer_lost_reads_as_landed_with_its_commit() {
        let mut b = brained();
        b.merge(&[relay(1)]);
        let (run, _) = b.finished(&relay_note(&id(1)));
        let real = b.tools.git.clone();
        let answerless = real.clone();
        b.tools.git = std::sync::Arc::new(move |path: &Path, args: &[&str]| {
            let answer = answerless(path, args)?;
            Ok(match publish::subcommand(args) {
                "push" => runtime::GitAnswer {
                    status: publish::tests::exit(128),
                    output: String::new(),
                    error: "fatal: the remote end hung up unexpectedly".into(),
                },
                _ => answer,
            })
        });
        let _ = b.sync_answering();
        b.tools.git = real;
        b.prepare();
        let run = b.run_named(&run);
        assert_eq!(run.outcome, CompileOutcome::Landed, "{}", run.reason);
        let index = format!("{BRAIN}/notes/INDEX.md");
        let last = ["log", "-1", "--format=%H", "main", "--", index.as_str()];
        let landed = crate::test_support::git(&b.github.bare, &last);
        assert_eq!(run.commit.as_deref(), Some(landed.trim()));
    }

    #[test]
    fn a_local_run_that_finished_lands_at_a_restart() {
        let mut f = publish::tests::fixture();
        let home = f.host.create_space("Home", "", "", None).unwrap().space;
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &home.id))
        .unwrap();
        let repository = home.storage.repository.key();
        let clone = f.host.storage_folder(&home.storage.repository);
        let (path, text) = relay(1);
        let file = clone.join(BRAIN).join(&path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        storage::commit_local(&clone, &[&format!("{BRAIN}/{path}")], "An entry").unwrap();
        // A run the host stopped between its finish and its commit: its files are in a worktree.
        let run = (f
            .host
            .start_compile(&repository, CompileTrigger::RunNow, None))
        .unwrap()
        .unwrap();
        f.host.take_compile_preparations();
        let worktree = tempfile::tempdir().unwrap();
        let index = format!("{BRAIN}/notes/INDEX.md");
        fs::create_dir_all(worktree.path().join(BRAIN).join("notes")).unwrap();
        let cursor = Cursor {
            through: Some(
                runtime::git_output(&clone, &["rev-parse", "HEAD"])
                    .unwrap()
                    .trim()
                    .to_owned(),
            ),
            after: vec![],
        };
        fs::write(worktree.path().join(&index), write_cursor("", &cursor)).unwrap();
        let landing = Landing {
            run: run.id.clone(),
            worktree: worktree.path().to_owned(),
            head: cursor.through.clone().unwrap(),
            cursor: cursor.clone(),
            files: vec![index.clone()],
            index: index.clone(),
            entries: 1,
            claim: None,
        };
        let work = Work {
            head: landing.head.clone(),
            brain_dir: BRAIN.into(),
            worktree: worktree.path().to_owned(),
            entries: vec![],
            verification: vec![],
            cursor,
            landing: Some(landing),
        };
        (f.host.db)
            .execute(
                "UPDATE compile_runs SET work=?2 WHERE id=?1",
                params![run.id, serde_json::to_string(&work).unwrap()],
            )
            .unwrap();
        f.host.end_interrupted_compiles().unwrap();
        let run = f.host.compile_run(&run.id).unwrap();
        assert_eq!(run.outcome, CompileOutcome::Landed, "{}", run.reason);
        assert!(clone.join(&index).is_file());
    }

    #[test]
    fn the_compile_procedure_is_v2_and_tells_the_model_how_to_keep_a_wiki_by_product() {
        assert_eq!(PROCEDURE_VERSION, 2);
        assert!(PROCEDURE.starts_with("# Brain compile procedure — v2"));
        for rule in [
            "`repos:`",
            "Keep every frontmatter key",
            "VYT-42",
            "plain text",
        ] {
            assert!(PROCEDURE.contains(rule), "{rule}");
        }
        assert!(!PROCEDURE.contains("compile PR"));
    }
}
