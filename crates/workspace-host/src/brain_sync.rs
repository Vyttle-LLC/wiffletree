//! The logs and wiki rule sets of a repository's sync engine (OpenSpec change
//! `one-space-repository`, decision 2): a Space's brain lands straight on `main`. A log entry is
//! written into the clone and pushed by the next pass, added and never compared, as one Mac
//! writes it once. An entry a scan fails stays here, in a state that survives restarts, until the
//! human edits, marks or discards it. A compile's run hands its exact files to the wiki rule set,
//! which stages them on the current head, all or nothing, with no model involved. Git brings the
//! other Macs' files down, as it does a file no rule set claims. Nothing here keeps an entry's
//! text or what a check matched.
use crate::*;
use compile::{Landing, at_or_past, file_at, read_cursor};
use folder::Root;
use repos::{Content, RuleSet};
use storage::Input;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entry_files (repository TEXT NOT NULL, path TEXT NOT NULL, log_id TEXT, project_id TEXT, state TEXT NOT NULL, check_name TEXT, rule TEXT, line INTEGER, reason TEXT, PRIMARY KEY(repository,path));
";

/// An entry waiting for a pass to push it.
const WAITING: &str = "waiting";
/// An entry a scan failed, kept here for the human.
const HELD: &str = "held";
/// An entry the human discarded, which never leaves this Mac and is never written again.
const DISCARDED: &str = "discarded";

/// A reason the human gives for Not sensitive, as long as the marker's line can carry.
pub(crate) const MAX_REASON_CHARS: usize = 200;
/// The most files one commit of the logs rule set holds, so a Bring in of hundreds of files
/// lands in commits a reviewer can read; the rest wait for the next pass.
pub(crate) const MAX_FILES_PER_COMMIT: usize = 200;

/// Adds the table of unlanded entries, and the line a held file's card names.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(SCHEMA)?;
    let has_line: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sync_held') WHERE name='line')",
        [],
        |r| r.get(0),
    )?;
    if !has_line {
        db.execute_batch("ALTER TABLE sync_held ADD COLUMN line INTEGER;")?;
    }
    Ok(())
}

/// The brain folder, as a path of the repository.
pub(crate) fn brain_dir(root: &Root) -> String {
    root.join(SpaceBrain::DEFAULT_FOLDER)
}

/// Whether `path` of the repository is in a brain, so its record stays out of sync's: a brain
/// grows without end, and the rule sets that hold its files read no record of them.
pub(crate) fn in_brain(root: &Root, path: &str) -> bool {
    path.starts_with(&format!("{}/", brain_dir(root)))
}

/// Refuses a path in `clone` that passes through a symbolic link, which a repository may hold
/// pointing outside it, so the host never writes through one.
pub(crate) fn refuse_links(clone: &Path, path: &str) -> Result<()> {
    let mut at = clone.to_path_buf();
    for component in Path::new(path).components() {
        at.push(component);
        ensure!(
            !fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()),
            "{path} goes through a symbolic link, which Wiffletree does not write"
        );
    }
    Ok(())
}

/// What the logs rule set found in a pass.
#[derive(Default)]
pub(crate) struct Entries {
    /// The rule set compared, so the rows below say all there is to say.
    compared: bool,
    /// Those it committed.
    committed: Vec<String>,
    /// Those GitHub already holds, as a push whose answer was lost landed them.
    landed: Vec<String>,
    /// Those no longer in the clone, which someone removed by hand.
    gone: Vec<String>,
    /// Those a scan held.
    held: Vec<scan::Held>,
    /// Every path the pass was given.
    given: Vec<String>,
    /// Files left for the next pass, past the commit's cap.
    more: bool,
    /// The brought-in ids it put in `compiled-after`, or found there already.
    compiled: Vec<String>,
}

/// `<folder>/second-brain/logs/`: entries this Mac wrote are pushed without comparing, in
/// commits that only add files, as are the pages a Bring in wrote. The engine gets the other
/// Macs' from Git. The ids of brought-in entries the original had compiled are added to
/// `notes/INDEX.md`'s `compiled-after` on the head it lands on, with its cursor kept.
pub(crate) struct LogRules {
    logs: String,
    index: String,
    /// The files waiting or held, by their paths in the repository. A held one is scanned
    /// again each pass, as a held issue file is, so a check that could not run catches up.
    entries: Vec<String>,
    /// The brought-in ids for `compiled-after`, left for a pass with no compile landing.
    compiled: Vec<String>,
    retries_alone: bool,
    files: Vec<(String, Content)>,
    found: Entries,
}
impl LogRules {
    pub(crate) fn new(input: &Input, retries_alone: bool) -> Self {
        let brain = brain_dir(&input.root);
        let logs = format!("{brain}/logs");
        // A compile landing in this pass owns the wiki's paths, so the brought-in pages and the
        // cursor wait for a pass without one.
        let (entries, compiled) = match input.landing {
            Some(_) => {
                let in_logs = |path: &&String| path.starts_with(&format!("{logs}/"));
                (
                    input.entries.iter().filter(in_logs).cloned().collect(),
                    vec![],
                )
            }
            None => (input.entries.clone(), input.compiled.clone()),
        };
        Self {
            logs,
            index: format!("{brain}/{}", compile::INDEX),
            found: Entries {
                given: entries.clone(),
                ..Entries::default()
            },
            entries,
            compiled,
            retries_alone,
            files: vec![],
        }
    }
    /// `compiled-after` on `remote` with the brought-in ids added: from GitHub's INDEX, else the
    /// one Bring in copied, else a new one. `None` when it already holds them all.
    fn index_with_compiled(&mut self, clone: &Path, remote: &str) -> Option<Vec<u8>> {
        let on_main = file_at(clone, remote, &self.index);
        let brought = (self.files.iter().position(|(path, _)| *path == self.index))
            .map(|at| self.files.remove(at).1.unwrap_or_default());
        let base = match (on_main.is_empty(), &brought) {
            (false, _) => Some(on_main.clone()),
            (true, Some(brought)) => Some(String::from_utf8_lossy(brought).into_owned()),
            (true, None) => None,
        };
        let text = legacy_logs::add_compiled(base.as_deref(), &self.compiled);
        self.found.compiled = self.compiled.clone();
        (text != on_main).then(|| text.into_bytes())
    }
    /// What it found, with the files `held` back from the push.
    pub(crate) fn found(self, held: Vec<scan::Held>) -> Entries {
        let is_held = |path: &String| held.iter().any(|h| &h.path == path);
        let committed = (self.files.iter()).map(|(path, _)| path);
        let committed = committed.filter(|path| !is_held(path)).cloned().collect();
        Entries {
            committed,
            held,
            ..self.found
        }
    }
}
impl RuleSet for LogRules {
    fn claims(&self, path: &str) -> bool {
        (path.strip_prefix(self.logs.as_str())).is_some_and(|rest| rest.starts_with('/'))
            || self.entries.iter().any(|entry| entry == path)
            || (!self.compiled.is_empty() && path == self.index)
    }
    fn pulled_by_git(&self) -> bool {
        true
    }
    fn compare(&mut self, clone: &Path, remote: &str) -> Result<()> {
        self.found.compared = true;
        if self.entries.is_empty() && self.compiled.is_empty() {
            return Ok(());
        }
        let wanted = |path: &str| self.entries.iter().any(|entry| entry == path);
        let there = repos::files_at(clone, remote, &wanted)?;
        for path in &self.entries {
            if there.contains_key(path) {
                self.found.landed.push(path.clone());
                continue;
            }
            match fs::read(clone.join(path)) {
                Ok(content) => self.files.push((path.clone(), Some(content))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.found.gone.push(path.clone());
                }
                Err(error) => return Err(error.into()),
            }
        }
        if !self.compiled.is_empty()
            && let Some(index) = self.index_with_compiled(clone, remote)
        {
            self.files.push((self.index.clone(), Some(index)));
        }
        if self.files.len() > MAX_FILES_PER_COMMIT {
            // The cursor goes with the first commit, as an id there before its entry is harmless.
            let index = (self.files.iter().position(|(path, _)| *path == self.index))
                .map(|at| self.files.remove(at));
            self.files
                .truncate(MAX_FILES_PER_COMMIT - usize::from(index.is_some()));
            self.files.extend(index);
            self.found.more = true;
        }
        Ok(())
    }
    fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)> {
        let entry_file = |path: &String| path.starts_with(&format!("{}/", self.logs));
        let only_entries = self.files.iter().all(|(path, _)| entry_file(path));
        (!self.files.is_empty()).then(|| {
            let message = match (only_entries, self.files.len()) {
                (true, 1) => "Logged 1 entry".to_owned(),
                (true, count) => format!("Logged {count} entries"),
                (false, count) => format!("Brought in {count} files"),
            };
            let message = format!("{message} · {}", sync::machine_name());
            (self.files.clone(), message)
        })
    }
    fn failed(&mut self, error: anyhow::Error) {
        eprintln!("Log entries: {error:#}");
        self.found.compared = false;
    }
    fn reset(&mut self) {
        self.files.clear();
        self.found = Entries {
            given: std::mem::take(&mut self.found.given),
            ..Entries::default()
        };
    }
    fn sit_out(&mut self) {
        self.files.clear();
    }
    fn pushes_again(&self) -> bool {
        self.retries_alone
    }
}

/// How a pass left a landing, for the host thread to end its run.
pub(crate) enum Landed {
    /// Its commit is made; it landed when the push went through.
    Committed,
    /// `main` already holds the run's own files, as a push whose answer was lost landed them, in
    /// this commit.
    AlreadyOnMain(String),
    /// `main` holds a cursor at or past the run's, which that Mac's commit wrote.
    CompiledElsewhere(Option<String>),
    /// Another Mac took the run's claim over: that Mac, when its claim could be read.
    Outrun(Option<String>),
    /// A file failed the scan, which ends the run with nothing landed.
    Refused(scan::Held),
    Failed(String),
}

/// What a landing's next step is on the head `remote`.
pub(crate) enum Verdict {
    Land(Vec<(String, Vec<u8>)>),
    /// `main` already holds the run's own files, in this commit.
    AlreadyOnMain(String),
    /// `main` already holds a cursor at or past the run's, which that Mac's commit wrote.
    CompiledElsewhere(Option<String>),
}

impl Landing {
    /// Reads this run against `remote`, the head it would land on: its own files there mean it
    /// landed, a cursor at or past its own lands nothing, a wiki file changed there since the
    /// run's commit fails it, as does a path through a symbolic link in the clone, and otherwise
    /// its exact files land.
    pub(crate) fn verdict(&self, clone: &Path, remote: &str) -> Result<Verdict> {
        let main = read_cursor(&file_at(clone, remote, &self.index));
        if at_or_past(clone, &main, &self.cursor) {
            if self.holds_its_files(clone, remote) {
                let last = ["log", "-1", "--format=%H", remote, "--", &self.index];
                let commit = runtime::git_output(clone, &last)?;
                return Ok(Verdict::AlreadyOnMain(commit.trim().to_owned()));
            }
            return Ok(Verdict::CompiledElsewhere(cursor_machine(
                clone,
                remote,
                &self.index,
            )));
        }
        let moved = self.changed_meanwhile(clone, remote)?;
        ensure!(
            moved.is_empty(),
            "main changed {} while the run worked, so nothing landed; the next run compiles again",
            moved.join(", ")
        );
        for path in &self.files {
            refuse_links(clone, path)?;
        }
        let read = |path: &String| -> Result<(String, Vec<u8>)> {
            Ok((path.clone(), fs::read(self.worktree.join(path))?))
        };
        Ok(Verdict::Land(
            self.files.iter().map(read).collect::<Result<_>>()?,
        ))
    }
    /// Whether `remote` holds every one of the run's files as the run wrote it.
    fn holds_its_files(&self, clone: &Path, remote: &str) -> bool {
        self.files.iter().all(|path| {
            let there = format!("{remote}:{path}");
            let there = runtime::git_bytes(clone, &["cat-file", "blob", &there], b"");
            let here = fs::read(self.worktree.join(path));
            matches!((there, here), (Ok(there), Ok(here)) if there == here)
        })
    }
    /// The run's files `main` changed since its commit, which it may not overwrite with what it
    /// compiled from there.
    fn changed_meanwhile(&self, clone: &Path, remote: &str) -> Result<Vec<String>> {
        if remote == self.head {
            return Ok(vec![]);
        }
        let mut args = vec![
            "diff",
            "--name-only",
            "--no-renames",
            self.head.as_str(),
            remote,
            "--",
        ];
        args.extend(self.files.iter().map(String::as_str));
        let changed = runtime::git_output(clone, &args)?;
        Ok(changed.lines().map(str::to_owned).collect())
    }
}

/// The Mac the last commit to change `index` by `remote` names, as a landing's message does.
fn cursor_machine(clone: &Path, remote: &str, index: &str) -> Option<String> {
    let last = ["log", "-1", "--format=%s", remote, "--", index];
    let subject = runtime::git_output(clone, &last).ok()?;
    let (_, machine) = subject.trim().rsplit_once(" · ")?;
    Some(machine.to_owned())
}

/// `<folder>/second-brain/notes/`, `STATUS.md` and `reconcile-queue.md`: a run's files land
/// together or not at all. The engine gets the other Macs' from Git.
pub(crate) struct WikiRules {
    brain: String,
    landing: Option<Landing>,
    /// Another Mac took the run over, and that Mac when known.
    outrun: Option<Option<String>>,
    retries_alone: bool,
    files: Vec<(String, Content)>,
    ended: Option<Landed>,
}
impl WikiRules {
    pub(crate) fn new(input: &Input, retries_alone: bool) -> Self {
        Self {
            brain: brain_dir(&input.root),
            landing: input.landing.clone(),
            outrun: input.outrun.then(|| input.outrun_by.clone()),
            retries_alone,
            files: vec![],
            ended: None,
        }
    }
    /// How the pass left its landing, with the files `held` back from the push, which end it.
    pub(crate) fn found(self, held: Vec<scan::Held>) -> Option<(Landing, Landed)> {
        let landing = self.landing?;
        let refused = (held
            .iter()
            .find(|h| h.rule.is_some() || h.path != landing.index))
        .or(held.first())
        .cloned();
        let ended = match refused {
            Some(refused) => Some(Landed::Refused(refused)),
            None if !self.files.is_empty() => Some(Landed::Committed),
            None => self.ended,
        };
        Some((landing, ended?))
    }
}
impl RuleSet for WikiRules {
    fn claims(&self, path: &str) -> bool {
        let Some(rest) = path.strip_prefix(&format!("{}/", self.brain)) else {
            return false;
        };
        rest.starts_with(compile::NOTES) || rest == compile::STATUS || rest == compile::QUEUE
    }
    fn pulled_by_git(&self) -> bool {
        true
    }
    fn compare(&mut self, clone: &Path, remote: &str) -> Result<()> {
        let Some(landing) = &self.landing else {
            return Ok(());
        };
        if let Some(by) = &self.outrun {
            self.ended = Some(Landed::Outrun(by.clone()));
            return Ok(());
        }
        ensure!(
            landing.files.iter().all(|path| self.claims(path)),
            "The repository moved while the run worked, so nothing landed; the next run compiles again"
        );
        match landing.verdict(clone, remote)? {
            Verdict::AlreadyOnMain(commit) => self.ended = Some(Landed::AlreadyOnMain(commit)),
            Verdict::CompiledElsewhere(machine) => {
                self.ended = Some(Landed::CompiledElsewhere(machine));
            }
            Verdict::Land(files) => {
                self.files = (files.into_iter())
                    .map(|(path, content)| (path, Some(content)))
                    .collect();
            }
        }
        Ok(())
    }
    fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)> {
        let landing = self.landing.as_ref()?;
        (!self.files.is_empty()).then(|| {
            let message = format!(
                "Compiled {} entries · {}",
                landing.entries,
                sync::machine_name()
            );
            (self.files.clone(), message)
        })
    }
    fn failed(&mut self, error: anyhow::Error) {
        self.ended = Some(Landed::Failed(format!("{error:#}")));
    }
    /// All or nothing: a file that fails holds every other file of the run back from this push.
    fn also_held(&self, held: &[scan::Held]) -> Vec<scan::Held> {
        let Some(first) = held.first() else {
            return vec![];
        };
        (self.files.iter())
            .filter(|(path, _)| !held.iter().any(|h| &h.path == path))
            .map(|(path, _)| scan::Held {
                path: path.clone(),
                rule: None,
                line: None,
                reason: Some("held with the run's other files".into()),
                check: first.check.clone(),
            })
            .collect()
    }
    fn reset(&mut self) {
        self.files.clear();
        self.ended = None;
    }
    fn sit_out(&mut self) {
        self.files.clear();
    }
    fn pushes_again(&self) -> bool {
        self.retries_alone
    }
}

impl Host {
    /// Writes a log entry's file into the repository of `space`'s clone and hands it to the
    /// engine: committed at once in a local repository, and in a GitHub one pushed by the pass
    /// due two seconds after the last entry.
    pub(crate) fn land_entry(
        &mut self,
        space: &Space,
        path: &str,
        text: &str,
        log: Option<(&str, Option<&str>)>,
    ) -> Result<()> {
        let clone = self.storage_folder(&space.storage.repository);
        refuse_links(&clone, path)?;
        shared::write_atomically(&clone.join(path), text.as_bytes())?;
        let Some(key) = Host::storage_key(&space.storage.repository) else {
            storage::commit_local(&clone, &[path], &format!("Logged {path}"))?;
            self.brains_now = true;
            return Ok(());
        };
        let (log_id, project) = log.unzip();
        self.db.execute(
            "INSERT OR REPLACE INTO entry_files(repository,path,log_id,project_id,state) VALUES (?1,?2,?3,?4,?5)",
            params![key, path, log_id, project.flatten(), WAITING],
        )?;
        self.issues_written.borrow_mut().insert(key);
        Ok(())
    }
    /// Hands a file Bring in wrote into the clone of the repository `key` to the next pass, as
    /// an entry is.
    pub(crate) fn wait_to_land(&self, key: &str, path: &str) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO entry_files(repository,path,state) VALUES (?1,?2,?3)",
            params![key, path, WAITING],
        )?;
        Ok(())
    }
    /// Refuses removing the last Space of a repository while entries there wait to land or are
    /// held, as nothing would push or settle them afterwards, naming them.
    pub(crate) fn refuse_unlanded_entries(&self, space: &Space) -> Result<()> {
        let Some(key) = Self::storage_key(&space.storage.repository) else {
            return Ok(());
        };
        let rows: Vec<(String, String)> = (self.db.prepare(
            "SELECT path,state FROM entry_files WHERE repository=?1 AND state<>?2 ORDER BY path",
        )?)
        .query_map(params![key, DISCARDED], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
        let named = |state: &str| -> Vec<&str> {
            (rows.iter())
                .filter(|(_, s)| s == state)
                .map(|(path, _)| path.rsplit('/').next().unwrap_or(path))
                .collect()
        };
        let files = |paths: &[&str]| match paths.len() {
            1 => format!("1 file ({})", paths[0]),
            n => format!("{n} files ({})", paths.join(", ")),
        };
        let (waiting, held) = (named(WAITING), named(HELD));
        let mut why = vec![];
        if !waiting.is_empty() {
            why.push(format!(
                "{} waiting to land: Sync now and let them land",
                files(&waiting)
            ));
        }
        if !held.is_empty() {
            why.push(format!(
                "{} held on this Mac: edit, mark or discard them on the Brains page",
                files(&held)
            ));
        }
        ensure!(
            why.is_empty(),
            "{} is the last Space here in {key}, which still has {}. Then remove it.",
            space.name,
            why.join("; and ")
        );
        Ok(())
    }
    /// The brought-in ids waiting to join the repository's `compiled-after`.
    pub(crate) fn brought_compiled(&self, key: &str) -> Result<Vec<String>> {
        Ok((self.db.prepare_cached(
            "SELECT entry_id FROM brought_compiled WHERE repository=?1 ORDER BY entry_id",
        )?)
        .query_map([key], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// The entries of the repository `key` for a pass to take: every one waiting or held.
    pub(crate) fn unlanded_entries(&self, key: &str) -> Result<Vec<String>> {
        Ok((self.db.prepare_cached(
            "SELECT path FROM entry_files WHERE repository=?1 AND state<>?2 ORDER BY path",
        )?)
        .query_map(params![key, DISCARDED], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// Records what a pass did with the repository's entries. `pushed`: its commits reached
    /// GitHub. The next pass scans a held entry again, so an edit made while this one ran is
    /// read then. An entry discarded meanwhile stays discarded.
    pub(crate) fn finish_entries(
        &mut self,
        key: &str,
        entries: Entries,
        pushed: bool,
    ) -> Result<()> {
        if !entries.compared {
            return Ok(());
        }
        let Entries {
            committed,
            landed,
            gone,
            held,
            given,
            more,
            compiled,
            ..
        } = entries;
        let tx = self.db.unchecked_transaction()?;
        let forget = "DELETE FROM entry_files WHERE repository=?1 AND path=?2 AND state<>?3";
        let pushed_ones = committed.iter().filter(|_| pushed);
        for path in landed.iter().chain(&gone).chain(pushed_ones) {
            tx.execute(forget, params![key, path, DISCARDED])?;
        }
        // The ids are in `compiled-after` once its commit reached GitHub, or it held them.
        let index = |path: &String| path.ends_with(compile::INDEX);
        let index_waits =
            held.iter().any(|h| index(&h.path)) || (!pushed && committed.iter().any(index));
        if !index_waits {
            for id in &compiled {
                tx.execute(
                    "DELETE FROM brought_compiled WHERE repository=?1 AND entry_id=?2",
                    params![key, id],
                )?;
            }
        }
        if more && pushed {
            self.issues_written.borrow_mut().insert(key.to_owned());
        }
        for path in &given {
            if !held.iter().any(|h| &h.path == path) {
                tx.execute(
                    "UPDATE entry_files SET state=?3,check_name=NULL,rule=NULL,line=NULL,reason=NULL WHERE repository=?1 AND path=?2 AND state=?4",
                    params![key, path, WAITING, HELD],
                )?;
            }
        }
        for held in &held {
            tx.execute(
                "UPDATE entry_files SET state=?3,check_name=?4,rule=?5,line=?6,reason=?7 WHERE repository=?1 AND path=?2 AND state<>?8",
                params![key, held.path, HELD, held.check, held.rule, held.line, held.reason, DISCARDED],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Every entry a scan holds on this Mac, naming the repository as the Brains page does.
    pub(crate) fn held_entries(&self) -> Result<Vec<HeldEntry>> {
        let keys = self.brain_keys()?;
        Ok((self.db.prepare_cached(
            "SELECT repository,path,project_id,check_name,rule,line,reason FROM entry_files WHERE state=?1 ORDER BY repository,path",
        )?)
        .query_map([HELD], |r| {
            let key: String = r.get(0)?;
            Ok(HeldEntry {
                repository: keys.get(&key).cloned().unwrap_or(key),
                path: r.get(1)?,
                project_id: r.get(2)?,
                check: r.get(3)?,
                rule: r.get(4)?,
                line: r.get(5)?,
                reason: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// The held entry at `path` of the repository `repository`, with its clone.
    fn held_entry(&self, repository: &str, path: &str) -> Result<(PathBuf, HeldEntry)> {
        let entry = (self.held_entries()?.into_iter())
            .find(|e| e.repository.eq_ignore_ascii_case(repository) && e.path == path)
            .with_context(|| format!("{path} is not held"))?;
        let brain = self.brain(repository)?;
        let clone = self.brain_clone(&brain)?;
        refuse_links(&clone, path)?;
        Ok((clone, entry))
    }
    /// A held entry's text, for Edit it, which this Mac alone holds.
    pub fn held_entry_text(&self, repository: &str, path: &str) -> Result<String> {
        let (clone, _) = self.held_entry(repository, path)?;
        Ok(fs::read_to_string(clone.join(path))?)
    }
    /// Sends a held entry back to the engine, which scans it again.
    fn scan_again(&mut self, repository: &str, path: &str) -> Result<()> {
        let key = repos::key(repository);
        self.db.execute(
            "UPDATE entry_files SET state=?3,check_name=NULL,rule=NULL,line=NULL,reason=NULL WHERE repository=?1 AND path=?2",
            params![key, path, WAITING],
        )?;
        self.issues_written.borrow_mut().insert(key);
        Ok(())
    }
    /// Why `text` may not stand as the entry at `path` of the repository, by the entry contract.
    /// A wiki page Bring in wrote has no contract to keep.
    fn entry_problem(&self, repository: &str, path: &str, text: &str) -> Result<Option<String>> {
        let brain = brain_dir(&self.root_of_key(&repos::key(repository))?);
        let in_brain = (path.strip_prefix(&format!("{brain}/")))
            .with_context(|| format!("{path} is not in the brain"))?;
        Ok((in_brain.strip_prefix("logs/"))
            .and_then(|in_logs| memory::contract_problem(in_logs, text)))
    }
    /// Edit it: the entry takes the human's text, refused while it breaks the contract, and is
    /// scanned again by the next pass.
    pub fn edit_held_entry(&mut self, repository: &str, path: &str, text: &str) -> Result<()> {
        let (clone, _) = self.held_entry(repository, path)?;
        if let Some(problem) = self.entry_problem(repository, path, text)? {
            bail!("This entry still cannot land: {problem}. Nothing was changed.");
        }
        shared::write_atomically(&clone.join(path), text.as_bytes())?;
        self.scan_again(repository, path)
    }
    /// Not sensitive: appends the inline marker to the line a `sensitive-info` finding names, so
    /// the next scan passes it, refused when the marker would break the entry contract. Offered
    /// for no other held entry.
    pub fn mark_entry_not_sensitive(
        &mut self,
        repository: &str,
        path: &str,
        reason: &str,
    ) -> Result<()> {
        let (clone, entry) = self.held_entry(repository, path)?;
        ensure!(
            entry.offers_not_sensitive(),
            "Only a sensitive-info finding can be marked; edit or discard this entry"
        );
        let reason = memory::one_line(reason);
        ensure!(
            !reason.is_empty() && reason.chars().count() <= MAX_REASON_CHARS,
            "Give a reason of at most {MAX_REASON_CHARS} characters"
        );
        let (rule, line) = (entry.rule.unwrap_or_default(), entry.line.unwrap_or(0));
        let content = fs::read_to_string(clone.join(path))?;
        let mut lines: Vec<String> = content.split('\n').map(str::to_owned).collect();
        let marked = (usize::try_from(line)?.checked_sub(1))
            .and_then(|at| lines.get_mut(at))
            .with_context(|| format!("{path} has no line {line}"))?;
        marked.push_str(&format!(" <!-- gate:allow {rule} {reason} -->"));
        let text = lines.join("\n");
        if let Some(problem) = self.entry_problem(repository, path, &text)? {
            bail!("The marker would break this entry: {problem}. Edit it instead.");
        }
        shared::write_atomically(&clone.join(path), text.as_bytes())?;
        self.scan_again(repository, path)
    }
    /// Discard: the entry leaves this Mac's clone, and its row stays so it is never written
    /// again. Nothing about it reaches GitHub.
    pub fn discard_held_entry(&mut self, repository: &str, path: &str) -> Result<()> {
        let (clone, _) = self.held_entry(repository, path)?;
        match fs::remove_file(clone.join(path)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error)?,
            _ => {}
        }
        self.db.execute(
            "UPDATE entry_files SET state=?3,check_name=NULL,rule=NULL,line=NULL,reason=NULL WHERE repository=?1 AND path=?2",
            params![repos::key(repository), path, DISCARDED],
        )?;
        Ok(())
    }
    /// Whether the human discarded the entry at `path`.
    pub(crate) fn entry_discarded(&self, repository: &str, path: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM entry_files WHERE repository=?1 AND path=?2 AND state=?3)",
            params![repos::key(repository), path, DISCARDED],
            |r| r.get(0),
        )?)
    }
    /// Whether the entry a log row names is held here, waiting, or neither.
    pub(crate) fn entry_state(
        &self,
        repository: &str,
        path: &str,
    ) -> Result<Option<EntryPullRequest>> {
        let state: Option<String> = self
            .db
            .query_row(
                "SELECT state FROM entry_files WHERE repository=?1 AND path=?2",
                params![repos::key(repository), path],
                |r| r.get(0),
            )
            .optional()?;
        Ok(state.and_then(|state| match state.as_str() {
            WAITING => Some(EntryPullRequest::Waiting),
            HELD => Some(EntryPullRequest::Held),
            _ => None,
        }))
    }
    /// This Mac's unlanded files of the repository no log row names, such as a ruling or what a
    /// Bring in wrote, by their paths in the repository.
    pub(crate) fn unlanded_files(&self, repository: &str) -> Result<Vec<String>> {
        Ok((self.db.prepare_cached(
            "SELECT path FROM entry_files WHERE repository=?1 AND log_id IS NULL AND state<>?2 ORDER BY path",
        )?)
        .query_map(params![repos::key(repository), DISCARDED], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// This Mac's unlanded entries of the repository, as the Logs page lists them.
    pub(crate) fn unlanded_logs(&self, repository: &str) -> Result<Vec<WorkLog>> {
        Ok((self.db.prepare_cached(
            "SELECT l.sequence,l.data FROM entry_files e JOIN work_logs l ON l.id=e.log_id WHERE e.repository=?1 AND e.state<>?2 ORDER BY l.id DESC",
        )?)
        .query_map(params![repos::key(repository), DISCARDED], |r| {
            let mut log: WorkLog = decode(r, 1)?;
            log.sequence = r.get(0)?;
            Ok(log)
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// Ends the run a pass left a landing's outcome for. Files that landed are written into the
    /// clone too, as a pass commits what it staged and leaves the clone's own copies alone.
    pub(crate) fn finish_landing(
        &mut self,
        landing: Landing,
        landed: Landed,
        pushed: Option<&str>,
        clone: &Path,
    ) -> Result<()> {
        match landed {
            Landed::Committed => {
                let Some(commit) = pushed else {
                    return Ok(());
                };
                // The commit is on GitHub by now, so a link that arrived since `verdict` read the
                // clone leaves its file for the next pull and fails nothing.
                for path in (landing.files.iter()).filter(|p| refuse_links(clone, p).is_ok()) {
                    let content = fs::read(landing.worktree.join(path))?;
                    shared::write_atomically(&clone.join(path), &content)?;
                }
                self.end_landing(&landing, CompileOutcome::Landed, None, Some(commit))
            }
            Landed::AlreadyOnMain(commit) => {
                self.end_landing(&landing, CompileOutcome::Landed, None, Some(&commit))
            }
            Landed::CompiledElsewhere(machine) => {
                let reason = compile::compiled_elsewhere(machine.as_deref());
                let outcome = CompileOutcome::CompiledElsewhere;
                self.end_landing(&landing, outcome, Some(reason), None)
            }
            Landed::Outrun(by) => {
                let who = by.unwrap_or_else(|| "Another Mac".to_owned());
                let reason = format!("{who} took this run over, so nothing landed");
                self.end_landing(&landing, CompileOutcome::Lost, Some(reason), None)
            }
            Landed::Refused(held) => {
                let name = held.path.clone();
                let why = match (&held.rule, &held.reason) {
                    (Some(rule), _) => format!("{} rule {rule}", held.check),
                    (None, Some(reason)) => format!("{}: {reason}", held.check),
                    (None, None) => held.check.clone(),
                };
                let line = held
                    .line
                    .map_or(String::new(), |line| format!(" line {line}"));
                let reason = format!(
                    "{name}{line} failed the scan ({why}), so nothing landed; the next run compiles these entries again"
                );
                self.end_landing(&landing, CompileOutcome::Failed, Some(reason), None)
            }
            Landed::Failed(reason) => {
                self.end_landing(&landing, CompileOutcome::Failed, Some(reason), None)
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::publish::tests::Fixture;
    use crate::storage::tests::{join_sage, sage, sync};
    use crate::sync::tests::{GitHub, NAME};
    use crate::test_support::{git, scan_tools};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    /// The repository of the Space `stored` makes.
    pub(crate) const REPO: &str = NAME;
    pub(crate) const FOLDER: &str = "wiffletree";
    /// An invented number.
    pub(crate) const PHONE: &str = "(415) 555-0134";

    /// The publish fixture's project, whose repository is `app`, in the Sage Space, joined from a
    /// repository on a fake GitHub whose folder is `wiffletree/`, with gh signed in.
    pub(crate) struct Stored {
        pub(crate) f: Fixture,
        pub(crate) github: Arc<GitHub>,
        pub(crate) tools: publish::Tools,
        pub(crate) sage: Space,
        _github_folder: tempfile::TempDir,
        pub(crate) bin: tempfile::TempDir,
    }

    pub(crate) fn stored() -> Stored {
        stored_with("")
    }

    /// `stored`, with `yaml` added to Sage's `space.yaml`.
    pub(crate) fn stored_with(yaml: &str) -> Stored {
        let mut f = publish::tests::fixture();
        let bin = scan_tools(true);
        f.host.scan_path = Some(bin.path().into());
        f.host.record_gh_login(Some("mhohlios".into()));
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let space = format!("{FOLDER}/spaces/sage/space.yaml");
        github.commit(&space, sage("Secure family messaging") + yaml);
        let tracker = format!("{FOLDER}/wiffletree-issues.yaml");
        github.commit(&tracker, "format: 1\nprefix: SAGE\n");
        let sage = join_sage(&mut f.host, &github).unwrap().remove(0).space;
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &sage.id))
        .unwrap();
        let tools = github.tools();
        let mut stored = Stored {
            f,
            github,
            tools,
            sage,
            _github_folder: folder,
            bin,
        };
        stored.index();
        stored
    }

    impl Stored {
        /// Runs the repository's pass.
        pub(crate) fn sync(&mut self) {
            sync(&mut self.f.host, &self.github);
        }
        /// Runs the pass with GitHub out of reach or back, keeping what it answers.
        pub(crate) fn sync_answering(&mut self) -> Result<()> {
            let key = repos::key(REPO);
            let pass = self.f.host.storage_pass(&key).unwrap().unwrap();
            let passed = storage::pass(pass, &self.tools);
            self.f.host.finish_storage_pass(&key, passed)
        }
        /// Brings the brain's index to what the clone last synced.
        pub(crate) fn index(&mut self) {
            let pass = self.f.host.brain_pass(REPO).unwrap().unwrap();
            assert!(brains::pass(pass, &self.tools).reached);
        }
        /// Commits files of the brain folder to GitHub, then pulls them and indexes them.
        pub(crate) fn merge(&mut self, files: &[(String, String)]) -> String {
            let commit = self.commit(files);
            self.sync();
            self.index();
            commit
        }
        /// Commits files of the brain folder to GitHub, as a teammate's Mac would.
        pub(crate) fn commit(&self, files: &[(String, String)]) -> String {
            let files: Vec<(String, String)> = (files.iter())
                .map(|(path, text)| (format!("{FOLDER}/second-brain/{path}"), text.clone()))
                .collect();
            self.github
                .commit_files(&files, "A teammate's change (#81)")
        }
        pub(crate) fn clone_folder(&self) -> PathBuf {
            repos::clone_folder(&self.f.host.home, REPO)
        }
        fn log(&mut self, changed: &str) -> Value {
            let args = json!({"type": "decision", "title": "Retry once", "changed": changed,
                "why": "Timeouts are transient.", "state": "Committed."});
            let implementer = self.f.implementer.id.clone();
            self.f
                .host
                .agent_tool(&implementer, "log_work", args)
                .unwrap()
        }
        fn path_of(answer: &Value) -> String {
            answer["path"].as_str().unwrap().to_owned()
        }
        fn held(&self) -> Vec<HeldEntry> {
            self.f.host.held_entries().unwrap()
        }
        /// The paths the last commit on GitHub changed, sorted.
        fn last_commit_paths(&self) -> Vec<String> {
            let shown = git(
                &self.github.bare,
                &["show", "--format=", "--name-only", "main"],
            );
            let mut paths: Vec<String> = shown.lines().map(str::to_owned).collect();
            paths.sort();
            paths
        }
    }

    #[test]
    fn one_entry_lands_in_one_commit_that_adds_only_its_file_soon_after_it_is_logged() {
        let mut s = stored();
        let answer = s.log("Retry once.");
        let path = Stored::path_of(&answer);
        assert!(
            path.starts_with(&format!("{FOLDER}/second-brain/logs/app/")),
            "{path}"
        );
        assert!(s.clone_folder().join(&path).is_file(), "written at once");
        // The push is due as soon after the last change as an issue's.
        let key = repos::key(REPO);
        assert!(s.f.host.issues_written.borrow().contains(&key));
        assert_eq!(issues_sync::PUSH_DELAY_MS, 2_000);
        s.sync();
        assert_eq!(s.last_commit_paths(), std::slice::from_ref(&path));
        assert!(s.github.show(&path).is_some());
        assert_eq!(
            s.f.host.unlanded_entries(&key).unwrap(),
            Vec::<String>::new()
        );
        s.sync();
        assert_eq!(s.github.log("%s").len(), 4, "a second pass pushes nothing");
    }

    #[test]
    fn only_a_pass_that_pushed_or_saw_github_move_refreshes_the_brains_at_once() {
        let mut s = stored();
        s.f.host.brains_now = false;
        s.sync();
        assert!(
            !s.f.host.brains_now,
            "a quiet pass leaves the five-minute recheck be"
        );
        s.commit(&[("notes/A.md".into(), "A\n".into())]);
        s.sync();
        assert!(std::mem::take(&mut s.f.host.brains_now));
        s.log("Retry once.");
        s.sync();
        assert!(s.f.host.brains_now, "its own push moved the head");
    }

    #[test]
    fn a_burst_of_entries_is_one_commit() {
        let mut s = stored();
        let mut paths: Vec<String> = (0..3)
            .map(|n| Stored::path_of(&s.log(&format!("Step {n}."))))
            .collect();
        s.sync();
        paths.sort();
        assert_eq!(s.last_commit_paths(), paths);
        assert!(s.github.log("%s")[0].starts_with("Logged 3 entries"));
    }

    #[test]
    fn a_log_entry_and_an_issue_comment_land_in_commits_of_their_own() {
        let mut s = stored();
        let path = Stored::path_of(&s.log("Retry once."));
        let new = NewIssue {
            title: "Crash".into(),
            description: "Sometimes.".into(),
            ..NewIssue::default()
        };
        s.f.host.create_issue(&s.sage.id, new).unwrap();
        s.sync();
        let log = git(&s.github.bare, &["log", "-2", "--name-only", "--format=%%"]);
        let commits: Vec<&str> = log.split('%').filter(|c| !c.trim().is_empty()).collect();
        assert_eq!(commits.len(), 2, "{log}");
        assert!(commits.iter().any(|c| c.trim() == path), "{log}");
    }

    #[test]
    fn a_push_refused_because_main_moved_fetches_and_lands_on_the_new_head() {
        let mut s = stored();
        let path = Stored::path_of(&s.log("Retry once."));
        let github = s.github.clone();
        *s.github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || github.commit("README.md", "A teammate's change.\n")),
        ));
        s.sync();
        assert!(s.github.show(&path).is_some());
        assert_eq!(s.github.log("%s")[1], "README.md");
        assert_eq!(
            s.f.host.unlanded_entries(&repos::key(REPO)).unwrap().len(),
            0
        );
    }

    #[test]
    fn an_entry_written_while_github_is_out_of_reach_waits_and_lands_at_the_first_pass_that_reaches_it()
     {
        let mut s = stored();
        let path = Stored::path_of(&s.log("Retry once."));
        s.github.offline.store(true, Ordering::SeqCst);
        assert!(s.sync_answering().is_err());
        assert!(s.github.show(&path).is_none());
        assert_eq!(
            s.f.host.unlanded_entries(&repos::key(REPO)).unwrap(),
            std::slice::from_ref(&path)
        );
        s.github.offline.store(false, Ordering::SeqCst);
        s.sync_answering().unwrap();
        assert!(s.github.show(&path).is_some());
    }

    #[test]
    fn an_entry_a_lost_answer_landed_is_taken_as_landed() {
        let mut s = stored();
        let path = Stored::path_of(&s.log("Retry once."));
        let text = fs::read_to_string(s.clone_folder().join(&path)).unwrap();
        s.github.commit(&path, text);
        s.sync();
        assert_eq!(
            s.f.host.unlanded_entries(&repos::key(REPO)).unwrap().len(),
            0
        );
        assert_eq!(s.github.log("%s").len(), 4, "nothing is pushed twice");
    }

    #[test]
    fn a_teammates_entry_comes_down_with_the_pull_and_is_never_committed() {
        let mut s = stored();
        let (path, text) = brain_index::tests::entry(
            "app",
            &brain_index::tests::id(7),
            ("tomas", "observation", "Health"),
            "—",
        );
        s.commit(&[(path.clone(), text.clone())]);
        s.sync();
        let pulled = s
            .clone_folder()
            .join(format!("{FOLDER}/second-brain/{path}"));
        assert_eq!(fs::read_to_string(pulled).unwrap(), text);
        assert_eq!(s.github.log("%s").len(), 4);
    }

    #[test]
    fn a_local_space_commits_at_once_and_pushes_nothing_with_gh_signed_out() {
        let mut f = publish::tests::fixture();
        let local = f.host.create_space("Home", "", "", None).unwrap().space;
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &local.id))
        .unwrap();
        f.host.record_gh_login(None);
        let args = json!({"type": "observation", "title": "Noted", "changed": "One.",
            "why": "Two.", "state": "Three."});
        let implementer = f.implementer.id.clone();
        let answer = f.host.agent_tool(&implementer, "log_work", args).unwrap();
        let path = answer["path"].as_str().unwrap();
        assert!(
            path.starts_with("wiffletree/second-brain/logs/app/"),
            "{path}"
        );
        let clone = f.host.storage_folder(&local.storage.repository);
        let shown = git(&clone, &["show", "--format=%s", "--name-only", "HEAD"]);
        assert!(shown.ends_with(path), "{shown}");
        let text = fs::read_to_string(clone.join(path)).unwrap();
        let author = text.lines().find(|l| l.starts_with("author: ")).unwrap();
        assert!(author.len() > "author: ".len(), "{text}");
        assert!(f.host.brains_now);
        assert_eq!(
            f.host
                .unlanded_entries(&local.storage.repository.key())
                .unwrap()
                .len(),
            0
        );
        assert!(git(&clone, &["remote"]).is_empty());
    }

    #[test]
    fn an_entry_for_a_github_space_needs_a_login_and_stores_nothing_without_one() {
        let mut s = stored();
        s.f.host.record_gh_login(None);
        let args = json!({"type": "decision", "title": "Retry", "changed": "One.", "why": "Two.",
            "state": "Three."});
        let implementer = s.f.implementer.id.clone();
        let refused =
            s.f.host
                .agent_tool(&implementer, "log_work", args)
                .unwrap_err();
        assert!(
            format!("{refused:#}").contains("GitHub login"),
            "{refused:#}"
        );
        let project = s.f.coordinator.project_id.clone();
        assert!(s.f.host.logs(&project, None, 10).unwrap().is_empty());
    }

    #[test]
    fn a_held_entry_is_never_pushed_others_land_and_no_text_reaches_a_card_or_record() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call the parent at {PHONE} tonight.")));
        let fine = Stored::path_of(&s.log("Retry once."));
        s.sync();
        assert!(s.github.show(&held).is_none(), "never pushed");
        assert!(s.github.show(&fine).is_some(), "the other lands");
        let entries = s.held();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(
            (
                entry.path.as_str(),
                entry.check.as_str(),
                entry.rule.as_deref(),
                entry.line
            ),
            (held.as_str(), "sensitive-info", Some("phone"), Some(10))
        );
        assert_eq!(
            entry.project_id.as_deref(),
            Some(s.f.coordinator.project_id.as_str())
        );
        assert!(entry.offers_not_sensitive());
        // The card on the project that wrote it, and a record of nothing it matched.
        let notes = s.f.host.workspace_notes(&[]).unwrap();
        let card = notes
            .iter()
            .find(|n| matches!(n, WorkspaceNote::HeldEntry(_)));
        assert!(card.unwrap().is_for(&s.f.coordinator.project_id));
        assert!(!card.unwrap().is_for("another"));
        let shown = format!("{notes:?}");
        assert!(!shown.contains("555-0134"), "{shown}");
        let row: String = (s.f.host.db.query_row(
            "SELECT repository||path||state||coalesce(check_name,'')||coalesce(rule,'')||coalesce(reason,'') FROM entry_files",
            [],
            |r| r.get(0),
        ))
        .unwrap();
        assert!(!row.contains("555-0134"), "{row}");
        // It stays on this Mac, and the next pass neither pushes it nor doubles its card.
        assert!(s.clone_folder().join(&held).is_file());
        s.sync();
        assert!(s.github.show(&held).is_none());
        assert_eq!(s.held().len(), 1);
    }

    #[test]
    fn the_last_space_in_a_repository_is_kept_while_its_entries_wait_or_are_held() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let waiting = Stored::path_of(&s.log("Retry once."));
        let project = s.f.coordinator.project_id.clone();
        let personal = (s.f.host.spaces().unwrap().into_iter())
            .find(|r| r.space.name == PERSONAL)
            .unwrap()
            .space
            .id;
        s.f.host.move_project_to_space(&project, &personal).unwrap();
        let refused = s.f.host.remove_space(&s.sage.id).unwrap_err();
        let refused = format!("{refused:#}");
        let name = |path: &str| path.rsplit('/').next().unwrap().to_owned();
        assert!(
            refused.contains(&name(&held)) && refused.contains("Brains page"),
            "{refused}"
        );
        assert!(
            refused.contains(&name(&waiting)) && refused.contains("Sync now"),
            "{refused}"
        );
        assert!(s.f.host.space(&s.sage.id).is_ok());
        s.sync();
        let card = s.held().remove(0);
        s.f.host.discard_held_entry(REPO, &card.path).unwrap();
        s.f.host.remove_space(&s.sage.id).unwrap();
    }

    #[test]
    fn a_held_entry_no_log_row_names_is_listed_in_logs_as_held() {
        let mut s = stored();
        let (path, text) = brain_index::tests::entry(
            "app",
            &brain_index::tests::id(3),
            ("mhohlios", "decision", "Ruling"),
            "—",
        );
        let path = format!("{FOLDER}/second-brain/{path}");
        let text = text.replace("- why: Because.", &format!("- why: Call {PHONE}."));
        let sage = s.sage.clone();
        s.f.host.land_entry(&sage, &path, &text, None).unwrap();
        s.sync();
        assert_eq!(s.held().len(), 1);
        let page = (s.f.host)
            .brain_logs(REPO, None, &Default::default(), &Default::default())
            .unwrap();
        let listed = page
            .pending
            .iter()
            .find(|e| e.id == brain_index::tests::id(3));
        let listed = listed.expect("the held ruling is listed");
        assert_eq!(
            (listed.title.as_str(), &listed.pull_request),
            ("Ruling", &EntryPullRequest::Held)
        );
    }

    #[test]
    fn a_held_entry_survives_a_restart() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let home = s.f.host.home.clone();
        let elsewhere = tempfile::tempdir().unwrap();
        let another = Host::open_set_up(elsewhere.path()).unwrap();
        drop(std::mem::replace(&mut s.f.host, another));
        let reopened = Host::open_set_up(&home).unwrap();
        let entries = reopened.held_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, held);
    }

    #[test]
    fn not_sensitive_appends_the_marker_and_the_entry_lands_with_it() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let refused = s.f.host.mark_entry_not_sensitive(REPO, &held, " ");
        assert!(refused.is_err(), "a reason is needed");
        s.f.host
            .mark_entry_not_sensitive(REPO, &held, "Fixture call ID")
            .unwrap();
        s.sync();
        let landed = s.github.show(&held).unwrap();
        let line = landed.lines().nth(9).unwrap();
        assert!(
            line.ends_with("<!-- gate:allow phone Fixture call ID -->"),
            "{line}"
        );
        assert!(s.held().is_empty(), "the card closes");
    }

    #[test]
    fn not_sensitive_is_refused_when_its_marker_would_break_the_entry() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let clone = s.clone_folder();
        let text = fs::read_to_string(clone.join(&held)).unwrap();
        let padded = text.replace(
            &format!("Call {PHONE}."),
            &format!("Call {PHONE}. {}", "x".repeat(memory::MAX_BODY_CHARS)),
        );
        fs::write(clone.join(&held), &padded).unwrap();
        let refused =
            s.f.host
                .mark_entry_not_sensitive(REPO, &held, "Fixture call ID")
                .unwrap_err();
        assert!(format!("{refused:#}").contains("cap"), "{refused:#}");
        assert_eq!(fs::read_to_string(clone.join(&held)).unwrap(), padded);
        // A ref that takes the marker stops being one the contract allows.
        let line = text.lines().position(|l| l.contains(PHONE)).unwrap();
        let moved: Vec<String> = (text.lines().enumerate())
            .map(|(at, l)| match at == line {
                true => "- changed: Clean.".to_owned(),
                false if l.starts_with("- ref: ") => format!("{l} {PHONE}"),
                false => l.to_owned(),
            })
            .collect();
        fs::write(
            clone.join(&held),
            moved.join(
                "
",
            ) + "
",
        )
        .unwrap();
        s.sync();
        let refused = (s.f.host)
            .mark_entry_not_sensitive(REPO, &held, "Fixture call ID")
            .unwrap_err();
        assert!(format!("{refused:#}").contains("ref"), "{refused:#}");
    }

    #[test]
    fn edit_it_finds_the_entry_in_a_custom_folder_with_a_part_named_logs() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let text = fs::read_to_string(s.clone_folder().join(&held)).unwrap();
        let (_, in_logs) = held.split_once("/second-brain/logs/").unwrap();
        let custom = "team/logs/wiffletree";
        let moved = "UPDATE spaces SET data=json_set(data,'$.storage.root',?1)";
        s.f.host.db.execute(moved, [custom]).unwrap();
        let nested = format!("{custom}/second-brain/logs/{in_logs}");
        let host = &s.f.host;
        assert_eq!(host.entry_problem(REPO, &nested, &text).unwrap(), None);
        assert!(host.entry_problem(REPO, &held, &text).is_err());
    }

    #[test]
    fn a_secret_scan_finding_offers_no_not_sensitive_and_refuses_one() {
        let mut s = stored();
        let held = Stored::path_of(&s.log("Retry once."));
        s.f.host
            .db
            .execute(
                "UPDATE entry_files SET state='held',check_name='secret-scan',rule='generic-api-key',line=10 WHERE path=?1",
                [&held],
            )
            .unwrap();
        let entries = s.held();
        assert!(!entries[0].offers_not_sensitive());
        let refused = s.f.host.mark_entry_not_sensitive(REPO, &held, "A fixture");
        assert!(format!("{:#}", refused.unwrap_err()).contains("sensitive-info"));
        let unchanged = fs::read_to_string(s.clone_folder().join(&held)).unwrap();
        assert!(!unchanged.contains("gate:allow"));
    }

    #[test]
    fn edit_it_scans_the_entry_again_and_refuses_text_that_breaks_the_contract() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let text = s.f.host.held_entry_text(REPO, &held).unwrap();
        let broken = s.f.host.edit_held_entry(REPO, &held, "no frontmatter");
        assert!(format!("{:#}", broken.unwrap_err()).contains("Nothing was changed"));
        assert_eq!(s.f.host.held_entry_text(REPO, &held).unwrap(), text);
        // Still sensitive: held again, by the next scan.
        let other = text.replace(PHONE, "(415) 555-0199");
        s.f.host.edit_held_entry(REPO, &held, &other).unwrap();
        assert_eq!(s.held().len(), 0, "waiting for its scan");
        s.sync();
        assert!(s.github.show(&held).is_none());
        assert_eq!(s.held().len(), 1);
        // Fixed: it lands.
        let fixed = text.replace(PHONE, "the family's contact");
        s.f.host.edit_held_entry(REPO, &held, &fixed).unwrap();
        s.sync();
        assert_eq!(s.github.show(&held).unwrap(), fixed);
        assert!(s.held().is_empty());
    }

    #[test]
    fn discard_removes_the_entry_here_pushes_nothing_and_it_never_comes_back() {
        let mut s = stored();
        let held = Stored::path_of(&s.log(&format!("Call {PHONE}.")));
        s.sync();
        let pushes = s.github.pushes();
        s.f.host.discard_held_entry(REPO, &held).unwrap();
        assert!(!s.clone_folder().join(&held).exists());
        assert!(s.held().is_empty(), "the card closes");
        s.sync();
        assert_eq!(s.github.pushes(), pushes, "nothing about it reaches GitHub");
        assert!(s.f.host.entry_discarded(REPO, &held).unwrap());
        let project = s.f.coordinator.project_id.clone();
        assert_eq!(
            s.f.host.logs(&project, None, 10).unwrap().len(),
            1,
            "its row stays"
        );
        assert!(
            s.f.host.held_entry_text(REPO, &held).is_err(),
            "nothing more is offered"
        );
    }

    #[test]
    fn a_check_that_cannot_run_holds_saying_what_is_missing_and_catches_up() {
        let mut s = stored();
        let without = scan_tools(false);
        s.f.host.scan_path = Some(without.path().into());
        let held = Stored::path_of(&s.log("Retry once."));
        s.sync();
        let entries = s.held();
        assert_eq!(entries[0].rule, None);
        assert!(!entries[0].offers_not_sensitive());
        let reason = entries[0].reason.as_deref().unwrap();
        assert!(reason.contains("gitleaks"), "{reason}");
        assert!(s.github.show(&held).is_none());
        s.f.host.scan_path = Some(s.bin.path().into());
        s.sync();
        assert!(
            s.github.show(&held).is_some(),
            "it lands once the check runs"
        );
        assert!(s.held().is_empty());
    }

    #[test]
    fn an_entry_in_a_repository_at_its_root_lands_there_and_an_unpushed_one_follows_the_move() {
        let mut f = publish::tests::fixture();
        let bin = scan_tools(true);
        f.host.scan_path = Some(bin.path().into());
        f.host.record_gh_login(Some("mhohlios".into()));
        let folder = tempfile::tempdir().unwrap();
        let github = crate::storage::tests::shared_storage(folder.path(), Some("SAGE"));
        let sage = join_sage(&mut f.host, &github).unwrap().remove(0).space;
        assert_eq!(sage.storage.root, "", "not moved yet");
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &sage.id))
        .unwrap();
        let implementer = f.implementer.id.clone();
        let log = |f: &mut Fixture, title: &str| {
            let args = json!({"type": "decision", "title": title, "changed": "One.",
                "why": "Two.", "state": "Three."});
            let logged = f.host.agent_tool(&implementer, "log_work", args).unwrap();
            logged["path"].as_str().unwrap().to_owned()
        };
        let at_root = log(&mut f, "At the root");
        assert!(at_root.starts_with("second-brain/logs/app/"), "{at_root}");
        sync(&mut f.host, &github);
        assert!(github.show(&at_root).is_some());
        let unpushed = log(&mut f, "Before the move");
        let moving = f.host.start_move(&sage.id).unwrap();
        let root = moving.run(&github.tools()).unwrap();
        f.host.finish_move(&moving.key, &root).unwrap();
        sync(&mut f.host, &github);
        assert!(github.show(&format!("{FOLDER}/{unpushed}")).is_some());
        assert!(github.show(&unpushed).is_none(), "{unpushed}");
        assert!(github.show(&format!("{FOLDER}/{at_root}")).is_some());
        let project = f.coordinator.project_id.clone();
        let rows = f.host.logs(&project, None, 10).unwrap();
        assert!(rows[0].path.as_ref().unwrap().starts_with("wiffletree/"));
    }

    #[test]
    fn a_row_held_at_the_root_lands_in_the_folder_when_it_is_fixed_after_the_move() {
        let mut f = publish::tests::fixture();
        let bin = scan_tools(true);
        f.host.scan_path = Some(bin.path().into());
        f.host.record_gh_login(Some("mhohlios".into()));
        let folder = tempfile::tempdir().unwrap();
        let github = crate::storage::tests::shared_storage(folder.path(), Some("SAGE"));
        let sage = join_sage(&mut f.host, &github).unwrap().remove(0).space;
        (f.host
            .move_project_to_space(&f.coordinator.project_id, &sage.id))
        .unwrap();
        let args = json!({"type": "decision", "title": "Held", "changed": "One.", "why": "Two.",
            "state": "Three."});
        let implementer = f.implementer.id.clone();
        let logged = f.host.agent_tool(&implementer, "log_work", args).unwrap();
        let (id, path) = (
            logged["id"].as_str().unwrap(),
            logged["path"].as_str().unwrap(),
        );
        // As the upgrade holds a row that breaks the contract: no file, no row of `entry_files`.
        let held =
            "UPDATE work_logs SET data=json_remove(json_set(data,'$.held','Its ref'),'$.written')";
        f.host
            .db
            .execute_batch(&format!("{held}; DELETE FROM entry_files;"))
            .unwrap();
        let clone = repos::clone_folder(&f.host.home, REPO);
        fs::remove_file(clone.join(path)).unwrap();
        let moving = f.host.start_move(&sage.id).unwrap();
        let root = moving.run(&github.tools()).unwrap();
        f.host.finish_move(&moving.key, &root).unwrap();
        let fix = LogFix {
            kind: LogKind::Decision,
            title: "Held".into(),
            repository: "app".into(),
            reference: memory::NO_REF.into(),
            changed: "One.".into(),
            why: "Two.".into(),
            state: "Three.".into(),
        };
        let fixed = f.host.fix_held_log(id, fix).unwrap();
        let landed = format!("{FOLDER}/{path}");
        assert_eq!(fixed.path.as_deref(), Some(landed.as_str()));
        sync(&mut f.host, &github);
        assert!(github.show(&landed).is_some(), "the push went through");
    }

    #[test]
    fn an_entry_is_never_written_through_a_symbolic_link() {
        let mut s = stored();
        let clone = s.clone_folder();
        let logs = clone.join(FOLDER).join("second-brain/logs");
        fs::create_dir_all(logs.parent().unwrap()).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), &logs).unwrap();
        let args = json!({"type": "decision", "title": "Retry", "changed": "One.", "why": "Two.",
            "state": "Three."});
        let implementer = s.f.implementer.id.clone();
        let refused =
            s.f.host
                .agent_tool(&implementer, "log_work", args)
                .unwrap_err();
        assert!(
            format!("{refused:#}").contains("symbolic link"),
            "{refused:#}"
        );
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }
}
