//! Moving a Space's storage (OpenSpec change `move-space-storage`): every Space on this Mac in
//! one storage repository goes to another, from this Mac to GitHub, from GitHub to this Mac, or
//! between two GitHub repositories. The steps, in order: `Host::check_storage_move` refuses,
//! `Host::begin_storage_move` starts, `copy` copies and scans into a staging clone,
//! `Host::note_push` and `push` reach GitHub, and `Host::switch_storage` rewrites every reference
//! to the repository in one transaction, which also records the renames
//! `Host::finish_storage_move` makes. That transaction's commit is the move: a stop before it
//! leaves the Spaces where they were, and a stop after it is finished by the next start, as
//! `repo_moves` is. design.md lists every reference to a storage repository.
use crate::*;
use publish::Tools;
use repos::{Content, Record, Remote};
use std::collections::BTreeMap;
use storage::{Staged, TRACKER, under};
use tempfile::TempDir;
use tracker::ISSUES;
use workspace_core::job::{Recipe, RunOutcome, Runner, Sharing};

/// The tables of a tracker's issues keyed by `StorageRepository::key`.
const ISSUE_TABLES: [&str; 9] = [
    "issue_index",
    "issue_read",
    "issue_temporary",
    "issue_preferred",
    "issue_links",
    "issue_derived",
    "issue_pending",
    "issue_follows",
    "issue_notes",
];
/// What this Mac knows of the old repository's collaborators, read again for the new one.
const COLLABORATOR_TABLES: [&str; 2] = ["issue_collaborators", "issue_collaborators_read"];
/// The tables that hold a file's state against the old repository, by `repos::key`.
const SYNC_TABLES: [&str; 4] = ["sync_record", "sync_paused", "sync_held", "issue_parts"];

/// Why a move ended at the switch, with its Spaces where they were.
const CHANGED: &str = "Storage changed during the move, so nothing switched. Move it again.";

/// A move under way, from `begin_storage_move` to `switch_storage`.
pub(crate) struct Move {
    spaces: Vec<Space>,
    to: StorageRepository,
    /// The repository moved to on GitHub; `None` for a new local one.
    repository: Option<SettingsRepository>,
    /// The folder the files move from.
    source: PathBuf,
    /// The clone of the target, or the new local repository, until the switch puts it in place.
    staging: TempDir,
    /// The paths that move: each Space's folder, and the tracker unless the target's is adopted.
    roots: Vec<String>,
    /// The files that move as other than the source holds them.
    rewritten: BTreeMap<String, Vec<u8>>,
    /// The tracker the Spaces take in place of the one they had: the target's, or a new one for
    /// a move to this Mac from storage with none.
    new_tracker: Option<TrackerConfig>,
    /// Scans what is pushed; local storage is never pushed, so never scanned.
    scanner: Option<scan::Scanner>,
}

/// A switched move, whose renames `finish_storage_move` makes.
pub(crate) struct Switched {
    asked: SpaceId,
    from: StorageRepository,
    /// Says that the Spaces moved, should the rest fail.
    moved: String,
}

/// What a move will do, as `Host::plan_storage_move` found it.
struct Plan {
    answer: StorageMove,
    roots: Vec<String>,
    new_tracker: Option<TrackerConfig>,
}

/// The rule set of a moved file, by its place among a storage repository's rule sets: a commit
/// holds one rule set's files.
fn rule_set(path: &str) -> usize {
    match (path.starts_with("spaces/"), path.split('/').nth(2)) {
        (true, Some("boards")) => 1,
        (true, _) => 0,
        (false, _) => 2,
    }
}

/// `first, second and third`.
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn names(spaces: &[Space]) -> String {
    listed(&spaces.iter().map(|s| s.name.clone()).collect::<Vec<_>>())
}

/// The commit `at` is on, `None` in a repository with no commit yet.
fn head(at: &Path) -> Result<Option<String>> {
    let answer = runtime::git_answer(at, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    Ok(answer
        .status
        .success()
        .then(|| answer.output.trim().to_owned()))
}

/// The paths a move takes: each Space's folder and, unless its tracker stays behind, the tracker.
fn moved_roots(spaces: &[Space], with_tracker: bool) -> Vec<String> {
    let mut roots: Vec<String> = (spaces.iter())
        .map(|space| format!("spaces/{}", space.storage.folder))
        .collect();
    if with_tracker {
        roots.extend([ISSUES.to_owned(), TRACKER.to_owned()]);
    }
    roots
}

/// The files under `roots` of `source`, with `rewritten` in place of what it holds.
fn files_to_move(
    source: &Path,
    roots: &[String],
    rewritten: &BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for root in roots {
        let at = source.join(root);
        let found = match at.is_dir() {
            true => storage::files_under(&at)?,
            false if at.is_file() => vec![at],
            false => vec![],
        };
        for file in found {
            let path = (file.strip_prefix(source)?.to_str())
                .context("A path is not UTF-8")?
                .to_owned();
            files.insert(path, fs::read(&file)?);
        }
    }
    files.extend(rewritten.clone());
    Ok(files)
}

impl Move {
    /// Whether the move reaches GitHub.
    pub(crate) fn pushes(&self) -> bool {
        self.repository.is_some()
    }
    /// How `files` differ from the staging clone's head under the moved paths.
    fn changes(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<(String, Content)>> {
        let at = self.staging.path();
        let there = match head(at)? {
            Some(head) => repos::files_at(at, &head, &|path| under(&self.roots, path))?,
            None => BTreeMap::new(),
        };
        let gone = (there.keys())
            .filter(|path| !files.contains_key(*path))
            .map(|path| (path.clone(), None));
        let changed = (files.iter())
            .filter(|(path, content)| there.get(*path) != Some(content))
            .map(|(path, content)| (path.clone(), Some(content.clone())));
        Ok(changed.chain(gone).collect())
    }
    /// The Spaces' names, for a commit's message.
    fn subject(&self, what: &str) -> String {
        format!("{}: {what} · {}", names(&self.spaces), sync::machine_name())
    }
}

/// Writes `changes` to the staging folder.
fn write_files(at: &Path, changes: &[(String, Content)]) -> Result<()> {
    for (path, content) in changes {
        match content {
            Some(content) => shared::write_atomically(&at.join(path), content)?,
            None => fs::remove_file(at.join(path))?,
        }
    }
    Ok(())
}

/// Copies the moved files into the staging clone and commits them, off the host thread. To
/// GitHub they are scanned first, and a file held ends the move with nothing committed; each
/// rule set's files then get a commit of their own, as a storage repository's always do. Pushes
/// nothing.
pub(crate) fn copy(run: &Move, tools: &Tools) -> Result<()> {
    let at = run.staging.path();
    let files = files_to_move(&run.source, &run.roots, &run.rewritten)?;
    let changes = run.changes(&files)?;
    let Some(scanner) = &run.scanner else {
        write_files(at, &changes)?;
        let roots: Vec<&str> = run.roots.iter().map(String::as_str).collect();
        return storage::commit_local(at, &roots, &run.subject("moved here"));
    };
    if head(at)?.is_none() {
        // Only in the staging clone: a held file leaves an empty repository as it was.
        storage::first_commit(at, tools)?;
    }
    let base = head(at)?.context("The staging clone has a commit")?;
    let held = scanner.scan(at, &base, &changes);
    if let Some(first) = held.first() {
        let more = match held.len() {
            1 => String::new(),
            more => format!(" (and {} more)", more - 1),
        };
        bail!(
            "{}{more} can’t be pushed, so nothing moved: {}. Edit the file to remove the value, or mark a false positive with gate:allow, then move again.",
            first.path,
            first.why()
        );
    }
    write_files(at, &changes)?;
    for (set, what) in [
        (0, "moved the Space here"),
        (1, "moved saved outputs here"),
        (2, "moved issues here"),
    ] {
        let files: Vec<(String, Content)> = (changes.iter())
            .filter(|(path, _)| rule_set(path) == set)
            .cloned()
            .collect();
        if !files.is_empty() {
            repos::stage(at, &files)?;
            repos::commit_staged(at, &run.subject(what), &tools.git)?;
        }
    }
    Ok(())
}

/// Pushes the staging clone's commits, off the host thread, never forced and to the default
/// branch only. A push whose answer was lost, which GitHub has, succeeded.
pub(crate) fn push(run: &Move, tools: &Tools) -> Result<()> {
    let repository = run
        .repository
        .as_ref()
        .context("A local move has no push")?;
    let at = run.staging.path();
    let remote = Remote {
        url: &repository.push_url,
        branch: &repository.branch,
    };
    let Err(error) = repos::push(at, &remote, &|path| Some(rule_set(path)), &tools.git) else {
        return Ok(());
    };
    let there = github_access::remote_head(at, remote.url, remote.branch, &tools.git);
    match there
        .ok()
        .flatten()
        .is_some_and(|there| Some(there) == head(at).ok().flatten())
    {
        true => Ok(()),
        false => Err(error.context("GitHub did not take the push, so nothing switched")),
    }
}

impl Host {
    /// The Spaces on this Mac stored with `space_id`, which move together, that one first.
    fn stored_with(&self, space_id: &SpaceId) -> Result<Vec<Space>> {
        let asked = self.space(space_id)?.space;
        let others = (self.spaces()?.into_iter().map(|record| record.space))
            .filter(|s| s.id != asked.id && s.storage.repository == asked.storage.repository);
        let others: Vec<Space> = others.collect();
        Ok([vec![asked], others].concat())
    }
    /// The refusals that come from what is being moved rather than where it goes.
    fn refuse_moving(&self, moved: &[Space]) -> Result<()> {
        let from = &moved[0].storage.repository;
        ensure!(
            self.settings_key().is_none() || self.settings_key() != Self::storage_key(from),
            "{} is also this Mac’s settings repository. Storage that holds your settings can’t move yet.",
            from.key()
        );
        ensure!(
            !self.linear.running(),
            "An import from Linear is running. Wait for it to finish, then move."
        );
        let recipes = self.recipes()?;
        for space in moved {
            for recipe in recipes.iter().filter(|r| r.space_id == space.id) {
                let last = self.job_runs(&recipe.key(), 1)?;
                ensure!(
                    !last
                        .first()
                        .is_some_and(|r| r.outcome == RunOutcome::Running),
                    "{}’s job {} is running. Wait for it to finish or stop it, then move.",
                    space.name,
                    recipe.name
                );
            }
        }
        // Only a pass numbers a new issue, and a temporary ID means nothing in another tracker.
        if let Ok(tracker) = self.tracker(&moved[0]) {
            let ids = self.store(&moved[0], &tracker.prefix).ids();
            ensure!(
                !ids.iter().any(IssueId::is_temporary),
                "{} has new issues with no number yet. Sync them, then move.",
                from.key()
            );
        }
        Ok(())
    }
    /// Refuses what needs nothing from GitHub, before the repository the human named is read.
    pub(crate) fn precheck_storage_move(
        &self,
        space_id: &SpaceId,
        repository: Option<&str>,
    ) -> Result<()> {
        self.refuse_moving(&self.stored_with(space_id)?)?;
        if let Some(input) = repository
            && let Ok((owner, name, _)) = github_access::candidates(input, "https")
        {
            self.refuse_brain(&format!("{owner}/{name}"))?;
        }
        Ok(())
    }
    fn refuse_brain(&self, name: &str) -> Result<()> {
        match self.space_with_brain(name)? {
            Some(space) => bail!(
                "{name} is {space}’s brain. A repository is a brain or storage, not both, because a brain changes only through reviewed pull requests."
            ),
            None => Ok(()),
        }
    }
    /// Whether the storage holds any issue.
    fn storage_has_issues(&self, storage: &StorageRepository) -> Result<bool> {
        let folder = self.storage_folder(storage);
        match Self::storage_key(storage) {
            None => Ok(!storage::files_under(&folder.join(ISSUES))?.is_empty()),
            Some(key) => {
                let record = Record::read(&self.db, &key)?;
                let commit = record
                    .context("The storage repository has no record")?
                    .commit;
                storage::has_issues(&folder, &commit)
            }
        }
    }
    /// Whether the target holds, under the paths it moved, only what this Mac pushed in an
    /// earlier move of the same storage that never switched: `None` when it does not, and else
    /// whether that move carried the tracker. One that did not took on the target's tracker,
    /// whose files are the target's own and never this Mac's to replace.
    fn left_by_earlier_move(&self, moved: &[Space], staged: &Staged) -> Result<Option<bool>> {
        let at = staged.staging.path();
        let from = moved[0].storage.repository.key();
        let target = storage::github(&staged.check.repository.name).key();
        let pushed: Vec<(String, bool)> = (self.db.prepare(
            "SELECT commit_id,with_tracker FROM storage_moves WHERE source=?1 AND target=?2",
        )?)
        .query_map(params![from, target], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
        let unchanged = |commit: &str, with_tracker: bool| {
            let diff = ["diff", "--quiet", commit, "HEAD", "--"];
            let roots = moved_roots(moved, with_tracker);
            let paths: Vec<&str> = roots.iter().map(String::as_str).collect();
            runtime::git_answer(at, &[&diff[..], &paths].concat())
                .is_ok_and(|answer| answer.status.success())
        };
        Ok(pushed
            .into_iter()
            .find(|(commit, with_tracker)| {
                publish::is_ancestor(at, commit, "HEAD") && unchanged(commit, *with_tracker)
            })
            .map(|(_, with_tracker)| with_tracker))
    }
    /// Every refusal of moving `moved` to the GitHub repository `staged`, or with none to this
    /// Mac, and what the move would do. Writes nothing.
    fn plan_storage_move(&self, moved: &[Space], staged: Option<&Staged>) -> Result<Plan> {
        self.refuse_moving(moved)?;
        let space = &moved[0];
        let from = &space.storage.repository;
        let status = self.storage_status(&space.id)?;
        let leaves = Self::storage_key(from).map(|_| from.key());
        let Some(staged) = staged else {
            ensure!(
                leaves.is_some(),
                "{} is already stored on this Mac.",
                names(moved)
            );
            let new_tracker = match self.tracker(space) {
                Ok(_) => None,
                Err(_) => {
                    let taken: Vec<String> = (self.trackers()?.into_iter())
                        .map(|(_, tracker)| tracker.prefix)
                        .collect();
                    Some(TrackerConfig::new(propose_prefix(&space.name, &taken)))
                }
            };
            return Ok(Plan {
                answer: StorageMove {
                    spaces: moved.iter().map(|s| s.name.clone()).collect(),
                    repository: None,
                    prefix: status.prefix,
                    issues: status.issues,
                    adopts: None,
                    leaves,
                },
                roots: moved_roots(moved, true),
                new_tracker,
            });
        };
        let repository = &staged.check.repository;
        let name = &repository.name;
        ensure!(
            !from.key().eq_ignore_ascii_case(name),
            "{} is already stored in {name}.",
            names(moved)
        );
        self.refuse_brain(name)?;
        repos::refuse_another(&self.home, repository)?;
        let to = storage::github(name);
        let at = staged.staging.path();
        let left = self.left_by_earlier_move(moved, staged)?;
        let tracker_left = left == Some(true);
        // A tracker set up here and not pushed yet counts as much as one on GitHub.
        let recorded: Option<TrackerConfig> = (self.db)
            .query_row(
                "SELECT data FROM trackers WHERE storage=?1",
                [to.key()],
                |r| decode(r, 0),
            )
            .optional()?;
        let theirs = (staged.tracker.clone().or(recorded)).filter(|_| !tracker_left);
        if theirs.is_none() && !tracker_left {
            ensure!(
                storage::files_under(&at.join(ISSUES))?.is_empty(),
                "{name} holds issues but no tracker. Choose another repository."
            );
        }
        ensure!(
            theirs.is_none() || !self.storage_has_issues(from)?,
            "{name} is already a tracker, and the storage you’re moving has issues. A tracker takes on only storage with no issues, as two trackers’ IDs can’t be merged."
        );
        let prefix = (theirs.as_ref().map(|t| t.prefix.clone())).or(status.prefix.clone());
        if let Some(prefix) = &prefix {
            self.refuse_prefix_outside(prefix, &[&from.key(), &to.key()])?;
        }
        let stored_here = self.spaces()?.into_iter().map(|record| record.space);
        let here: Vec<Space> = stored_here.filter(|s| s.storage.repository == to).collect();
        for space in moved {
            let folder = &space.storage.folder;
            let on_github = at.join("spaces").join(folder).exists();
            ensure!(
                !(on_github && left.is_none()) && !here.iter().any(|s| &s.storage.folder == folder),
                "{name} already holds a Space in spaces/{folder}/. Choose another repository."
            );
        }
        let adopts = theirs.as_ref().map(|tracker| tracker.prefix.clone());
        Ok(Plan {
            answer: StorageMove {
                spaces: moved.iter().map(|s| s.name.clone()).collect(),
                repository: Some(repository.clone()),
                prefix: status.prefix,
                issues: status.issues,
                adopts,
                leaves,
            },
            roots: moved_roots(moved, theirs.is_none()),
            new_tracker: theirs,
        })
    }
    /// What moving the Spaces stored with `space_id` would do, or why it is refused.
    pub(crate) fn check_storage_move(
        &self,
        space_id: &SpaceId,
        staged: Option<&Staged>,
    ) -> Result<StorageMove> {
        let moved = self.stored_with(space_id)?;
        Ok(self.plan_storage_move(&moved, staged)?.answer)
    }
    /// The recipes of "One runner" jobs that move to GitHub with no runner, naming this Mac as
    /// `save_job` would have: a job stored on GitHub runs only on its runner's Mac.
    fn jobs_given_a_runner(
        &self,
        moved: &[Space],
        to: &StorageRepository,
    ) -> Result<Vec<(Space, Recipe)>> {
        let from_local = matches!(moved[0].storage.repository, StorageRepository::Local { .. });
        if !from_local || matches!(to, StorageRepository::Local { .. }) {
            return Ok(vec![]);
        }
        let mut given = vec![];
        for mut recipe in self.recipes()? {
            let space = moved.iter().find(|s| s.id == recipe.space_id);
            if let Some(space) = space
                && recipe.sharing == Sharing::OneRunner
                && recipe.runner.is_none()
            {
                recipe.runner = Some(Runner {
                    login: self.gh_login.clone().unwrap_or_default(),
                    machine: sync::machine_name(),
                });
                given.push((space.clone(), recipe));
            }
        }
        Ok(given)
    }
    /// The files that move as other than the source holds them.
    fn rewritten_files(
        &self,
        moved: &[Space],
        to: &StorageRepository,
        new_tracker: Option<&TrackerConfig>,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut rewritten = BTreeMap::new();
        for (space, recipe) in self.jobs_given_a_runner(moved, to)? {
            let file = jobs::recipe_file(&space.storage.folder, &recipe.id);
            rewritten.insert(file, jobs::recipe_yaml(&recipe)?);
        }
        if let Some(tracker) = new_tracker.filter(|_| matches!(to, StorageRepository::Local { .. }))
        {
            rewritten.insert(TRACKER.to_owned(), storage::tracker_yaml(tracker)?);
        }
        Ok(rewritten)
    }
    /// Starts a move, every refusal run again, with `staged` as the target or a new local
    /// repository without it. No sync pass starts until `end_storage_move`.
    pub(crate) fn begin_storage_move(
        &mut self,
        space_id: &SpaceId,
        staged: Option<Staged>,
    ) -> Result<Move> {
        ensure!(!self.storage_moving, "Another storage move is running.");
        let spaces = self.stored_with(space_id)?;
        let plan = self.plan_storage_move(&spaces, staged.as_ref())?;
        drop(sync::no_pass_running(&self.sync_passes)?);
        let (to, repository, staging, scanner) = match staged {
            Some(staged) => {
                let repository = staged.check.repository;
                let scanner = scan::Scanner::bundled(&self.home, self.scan_path.clone())?;
                let to = storage::github(&repository.name);
                (to, Some(repository), staged.staging, Some(scanner))
            }
            None => {
                let staging = tempfile::Builder::new()
                    .prefix("shared-staging-")
                    .tempdir_in(&self.home)?;
                let to = StorageRepository::Local { id: new_id() };
                (to, None, staging, None)
            }
        };
        let rewritten = self.rewritten_files(&spaces, &to, plan.new_tracker.as_ref())?;
        let source = self.storage_folder(&spaces[0].storage.repository);
        self.storage_moving = true;
        Ok(Move {
            spaces,
            to,
            repository,
            source,
            staging,
            roots: plan.roots,
            rewritten,
            new_tracker: plan.new_tracker,
            scanner,
        })
    }
    /// The whole of a move on this thread, as a host with no service makes one.
    pub(crate) fn move_storage(
        &mut self,
        space_id: &SpaceId,
        repository: Option<&str>,
        tools: &Tools,
    ) -> Result<SpaceRecord> {
        self.precheck_storage_move(space_id, repository)?;
        let staged = repository
            .map(|input| storage::stage(&self.home, input, tools, &self.space_ids()?))
            .transpose()?;
        let run = self.begin_storage_move(space_id, staged)?;
        let moved = (|| {
            copy(&run, tools)?;
            if run.pushes() {
                self.note_push(&run)?;
                push(&run, tools)?;
            }
            self.switch_storage(run)
        })();
        self.end_storage_move();
        self.finish_storage_move(moved?)
    }
    /// Ends a move, however it went: sync passes start again.
    pub(crate) fn end_storage_move(&mut self) {
        self.storage_moving = false;
    }
    /// Records what `push` is about to push, before it does, so that files reaching GitHub for
    /// a move that never switched are known as this Mac's own.
    pub(crate) fn note_push(&self, run: &Move) -> Result<()> {
        let commit = head(run.staging.path())?.context("The staging clone has a commit")?;
        self.db.execute(
            "INSERT OR IGNORE INTO storage_moves(source,target,commit_id,with_tracker) VALUES (?1,?2,?3,?4)",
            params![
                run.spaces[0].storage.repository.key(),
                run.to.key(),
                commit,
                run.new_tracker.is_none()
            ],
        )?;
        Ok(())
    }
    /// Switches the Spaces to the storage `run` copied them to, once GitHub has it: refused when
    /// anything moved since, and otherwise one transaction, whose commit is the move. It records
    /// the folder renames for `finish_storage_move`, or the next start, to make.
    pub(crate) fn switch_storage(&mut self, run: Move) -> Result<Switched> {
        let passes = self.sync_passes.clone();
        let _no_pass = sync::no_pass_running(&passes)?;
        let asked = run.spaces[0].id.clone();
        let moved = self.stored_with(&asked)?;
        let ids = |spaces: &[Space]| {
            let mut ids: Vec<SpaceId> = spaces.iter().map(|s| s.id.clone()).collect();
            ids.sort();
            ids
        };
        ensure!(ids(&moved) == ids(&run.spaces), CHANGED);
        self.refuse_moving(&moved)?;
        let from = moved[0].storage.repository.clone();
        let prefix = (run.new_tracker.clone().or(self.tracker(&moved[0]).ok()))
            .map(|tracker| tracker.prefix);
        if let Some(prefix) = &prefix {
            self.refuse_prefix_outside(prefix, &[&from.key(), &run.to.key()])?;
        }
        if let Some(repository) = &run.repository {
            self.refuse_brain(&repository.name)?;
        }
        let rewritten = self.rewritten_files(&moved, &run.to, run.new_tracker.as_ref())?;
        let files = files_to_move(&run.source, &run.roots, &rewritten)?;
        ensure!(run.changes(&files)?.is_empty(), CHANGED);
        // A move that takes on the target's tracker compares no issue, and leaves none behind.
        let takes_issues = run.roots.iter().any(|root| root == ISSUES);
        ensure!(takes_issues || !self.storage_has_issues(&from)?, CHANGED);
        let clone = self.storage_folder(&run.to);
        // A clone already here, of a repository with another use, takes the files in place and
        // reaches GitHub's head at its next pass; else the staging clone becomes the clone.
        let placed = clone.exists();
        let at = if placed {
            clone.as_path()
        } else {
            run.staging.path()
        };
        if placed {
            for (path, content) in &files {
                shared::write_atomically(&clone.join(path), content)?;
            }
        }
        let synced_at = head(at)?.context("The target has a commit")?;
        let given = self.jobs_given_a_runner(&moved, &run.to)?;
        let staged_name = (run.staging.path().file_name())
            .context("A staging folder has a name")?
            .to_string_lossy()
            .into_owned();
        let to_path = (clone.strip_prefix(self.home.join(repos::FOLDER))?.to_str())
            .context("A clone's path is UTF-8")?
            .to_owned();
        let backed_up = format!(
            "{}-{}",
            from.key(),
            chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S%.3fZ")
        );
        self.atomically(|host| {
            let db = &host.db;
            if let Some(repository) = &run.repository {
                let key = repos::key(&repository.name);
                if Record::read(db, &key)?.is_none() {
                    let held = repos::files_at(at, &synced_at, &scan::scanned)?;
                    Record::restart_within(db, &key, &synced_at, &held, &held, &scan::scanned)?;
                }
                for (path, content) in &files {
                    db.execute(
                        "INSERT INTO sync_record(repository,path,synced,written) VALUES (?1,?2,?3,?3) ON CONFLICT(repository,path) DO UPDATE SET synced=excluded.synced, written=excluded.written",
                        params![key, path, content],
                    )?;
                }
                db.execute(
                    "INSERT OR REPLACE INTO storage_repositories VALUES (?1,?2)",
                    params![key, encode(repository)?],
                )?;
            }
            rekey_issues(db, &from.key(), &run.to.key(), run.new_tracker.as_ref())?;
            let old_key = repos::key(&from.key());
            for table in SYNC_TABLES {
                db.execute(&format!("DELETE FROM {table} WHERE repository=?1"), [&old_key])?;
            }
            db.execute("DELETE FROM storage_repositories WHERE key=?1", [&old_key])?;
            for space in &moved {
                let mut space = space.clone();
                space.storage.repository = run.to.clone();
                db.execute(
                    "UPDATE spaces SET data=?2 WHERE id=?1",
                    params![space.id.as_str(), encode(&space)?],
                )?;
            }
            for (_, recipe) in &given {
                db.execute(
                    "UPDATE jobs SET data=?2 WHERE key=?1",
                    params![recipe.key().as_str(), encode(recipe)?],
                )?;
            }
            db.execute("DELETE FROM storage_moves WHERE source=?1", [from.key()])?;
            let rename = "INSERT OR REPLACE INTO repo_moves(from_path,to_path) VALUES (?1,?2)";
            if !placed {
                db.execute(rename, params![format!("../{staged_name}"), to_path])?;
            }
            if let StorageRepository::Local { .. } = from {
                db.execute(rename, params![from.key(), format!("../backups/{backed_up}")])?;
            }
            Ok(())
        })?;
        if !placed {
            // The rename now belongs to `repo_moves`, and only it may remove this folder.
            let _ = run.staging.keep();
        }
        let place = match &run.to {
            StorageRepository::Local { .. } => "this Mac".to_owned(),
            repository => repository.key(),
        };
        Ok(Switched {
            asked,
            from,
            moved: format!("{} moved to {place}", names(&moved)),
        })
    }
    /// Makes a switched move's renames, which the next start makes after a stop, and reads the
    /// moved tracker into the index. A failure here is of the rest: the Spaces have moved.
    pub(crate) fn finish_storage_move(&mut self, switched: Switched) -> Result<SpaceRecord> {
        self.end_storage_move();
        self.storage_synced
            .remove(&repos::key(&switched.from.key()));
        let finished = repos::finish_moves(&self.db, &self.home)
            .and_then(|()| self.space(&switched.asked))
            .and_then(|record| {
                if let Ok(tracker) = self.tracker(&record.space) {
                    self.reindex_tracker(&record.space, &tracker.prefix)?;
                }
                Ok(record)
            });
        finished.with_context(|| format!("{}. The next start finishes the rest", switched.moved))
    }
}

/// Moves a tracker's rows to its new key, or, when `new_tracker` takes its place, removes them,
/// as storage that takes on another tracker has no issue they could describe.
fn rekey_issues(
    db: &Connection,
    from: &str,
    to: &str,
    new_tracker: Option<&TrackerConfig>,
) -> Result<()> {
    for table in ISSUE_TABLES {
        match new_tracker {
            Some(_) => db.execute(&format!("DELETE FROM {table} WHERE storage=?1"), [from])?,
            None => db.execute(
                &format!("UPDATE OR REPLACE {table} SET storage=?2 WHERE storage=?1"),
                [from, to],
            )?,
        };
    }
    for table in COLLABORATOR_TABLES {
        db.execute(&format!("DELETE FROM {table} WHERE storage=?1"), [from])?;
    }
    match new_tracker {
        Some(tracker) => {
            db.execute("DELETE FROM trackers WHERE storage=?1", [from])?;
            db.execute(
                "INSERT OR IGNORE INTO trackers VALUES (?1,?2)",
                params![to, encode(tracker)?],
            )?;
        }
        None => {
            db.execute(
                "UPDATE OR REPLACE trackers SET storage=?2 WHERE storage=?1",
                [from, to],
            )?;
            for table in ["issue_index", "issue_notes"] {
                db.execute(
                    &format!(
                        "UPDATE {table} SET data=json_set(data,'$.tracker',?1) WHERE storage=?1"
                    ),
                    [to],
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
