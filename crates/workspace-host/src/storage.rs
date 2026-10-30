//! Storage repositories, where Spaces' definitions and their tracker live (OpenSpec change
//! `spaces`, stage 3). Local storage is a Git repository at `repos/local-<id>/` with no remote,
//! committed to on every change and never pushed. GitHub storage is a clone under `repos/` that
//! its one sync engine runs with two rule sets: Spaces, over the folders of the Spaces this Mac
//! added, and the tracker, over `wiffletree-issues.yaml` with `issues/` reserved for
//! `shared-issues`. The store keeps each Space as the host uses it; the files are what travel.
use crate::*;
use publish::Tools;
use repos::{Content, Record, Remote, RuleSet};
use serde::Deserialize;
use std::collections::BTreeMap;
use tempfile::TempDir;

/// The tracker's config at a storage repository's root.
pub(crate) const TRACKER: &str = "wiffletree-issues.yaml";
/// Where `shared-issues` keeps one folder per issue.
const ISSUES: &str = "issues";
const SPACE_FORMAT: u32 = 1;
/// How often a storage repository is fetched, and once it holds a tracker.
const CADENCE_MS: i64 = 300_000;
pub(crate) const TRACKER_CADENCE_MS: i64 = 60_000;

/// A Space's definition file, relative to its storage repository.
fn space_file(folder: &str) -> String {
    format!("spaces/{folder}/space.yaml")
}

/// `space.yaml` as written: the Space without its storage, which is where the file is. The keys
/// this release does not know are kept in `extra`, here and in the Space's parts, so a write
/// puts each back where it was read.
#[derive(Serialize, Deserialize)]
struct SpaceFile {
    format: u32,
    id: SpaceId,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brain: Option<SpaceBrain>,
    #[serde(default)]
    repositories: Vec<String>,
    #[serde(default)]
    models: SpaceModels,
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(flatten)]
    extra: Extra,
}

/// The bytes of `space.yaml` for `space`, with the keys it was read with that this release does
/// not know.
fn space_yaml(space: &Space) -> Result<Vec<u8>> {
    let file = SpaceFile {
        format: SPACE_FORMAT,
        id: space.id.clone(),
        name: space.name.clone(),
        description: space.description.clone(),
        instructions: space.instructions.clone(),
        brain: space.brain.clone(),
        repositories: space.repositories.clone(),
        models: space.models.clone(),
        sources: space.sources.clone(),
        extra: space.extra.clone(),
    };
    Ok(serde_yaml::to_string(&file)?.into_bytes())
}

/// The Space a `space.yaml` defines, stored in `storage`.
fn read_space(content: &[u8], storage: SpaceStorage) -> Result<Space> {
    let file: SpaceFile = serde_yaml::from_slice(content).context("space.yaml can't be read")?;
    ensure!(
        file.format == SPACE_FORMAT,
        "This Space was written by a newer Wiffletree. Update to use it."
    );
    let space = Space {
        id: file.id,
        name: file.name,
        description: file.description,
        instructions: file.instructions,
        brain: file.brain,
        storage,
        repositories: file.repositories,
        models: file.models,
        sources: file.sources,
        extra: file.extra,
    };
    spaces::validate(&space)?;
    Ok(space)
}

fn tracker_yaml(tracker: &TrackerConfig) -> Result<Vec<u8>> {
    Ok(serde_yaml::to_string(tracker)?.into_bytes())
}

fn read_tracker(content: &[u8]) -> Result<TrackerConfig> {
    let tracker: TrackerConfig =
        serde_yaml::from_slice(content).context("wiffletree-issues.yaml can't be read")?;
    ensure!(
        tracker.format == TrackerConfig::FORMAT,
        "This tracker was written by a newer Wiffletree. Update to use it."
    );
    validate_prefix(&tracker.prefix)?;
    Ok(tracker)
}

/// Whether a storage repository's file at `path` can be used here; sync holds back one that
/// cannot, as the settings repository does.
pub(crate) fn check(path: &str, content: &[u8]) -> Result<()> {
    if path == TRACKER {
        read_tracker(content)?;
    } else if path.ends_with("/space.yaml") {
        let storage = SpaceStorage {
            repository: StorageRepository::Local { id: String::new() },
            folder: String::new(),
        };
        read_space(content, storage)?;
    }
    Ok(())
}

/// Rewrites a local tracker's issue IDs from `old` to `new`: each `issues/<old>-<n>/` folder,
/// and every `<old>-<n>` in the files under `issues/`.
fn rename_issues(folder: &Path, old: &str, new: &str) -> Result<()> {
    let issues = folder.join(ISSUES);
    let Ok(entries) = fs::read_dir(&issues) else {
        return Ok(());
    };
    for entry in entries {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(number) = issue_number(&name, old) {
            fs::rename(issues.join(&name), issues.join(format!("{new}-{number}")))?;
        }
    }
    for file in files_under(&issues)? {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let renamed = rename_ids(&text, old, new);
        if renamed != text {
            shared::write_atomically(&file, renamed.as_bytes())?;
        }
    }
    Ok(())
}

/// The number of an issue ID such as `SAGE-12` with the prefix `prefix`.
fn issue_number<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    let number = id.strip_prefix(prefix)?.strip_prefix('-')?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some(number)
}

/// `text` with each whole `<old>-<n>` written `<new>-<n>`; `XSAGE-1` and `SAGE-1a` are left.
fn rename_ids(text: &str, old: &str, new: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(old) {
        let before = rest[..at].chars().next_back();
        let after = &rest[at + old.len()..];
        let digits = after.strip_prefix('-').map_or(0, |number| {
            number.bytes().take_while(u8::is_ascii_digit).count()
        });
        let ends = after.get(1 + digits..).and_then(|tail| tail.chars().next());
        out.push_str(&rest[..at]);
        if digits > 0 && !before.is_some_and(word) && !ends.is_some_and(word) {
            out.push_str(new);
        } else {
            out.push_str(old);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Every file under `folder`, hidden ones aside, never following a link.
fn files_under(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    let Ok(entries) = fs::read_dir(folder) else {
        return Ok(files);
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            files.extend(files_under(&entry.path())?);
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

/// Commits `paths` of the local repository at `folder`, creating it first when it has none,
/// with the human's Git identity or, without one, Wiffletree's, as nothing here is pushed.
fn commit_local(folder: &Path, paths: &[&str], message: &str) -> Result<()> {
    if !folder.join(".git").exists() {
        runtime::git_output(folder, &["init", "--quiet", "--initial-branch=main"])?;
    }
    // A tracker with no issue has no `issues/` to add.
    let present: Vec<&str> = (paths.iter().copied())
        .filter(|path| folder.join(path).exists())
        .collect();
    runtime::git_output(folder, &[&["add", "--all", "--"][..], &present].concat())?;
    if runtime::git_answer(folder, &["diff", "--cached", "--quiet"])?
        .status
        .success()
    {
        return Ok(());
    }
    let named =
        |key| runtime::git_output(folder, &["config", key]).is_ok_and(|v| !v.trim().is_empty());
    let mut commit = vec!["-c", "commit.gpgsign=false"];
    if !(named("user.name") && named("user.email")) {
        commit.extend([
            "-c",
            "user.name=Wiffletree",
            "-c",
            "user.email=wiffletree@localhost",
        ]);
    }
    commit.extend(["commit", "--quiet", "--no-verify", "-m", message]);
    runtime::git_output(folder, &commit)?;
    Ok(())
}

/// Whether a tracker's clone holds any issue.
fn has_issues(folder: &Path) -> Result<bool> {
    Ok(!files_under(&folder.join(ISSUES))?.is_empty())
}

/// A GitHub repository checked as a Space's storage, with a clone of its branch in a staging
/// folder of the data folder that is removed when dropped.
pub(crate) struct Staged {
    pub(crate) check: StorageCheck,
    /// Its tracker's config as read, with the keys this release does not know.
    tracker: Option<TrackerConfig>,
    staging: TempDir,
}

/// Checks `input` as a storage repository and clones it, off the host thread. `added` names the
/// Spaces already on this Mac. Writes nothing that outlasts the check.
pub(crate) fn stage(home: &Path, input: &str, tools: &Tools, added: &[SpaceId]) -> Result<Staged> {
    let (repository, organization, staging) = repos::clone_to_staging(home, input, tools)?;
    let mut spaces = vec![];
    for entry in fs::read_dir(staging.path().join("spaces"))
        .into_iter()
        .flatten()
    {
        let folder = entry?.file_name().to_string_lossy().into_owned();
        let Ok(content) = fs::read(staging.path().join(space_file(&folder))) else {
            continue;
        };
        let storage = SpaceStorage {
            repository: github(&repository.name),
            folder: folder.clone(),
        };
        let space = read_space(&content, storage)
            .with_context(|| format!("The Space in spaces/{folder}/ can't be read"))?;
        spaces.push(SpaceListing {
            added: added.contains(&space.id),
            folder,
            id: space.id,
            name: space.name,
            description: space.description,
        });
    }
    spaces.sort_by(|a, b| a.folder.cmp(&b.folder));
    let tracker = match fs::read(staging.path().join(TRACKER)) {
        Ok(content) => Some(read_tracker(&content)?),
        Err(_) => None,
    };
    let check = StorageCheck {
        repository,
        organization,
        spaces,
        prefix: tracker.as_ref().map(|tracker| tracker.prefix.clone()),
    };
    Ok(Staged {
        check,
        tracker,
        staging,
    })
}

/// Gives a repository with no commit yet, as GitHub creates one empty, a first commit with no
/// file and pushes it, off the host thread, so its clone has a head and its passes a branch to
/// fetch. The Space's files follow in the next pass, scanned. Nothing for any other repository.
/// Runs after `Host::check_new_stored_space` passed for `requested`, the repository as the human
/// named it, so nothing is pushed for a Space that would be refused: the clone's folder is
/// checked here, and a repository GitHub knows by another name is refused, as the checks were
/// made for the requested one.
pub(crate) fn start_if_empty(
    home: &Path,
    staged: &Staged,
    requested: &str,
    tools: &Tools,
) -> Result<()> {
    let at = staged.staging.path();
    let head = ["rev-parse", "--verify", "--quiet", "HEAD"];
    if runtime::git_answer(at, &head)?.status.success() {
        return Ok(());
    }
    let repository = &staged.check.repository;
    repos::refuse_another(home, repository)?;
    ensure!(
        repository.name.eq_ignore_ascii_case(requested),
        "GitHub names this repository {}; choose it by that name",
        repository.name
    );
    let message = format!("Start Wiffletree storage · {}", sync::machine_name());
    let commit = ["commit", "--quiet", "--allow-empty", "--no-verify", "-m"];
    github_access::run(&tools.git, at, &[&commit[..], &[message.as_str()]].concat())?;
    let remote = Remote {
        url: &repository.push_url,
        branch: &repository.branch,
    };
    repos::push(at, &remote, &|_| None, &tools.git)
}

/// The storage repository `owner/name` on GitHub.
pub(crate) fn github(name: &str) -> StorageRepository {
    let (owner, name) = name.split_once('/').unwrap_or((name, ""));
    StorageRepository::GitHub {
        owner: owner.into(),
        name: name.into(),
    }
}

/// A new Space as the human's form gives it.
#[derive(Clone, Copy)]
pub(crate) struct NewSpace<'a> {
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
    pub(crate) instructions: &'a str,
    pub(crate) prefix: Option<&'a str>,
}

/// A repository's use on this Mac, of which it has one: a brain, or storage and settings.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Use {
    Brain,
    Storage,
    Settings,
}

/// What a storage repository's pass compares: the folders of the Spaces this Mac added, by
/// folder with their names, and the record.
pub(crate) struct Input {
    key: String,
    names: BTreeMap<String, String>,
    synced: BTreeMap<String, Content>,
    written: BTreeMap<String, Content>,
    two_machines: Vec<String>,
}

/// The files of a storage repository's paths a pass found, for the host thread to write and
/// record.
#[derive(Default)]
pub(crate) struct Found {
    key: String,
    /// Each file changed only on GitHub: this machine's content as read, then GitHub's.
    pulled: Vec<(String, Content, Content)>,
    alike: Vec<(String, Content)>,
    committed: Vec<(String, Content)>,
    paused: Vec<(String, Paused, Content)>,
    held: Vec<scan::Held>,
    /// Why a rule set's compare failed.
    failed: Vec<String>,
    /// The paths of the rule sets whose compare failed, whose notes stay as they were.
    failed_roots: Vec<String>,
}

/// A rule set comparing whole files under `roots` with the version last synced, as the settings
/// rule set does: what changed on both sides, changed outside Wiffletree or arrived in a form
/// this Mac cannot use is paused and kept here. It also claims `reserved`, which it leaves
/// alone for the rule set another change adds.
struct WholeFiles {
    roots: Vec<String>,
    reserved: Option<&'static str>,
    subject: String,
    synced: BTreeMap<String, Content>,
    written: BTreeMap<String, Content>,
    two_machines: Vec<String>,
    found: Found,
}
impl WholeFiles {
    fn new(
        input: &Input,
        roots: Vec<String>,
        reserved: Option<&'static str>,
        subject: String,
    ) -> Self {
        let mine = |map: &BTreeMap<String, Content>| -> BTreeMap<String, Content> {
            (map.iter())
                .filter(|(path, _)| under(&roots, path))
                .map(|(path, content)| (path.clone(), content.clone()))
                .collect()
        };
        Self {
            synced: mine(&input.synced),
            written: mine(&input.written),
            two_machines: (input.two_machines.iter())
                .filter(|path| under(&roots, path))
                .cloned()
                .collect(),
            roots,
            reserved,
            subject,
            found: Found::default(),
        }
    }
    fn compares(&self, path: &str) -> bool {
        under(&self.roots, path)
    }
    fn here(&self, clone: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut files = BTreeMap::new();
        for root in &self.roots {
            let at = clone.join(root);
            let found = match at.is_dir() {
                true => files_under(&at)?,
                false if at.is_file() => vec![at],
                false => vec![],
            };
            for file in found {
                let path = file
                    .strip_prefix(clone)?
                    .to_str()
                    .context("A path is not UTF-8")?;
                if self.compares(path) {
                    files.insert(path.to_owned(), fs::read(&file)?);
                }
            }
        }
        Ok(files)
    }
}
impl RuleSet for WholeFiles {
    fn claims(&self, path: &str) -> bool {
        self.compares(path)
            || self
                .reserved
                .is_some_and(|reserved| path.starts_with(reserved))
    }
    fn compare(&mut self, clone: &Path, remote: &str) -> Result<()> {
        let here = self.here(clone)?;
        let there = repos::files_at(clone, remote, &|path| self.compares(path))?;
        let paths: std::collections::BTreeSet<&String> = (self.synced.keys())
            .chain(here.keys())
            .chain(there.keys())
            .collect();
        for path in paths {
            let base = self.synced.get(path).cloned().flatten();
            let (mine, theirs) = (here.get(path).cloned(), there.get(path).cloned());
            let found = &mut self.found;
            if mine == theirs {
                if mine != base {
                    found.alike.push((path.clone(), mine));
                }
                continue;
            }
            let written = self.written.get(path).cloned().flatten();
            let refused = (theirs != base)
                .then(|| theirs.as_deref().and_then(|c| check(path, c).err()))
                .flatten();
            let machine = || "another machine".to_owned();
            let why = if mine != base && mine != written {
                Some(Paused::OutsideWiffletree)
            } else if let Some(error) = refused {
                Some(Paused::CannotBeRead {
                    machine: machine(),
                    reason: format!("{error:#}"),
                })
            } else if mine != base && theirs != base || self.two_machines.contains(path) {
                Some(Paused::TwoMachines { machine: machine() })
            } else {
                None
            };
            match why {
                Some(why) => found.paused.push((path.clone(), why, theirs)),
                None if mine == base => found.pulled.push((path.clone(), mine, theirs)),
                None => found.committed.push((path.clone(), mine)),
            }
        }
        Ok(())
    }
    fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)> {
        let committed = &self.found.committed;
        (!committed.is_empty()).then(|| {
            let message = format!("{} · {}", self.subject, sync::machine_name());
            (committed.clone(), message)
        })
    }
    fn failed(&mut self, error: anyhow::Error) {
        self.found.failed.push(format!("{error:#}"));
        let reserved = self.reserved.map(|r| r.trim_end_matches('/').to_owned());
        self.found
            .failed_roots
            .extend(self.roots.iter().cloned().chain(reserved));
    }
}

/// Whether `path` is one of `roots` or under one, hidden files aside.
fn under(roots: &[String], path: &str) -> bool {
    !path.split('/').any(|part| part.starts_with('.'))
        && (roots.iter()).any(|root| path == root || path.starts_with(&format!("{root}/")))
}

/// A storage repository's two rule sets for one pass.
pub(crate) struct RuleSets {
    spaces: WholeFiles,
    tracker: WholeFiles,
}
impl RuleSets {
    pub(crate) fn new(input: Input) -> Self {
        let roots = input
            .names
            .keys()
            .map(|folder| format!("spaces/{folder}"))
            .collect();
        let names: Vec<&str> = input.names.values().map(String::as_str).collect();
        let subject = format!("{}: changed the Space", names.join(", "));
        let spaces = WholeFiles::new(&input, roots, None, subject);
        let issues = Some("issues/");
        let tracker = WholeFiles::new(
            &input,
            vec![TRACKER.into()],
            issues,
            "Issues: changed the tracker".into(),
        );
        let mut rule_sets = Self { spaces, tracker };
        rule_sets.spaces.found.key = input.key;
        rule_sets
    }
    pub(crate) fn as_mut(&mut self) -> [&mut dyn RuleSet; 2] {
        [&mut self.spaces, &mut self.tracker]
    }
    /// What both found, with the files `held` back from the push left out of what was committed.
    pub(crate) fn found(self, held: Vec<scan::Held>) -> Found {
        let (mut found, tracker) = (self.spaces.found, self.tracker.found);
        found.pulled.extend(tracker.pulled);
        found.alike.extend(tracker.alike);
        found.committed.extend(tracker.committed);
        found.paused.extend(tracker.paused);
        found.failed.extend(tracker.failed);
        found.failed_roots.extend(tracker.failed_roots);
        found
            .committed
            .retain(|(path, _)| !held.iter().any(|h| &h.path == path));
        found.held = held;
        found
    }
}

/// A pass's input from the host thread for a GitHub storage repository that is not this Mac's
/// settings repository, whose pass runs with the settings rule set (see `sync`).
pub(crate) struct Pass {
    clone: PathBuf,
    repository: SettingsRepository,
    commit: String,
    input: Input,
    scanner: scan::Scanner,
    /// The host's one pass lock, which the settings pass and adopting a clone take too, so no
    /// two of them work in a clone at once.
    running: std::sync::Arc<std::sync::Mutex<()>>,
}

/// What a storage repository's pass did.
pub(crate) struct Passed {
    remote: String,
    pushed: Option<String>,
    error: Option<String>,
    found: Found,
}

/// Runs a storage repository's engine with its Space and tracker rule sets, scanning what it
/// pushes.
pub(crate) fn pass(pass: Pass, tools: &Tools) -> Result<Passed> {
    let Pass {
        clone,
        repository,
        commit,
        input,
        scanner,
        running,
    } = pass;
    let _running = running
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut rule_sets = RuleSets::new(input);
    let remote = Remote {
        url: &repository.push_url,
        branch: &repository.branch,
    };
    let outcome = repos::pass(
        &clone,
        remote,
        &commit,
        &mut rule_sets.as_mut(),
        Some(&scanner),
        &tools.git,
    )?;
    let head = runtime::git_output(&clone, &["rev-parse", "FETCH_HEAD"])?;
    let (pushed, error) = match outcome.pushed {
        Ok(pushed) => (pushed, None),
        Err(error) => (None, Some(error)),
    };
    Ok(Passed {
        remote: head.trim().to_owned(),
        pushed,
        error,
        found: rule_sets.found(outcome.held),
    })
}

impl Host {
    /// Where a storage repository is on this Mac.
    pub(crate) fn storage_folder(&self, repository: &StorageRepository) -> PathBuf {
        match repository {
            StorageRepository::Local { .. } => repos::local_folder(&self.home, &repository.key()),
            StorageRepository::GitHub { owner, name } => {
                repos::clone_folder(&self.home, &format!("{owner}/{name}"))
            }
        }
    }
    /// The record key of a GitHub storage repository, or `None` for local storage.
    fn storage_key(repository: &StorageRepository) -> Option<String> {
        match repository {
            StorageRepository::Local { .. } => None,
            StorageRepository::GitHub { .. } => Some(repos::key(&repository.key())),
        }
    }
    /// Gives every Space with local storage its repository, at the first start of this release
    /// and after one cut short: `space.yaml` and the tracker's config, committed. A repository
    /// whose first commit holds the Space's file is left as it is.
    pub(crate) fn ensure_local_storage(&self) -> Result<()> {
        for record in self.spaces()? {
            let space = &record.space;
            if !matches!(space.storage.repository, StorageRepository::Local { .. }) {
                continue;
            }
            let folder = self.storage_folder(&space.storage.repository);
            let committed = runtime::git_answer(
                &folder,
                &[
                    "cat-file",
                    "-e",
                    &format!("HEAD:{}", space_file(&space.storage.folder)),
                ],
            )
            .is_ok_and(|answer| answer.status.success());
            if !committed {
                self.write_local(space, &format!("Created the {} Space", space.name))?;
            }
        }
        Ok(())
    }
    /// Writes a Space with local storage to its repository, with the tracker's config when it
    /// has none, and commits them.
    fn write_local(&self, space: &Space, message: &str) -> Result<()> {
        let folder = self.storage_folder(&space.storage.repository);
        let file = space_file(&space.storage.folder);
        shared::write_atomically(&folder.join(&file), &space_yaml(space)?)?;
        let mut paths = vec![file.as_str()];
        if !folder.join(TRACKER).exists() {
            let tracker = self.tracker(space)?;
            shared::write_atomically(&folder.join(TRACKER), &tracker_yaml(&tracker)?)?;
            paths.push(TRACKER);
        }
        commit_local(&folder, &paths, message)
    }
    /// Writes a file of a GitHub storage repository's clone and notes it for the next pass.
    fn write_stored(&self, key: &str, folder: &Path, path: &str, content: &[u8]) -> Result<()> {
        shared::write_atomically(&folder.join(path), content)?;
        Record::wrote(&self.db, key, path, Some(content))?;
        self.storage_written.borrow_mut().insert(key.to_owned());
        Ok(())
    }
    /// Writes a Space's `space.yaml` to its storage: committed at once in a local repository,
    /// and pushed by the next pass from a GitHub one.
    pub(crate) fn write_space_file(&self, space: &Space) -> Result<()> {
        let Some(key) = Self::storage_key(&space.storage.repository) else {
            return self.write_local(space, &format!("{}: changed the Space", space.name));
        };
        let folder = self.storage_folder(&space.storage.repository);
        let file = space_file(&space.storage.folder);
        self.write_stored(&key, &folder, &file, &space_yaml(space)?)
    }
    /// The prefixes the other storage repositories on this Mac use, with a Space of each.
    fn other_prefixes(&self, except: &str) -> Result<Vec<(String, Space)>> {
        let mut taken = vec![];
        for record in self.spaces()? {
            let storage = record.space.storage.repository.key();
            if !storage.eq_ignore_ascii_case(except)
                && !taken
                    .iter()
                    .any(|(_, s): &(String, Space)| s.storage.repository.key() == storage)
                && let Ok(tracker) = self.tracker(&record.space)
            {
                taken.push((tracker.prefix, record.space));
            }
        }
        Ok(taken)
    }
    /// Refuses `prefix` when another storage repository on this Mac uses it, naming its Space;
    /// a local one can be renamed to make way.
    pub(crate) fn refuse_used_prefix(&self, prefix: &str, storage: &str) -> Result<()> {
        let taken = self.other_prefixes(storage)?;
        let Some((_, space)) = taken.iter().find(|(taken, _)| taken == prefix) else {
            return Ok(());
        };
        match space.storage.repository {
            StorageRepository::Local { .. } => bail!(
                "The {} Space's local tracker uses the prefix {prefix}. Rename its prefix on its Space's page, then try again.",
                space.name
            ),
            StorageRepository::GitHub { .. } => bail!(
                "{} uses the prefix {prefix} for the {} Space's issues; choose a prefix no other storage repository on this Mac uses",
                space.storage.repository.key(),
                space.name
            ),
        }
    }
    /// Refuses a repository for `wanted` when it has a use on this Mac it cannot share: a brain
    /// is never storage or settings, and storage may also be the settings repository.
    pub(crate) fn refuse_second_use(&self, name: &str, wanted: Use) -> Result<()> {
        for record in self.spaces()? {
            let space = &record.space;
            if wanted != Use::Brain
                && let Some(SpaceBrain {
                    repository:
                        BrainRepository::GitHub {
                            owner, name: brain, ..
                        },
                    ..
                }) = &space.brain
                && format!("{owner}/{brain}").eq_ignore_ascii_case(name)
            {
                bail!(
                    "{name} is the {} Space's brain, which changes only through pull requests; choose another repository",
                    space.name
                );
            }
            if wanted == Use::Brain && space.storage.repository.key().eq_ignore_ascii_case(name) {
                bail!(
                    "{name} is the {} Space's storage; a brain needs a repository of its own",
                    space.name
                );
            }
        }
        let settings = self.settings().settings_repository;
        if wanted == Use::Brain && settings.is_some_and(|r| r.name.eq_ignore_ascii_case(name)) {
            bail!(
                "{name} is this Mac's settings repository; a brain needs a repository of its own"
            );
        }
        Ok(())
    }
    /// Refuses a Space from another Mac whose GitHub brain is its own storage or has another use
    /// here, as `UpdateSpace` refuses it from the human.
    fn refuse_brain_of(&self, space: &Space) -> Result<()> {
        let Some(SpaceBrain {
            repository: BrainRepository::GitHub { owner, name, .. },
            ..
        }) = &space.brain
        else {
            return Ok(());
        };
        let brain = format!("{owner}/{name}");
        ensure!(
            !brain.eq_ignore_ascii_case(&space.storage.repository.key()),
            "{brain} is the {} Space's storage; a brain needs a repository of its own",
            space.name
        );
        self.refuse_second_use(&brain, Use::Brain)
    }
    /// Refuses a file another Mac changed that this Mac cannot take: a tracker config whose
    /// prefix another storage repository here uses, or a Space whose brain has another use here.
    fn refuse_pulled(&self, storage: &StorageRepository, path: &str, content: &[u8]) -> Result<()> {
        if path == TRACKER {
            return self.refuse_used_prefix(&read_tracker(content)?.prefix, &storage.key());
        }
        for record in self.spaces()? {
            let stored = &record.space.storage;
            if &stored.repository == storage && path == space_file(&stored.folder) {
                self.refuse_brain_of(&read_space(content, stored.clone())?)?;
            }
        }
        Ok(())
    }
    /// `refuse_second_use` for a repository named as `owner/name` or by its URL.
    pub(crate) fn refuse_second_use_of(&self, input: &str, wanted: Use) -> Result<()> {
        match github_access::candidates(input, "https") {
            Ok((owner, name, _)) => self.refuse_second_use(&format!("{owner}/{name}"), wanted),
            Err(_) => Ok(()),
        }
    }
    /// Changes the prefix of the tracker that is the Space's storage repository. A local
    /// tracker's issue folders, IDs and references are rewritten and committed; a GitHub
    /// tracker's prefix changes only while it has no issue.
    pub fn set_tracker_prefix(
        &mut self,
        space_id: &SpaceId,
        prefix: &str,
    ) -> Result<TrackerConfig> {
        validate_prefix(prefix)?;
        let space = self.space(space_id)?.space;
        let current = self.tracker(&space)?;
        if current.prefix == prefix {
            return Ok(current);
        }
        let storage = space.storage.repository.key();
        self.refuse_used_prefix(prefix, &storage)?;
        let folder = self.storage_folder(&space.storage.repository);
        let tracker = TrackerConfig {
            prefix: prefix.into(),
            ..current.clone()
        };
        let content = tracker_yaml(&tracker)?;
        match Self::storage_key(&space.storage.repository) {
            None => {
                rename_issues(&folder, &current.prefix, prefix)?;
                shared::write_atomically(&folder.join(TRACKER), &content)?;
                let message = format!("Renamed the issue prefix {} to {prefix}", current.prefix);
                commit_local(&folder, &[TRACKER, ISSUES], &message)?;
            }
            Some(key) => {
                ensure!(
                    !has_issues(&folder)?,
                    "{storage} has issues, and a shared tracker's prefix changes only while it has none"
                );
                self.write_stored(&key, &folder, TRACKER, &content)?;
            }
        }
        self.db.execute(
            "UPDATE trackers SET data=?2 WHERE storage=?1",
            params![storage, encode(&tracker)?],
        )?;
        Ok(tracker)
    }
    /// Makes a GitHub storage repository joined without `wiffletree-issues.yaml` a tracker with
    /// `prefix`: commits only that file.
    pub fn set_up_issues(&mut self, space_id: &SpaceId, prefix: &str) -> Result<TrackerConfig> {
        validate_prefix(prefix)?;
        let space = self.space(space_id)?.space;
        let key = Self::storage_key(&space.storage.repository)
            .context("A Space with local storage always has a tracker")?;
        ensure!(
            self.tracker(&space).is_err(),
            "This storage repository already has a tracker"
        );
        let storage = space.storage.repository.key();
        let folder = self.storage_folder(&space.storage.repository);
        ensure!(
            !folder.join(TRACKER).exists(),
            "{storage} already has {TRACKER}; it is set up at the next sync"
        );
        self.refuse_used_prefix(prefix, &storage)?;
        let tracker = TrackerConfig::new(prefix.into());
        self.write_stored(&key, &folder, TRACKER, &tracker_yaml(&tracker)?)?;
        self.db.execute(
            "INSERT INTO trackers VALUES (?1,?2)",
            params![storage, encode(&tracker)?],
        )?;
        Ok(tracker)
    }
    /// Puts a staged clone at its folder under `repos/`, or keeps the clone already there, and
    /// records the repository as storage. A clone new to this Mac starts its record afresh.
    fn place_storage(&self, staged: Staged) -> Result<(String, PathBuf)> {
        let repository = &staged.check.repository;
        let key = repos::key(&repository.name);
        let clone = repos::clone_folder(&self.home, &repository.name);
        if !clone.exists() {
            fs::create_dir_all(clone.parent().context("A clone is in a folder")?)?;
            fs::rename(staged.staging.keep(), &clone)?;
        }
        let head = runtime::git_output(&clone, &["rev-parse", "HEAD"])?;
        let head = head.trim();
        if Record::read(&self.db, &key)?.is_none() {
            let all = |_: &str| true;
            let committed = repos::files_at(&clone, head, &all)?;
            Record::restart(&self.db, &key, head, &committed, &committed, &all)?;
        } else {
            // The settings repository's clone: its Space and tracker files start their record
            // as its head holds them, and what the record has already stays.
            for (path, content) in repos::files_at(&clone, head, &scan::scanned)? {
                self.db.execute(
                    "INSERT OR IGNORE INTO sync_record(repository,path,synced,written) VALUES (?1,?2,?3,?3)",
                    params![key, path, content],
                )?;
            }
        }
        self.db.execute(
            "INSERT OR IGNORE INTO storage_repositories VALUES (?1,?2)",
            params![key, encode(repository)?],
        )?;
        Ok((key, clone))
    }
    /// A new Space stored in a GitHub repository, checked and cloned in `staged`. It takes the
    /// repository's tracker, or without one starts it with `prefix` or one proposed from its
    /// name. Its folder and, when new, the tracker's config are pushed by the next pass, each
    /// rule set in its own commit.
    pub(crate) fn create_stored_space(
        &mut self,
        new: &NewSpace,
        staged: Staged,
    ) -> Result<SpaceRecord> {
        let repository = staged.check.repository.clone();
        repos::refuse_another(&self.home, &repository)?;
        let spaces = staged.staging.path().join("spaces");
        let (space, tracker) = self.check_new_stored_space(
            new,
            &repository.name,
            staged.tracker.as_ref(),
            &|folder| spaces.join(folder).exists(),
        )?;
        let new_tracker = staged.tracker.is_none();
        // No pass works in the clone while the Space's files and record are put in it.
        let passes = self.sync_passes.clone();
        let _no_pass = sync::no_pass_running(&passes)?;
        // The first Space here in a repository that is already a tracker records its config.
        let recorded: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM trackers WHERE storage=?1)",
            [space.storage.repository.key()],
            |r| r.get(0),
        )?;
        let (key, clone) = self.place_storage(staged)?;
        let local = SpaceLocal::default();
        self.atomically(|host| {
            spaces::insert_space(&host.db, &space, &local, (!recorded).then_some(&tracker))?;
            host.write_space_file(&space)?;
            if new_tracker && !clone.join(TRACKER).exists() {
                host.write_stored(&key, &clone, TRACKER, &tracker_yaml(&tracker)?)?;
            }
            Ok(())
        })?;
        Ok(SpaceRecord { space, local })
    }
    /// `check_new_stored_space` for the repository the human named, before it is read: the
    /// refusals that need nothing from GitHub, run before an empty repository is started. Answers
    /// with the repository as `owner/name`.
    pub(crate) fn precheck_stored_space(&self, new: &NewSpace, input: &str) -> Result<String> {
        let (owner, name, _) = github_access::candidates(input, "https")?;
        let repository = format!("{owner}/{name}");
        self.check_new_stored_space(new, &repository, None, &|_| false)?;
        Ok(repository)
    }
    /// Every refusal of a new Space stored in the GitHub repository `repository`, which is a
    /// tracker as `theirs` when it has one, and the Space and tracker it would get: its name is
    /// unused and valid, the repository has no other use here, and the prefix, the repository's
    /// own or else `prefix` or one proposed from the name, is free here. Its folder is the first
    /// one `taken` does not hold. Writes nothing, so it runs before anything is pushed.
    pub(crate) fn check_new_stored_space(
        &self,
        new: &NewSpace,
        repository: &str,
        theirs: Option<&TrackerConfig>,
        taken: &dyn Fn(&str) -> bool,
    ) -> Result<(Space, TrackerConfig)> {
        let NewSpace {
            name,
            description,
            instructions,
            prefix,
        } = *new;
        self.ensure_unused_name(name, None)?;
        self.refuse_second_use(repository, Use::Storage)?;
        let storage = github(repository);
        let tracker = match (theirs, prefix) {
            (Some(theirs), Some(asked)) if theirs.prefix != asked => bail!(
                "{repository} is already a tracker with the prefix {}, which its Spaces share",
                theirs.prefix
            ),
            (Some(theirs), _) => theirs.clone(),
            (None, Some(asked)) => TrackerConfig::new(asked.into()),
            (None, None) => {
                let taken: Vec<String> = (self.other_prefixes(&storage.key())?.into_iter())
                    .map(|(prefix, _)| prefix)
                    .collect();
                TrackerConfig::new(propose_prefix(name, &taken))
            }
        };
        validate_prefix(&tracker.prefix)?;
        self.refuse_used_prefix(&tracker.prefix, &storage.key())?;
        let base = settings::slug(name.trim(), "space");
        let folder = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base}-{n}"),
            })
            .find(|folder| !taken(folder))
            .unwrap_or(base);
        let space = Space {
            id: SpaceId(new_id()),
            name: name.trim().into(),
            description: description.into(),
            instructions: instructions.into(),
            brain: None,
            storage: SpaceStorage {
                repository: storage,
                folder,
            },
            repositories: vec![],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        };
        spaces::validate(&space)?;
        Ok((space, tracker))
    }
    /// Adds the ticked Spaces of a GitHub storage repository, checked and cloned in `staged`,
    /// with where each one's brain is on this Mac. Refused when its tracker's prefix is used by
    /// another storage repository here.
    pub(crate) fn join_spaces(
        &mut self,
        staged: Staged,
        ticked: &[JoinSpace],
    ) -> Result<Vec<SpaceRecord>> {
        ensure!(!ticked.is_empty(), "Tick at least one Space to add");
        let repository = staged.check.repository.clone();
        self.refuse_second_use(&repository.name, Use::Storage)?;
        repos::refuse_another(&self.home, &repository)?;
        let storage = github(&repository.name);
        if let Some(prefix) = &staged.check.prefix {
            self.refuse_used_prefix(prefix, &storage.key())?;
        }
        let mut joined = vec![];
        for tick in ticked {
            let listing = (staged.check.spaces.iter())
                .find(|listing| listing.folder == tick.folder)
                .with_context(|| {
                    format!(
                        "{} holds no Space in spaces/{}/",
                        repository.name, tick.folder
                    )
                })?;
            ensure!(
                self.find_space(&listing.id)?.is_none(),
                "{} is already on this Mac",
                listing.name
            );
            self.ensure_unused_name(&listing.name, None)?;
            let content = fs::read(staged.staging.path().join(space_file(&tick.folder)))?;
            let space = read_space(
                &content,
                SpaceStorage {
                    repository: storage.clone(),
                    folder: tick.folder.clone(),
                },
            )?;
            self.refuse_brain_of(&space)?;
            let local = SpaceLocal {
                brain_path: tick.brain_path.clone(),
                notes: self.model_notes(&space)?,
                ..SpaceLocal::default()
            };
            joined.push(SpaceRecord { space, local });
        }
        let tracker = staged.tracker.clone();
        let has_tracker = self.tracker(&joined[0].space).is_ok();
        let passes = self.sync_passes.clone();
        let _no_pass = sync::no_pass_running(&passes)?;
        self.place_storage(staged)?;
        self.atomically(|host| {
            for (at, record) in joined.iter().enumerate() {
                let tracker = tracker.as_ref().filter(|_| at == 0 && !has_tracker);
                spaces::insert_space(&host.db, &record.space, &record.local, tracker)?;
            }
            Ok(())
        })?;
        Ok(joined)
    }
    /// The Space page's notes about a model the Space names that this Mac cannot run, and the
    /// Who runs what choice it uses instead.
    pub(crate) fn model_notes(&self, space: &Space) -> Result<Vec<String>> {
        let selection = self.model_selection()?;
        let instead = (selection.row(&Job::Coordinator))
            .map_or("this Mac's Who runs what choice".into(), |row| {
                row.main.first.short()
            });
        Ok([&space.models.threads, &space.models.jobs]
            .into_iter()
            .flatten()
            .filter(|profile| {
                !self
                    .detected
                    .get(&profile.provider)
                    .copied()
                    .unwrap_or(false)
            })
            .map(|profile| format!("{} is not available here; using {instead}", profile.short()))
            .collect())
    }
    /// What a pass of the GitHub storage repository `key` compares, or `None` when no Space on
    /// this Mac is stored there.
    pub(crate) fn storage_input(&self, key: &str) -> Result<Option<Input>> {
        let names: BTreeMap<String, String> = (self.spaces()?.into_iter())
            .filter(|r| Self::storage_key(&r.space.storage.repository).as_deref() == Some(key))
            .map(|r| (r.space.storage.folder, r.space.name))
            .collect();
        if names.is_empty() {
            return Ok(None);
        }
        let record =
            Record::read(&self.db, key)?.context("The storage repository has no record")?;
        let two_machines = (self.db.prepare(
            "SELECT path FROM sync_paused WHERE repository=?1 AND json_extract(why,'$.kind')='two_machines'",
        )?)
        .query_map([key], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        Ok(Some(Input {
            key: key.to_owned(),
            names,
            synced: record.synced,
            written: record.written,
            two_machines,
        }))
    }
    /// The GitHub storage repositories that sync on their own, being no settings repository,
    /// each with how often it is fetched.
    pub(crate) fn storage_cadences(&self) -> Result<Vec<(String, i64)>> {
        let settings = self.settings_key();
        let mut cadences = vec![];
        for record in self.spaces()? {
            let Some(key) = Self::storage_key(&record.space.storage.repository) else {
                continue;
            };
            if Some(&key) == settings.as_ref() || cadences.iter().any(|(k, _)| k == &key) {
                continue;
            }
            let cadence = match self.tracker(&record.space) {
                Ok(_) => TRACKER_CADENCE_MS,
                Err(_) => CADENCE_MS,
            };
            cadences.push((key, cadence));
        }
        Ok(cadences)
    }
    /// How often the settings repository is fetched: every minute once it is also a tracker.
    pub(crate) fn settings_cadence(&self) -> Result<i64> {
        let key = self.settings_key();
        for record in self.spaces()? {
            let stored = Self::storage_key(&record.space.storage.repository);
            if stored.is_some() && stored == key && self.tracker(&record.space).is_ok() {
                return Ok(TRACKER_CADENCE_MS);
            }
        }
        Ok(sync::CADENCE_MS)
    }
    /// The pass a storage repository that syncs on its own runs next.
    pub(crate) fn storage_pass(&self, key: &str) -> Result<Option<Pass>> {
        let Some(input) = self.storage_input(key)? else {
            return Ok(None);
        };
        let data: String = self.db.query_row(
            "SELECT data FROM storage_repositories WHERE key=?1",
            [key],
            |r| r.get(0),
        )?;
        let repository: SettingsRepository = serde_json::from_str(&data)?;
        let commit = Record::read(&self.db, key)?
            .context("The storage repository has no record")?
            .commit;
        Ok(Some(Pass {
            clone: repos::clone_folder(&self.home, &repository.name),
            repository,
            commit,
            input,
            scanner: scan::Scanner::bundled(&self.home, self.scan_path.clone())?,
            running: self.sync_passes.clone(),
        }))
    }
    /// Records a storage repository's own pass.
    pub(crate) fn finish_storage_pass(&mut self, passed: Result<Passed>) -> Result<()> {
        let passed = passed?;
        let key = passed.found.key.clone();
        if let Some(error) = &passed.error {
            eprintln!("Syncing {key}: {error}");
        }
        let commit = passed.pushed.clone().unwrap_or(passed.remote);
        let pushed = passed.pushed.is_some();
        self.finish_storage(passed.found, pushed)?;
        self.db.execute(
            "UPDATE sync_record SET synced=?2 WHERE repository=?1 AND path=''",
            params![key, commit.as_bytes()],
        )?;
        Ok(())
    }
    /// Writes what a storage repository's rule sets pulled, unless the host wrote it meanwhile,
    /// records what they synced, paused and held, and takes pulled Space definitions and tracker
    /// configs into the store. `pushed`: their commits reached GitHub.
    pub(crate) fn finish_storage(&mut self, found: Found, pushed: bool) -> Result<()> {
        let key = found.key.clone();
        for error in &found.failed {
            eprintln!("Syncing {key}: {error}");
        }
        let Some(record) = self.spaces()?.into_iter().find(|r| {
            Self::storage_key(&r.space.storage.repository).as_deref() == Some(key.as_str())
        }) else {
            return Ok(());
        };
        let storage = &record.space.storage.repository;
        let clone = self.storage_folder(storage);
        let mut pulled = vec![];
        let mut paused = found.paused;
        for (path, here, there) in found.pulled {
            if fs::read(clone.join(&path)).ok() != here {
                continue;
            }
            let refused = there
                .as_deref()
                .map(|c| self.refuse_pulled(storage, &path, c));
            if let Some(Err(error)) = refused {
                let machine = "another machine".into();
                let reason = format!("{error:#}");
                paused.push((path, Paused::CannotBeRead { machine, reason }, there));
                continue;
            }
            match &there {
                Some(content) => shared::write_atomically(&clone.join(&path), content)?,
                None => drop(fs::remove_file(clone.join(&path))),
            }
            pulled.push((path, there));
        }
        let mut synced = found.alike;
        synced.extend(found.committed.into_iter().filter(|_| pushed));
        // A rule set whose compare failed keeps its notes until a pass compares it again.
        let ours = |path: &str| scan::scanned(path) && !under(&found.failed_roots, path);
        let tx = self.db.unchecked_transaction()?;
        for (rows, written) in [(&pulled, "excluded.written"), (&synced, "written")] {
            for (path, content) in rows {
                tx.execute(
                    &format!("INSERT INTO sync_record(repository,path,synced,written) VALUES (?1,?2,?3,?3) ON CONFLICT(repository,path) DO UPDATE SET synced=excluded.synced, written={written}"),
                    params![key, path, content],
                )?;
            }
        }
        let was_paused: Vec<String> = (tx
            .prepare("SELECT path FROM sync_paused WHERE repository=?1")?)
        .query_map([&key], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        for path in was_paused.iter().filter(|path| ours(path)) {
            tx.execute(
                "DELETE FROM sync_paused WHERE repository=?1 AND path=?2",
                params![key, path],
            )?;
        }
        for (path, why, theirs) in &paused {
            tx.execute(
                "INSERT OR REPLACE INTO sync_paused(repository,path,why,theirs) VALUES (?1,?2,?3,?4)",
                params![key, path, serde_json::to_string(why)?, theirs],
            )?;
        }
        let was_held: Vec<String> = (tx
            .prepare("SELECT path FROM sync_held WHERE repository=?1")?)
        .query_map([&key], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        for path in was_held.iter().filter(|path| ours(path)) {
            tx.execute(
                "DELETE FROM sync_held WHERE repository=?1 AND path=?2",
                params![key, path],
            )?;
        }
        for held in &found.held {
            tx.execute(
                "INSERT INTO sync_held(repository,path,check_name,rule,reason) VALUES (?1,?2,?3,?4,?5)",
                params![key, held.path, held.check, held.rule, held.reason],
            )?;
        }
        tx.commit()?;
        for (path, content) in pulled {
            self.take_pulled(storage, &path, content.as_deref())?;
        }
        Ok(())
    }
    /// Takes a pulled `space.yaml` of a Space on this Mac, or the tracker's config, into the store.
    pub(crate) fn take_pulled(
        &mut self,
        storage: &StorageRepository,
        path: &str,
        content: Option<&[u8]>,
    ) -> Result<()> {
        let Some(content) = content else {
            return Ok(());
        };
        if path == TRACKER {
            let tracker = read_tracker(content)?;
            self.db.execute(
                "INSERT INTO trackers VALUES (?1,?2) ON CONFLICT(storage) DO UPDATE SET data=excluded.data",
                params![storage.key(), encode(&tracker)?],
            )?;
            return Ok(());
        }
        for record in self.spaces()? {
            let stored = &record.space.storage;
            if &stored.repository == storage && path == space_file(&stored.folder) {
                let space = read_space(content, stored.clone())?;
                ensure!(space.id == record.space.id, "{path} names another Space");
                let notes = self.model_notes(&space)?;
                let local = SpaceLocal {
                    notes,
                    ..record.local
                };
                self.db.execute(
                    "UPDATE spaces SET data=?2,local=?3 WHERE id=?1",
                    params![space.id.as_str(), encode(&space)?, encode(&local)?],
                )?;
            }
        }
        Ok(())
    }
    /// The settings repository's record key, `None` without one. Reads only this Mac's settings
    /// file, as the shared folder's recorder asks while the shared settings are read.
    pub(crate) fn settings_key(&self) -> Option<String> {
        shared::chosen(&self.home).map(|r| repos::key(&r.name))
    }
    /// One note per file a scan held back from a push.
    pub(crate) fn held_notes(&self) -> Result<Vec<WorkspaceNote>> {
        let rows: Vec<(String, scan::Held)> = (self.db.prepare(
            "SELECT repository,path,check_name,rule,reason FROM sync_held ORDER BY repository,path",
        )?)
        .query_map([], |r| {
            let held = scan::Held {
                path: r.get(1)?,
                check: r.get(2)?,
                rule: r.get(3)?,
                reason: r.get(4)?,
            };
            Ok((r.get(0)?, held))
        })?
        .collect::<rusqlite::Result<_>>()?;
        let storage: Vec<SettingsRepository> =
            (self.db.prepare("SELECT data FROM storage_repositories")?)
                .query_map([], |r| r.get::<_, String>(0))?
                .filter_map(|data| serde_json::from_str(&data.ok()?).ok())
                .collect();
        Ok(rows
            .into_iter()
            .map(|(key, held)| {
                let scan::Held {
                    path,
                    check,
                    rule,
                    reason,
                } = held;
                let repository = (storage.iter())
                    .find(|r| repos::key(&r.name) == key)
                    .map_or(key, |r| r.name.clone());
                let why = match (&rule, &reason) {
                    (Some(rule), _) => format!("the {check} check found a possible match of its {rule} rule"),
                    (None, Some(reason)) => format!("the {check} check could not run: {reason}"),
                    (None, None) => format!("the {check} check could not run"),
                };
                let text = format!(
                    "{path} in {repository} isn’t pushed: {why}. This Mac keeps using it. Edit the file to remove the value, or mark a false positive with gate:allow, and it is pushed at the next sync."
                );
                WorkspaceNote::Held(HeldFile {
                    repository,
                    path,
                    check,
                    rule,
                    text,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::{GitHub, NAME};
    use crate::test_support::{git, scan_tools};
    use std::sync::Arc;

    fn host() -> (TempDir, Host) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.scan_path = Some(SCAN_TOOLS.with(|bin| bin.path().into()));
        (home, host)
    }
    thread_local! {
        static SCAN_TOOLS: TempDir = scan_tools(true);
    }

    fn personal(host: &Host) -> Space {
        host.spaces().unwrap().remove(0).space
    }

    fn committed(folder: &Path) -> Vec<String> {
        let listed = git(folder, &["ls-tree", "-r", "--name-only", "HEAD"]);
        listed.lines().map(str::to_owned).collect()
    }

    /// The Sage Space as a teammate's Mac wrote it to `spaces/sage/space.yaml`.
    fn sage(description: &str) -> String {
        let space = Space {
            id: SpaceId("sage-id".into()),
            name: "Sage".into(),
            description: description.into(),
            instructions: "Name the ticket.".into(),
            brain: None,
            storage: SpaceStorage {
                repository: github(NAME),
                folder: "sage".into(),
            },
            repositories: vec!["Sage-Social/api".into()],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        };
        String::from_utf8(space_yaml(&space).unwrap()).unwrap()
    }

    /// The human's form for a new Space named `name`.
    fn form<'a>(name: &'a str, prefix: Option<&'a str>) -> NewSpace<'a> {
        NewSpace {
            name,
            description: "",
            instructions: "",
            prefix,
        }
    }

    /// A storage repository on GitHub holding Sage and, with `prefix`, a tracker.
    fn shared_storage(folder: &Path, prefix: Option<&str>) -> Arc<GitHub> {
        let github = Arc::new(GitHub::new(folder, true));
        github.commit("spaces/sage/space.yaml", sage("Secure family messaging"));
        if let Some(prefix) = prefix {
            github.commit(TRACKER, format!("format: 1\nprefix: {prefix}\n"));
        }
        github
    }

    fn join_sage(host: &mut Host, github: &Arc<GitHub>) -> Result<Vec<SpaceRecord>> {
        let staged = stage(&host.home, NAME, &github.tools(), &[])?;
        let ticked = [JoinSpace {
            folder: "sage".into(),
            brain_path: None,
        }];
        host.join_spaces(staged, &ticked)
    }

    /// Runs the pass of the storage repository `NAME`.
    fn sync(host: &mut Host, github: &Arc<GitHub>) {
        let key = repos::key(NAME);
        let pass = host.storage_pass(&key).unwrap().unwrap();
        host.finish_storage_pass(self::pass(pass, &github.tools()))
            .unwrap();
    }

    fn space_named(host: &Host, name: &str) -> Space {
        let spaces = host.spaces().unwrap().into_iter();
        spaces.map(|r| r.space).find(|s| s.name == name).unwrap()
    }

    #[test]
    fn a_new_space_gets_a_local_repository_whose_first_commit_holds_its_file_and_the_tracker() {
        let (home, mut host) = host();
        let space = host
            .create_space("Woodworking", "", "", Some("WOOD"))
            .unwrap()
            .space;
        let folder = host.storage_folder(&space.storage.repository);
        assert_eq!(folder.parent().unwrap(), host.home.join(repos::FOLDER));
        assert!(
            folder
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("local-")
        );
        assert_eq!(git(&folder, &["remote"]), "", "no remote");
        assert_eq!(
            committed(&folder),
            ["spaces/woodworking/space.yaml", TRACKER]
        );
        assert_eq!(git(&folder, &["rev-list", "--count", "HEAD"]), "1");
        let tracker = read_tracker(&fs::read(folder.join(TRACKER)).unwrap()).unwrap();
        assert_eq!(tracker.prefix, "WOOD");
        let file = fs::read(folder.join("spaces/woodworking/space.yaml")).unwrap();
        assert_eq!(read_space(&file, space.storage.clone()).unwrap(), space);
        // Personal, from the store's upgrade, gets its repository at start, once.
        let personal = host.storage_folder(&personal(&host).storage.repository);
        assert_eq!(
            committed(&personal),
            ["spaces/personal/space.yaml", TRACKER]
        );
        let heads = |host: &Host| {
            (host.spaces().unwrap().iter())
                .map(|r| {
                    git(
                        &host.storage_folder(&r.space.storage.repository),
                        &["rev-parse", "HEAD"],
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = heads(&host);
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(heads(&host), before, "a second start changes nothing");
    }

    #[test]
    fn repository_membership_is_written_to_space_yaml_and_syncs_with_the_space() {
        let (_home, mut host) = host();
        let space = personal(&host);
        host.set_repository_spaces("Sage-Social/api", std::slice::from_ref(&space.id))
            .unwrap();
        let folder = host.storage_folder(&space.storage.repository);
        let committed = git(&folder, &["show", "HEAD:spaces/personal/space.yaml"]);
        assert!(committed.contains("- Sage-Social/api"), "{committed}");
        // A shared Space's membership is pushed by its pass.
        let github_folder = tempfile::tempdir().unwrap();
        let github = shared_storage(github_folder.path(), Some("SAGE"));
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.set_repository_spaces("Sage-Social/watch-app", std::slice::from_ref(&sage.id))
            .unwrap();
        sync(&mut host, &github);
        let pushed = github.show("spaces/sage/space.yaml").unwrap();
        assert!(pushed.contains("- Sage-Social/watch-app"), "{pushed}");
    }

    #[test]
    fn a_duplicate_prefix_is_refused() {
        let (_home, mut host) = host();
        let prefix = host.tracker(&personal(&host)).unwrap().prefix;
        let error = host
            .create_space("Sage", "", "", Some(&prefix))
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("uses the prefix"),
            "{error:#}"
        );
        assert_eq!(host.spaces().unwrap().len(), 1);
    }

    /// Sage as a newer Wiffletree wrote it: unknown keys at the top, in its brain, models and
    /// brain repository, and in each of two sources.
    const FROM_A_NEWER_MAC: &str = "format: 1
id: sage-id
name: Sage
boards:
  weekly: true
brain:
  repository:
    kind: git_hub
    owner: Sage-Social
    name: sagehaven-docs
    mirror: true
  folder: second-brain
  cache: 7
models:
  future_default: opus
sources:
- kind: mcp
  server: linear
  tools: [list]
  note: first
- kind: cli
  command: gh
  prefixes: [gh issue]
  note: second
";

    fn read_newer() -> Space {
        let storage = SpaceStorage {
            repository: super::github(NAME),
            folder: "sage".into(),
        };
        read_space(FROM_A_NEWER_MAC.as_bytes(), storage).unwrap()
    }

    fn written(space: &Space) -> serde_yaml::Value {
        serde_yaml::from_slice(&space_yaml(space).unwrap()).unwrap()
    }

    #[test]
    fn unknown_keys_at_the_top_nested_and_in_list_items_survive_an_edit() {
        let space = Space {
            description: "Changed here".into(),
            ..read_newer()
        };
        let file = written(&space);
        assert_eq!(file["description"], "Changed here");
        assert_eq!(file["boards"]["weekly"], true);
        assert_eq!(file["brain"]["cache"], 7);
        assert_eq!(file["brain"]["repository"]["mirror"], true);
        assert_eq!(file["models"]["future_default"], "opus");
        assert_eq!(file["sources"][0]["note"], "first");
        assert_eq!(file["sources"][1]["note"], "second");
        // Read back by this release, the file is the same Space.
        let again = read_space(&space_yaml(&space).unwrap(), space.storage.clone()).unwrap();
        assert_eq!(again, space);
    }

    #[test]
    fn a_list_that_grows_or_shrinks_keeps_each_items_own_unknown_keys() {
        let mut space = read_newer();
        space.sources.remove(0);
        let file = written(&space);
        assert_eq!(file["sources"].as_sequence().unwrap().len(), 1);
        assert_eq!(file["sources"][0]["command"], "gh");
        assert_eq!(file["sources"][0]["note"], "second");
        space.sources.insert(
            0,
            Source::Mcp {
                server: "notion".into(),
                tools: vec!["search".into()],
                extra: Extra::new(),
            },
        );
        let file = written(&space);
        assert_eq!(file["sources"][0].get("note"), None, "a new item has none");
        assert_eq!(file["sources"][1]["note"], "second");
    }

    #[test]
    fn clearing_a_brain_drops_its_unknown_keys_and_writes_valid_yaml() {
        let space = Space {
            brain: None,
            ..read_newer()
        };
        let file = written(&space);
        assert_eq!(file.get("brain"), None);
        assert!(
            !String::from_utf8(space_yaml(&space).unwrap())
                .unwrap()
                .contains("cache")
        );
        let again = read_space(&space_yaml(&space).unwrap(), space.storage.clone()).unwrap();
        assert_eq!(again.brain, None);
        assert_eq!(again.extra, space.extra, "the top-level keys stay");
    }

    #[test]
    fn a_shared_trackers_unknown_keys_survive_a_prefix_change() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), None);
        github.commit(TRACKER, "format: 1\nprefix: SAGE\nstates: [triage, done]\n");
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.set_tracker_prefix(&sage.id, "SG").unwrap();
        sync(&mut host, &github);
        let tracker = github.show(TRACKER).unwrap();
        assert!(
            tracker.contains("prefix: SG") && tracker.contains("- triage"),
            "{tracker}"
        );
    }

    #[test]
    fn renaming_a_local_prefix_rewrites_its_issue_ids_folders_and_references() {
        let (_home, mut host) = host();
        let space = personal(&host);
        let folder = host.storage_folder(&space.storage.repository);
        let old = host.tracker(&space).unwrap().prefix;
        let issue = |id: &str, body: String| {
            let file = folder.join(format!("issues/{id}/issue.md"));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, body).unwrap();
        };
        issue(
            &format!("{old}-1"),
            format!("---\nid: {old}-1\nblocks: [{old}-2]\n---\nNot X{old}-1 or {old}-1a.\n"),
        );
        issue(
            &format!("{old}-2"),
            format!("---\nid: {old}-2\nblocked_by: [{old}-1]\n---\n"),
        );
        git(&folder, &["add", "--all"]);
        git(&folder, &["commit", "-q", "-m", "Two issues"]);
        host.set_tracker_prefix(&space.id, "ME").unwrap();
        let one = fs::read_to_string(folder.join("issues/ME-1/issue.md")).unwrap();
        assert_eq!(
            one,
            format!("---\nid: ME-1\nblocks: [ME-2]\n---\nNot X{old}-1 or {old}-1a.\n")
        );
        let two = fs::read_to_string(folder.join("issues/ME-2/issue.md")).unwrap();
        assert!(two.contains("blocked_by: [ME-1]"));
        assert!(!folder.join(format!("issues/{old}-1")).exists());
        assert_eq!(host.tracker(&space).unwrap().prefix, "ME");
        assert_eq!(git(&folder, &["status", "--porcelain"]), "");
    }

    #[test]
    fn a_join_refused_for_a_prefix_used_here_succeeds_after_the_local_rename() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        let personal = personal(&host);
        host.set_tracker_prefix(&personal.id, "SAGE").unwrap();
        let error = format!("{:#}", join_sage(&mut host, &github).unwrap_err());
        assert!(
            error.contains(
                "The Personal Space's local tracker uses the prefix SAGE. Rename its prefix"
            ),
            "{error}"
        );
        assert!(
            !repos::clone_folder(home.path(), NAME).exists(),
            "nothing is written"
        );
        host.set_tracker_prefix(&personal.id, "ME").unwrap();
        let joined = join_sage(&mut host, &github).unwrap();
        assert_eq!(joined[0].space.description, "Secure family messaging");
        assert_eq!(joined[0].space.storage.repository, super::github(NAME));
        let sage = space_named(&host, "Sage");
        assert_eq!(host.tracker(&sage).unwrap().prefix, "SAGE");
        assert!(
            repos::clone_folder(home.path(), NAME)
                .join("spaces/sage/space.yaml")
                .exists()
        );
    }

    #[test]
    fn a_shared_trackers_prefix_changes_only_while_it_has_no_issue() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.set_tracker_prefix(&sage.id, "SG").unwrap();
        sync(&mut host, &github);
        assert_eq!(github.show(TRACKER).unwrap(), "format: 1\nprefix: SG\n");
        let issue = repos::clone_folder(home.path(), NAME).join("issues/SG-1/issue.md");
        fs::create_dir_all(issue.parent().unwrap()).unwrap();
        fs::write(issue, "---\nid: SG-1\n---\n").unwrap();
        let error = host.set_tracker_prefix(&sage.id, "SAGE").unwrap_err();
        assert!(format!("{error:#}").contains("changes only while it has none"));
    }

    #[test]
    fn a_new_space_on_github_pushes_its_folder_and_the_new_trackers_config_a_commit_each() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (home, mut host) = host();
        let staged = stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        let new = NewSpace {
            instructions: "Name the ticket.",
            ..form("Sage", Some("SAGE"))
        };
        let space = host.create_stored_space(&new, staged).unwrap().space;
        assert_eq!(space.storage.folder, "sage");
        let key = repos::key(NAME);
        assert_eq!(
            host.storage_cadences().unwrap(),
            [(key, TRACKER_CADENCE_MS)]
        );
        sync(&mut host, &github);
        let machine = sync::machine_name();
        let subjects = github.log("%s");
        assert_eq!(subjects.len(), 3, "{subjects:?}");
        assert!(subjects[..2].contains(&format!("Issues: changed the tracker · {machine}")));
        assert!(subjects[..2].contains(&format!("Sage: changed the Space · {machine}")));
        for commit in ["main", "main~"] {
            let changed = git(&github.bare, &["show", "--format=", "--name-only", commit]);
            assert!(
                changed == TRACKER || changed == "spaces/sage/space.yaml",
                "{changed}"
            );
        }
        let pushed = github.show("spaces/sage/space.yaml").unwrap();
        assert!(pushed.contains("instructions: Name the ticket."));
        assert_eq!(github.show(TRACKER).unwrap(), "format: 1\nprefix: SAGE\n");
        assert!(repos::clone_folder(home.path(), NAME).join(".git").exists());
    }

    #[test]
    fn a_space_in_an_empty_repository_starts_it_and_pushes_its_folder() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), false));
        let (_home, mut host) = host();
        let tools = github.tools();
        let new = form("Sage", Some("SAGE"));
        let requested = host.precheck_stored_space(&new, NAME).unwrap();
        let staged = stage(&host.home, NAME, &tools, &[]).unwrap();
        start_if_empty(&host.home, &staged, &requested, &tools).unwrap();
        host.create_stored_space(&new, staged).unwrap();
        sync(&mut host, &github);
        assert_eq!(
            github.log("%s").len(),
            3,
            "the start and one commit per rule set"
        );
        assert!(github.show("spaces/sage/space.yaml").is_some());
        assert_eq!(github.show(TRACKER).unwrap(), "format: 1\nprefix: SAGE\n");
    }

    #[test]
    fn nothing_is_pushed_to_an_empty_repository_for_a_space_that_would_be_refused() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), false));
        let (home, host) = host();
        let tools = github.tools();
        let error = host.precheck_stored_space(&form("Personal", None), NAME);
        assert!(format!("{:#}", error.unwrap_err()).contains("Another Space is named"));
        let taken = host.tracker(&personal(&host)).unwrap().prefix;
        let error = host.precheck_stored_space(&form("Sage", Some(&taken)), NAME);
        assert!(format!("{:#}", error.unwrap_err()).contains("uses the prefix"));
        // A clone of another repository in the folder this one would take.
        let clone = repos::clone_folder(home.path(), NAME);
        fs::create_dir_all(&clone).unwrap();
        git(&clone, &["init", "-q"]);
        git(
            &clone,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/someone/else.git",
            ],
        );
        let requested = host
            .precheck_stored_space(&form("Sage", None), NAME)
            .unwrap();
        let staged = stage(&host.home, NAME, &tools, &[]).unwrap();
        let error = start_if_empty(&host.home, &staged, &requested, &tools).unwrap_err();
        assert!(format!("{error:#}").contains("holds a clone of another repository"));
        let branches = git(&github.bare, &["branch", "--list"]);
        assert_eq!(branches, "", "nothing is pushed");
    }

    #[test]
    fn a_storage_pass_waits_for_a_pass_or_adoption_already_in_the_clone() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let passes = host.sync_passes.clone();
        let held = passes.lock().unwrap();
        // While another pass or an adoption holds the lock, no Space is put in a clone.
        let staged = stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        let refused = host.create_stored_space(&form("Sage Watch", None), staged);
        assert!(format!("{:#}", refused.unwrap_err()).contains("Sync is running"));
        let pass = host.storage_pass(&repos::key(NAME)).unwrap().unwrap();
        let tools = github.tools();
        let (done, finished) = std::sync::mpsc::channel();
        let running = std::thread::spawn(move || {
            let passed = self::pass(pass, &tools);
            done.send(()).unwrap();
            passed
        });
        let waited = finished.recv_timeout(std::time::Duration::from_millis(500));
        assert!(waited.is_err(), "the pass waits for the lock");
        drop(held);
        finished
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        host.finish_storage_pass(running.join().unwrap()).unwrap();
    }

    #[test]
    fn a_rule_set_whose_compare_fails_keeps_its_notes() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let space = repos::clone_folder(home.path(), NAME).join("spaces/sage");
        fs::write(space.join("space.yaml"), sage("Edited by a shell")).unwrap();
        sync(&mut host, &github);
        let paused = |host: &Host| {
            (host.workspace_notes(&[]).unwrap().iter())
                .filter(|note| matches!(note, WorkspaceNote::Paused(_)))
                .count()
        };
        assert_eq!(paused(&host), 1);
        // The Spaces rule set cannot read its folder this pass.
        let locked = space.join("notes.md");
        fs::write(&locked, "Notes\n").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        sync(&mut host, &github);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(paused(&host), 1, "the failed rule set's note stays");
    }

    #[test]
    fn a_new_space_in_a_repository_that_is_a_tracker_shares_its_prefix_at_once() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        let staged = stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        let space = (host.create_stored_space(&form("Sage Watch", None), staged))
            .unwrap()
            .space;
        assert_eq!(host.tracker(&space).unwrap().prefix, "SAGE");
        assert_eq!(
            host.storage_cadences().unwrap(),
            [(repos::key(NAME), TRACKER_CADENCE_MS)]
        );
    }

    #[test]
    fn choosing_storage_as_the_settings_repository_keeps_a_paused_space_file_paused() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let file = repos::clone_folder(home.path(), NAME).join("spaces/sage/space.yaml");
        fs::write(&file, sage("Edited by a shell")).unwrap();
        sync(&mut host, &github);
        let tools = github.tools();
        host.choose_settings_repository(NAME, None, &tools).unwrap();
        host.sync_now(&tools).unwrap();
        let shown = github.show("spaces/sage/space.yaml").unwrap();
        assert!(
            !shown.contains("Edited by a shell"),
            "never pushed without a choice"
        );
        let notes = host.workspace_notes(&[]).unwrap();
        assert!(
            notes
                .iter()
                .any(|note| matches!(note, WorkspaceNote::Paused(p)
                if p.paused == Paused::OutsideWiffletree && p.repository.as_deref() == Some(NAME))),
            "{notes:?}"
        );
    }

    #[test]
    fn a_pulled_prefix_used_by_another_storage_repository_here_is_held() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        let taken = host.tracker(&personal(&host)).unwrap().prefix;
        join_sage(&mut host, &github).unwrap();
        github.commit(TRACKER, format!("format: 1\nprefix: {taken}\n"));
        sync(&mut host, &github);
        assert_eq!(
            host.tracker(&space_named(&host, "Sage")).unwrap().prefix,
            "SAGE"
        );
        let notes = host.workspace_notes(&[]).unwrap();
        let [WorkspaceNote::Paused(note)] = notes.as_slice() else {
            panic!("{notes:?}");
        };
        assert!(
            matches!(&note.paused, Paused::CannotBeRead { reason, .. } if reason.contains(&taken))
        );
    }

    #[test]
    fn a_space_whose_brain_has_another_use_here_is_refused_on_join_and_held_when_pulled() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (owner, name) = NAME.split_once('/').unwrap();
        let brain = format!(
            "brain:\n  repository:\n    kind: git_hub\n    owner: {owner}\n    name: {name}\n  folder: second-brain\n"
        );
        let own_brain = sage("Its storage as its brain") + &brain;
        github.commit("spaces/sage/space.yaml", &own_brain);
        let (_home, mut host) = host();
        let error = format!("{:#}", join_sage(&mut host, &github).unwrap_err());
        assert!(error.contains("is the Sage Space's storage"), "{error}");
        github.commit("spaces/sage/space.yaml", sage("Fine"));
        join_sage(&mut host, &github).unwrap();
        github.commit("spaces/sage/space.yaml", &own_brain);
        sync(&mut host, &github);
        assert_eq!(space_named(&host, "Sage").brain, None);
        let notes = host.workspace_notes(&[]).unwrap();
        assert!(
            matches!(notes.as_slice(), [WorkspaceNote::Paused(p)]
            if matches!(p.paused, Paused::CannotBeRead { .. })),
            "{notes:?}"
        );
    }

    #[test]
    fn a_public_repository_is_refused_as_storage_and_nothing_is_written() {
        let folder = tempfile::tempdir().unwrap();
        let mut github = GitHub::new(folder.path(), true);
        github.repository["private"] = json!(false);
        let (home, host) = host();
        let error = stage(&host.home, NAME, &Arc::new(github).tools(), &[])
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("Choose a private repository"));
        assert!(!home.path().join("repos/github.com").exists());
    }

    #[test]
    fn a_brain_is_refused_as_storage_or_settings_and_storage_as_a_brain() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        let (owner, name) = NAME.split_once('/').unwrap();
        let brain = SpaceBrain {
            extra: Extra::new(),
            repository: BrainRepository::GitHub {
                owner: owner.into(),
                name: name.into(),
                extra: Extra::new(),
            },
            folder: "second-brain".into(),
        };
        let personal = personal(&host);
        host.update_space(Space {
            brain: Some(brain.clone()),
            ..personal.clone()
        })
        .unwrap();
        let named = format!("{NAME} is the Personal Space's brain");
        let error = host.refuse_second_use_of(NAME, Use::Storage).unwrap_err();
        assert!(format!("{error:#}").contains(&named), "{error:#}");
        let error = (host.choose_settings_repository(NAME, None, &github.tools())).unwrap_err();
        assert!(format!("{error:#}").contains(&named), "{error:#}");
        assert_eq!(github.log("%s").len(), 3, "nothing is pushed");
        host.update_space(personal.clone()).unwrap();
        join_sage(&mut host, &github).unwrap();
        let error = host.update_space(Space {
            brain: Some(brain),
            ..personal
        });
        let error = format!("{:#}", error.unwrap_err());
        assert!(error.contains("is the Sage Space's storage"), "{error}");
    }

    #[test]
    fn storage_that_is_also_the_settings_repository_shares_one_clone_and_its_pass() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        let tools = github.tools();
        host.choose_settings_repository(NAME, None, &tools).unwrap();
        join_sage(&mut host, &github).unwrap();
        let clone = repos::clone_folder(home.path(), NAME);
        assert_eq!(host.shared_folder(), clone.canonicalize().unwrap());
        assert_eq!(
            host.storage_cadences().unwrap(),
            [],
            "the settings pass runs it"
        );
        assert_eq!(host.settings_cadence().unwrap(), TRACKER_CADENCE_MS);
        let sage = space_named(&host, "Sage");
        host.set_check_ins(20, 200).unwrap();
        host.update_space(Space {
            description: "Changed here".into(),
            ..sage
        })
        .unwrap();
        host.sync_now(&tools).unwrap();
        let machine = sync::machine_name();
        let subjects = github.log("%s");
        assert!(
            subjects.contains(&format!("Sage: changed the Space · {machine}")),
            "{subjects:?}"
        );
        assert!(
            subjects.contains(&format!("Check-ins · {machine}")),
            "{subjects:?}"
        );
        assert!(
            github
                .show("spaces/sage/space.yaml")
                .unwrap()
                .contains("Changed here")
        );
        // Stopping settings sync leaves the clone syncing as storage.
        host.stop_settings_sync().unwrap();
        let key = repos::key(NAME);
        assert_eq!(
            host.storage_cadences().unwrap(),
            [(key, TRACKER_CADENCE_MS)]
        );
        assert!(clone.join(".git").exists());
    }

    #[test]
    fn an_outside_edit_to_a_shared_space_is_paused_until_this_version_is_synced() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let file = repos::clone_folder(home.path(), NAME).join("spaces/sage/space.yaml");
        fs::write(&file, sage("Edited by a shell")).unwrap();
        sync(&mut host, &github);
        assert!(
            !github
                .show("spaces/sage/space.yaml")
                .unwrap()
                .contains("Edited by a shell")
        );
        let notes = host.workspace_notes(&[]).unwrap();
        let [WorkspaceNote::Paused(note)] = notes.as_slice() else {
            panic!("{notes:?}");
        };
        assert_eq!(note.repository.as_deref(), Some(NAME));
        assert_eq!(note.setting, "Sage · Space");
        assert_eq!(note.paused, Paused::OutsideWiffletree);
        host.settle_paused(
            Some(NAME),
            "spaces/sage/space.yaml",
            Settle::SyncThisVersion,
        )
        .unwrap();
        assert_eq!(space_named(&host, "Sage").description, "Edited by a shell");
        sync(&mut host, &github);
        assert!(
            github
                .show("spaces/sage/space.yaml")
                .unwrap()
                .contains("Edited by a shell")
        );
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
    }

    #[test]
    fn a_teammates_edit_is_pulled_and_a_space_not_added_here_is_never_committed() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit(
            "spaces/watch/space.yaml",
            "format: 1\nid: watch-id\nname: Sage Watch\n",
        );
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        github.commit("spaces/sage/space.yaml", sage("From a teammate"));
        let clone = repos::clone_folder(home.path(), NAME);
        fs::write(
            clone.join("spaces/watch/space.yaml"),
            "format: 1\nid: watch-id\nname: Mine\n",
        )
        .unwrap();
        sync(&mut host, &github);
        assert_eq!(space_named(&host, "Sage").description, "From a teammate");
        assert!(
            github
                .show("spaces/watch/space.yaml")
                .unwrap()
                .contains("Sage Watch")
        );
        assert_eq!(
            github.log("%s")[0],
            "spaces/sage/space.yaml",
            "nothing pushed"
        );
    }

    #[test]
    fn joined_storage_without_a_tracker_sets_up_issues_committing_only_that_file() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), None);
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        assert!(host.tracker(&sage).is_err(), "Set up issues is offered");
        sync(&mut host, &github);
        assert_eq!(
            github.show(TRACKER),
            None,
            "nothing written before the human chooses"
        );
        host.set_up_issues(&sage.id, "SAGE").unwrap();
        sync(&mut host, &github);
        let changed = git(&github.bare, &["show", "--format=", "--name-only", "main"]);
        assert_eq!(changed, TRACKER);
        assert_eq!(host.tracker(&sage).unwrap().prefix, "SAGE");
    }

    #[test]
    fn checking_a_repository_lists_its_spaces_and_joining_notes_a_model_this_mac_lacks() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let mut sage = read_space(
            sage("").as_bytes(),
            SpaceStorage {
                repository: super::github(NAME),
                folder: "sage".into(),
            },
        )
        .unwrap();
        sage.models.jobs = Some(ModelProfile {
            provider: Provider::Claude,
            model: "opus".into(),
            effort: "high".into(),
        });
        github.commit("spaces/sage/space.yaml", space_yaml(&sage).unwrap());
        let (_home, mut host) = host();
        let check = stage(&host.home, NAME, &github.tools(), &[]).unwrap().check;
        assert_eq!(check.prefix.as_deref(), Some("SAGE"));
        assert_eq!(
            (check.spaces.iter())
                .map(|s| (&*s.folder, &*s.name))
                .collect::<Vec<_>>(),
            [("sage", "Sage")]
        );
        host.detected.insert(Provider::Claude, false);
        let joined = join_sage(&mut host, &github).unwrap();
        let notes = &joined[0].local.notes;
        assert!(
            notes.len() == 1 && notes[0].starts_with("Opus · high is not available here; using "),
            "{notes:?}"
        );
    }

    #[test]
    fn a_held_file_is_not_pushed_and_its_note_names_the_rule_but_not_the_number() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let number = "(415) 555-0134";
        host.update_space(Space {
            instructions: format!("Text the parent at {number}."),
            ..sage
        })
        .unwrap();
        sync(&mut host, &github);
        assert!(
            !github
                .show("spaces/sage/space.yaml")
                .unwrap()
                .contains("555")
        );
        let notes = host.workspace_notes(&[]).unwrap();
        let [WorkspaceNote::Held(held)] = notes.as_slice() else {
            panic!("{notes:?}");
        };
        assert_eq!(
            (&*held.path, &*held.check),
            ("spaces/sage/space.yaml", "sensitive-info")
        );
        assert_eq!(held.rule.as_deref(), Some("phone"));
        assert!(
            !held.text.contains("555") && held.text.contains(NAME),
            "{}",
            held.text
        );
    }

    #[test]
    fn a_local_repository_is_never_scanned() {
        let (_home, mut host) = host();
        host.scan_path = Some("/nowhere".into());
        let space = personal(&host);
        host.update_space(Space {
            instructions: "Text the parent at (415) 555-0134.".into(),
            ..space.clone()
        })
        .unwrap();
        let folder = host.storage_folder(&space.storage.repository);
        let committed = git(&folder, &["show", "HEAD:spaces/personal/space.yaml"]);
        assert!(committed.contains("555-0134"));
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
    }
}
