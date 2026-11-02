//! Storage repositories, where Spaces' definitions, their tracker and their brain live (OpenSpec
//! change `spaces`, stage 3). Local storage is a Git repository at `repos/local-<id>/` with no
//! remote, committed to on every change and never pushed. GitHub storage is a clone under `repos/`
//! that its one sync engine runs with five rule sets, inside the repository's folder (`folder`):
//! Spaces, over the folders of the Spaces this Mac added and `compilers/`; boards, over their
//! `boards/`, where the outputs of the jobs this Mac runs push without comparing; issues
//! (`issues_sync`), over `wiffletree-issues.yaml` and `issues/`; and logs and the wiki
//! (`brain_sync`), over `second-brain/`. The store keeps each Space as
//! the host uses it; the files are what travel. Sync's record names a file by its path in the
//! repository; the paths here without a `Root` are relative to the folder.
use crate::*;
use brain_sync::{LogRules, WikiRules};
use folder::Root;
use issues_sync::IssueRules;
use publish::Tools;
use repos::{Content, Record, Remote, RuleSet};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use tempfile::TempDir;
use tracker::ISSUES;
use workspace_core::cascade::{Choices, RepositorySettings, ReviewOverrides};

/// The tracker's config at the root of a repository's folder.
pub(crate) const TRACKER: &str = "wiffletree-issues.yaml";
/// The folder of the files each Mac that compiles writes for itself.
pub(crate) const COMPILERS: &str = "compilers";
const SPACE_FORMAT: u32 = 1;
/// How often a repository holding a Space is fetched, with or without a tracker.
pub(crate) const TRACKER_CADENCE_MS: i64 = 60_000;

/// A Space's definition file, relative to its repository's folder.
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
    /// Stored as `instructions`, the name every release reads, so a teammate on an older host
    /// keeps getting them and no save leaves two keys.
    #[serde(default, rename = "instructions")]
    additions: String,
    #[serde(default, skip_serializing_if = "Choices::is_empty")]
    behaviors: Choices,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    repository_settings: BTreeMap<String, RepositorySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brain: Option<SpaceBrain>,
    #[serde(default)]
    repositories: Vec<String>,
    #[serde(default)]
    models: SpaceModels,
    #[serde(default, skip_serializing_if = "ReviewOverrides::is_empty")]
    reviews: ReviewOverrides,
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
        additions: space.additions.clone(),
        behaviors: space.behaviors.clone(),
        repository_settings: space.repository_settings.clone(),
        brain: space.brain.clone(),
        repositories: space.repositories.clone(),
        models: space.models.clone(),
        reviews: space.reviews.clone(),
        sources: space.sources.clone(),
        extra: space.extra.clone(),
    };
    Ok(serde_yaml::to_string(&file)?.into_bytes())
}

/// `compilers/<host id>.yaml`: a Mac that compiles its folder's wiki.
#[derive(Serialize)]
struct CompilerFile {
    machine: String,
    login: Option<String>,
}

/// The Space a `space.yaml` defines, stored in `storage`.
fn read_space(content: &[u8], storage: SpaceStorage) -> Result<Space> {
    let file: SpaceFile = serde_yaml::from_slice(content).context("space.yaml can't be read")?;
    ensure!(
        file.format == SPACE_FORMAT,
        "This Space was written by a newer Wiffletree. Update to use it."
    );
    // The file's place is the Space's storage, which the file itself never names.
    ensure!(
        !file.extra.contains_key("storage"),
        "space.yaml names a storage, which a Space's file cannot"
    );
    let space = Space {
        id: file.id,
        name: file.name,
        description: file.description,
        additions: file.additions,
        behaviors: file.behaviors,
        repository_settings: file.repository_settings,
        brain: file.brain,
        storage,
        repositories: file.repositories,
        models: file.models,
        reviews: file.reviews,
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
    ensure!(
        tracker.states == IssueState::ALL,
        "This tracker uses states this release does not know"
    );
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
            root: String::new(),
        };
        read_space(content, storage)?;
    } else if let Some((_, id)) = recipe_path(path) {
        // Which Space it names is checked when it is taken in, against its folder's Space.
        let file: serde_yaml::Value = serde_yaml::from_slice(content)?;
        let space = SpaceId(file["space"].as_str().unwrap_or_default().into());
        jobs::read_recipe(content, &space, id)?;
    }
    Ok(())
}

/// The Space folder and job id of a path `spaces/<folder>/jobs/<id>.yaml`.
fn recipe_path(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("spaces/")?;
    let (folder, file) = rest.split_once("/jobs/")?;
    let id = file.strip_suffix(".yaml")?;
    (!folder.contains('/') && !id.contains('/') && !id.is_empty()).then_some((folder, id))
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
pub(crate) fn files_under(folder: &Path) -> Result<Vec<PathBuf>> {
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
pub(crate) fn commit_local(folder: &Path, paths: &[&str], message: &str) -> Result<()> {
    if !folder.join(".git").exists() {
        runtime::git_output(folder, &["init", "--quiet", "--initial-branch=main"])?;
    }
    // A tracker with no issue has no `issues/` to add; a removed file that was committed is
    // added as its removal.
    let tracked = |path: &str| {
        runtime::git_answer(folder, &["ls-files", "--error-unmatch", "--", path])
            .is_ok_and(|answer| answer.status.success())
    };
    let present: Vec<&str> = (paths.iter().copied())
        .filter(|path| folder.join(path).exists() || tracked(path))
        .collect();
    if present.is_empty() {
        return Ok(());
    }
    runtime::git_output(folder, &[&["add", "--all", "--"][..], &present].concat())?;
    commit_index(folder, message)
}

/// Commits what is staged in the local repository at `folder`, if anything.
fn commit_index(folder: &Path, message: &str) -> Result<()> {
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

/// Refuses `clone`, a folder on this Mac, unless it is a repository's root with a committed
/// base, as the workspace adds it, and its `origin` is one of `space`'s repositories.
fn check_clone(space: &Space, clone: &str) -> Result<()> {
    let path = repositories::folder_path(clone)?;
    repositories::candidate(&path, &Default::default()).with_context(|| clone.to_owned())?;
    let identity = match repositories::read_origin(&path) {
        repositories::Origin::GitHub { owner, name, .. } => format!("{owner}/{name}"),
        _ => bail!("{clone} is not a clone of a GitHub repository"),
    };
    ensure!(
        (space.repositories.iter()).any(|listed| listed.eq_ignore_ascii_case(&identity)),
        "{clone} is a clone of {identity}, which {} does not list",
        space.name
    );
    Ok(())
}

/// Whether a tracker holds any issue: in its clone, or on GitHub as of `synced`, the commit last
/// synced, which may hold one a pass has not brought into the clone yet.
fn has_issues(clone: &Path, root: &Root, synced: &str) -> Result<bool> {
    let issues = root.join(ISSUES);
    let listed = ["ls-tree", "-r", "--name-only", synced, "--", &issues];
    let on_github = !runtime::git_output(clone, &listed)?.trim().is_empty();
    Ok(on_github || !files_under(&clone.join(&issues))?.is_empty())
}

/// A GitHub repository checked as a Space's storage, with a clone of its branch in a staging
/// folder of the data folder that is removed when dropped.
pub(crate) struct Staged {
    pub(crate) check: StorageCheck,
    /// Its tracker's config as read, with the keys this release does not know.
    tracker: Option<TrackerConfig>,
    /// The folder its Spaces are in, or a new one's when it holds none.
    root: Root,
    staging: TempDir,
}

/// The folder a joining Mac finds in the clone at `clone`, by reading the tree at its head:
/// a repository with no Wiffletree files gets the default folder, unless something else is
/// already there.
fn folder_of(clone: &Path) -> Result<Root> {
    let head = ["rev-parse", "--verify", "--quiet", "HEAD"];
    let head = runtime::git_answer(clone, &head)?;
    let found = match head.status.success() {
        true => folder::find(clone, head.output.trim())?,
        false => None,
    };
    if let Some(root) = found {
        return Ok(root);
    }
    let root = Root::default_folder();
    ensure!(
        !root.in_clone(clone).exists(),
        "{}/ holds other files, and the folder must be Wiffletree's",
        root.name()
    );
    Ok(root)
}

/// Checks `input` as a storage repository and clones it, off the host thread. `added` names the
/// Spaces already on this Mac. Writes nothing that outlasts the check.
pub(crate) fn stage(home: &Path, input: &str, tools: &Tools, added: &[SpaceId]) -> Result<Staged> {
    let (repository, organization, staging) = repos::clone_to_staging(home, input, tools)?;
    let root = folder_of(staging.path())?;
    let in_staging = root.in_clone(staging.path());
    let mut spaces = vec![];
    for entry in fs::read_dir(in_staging.join("spaces"))
        .into_iter()
        .flatten()
    {
        let folder = entry?.file_name().to_string_lossy().into_owned();
        let Ok(content) = fs::read(in_staging.join(space_file(&folder))) else {
            continue;
        };
        let storage = SpaceStorage {
            repository: github(&repository.name),
            folder: folder.clone(),
            root: root.name().into(),
        };
        let space = read_space(&content, storage)
            .with_context(|| format!("The Space in spaces/{folder}/ can't be read"))?;
        spaces.push(SpaceListing {
            added: added.contains(&space.id),
            folder,
            id: space.id,
            name: space.name,
            description: space.description,
            brain: space.brain,
            repositories: space.repositories,
        });
    }
    spaces.sort_by(|a, b| a.folder.cmp(&b.folder));
    let tracker = match fs::read(in_staging.join(TRACKER)) {
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
        root,
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
    pub(crate) additions: &'a str,
    pub(crate) prefix: Option<&'a str>,
}

/// What a storage repository's pass compares: the folders of the Spaces this Mac added, by
/// folder with their names, its tracker's prefix, and the record.
#[derive(Clone)]
pub(crate) struct Input {
    key: String,
    /// The repository's folder.
    pub(crate) root: Root,
    names: BTreeMap<String, String>,
    pub(crate) prefix: Option<String>,
    synced: BTreeMap<String, Content>,
    written: BTreeMap<String, Content>,
    two_machines: Vec<String>,
    /// The paused parts of each `issue.md`, by its path.
    pub(crate) parts: BTreeMap<String, Vec<String>>,
    /// The number each of this Mac's new issues would rather have, by its temporary ID.
    pub(crate) preferred: BTreeMap<IssueId, u64>,
    /// The board folders of the jobs this Mac is the runner of.
    runs_here: Vec<String>,
    /// The log entries waiting or held here, by their paths in the repository.
    pub(crate) entries: Vec<String>,
    /// A compile run's files waiting to land.
    pub(crate) landing: Option<compile::Landing>,
}

/// A part of an `issue.md` both sides changed to different values, which this Mac keeps and
/// does not push: a front matter field, or the description.
pub(crate) struct PausedPart {
    pub(crate) path: String,
    pub(crate) part: String,
    /// The other side's value as written, `None` when it has none.
    pub(crate) theirs: Option<String>,
    /// Who last changed the file on GitHub.
    pub(crate) by: String,
}

/// The files of a storage repository's paths a pass found, for the host thread to write and
/// record.
#[derive(Default)]
pub(crate) struct Found {
    key: String,
    /// Each file changed only on GitHub: this machine's content as read, then GitHub's.
    pulled: Vec<(String, Content, Content)>,
    alike: Vec<(String, Content)>,
    pub(crate) committed: Vec<(String, Content)>,
    paused: Vec<(String, Paused, Content)>,
    held: Vec<scan::Held>,
    /// Why a rule set's compare failed.
    failed: Vec<String>,
    /// The paths of the rule sets whose compare failed, whose notes stay as they were.
    failed_roots: Vec<String>,
    /// The new issues the commit numbered: each temporary ID and its number.
    pub(crate) numbered: Vec<(IssueId, IssueId)>,
    /// The new issues GitHub already held, as this Mac pushed them before: each temporary ID
    /// and the number it has there.
    pub(crate) adopted: Vec<(IssueId, IssueId)>,
    /// Each file merged with GitHub's: this machine's content as read, what it keeps here, and
    /// GitHub's, which is what it last synced unless the merge is pushed.
    merged: Vec<(String, Content, Vec<u8>, Content)>,
    /// The `issue.md` files whose parts were merged, and the parts that paused.
    parts_compared: Vec<String>,
    parts: Vec<PausedPart>,
    /// What the logs rule set did with this Mac's entries.
    entries: brain_sync::Entries,
    /// How a compile run's landing stands.
    landing: Option<(compile::Landing, brain_sync::Landed)>,
}

/// The output a file of a job's board folder belongs to, by its place there: its metadata
/// `spaces/<folder>/boards/<job>/<slug>.json` and its file `…/<job>/<slug>/<file>`, whatever
/// that file's own type, are both `…/<job>/<slug>`.
fn output_of(path: &str) -> &str {
    let end = (path.match_indices('/').nth(4)).map_or(path.len(), |(at, _)| at);
    let output = &path[..end];
    match end == path.len() {
        true => output.strip_suffix(".json").unwrap_or(output),
        false => output,
    }
}

/// A rule set comparing whole files under `roots`, except those under `excluded`, with the
/// version last synced, as the settings rule set does: what changed on both sides, changed
/// outside Wiffletree or arrived in a form this Mac cannot use is paused and kept here. A file
/// under `runs_here` is a runner's output only this Mac writes, so what Wiffletree wrote here is
/// pushed without comparing it with GitHub's.
pub(crate) struct WholeFiles {
    /// The repository's folder.
    root: Root,
    /// Whether this rule set watches for the repository moving into a folder, as one does.
    watches_move: bool,
    roots: Vec<String>,
    excluded: Vec<String>,
    runs_here: Vec<String>,
    /// Whether a runner's output here fetches and pushes again on its own when GitHub moves,
    /// for a pass that does not run whole again (see `RuleSets::new`).
    retries_alone: bool,
    subject: String,
    pub(crate) synced: BTreeMap<String, Content>,
    written: BTreeMap<String, Content>,
    two_machines: Vec<String>,
    /// For issues: the paused parts of each `issue.md`, which its files merge by part and
    /// `events.jsonl` by line instead of pausing whole.
    pub(crate) parts: Option<BTreeMap<String, Vec<String>>>,
    pub(crate) found: Found,
}
impl WholeFiles {
    pub(crate) fn new(
        input: &Input,
        roots: Vec<String>,
        excluded: Vec<String>,
        subject: String,
    ) -> Self {
        let compares = |path: &str| under(&roots, path) && !under(&excluded, path);
        let mine = |map: &BTreeMap<String, Content>| -> BTreeMap<String, Content> {
            (map.iter())
                .filter(|(path, _)| compares(path))
                .map(|(path, content)| (path.clone(), content.clone()))
                .collect()
        };
        Self {
            synced: mine(&input.synced),
            written: mine(&input.written),
            two_machines: (input.two_machines.iter())
                .filter(|path| compares(path))
                .cloned()
                .collect(),
            runs_here: (input.runs_here.iter())
                .filter(|folder| compares(folder))
                .cloned()
                .collect(),
            root: input.root.clone(),
            watches_move: false,
            roots,
            excluded,
            retries_alone: false,
            subject,
            parts: None,
            found: Found::default(),
        }
    }
    fn compares(&self, path: &str) -> bool {
        under(&self.roots, path) && !under(&self.excluded, path)
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
    }
    fn moved_to(&self, clone: &Path, remote: &str) -> Result<Option<Root>> {
        match self.watches_move && self.root.is_empty() {
            true => folder::moved_to(clone, remote),
            false => Ok(None),
        }
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
            let written = self.written.get(path).cloned().flatten();
            let outside = mine != base && mine != written;
            // An issue file changed outside Wiffletree pauses whole, its parts as they were.
            if self.parts.is_some() && path.ends_with("/issue.md") && !outside {
                self.found.parts_compared.push(path.clone());
            }
            let found = &mut self.found;
            if mine == theirs {
                if mine != base {
                    found.alike.push((path.clone(), mine));
                }
                continue;
            }
            if let (Some(parts), Some(m), Some(t)) = (&self.parts, &mine, &theirs)
                && !outside
                && let Some((here, pushed, paused)) =
                    issues_sync::merge_file(path, base.as_deref(), m, t, parts)
            {
                let by = match paused.is_empty() {
                    true => String::new(),
                    false => issues_sync::last_author(clone, remote, path),
                };
                for (part, theirs) in paused {
                    let path = path.clone();
                    let by = by.clone();
                    found.parts.push(PausedPart {
                        path,
                        part,
                        theirs,
                        by,
                    });
                }
                if &pushed != t {
                    found.committed.push((path.clone(), Some(pushed.clone())));
                }
                match &here == t && &pushed == t {
                    true => found.pulled.push((path.clone(), mine, theirs)),
                    false => found.merged.push((path.clone(), mine, here, theirs)),
                }
                continue;
            }
            let in_folder = self.root.strip(path).unwrap_or(path);
            let refused = (theirs != base)
                .then(|| theirs.as_deref().and_then(|c| check(in_folder, c).err()))
                .flatten();
            let machine = || "another machine".to_owned();
            let runs_here = under(&self.runs_here, path);
            let why = if outside {
                Some(Paused::OutsideWiffletree)
            } else if let Some(error) = refused {
                Some(Paused::CannotBeRead {
                    machine: machine(),
                    reason: format!("{error:#}"),
                })
            } else if runs_here {
                None
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
    /// A runner's output with a file paused this pass waits whole, so its metadata and its
    /// file are never pushed apart.
    fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)> {
        let root = &self.root;
        let output = |path: &'_ str| output_of(root.strip(path).unwrap_or(path)).to_owned();
        let paused: BTreeSet<String> = (self.found.paused.iter())
            .filter(|(path, ..)| under(&self.runs_here, path))
            .map(|(path, ..)| output(path))
            .collect();
        (self.found.committed)
            .retain(|(path, _)| !(under(&self.runs_here, path) && paused.contains(&output(path))));
        let committed = &self.found.committed;
        (!committed.is_empty()).then(|| {
            let message = format!("{} · {}", self.subject, sync::machine_name());
            (committed.clone(), message)
        })
    }
    fn failed(&mut self, error: anyhow::Error) {
        self.found.failed.push(format!("{error:#}"));
        self.found.failed_roots.extend(self.roots.iter().cloned());
    }
    fn reset(&mut self) {
        self.found = Found {
            key: std::mem::take(&mut self.found.key),
            ..Found::default()
        };
    }
    fn sit_out(&mut self) {
        self.found.committed.clear();
    }
    /// A runner's output, which only this Mac writes, is pushed again on GitHub's new head.
    fn pushes_again(&self) -> bool {
        self.retries_alone && !self.runs_here.is_empty()
    }
}

/// Whether `path` is one of `roots` or under one, hidden files aside.
fn under(roots: &[String], path: &str) -> bool {
    !path.split('/').any(|part| part.starts_with('.'))
        && (roots.iter()).any(|root| path == root || path.starts_with(&format!("{root}/")))
}

/// A storage repository's five rule sets for one pass: its Spaces' files, their boards, issues,
/// log entries and the wiki, last, so a landing's commit is the head a pass pushes.
pub(crate) struct RuleSets {
    root: Root,
    spaces: WholeFiles,
    boards: WholeFiles,
    issues: IssueRules,
    logs: LogRules,
    wiki: WikiRules,
}
impl RuleSets {
    /// `retries_alone`: a runner's boards, log entries and a wiki landing fetch and push again on
    /// their own when GitHub moves under the push, while the others sit that try out. Only the settings pass, which never
    /// runs whole again, asks for it; a storage repository's own pass (`pass`) fetches,
    /// compares and pushes every rule set again instead, so issues renumber on the new head.
    pub(crate) fn new(input: Input, retries_alone: bool) -> Self {
        let root = input.root.clone();
        let folders = |under: &str| -> Vec<String> {
            (input.names.keys())
                .map(|folder| root.join(&format!("spaces/{folder}{under}")))
                .collect()
        };
        let names: Vec<&str> = input.names.values().map(String::as_str).collect();
        let names = names.join(", ");
        let subject = format!("{names}: changed the Space");
        let mut covered = folders("");
        covered.push(root.join(COMPILERS));
        let mut spaces = WholeFiles::new(&input, covered, folders("/boards"), subject);
        spaces.watches_move = true;
        let subject = format!("{names}: saved outputs");
        let mut boards = WholeFiles::new(&input, folders("/boards"), vec![], subject);
        boards.retries_alone = retries_alone;
        let mut issues = IssueRules::new(&input);
        issues.files.parts = Some(input.parts.clone());
        let logs = LogRules::new(&input, retries_alone);
        let wiki = WikiRules::new(&input, retries_alone);
        let mut rule_sets = Self {
            root,
            spaces,
            boards,
            issues,
            logs,
            wiki,
        };
        rule_sets.spaces.found.key = input.key;
        rule_sets
    }
    pub(crate) fn as_mut(&mut self) -> [&mut dyn RuleSet; 5] {
        [
            &mut self.spaces,
            &mut self.boards,
            &mut self.issues,
            &mut self.logs,
            &mut self.wiki,
        ]
    }
    /// What all found, with the files `held` back from the push left out of what was
    /// committed, and a new issue held whole kept under its temporary ID.
    pub(crate) fn found(self, held: Vec<scan::Held>) -> Found {
        let (entries_held, held): (Vec<_>, Vec<_>) =
            held.into_iter().partition(|h| self.logs.claims(&h.path));
        let (wiki_held, mut held): (Vec<_>, Vec<_>) =
            held.into_iter().partition(|h| self.wiki.claims(&h.path));
        let (mut found, mut boards) = (self.spaces.found, self.boards.found);
        let mut issues = self.issues.files.found;
        issues_sync::keep_held_unnumbered(&mut issues, &mut held, &self.root);
        for other in [&mut boards, &mut issues] {
            found.pulled.append(&mut other.pulled);
            found.alike.append(&mut other.alike);
            found.committed.append(&mut other.committed);
            found.paused.append(&mut other.paused);
            found.failed.append(&mut other.failed);
            found.failed_roots.append(&mut other.failed_roots);
        }
        found.merged = issues.merged;
        found.parts_compared = issues.parts_compared;
        found.parts = issues.parts;
        found.numbered = issues.numbered;
        found.adopted = issues.adopted;
        found.entries = self.logs.found(entries_held);
        found.landing = self.wiki.found(wiki_held);
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
    /// The folder GitHub's head moved into, which ended the pass before it compared anything.
    moved_to: Option<Root>,
}

/// What the latest pass of a storage repository that syncs on its own did.
#[derive(Default)]
pub(crate) struct Synced {
    /// When a pass last reached GitHub.
    at: Option<i64>,
    /// Why the latest pass failed.
    error: Option<String>,
}

/// Runs a storage repository's engine with its Space, boards and issue rule sets, scanning what
/// it pushes. When GitHub refuses the push because the branch moved, the same pass fetches,
/// compares, numbers and pushes every rule set again, a runner's boards included, up to
/// `issues_sync::PUSH_TRIES` times. This is the pass's one retry: its boards never retry alone
/// (`RuleSets::new` with `retries_alone` false), as they do in the settings pass.
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
    let remote = || Remote {
        url: &repository.push_url,
        branch: &repository.branch,
    };
    // The commit whose files outside every rule set the clone holds.
    let mut previous = commit;
    let mut again = 0;
    loop {
        let mut rule_sets = RuleSets::new(input.clone(), false);
        let outcome = repos::pass(
            &clone,
            remote(),
            &previous,
            &mut rule_sets.as_mut(),
            Some(&scanner),
            &tools.git,
        )?;
        let head = runtime::git_output(&clone, &["rev-parse", "FETCH_HEAD"])?;
        let head = head.trim().to_owned();
        let (pushed, error) = match outcome.pushed {
            Ok(pushed) => (pushed, None),
            Err(error) => (None, Some(error)),
        };
        let moved = || {
            let now = github_access::remote_head(&clone, remote().url, remote().branch, &tools.git);
            now.is_ok_and(|now| now.as_deref() != Some(head.as_str()))
        };
        if error.is_some() && again < issues_sync::PUSH_TRIES && moved() {
            again += 1;
            previous = head;
            continue;
        }
        return Ok(Passed {
            remote: head,
            pushed,
            error,
            found: rule_sets.found(outcome.held),
            moved_to: outcome.moved_to,
        });
    }
}

/// A GitHub repository's move into its folder, to run off the host thread.
pub(crate) struct Move {
    pub(crate) key: String,
    clone: PathBuf,
    repository: SettingsRepository,
    /// The host's one pass lock.
    running: std::sync::Arc<std::sync::Mutex<()>>,
}
impl Move {
    /// Pushes the move, or finds a teammate's; the folder the repository is in now.
    pub(crate) fn run(&self, tools: &Tools) -> Result<Root> {
        let _running = self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let remote = Remote {
            url: &self.repository.push_url,
            branch: &self.repository.branch,
        };
        let root = Root::default_folder();
        repos::move_into_folder(&self.clone, &remote, &root, &tools.git)
    }
}

impl Host {
    /// A Space's repository folder as this Mac records it.
    pub(crate) fn root_of(&self, repository: &StorageRepository) -> Result<Root> {
        let spaces = self.spaces()?;
        let stored = spaces.iter().map(|record| &record.space.storage);
        let root = stored.into_iter().find(|s| &s.repository == repository);
        Ok(root.map_or_else(Root::default, |storage| Root::new(&storage.root)))
    }
    /// `root_of` by sync's record key, which an empty folder answers for any other repository.
    pub(crate) fn root_of_key(&self, key: &str) -> Result<Root> {
        let spaces = self.spaces()?;
        let stored = spaces.iter().map(|record| &record.space.storage);
        let root =
            (stored.into_iter()).find(|s| Self::storage_key(&s.repository).as_deref() == Some(key));
        Ok(root.map_or_else(Root::default, |storage| Root::new(&storage.root)))
    }
    /// Where a repository's folder is on this Mac.
    pub(crate) fn root_folder(&self, repository: &StorageRepository) -> Result<PathBuf> {
        let root = self.root_of(repository)?;
        Ok(root.in_clone(&self.storage_folder(repository)))
    }
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
    pub(crate) fn storage_key(repository: &StorageRepository) -> Option<String> {
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
            let root = Root::new(&space.storage.root);
            let committed = runtime::git_answer(
                &folder,
                &[
                    "cat-file",
                    "-e",
                    &format!("HEAD:{}", root.join(&space_file(&space.storage.folder))),
                ],
            )
            .is_ok_and(|answer| answer.status.success());
            if !committed {
                self.write_local(space, &format!("Created the {} Space", space.name))?;
            }
        }
        Ok(())
    }
    /// Moves each local repository with its Spaces' files at its root into the default folder,
    /// at the first start of this release, in one commit with no question, and finishes a move
    /// a start cut short. A repository that cannot move stays at its root, which still works.
    pub(crate) fn move_local_repositories(&mut self) -> Result<()> {
        let mut waiting: Vec<StorageRepository> = vec![];
        for record in self.spaces()? {
            let storage = record.space.storage;
            if matches!(storage.repository, StorageRepository::Local { .. })
                && storage.root.is_empty()
                && !waiting.contains(&storage.repository)
            {
                waiting.push(storage.repository);
            }
        }
        for repository in waiting {
            if let Err(error) = self.move_local(&repository) {
                let name = repository.key();
                eprintln!("Not moving {name} into its folder: {error:#}");
            }
        }
        Ok(())
    }
    fn move_local(&mut self, repository: &StorageRepository) -> Result<()> {
        let clone = self.storage_folder(repository);
        let root = Root::default_folder();
        let head = runtime::git_output(&clone, &["rev-parse", "HEAD"])?;
        let head = head.trim();
        if folder::moved_to(&clone, head)?.is_none() {
            runtime::git_output(&clone, &["reset", "--quiet", head])?;
            folder::stage_move(&clone, head, &root)?;
            commit_index(&clone, &format!("Move into {}/", root.name()))?;
            folder::check_move(&clone, head, "HEAD", &root)?;
        }
        self.adopt_folder(repository, &root)?;
        Ok(())
    }
    /// Takes a repository's move into `root`, on the Mac that made it and on every other: this
    /// Mac's files move under the folder, unpushed ones with them, and then its Spaces and
    /// record. In that order, so a start cut short only moves the rest: a record that named the
    /// folder before its files were there would read each file as removed here. What a move
    /// left at the root comes down with the next pass, or for a local repository from its head.
    pub(crate) fn adopt_folder(&mut self, storage: &StorageRepository, root: &Root) -> Result<()> {
        let unmoved: Vec<SpaceRecord> = (self.spaces()?.into_iter())
            .filter(|r| &r.space.storage.repository == storage && r.space.storage.root.is_empty())
            .collect();
        if unmoved.is_empty() {
            return Ok(());
        }
        let clone = self.storage_folder(storage);
        folder::move_files(&clone, root)?;
        if matches!(storage, StorageRepository::Local { .. }) {
            folder::restore_stubs(&clone, "HEAD", root)?;
        }
        let key = Self::storage_key(storage);
        let tx = self.db.unchecked_transaction()?;
        // A log row keeps naming its unlanded entry, which the Logs page finds it by.
        tx.execute(
            "UPDATE work_logs SET data=json_set(data,'$.path',?2||'/'||json_extract(data,'$.path')) WHERE id IN (SELECT log_id FROM entry_files WHERE repository=?1)",
            params![key, root.name()],
        )?;
        for mut record in unmoved {
            record.space.storage.root = root.name().into();
            tx.execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![record.space.id.as_str(), encode(&record.space)?],
            )?;
        }
        for table in [
            "sync_record",
            "sync_paused",
            "sync_held",
            "issue_parts",
            "entry_files",
        ] {
            let paths: Vec<String> = (tx.prepare(&format!(
                "SELECT DISTINCT path FROM {table} WHERE repository=?1"
            ))?)
            .query_map([&key], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
            for path in paths.iter().filter(|path| folder::moves(path)) {
                tx.execute(
                    &format!(
                        "UPDATE OR REPLACE {table} SET path=?3 WHERE repository=?1 AND path=?2"
                    ),
                    params![key, path, root.join(path)],
                )?;
            }
        }
        tx.commit()?;
        if let Some(key) = key {
            self.storage_written.borrow_mut().insert(key);
        }
        Ok(())
    }
    /// Starts the human's Move into the folder for the Space's repository: refused for a local
    /// one, which moved at start, and for one already in its folder.
    pub(crate) fn start_move(&self, space_id: &SpaceId) -> Result<Move> {
        let space = self.space(space_id)?.space;
        let key = Self::storage_key(&space.storage.repository)
            .context("A repository on this Mac alone moves into its folder at start")?;
        ensure!(
            space.storage.root.is_empty(),
            "{} is already in {}/",
            space.storage.repository.key(),
            space.storage.root
        );
        let repository = self.stored_repository(&key)?;
        Ok(Move {
            clone: repos::clone_folder(&self.home, &repository.name),
            repository,
            key,
            running: self.sync_passes.clone(),
        })
    }
    /// Takes a finished move, by `key`.
    pub(crate) fn finish_move(&mut self, key: &str, root: &Root) -> Result<()> {
        let space = (self.spaces()?.into_iter())
            .find(|r| Self::storage_key(&r.space.storage.repository).as_deref() == Some(key));
        match space {
            Some(record) => self.adopt_folder(&record.space.storage.repository, root),
            None => Ok(()),
        }
    }
    /// The GitHub storage repository `key` as this Mac recorded it.
    fn stored_repository(&self, key: &str) -> Result<SettingsRepository> {
        let data: String = self.db.query_row(
            "SELECT data FROM storage_repositories WHERE key=?1",
            [key],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&data)?)
    }
    /// Writes a Space with local storage to its repository, with the tracker's config when it
    /// has none, and commits them.
    fn write_local(&self, space: &Space, message: &str) -> Result<()> {
        let folder = self.storage_folder(&space.storage.repository);
        let root = Root::new(&space.storage.root);
        let (file, tracker_file) = (
            root.join(&space_file(&space.storage.folder)),
            root.join(TRACKER),
        );
        shared::write_atomically(&folder.join(&file), &space_yaml(space)?)?;
        let mut paths = vec![file.as_str()];
        if !folder.join(&tracker_file).exists() {
            let tracker = self.tracker(space)?;
            shared::write_atomically(&folder.join(&tracker_file), &tracker_yaml(&tracker)?)?;
            paths.push(&tracker_file);
        }
        commit_local(&folder, &paths, message)
    }
    /// Writes a file of a GitHub storage repository's clone, by its path there, and notes it
    /// for the next pass.
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
        let file = Root::new(&space.storage.root).join(&space_file(&space.storage.folder));
        self.write_stored(&key, &folder, &file, &space_yaml(space)?)
    }
    /// Writes this Mac's file saying it compiles the Space's wiki, or with `compiles` false
    /// removes it. It is the one shared file only this Mac writes, so it never conflicts.
    pub fn write_compiler_file(&self, space: &Space, compiles: bool) -> Result<()> {
        let file = format!("{COMPILERS}/{}.yaml", self.host_id()?);
        let content = compiles
            .then(|| {
                serde_yaml::to_string(&CompilerFile {
                    machine: sync::machine_name(),
                    login: self.gh_login.clone(),
                })
            })
            .transpose()?;
        let message = format!("{}: changed who compiles", space.name);
        let files = [(file, content.as_deref().map(str::as_bytes))];
        self.write_storage_files(space, &files, &message)
    }
    /// Writes a job's recipe file to its Space's storage, or with `None` removes it: committed
    /// at once in a local repository, and pushed by the next pass from a GitHub one.
    pub(crate) fn write_recipe_file(
        &self,
        space: &Space,
        id: &str,
        content: Option<&[u8]>,
        message: &str,
    ) -> Result<()> {
        let file = jobs::recipe_file(&space.storage.folder, id);
        self.write_storage_files(space, &[(file, content)], message)
    }
    /// Writes files of a Space's storage, relative to the repository's folder, or with `None`
    /// removes them: committed at once in a local repository, and pushed by the next pass from a
    /// GitHub one.
    pub(crate) fn write_storage_files(
        &self,
        space: &Space,
        files: &[(String, Option<&[u8]>)],
        message: &str,
    ) -> Result<()> {
        let folder = self.storage_folder(&space.storage.repository);
        let root = Root::new(&space.storage.root);
        let files: Vec<(String, Option<&[u8]>)> = (files.iter())
            .map(|(file, content)| (root.join(file), *content))
            .collect();
        for (file, content) in &files {
            match content {
                Some(content) => shared::write_atomically(&folder.join(file), content)?,
                None => match fs::remove_file(folder.join(file)) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(error.into());
                    }
                    _ => {}
                },
            }
        }
        let Some(key) = Self::storage_key(&space.storage.repository) else {
            let paths: Vec<&str> = files.iter().map(|(file, _)| file.as_str()).collect();
            return commit_local(&folder, &paths, message);
        };
        for (file, content) in &files {
            Record::wrote(&self.db, &key, file, *content)?;
        }
        self.storage_written.borrow_mut().insert(key);
        Ok(())
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
    /// Refuses a file another Mac changed that this Mac cannot take: a tracker config whose
    /// prefix another storage repository here uses.
    fn refuse_pulled(&self, storage: &StorageRepository, path: &str, content: &[u8]) -> Result<()> {
        if self.root_of(storage)?.strip(path) == Some(TRACKER) {
            return self.refuse_used_prefix(&read_tracker(content)?.prefix, &storage.key());
        }
        Ok(())
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
        let root = Root::new(&space.storage.root);
        let file = root.join(TRACKER);
        let tracker = TrackerConfig {
            prefix: prefix.into(),
            ..current.clone()
        };
        let content = tracker_yaml(&tracker)?;
        match Self::storage_key(&space.storage.repository) {
            None => {
                rename_issues(&root.in_clone(&folder), &current.prefix, prefix)?;
                shared::write_atomically(&folder.join(&file), &content)?;
                let message = format!("Renamed the issue prefix {} to {prefix}", current.prefix);
                commit_local(&folder, &[file.as_str(), &root.join(ISSUES)], &message)?;
                // The issues' links, derived states and follows follow them to their new IDs.
                for table in ["issue_links", "issue_derived", "issue_follows"] {
                    self.db.execute(
                        &format!("UPDATE {table} SET id=?3||substr(id,length(?2)+1) WHERE storage=?1 AND substr(id,1,length(?2)+1)=?2||'-'"),
                        params![storage, current.prefix, prefix],
                    )?;
                }
            }
            Some(key) => {
                let synced = Record::read(&self.db, &key)?
                    .context("The storage repository has no record")?
                    .commit;
                ensure!(
                    !has_issues(&folder, &root, &synced)?,
                    "{storage} has issues, and a shared tracker's prefix changes only while it has none"
                );
                self.write_stored(&key, &folder, &file, &content)?;
            }
        }
        self.db.execute(
            "UPDATE trackers SET data=?2 WHERE storage=?1",
            params![storage, encode(&tracker)?],
        )?;
        self.reindex_tracker(&space, prefix)?;
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
        let file = Root::new(&space.storage.root).join(TRACKER);
        ensure!(
            !folder.join(&file).exists(),
            "{storage} already has {TRACKER}; it is set up at the next sync"
        );
        self.refuse_used_prefix(prefix, &storage)?;
        let tracker = TrackerConfig::new(prefix.into());
        self.write_stored(&key, &folder, &file, &tracker_yaml(&tracker)?)?;
        self.db.execute(
            "INSERT INTO trackers VALUES (?1,?2)",
            params![storage, encode(&tracker)?],
        )?;
        self.reindex_tracker(&space, prefix)?;
        Ok(tracker)
    }
    /// Puts a staged clone at its folder under `repos/`, or keeps the clone already there, and
    /// records the repository as storage. A clone new to this Mac starts its record afresh.
    fn place_storage(&self, staged: Staged) -> Result<(String, PathBuf)> {
        let repository = &staged.check.repository;
        let key = repos::key(&repository.name);
        let clone = repos::clone_folder(&self.home, &repository.name);
        let root = staged.root.clone();
        if !clone.exists() {
            fs::create_dir_all(clone.parent().context("A clone is in a folder")?)?;
            fs::rename(staged.staging.keep(), &clone)?;
        }
        let head = runtime::git_output(&clone, &["rev-parse", "HEAD"])?;
        let head = head.trim();
        if Record::read(&self.db, &key)?.is_none() {
            let recorded = |path: &str| !brain_sync::in_brain(&root, path);
            let committed = repos::files_at(&clone, head, &recorded)?;
            Record::restart(&self.db, &key, head, &committed, &committed, &recorded)?;
        } else {
            // The settings repository's clone: its Space and tracker files start their record
            // as its head holds them, and what the record has already stays.
            let recorded = |path: &str| root.scanned(path) && !brain_sync::in_brain(&root, path);
            for (path, content) in repos::files_at(&clone, head, &recorded)? {
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
        let root = staged.root.clone();
        let spaces = root.in_clone(staged.staging.path()).join("spaces");
        let (mut space, tracker) = self.check_new_stored_space(
            new,
            &repository.name,
            staged.tracker.as_ref(),
            &|folder| spaces.join(folder).exists(),
        )?;
        self.refuse_another_root(&space.storage.repository, &root)?;
        space.storage.root = root.name().into();
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
            let file = root.join(TRACKER);
            if new_tracker && !clone.join(&file).exists() {
                host.write_stored(&key, &clone, &file, &tracker_yaml(&tracker)?)?;
            }
            Ok(())
        })?;
        self.reindex_tracker(&space, &self.tracker(&space)?.prefix)?;
        Ok(SpaceRecord { space, local })
    }
    /// Refuses a Space in a repository whose head is in another folder than this Mac's other
    /// Spaces there, which adopt it at the next pass, as Spaces in a repository share one.
    fn refuse_another_root(&self, repository: &StorageRepository, root: &Root) -> Result<()> {
        let here = self.spaces()?;
        let here = here
            .iter()
            .find(|r| &r.space.storage.repository == repository);
        ensure!(
            here.is_none_or(|record| record.space.storage.root == root.name()),
            "{} moved into its folder. Sync it, then try again.",
            repository.key()
        );
        Ok(())
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
    /// unused and valid, and the prefix, the repository's own or else `prefix` or one proposed
    /// from the name, is free here. Its folder is the first one `taken` does not hold. Writes
    /// nothing, so it runs before anything is pushed.
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
            additions,
            prefix,
        } = *new;
        self.ensure_unused_name(name, None)?;
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
            additions: additions.into(),
            behaviors: Default::default(),
            repository_settings: Default::default(),
            brain: None,
            storage: SpaceStorage {
                repository: storage,
                folder,
                root: DEFAULT_ROOT.into(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            reviews: Default::default(),
            sources: vec![],
            extra: Extra::new(),
        };
        spaces::validate(&space)?;
        Ok((space, tracker))
    }
    /// Adds the ticked Spaces of a GitHub storage repository, checked and cloned in `staged`,
    /// with where each one's brain is on this Mac and the clones of its code repositories here,
    /// which join the workspace. Refused when its tracker's prefix is used by another storage
    /// repository here, or a clone is of a repository the Space does not list.
    pub(crate) fn join_spaces(
        &mut self,
        staged: Staged,
        ticked: &[JoinSpace],
    ) -> Result<Vec<SpaceRecord>> {
        ensure!(!ticked.is_empty(), "Tick at least one Space to add");
        let repository = staged.check.repository.clone();
        repos::refuse_another(&self.home, &repository)?;
        let storage = github(&repository.name);
        if let Some(prefix) = &staged.check.prefix {
            self.refuse_used_prefix(prefix, &storage.key())?;
        }
        self.refuse_another_root(&storage, &staged.root)?;
        let in_staging = staged.root.in_clone(staged.staging.path());
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
            let content = fs::read(in_staging.join(space_file(&tick.folder)))?;
            let space = read_space(
                &content,
                SpaceStorage {
                    repository: storage.clone(),
                    folder: tick.folder.clone(),
                    root: staged.root.name().into(),
                },
            )?;
            for clone in &tick.clones {
                check_clone(&space, clone)?;
            }
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
                host.take_cloned_recipes(&record.space)?;
            }
            let clones: Vec<String> = ticked.iter().flat_map(|t| t.clones.clone()).collect();
            if !clones.is_empty() {
                let imported = host.import_repositories(None, &clones, &[], &[])?;
                ensure!(
                    imported.failures.is_empty(),
                    "{}",
                    imported.failures.join("; ")
                );
            }
            Ok(())
        })?;
        if let Ok(tracker) = self.tracker(&joined[0].space) {
            self.reindex_tracker(&joined[0].space, &tracker.prefix)?;
        }
        Ok(joined)
    }
    /// Takes the jobs a joined Space's clone already holds into the store; a recipe this Mac
    /// cannot read is left out, as a pulled one would be held.
    fn take_cloned_recipes(&mut self, space: &Space) -> Result<()> {
        let clone = self.root_folder(&space.storage.repository)?;
        let folder = clone
            .join("spaces")
            .join(&space.storage.folder)
            .join("jobs");
        for file in files_under(&folder)? {
            let path = file.strip_prefix(&clone)?.to_string_lossy().into_owned();
            let Some((_, id)) = recipe_path(&path) else {
                continue;
            };
            if let Err(error) = self.take_pulled_recipe(space, id, Some(&fs::read(&file)?)) {
                eprintln!("Joining {}: {path} cannot be used: {error:#}", space.name);
            }
        }
        Ok(())
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
            .map(|model| {
                let named = model.profile().short();
                format!("{named} is not available here; using {instead}")
            })
            .collect())
    }
    /// What a pass of the GitHub storage repository `key` compares, or `None` when no Space on
    /// this Mac is stored there.
    pub(crate) fn storage_input(&self, key: &str) -> Result<Option<Input>> {
        let stored: Vec<Space> = (self.spaces()?.into_iter())
            .filter(|r| Self::storage_key(&r.space.storage.repository).as_deref() == Some(key))
            .map(|r| r.space)
            .collect();
        let Some(first) = stored.first() else {
            return Ok(None);
        };
        let prefix = self.tracker(first).ok().map(|tracker| tracker.prefix);
        let storage = first.storage.repository.key();
        let root = Root::new(&first.storage.root);
        let landing = self.landing_of(&storage)?;
        let mut runs_here = vec![];
        for recipe in self.recipes()? {
            let space = stored.iter().find(|space| space.id == recipe.space_id);
            if let Some(space) = space.filter(|_| jobs::runs_here(&recipe)) {
                runs_here.push(root.join(&jobs::board_folder(&space.storage.folder, &recipe.id)));
            }
        }
        let names: BTreeMap<String, String> = (stored.into_iter())
            .map(|space| (space.storage.folder, space.name))
            .collect();
        let record =
            Record::read(&self.db, key)?.context("The storage repository has no record")?;
        let two_machines = (self.db.prepare(
            "SELECT path FROM sync_paused WHERE repository=?1 AND json_extract(why,'$.kind')='two_machines'",
        )?)
        .query_map([key], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        let mut parts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut rows =
            (self.db).prepare("SELECT path,part FROM issue_parts WHERE repository=?1")?;
        for row in rows.query_map([key], |r| Ok((r.get(0)?, r.get(1)?)))? {
            let (path, part): (String, String) = row?;
            parts.entry(path).or_default().push(part);
        }
        let mut preferred = BTreeMap::new();
        let mut rows =
            (self.db).prepare("SELECT temporary,number FROM issue_preferred WHERE storage=?1")?;
        for row in rows.query_map([storage], |r| Ok((r.get(0)?, r.get(1)?)))? {
            let (temporary, number): (String, u64) = row?;
            preferred.insert(IssueId::parse(&temporary)?, number);
        }
        let entries = self.unlanded_entries(key)?;
        Ok(Some(Input {
            key: key.to_owned(),
            root,
            entries,
            landing,
            names,
            prefix,
            synced: record.synced,
            written: record.written,
            two_machines,
            parts,
            preferred,
            runs_here,
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
            cadences.push((key, TRACKER_CADENCE_MS));
        }
        Ok(cadences)
    }
    /// How often the settings repository is fetched: every minute once it also holds a Space.
    pub(crate) fn settings_cadence(&self) -> Result<i64> {
        let key = self.settings_key();
        for record in self.spaces()? {
            let stored = Self::storage_key(&record.space.storage.repository);
            if stored.is_some() && stored == key {
                return Ok(TRACKER_CADENCE_MS);
            }
        }
        Ok(sync::CADENCE_MS)
    }
    /// The domains whose email addresses the scans of the repository `key` let through.
    pub(crate) fn scan_domains(&self, key: &str) -> Vec<String> {
        let space = (self.spaces().into_iter().flatten())
            .find(|r| Self::storage_key(&r.space.storage.repository).as_deref() == Some(key));
        (space.and_then(|record| self.tracker(&record.space).ok()))
            .map_or_else(Vec::new, |tracker| tracker.allowed_email_domains)
    }
    /// The pass a storage repository that syncs on its own runs next.
    pub(crate) fn storage_pass(&self, key: &str) -> Result<Option<Pass>> {
        let Some(input) = self.storage_input(key)? else {
            return Ok(None);
        };
        let repository = self.stored_repository(key)?;
        let commit = Record::read(&self.db, key)?
            .context("The storage repository has no record")?
            .commit;
        let scanner = scan::Scanner::bundled(&self.home, self.scan_path.clone())?;
        Ok(Some(Pass {
            clone: repos::clone_folder(&self.home, &repository.name),
            repository,
            commit,
            scanner: scanner.in_folder(input.root.clone(), self.scan_domains(key)),
            input,
            running: self.sync_passes.clone(),
        }))
    }
    /// Records a storage repository's own pass of `key`.
    pub(crate) fn finish_storage_pass(&mut self, key: &str, passed: Result<Passed>) -> Result<()> {
        let synced = self.storage_synced.entry(key.to_owned()).or_default();
        let passed = match passed {
            Ok(passed) => passed,
            Err(error) => {
                synced.error = Some(format!("{error:#}"));
                return Err(error);
            }
        };
        synced.at = Some(now());
        synced.error = passed.error.clone();
        if let Some(error) = &passed.error {
            eprintln!("Syncing {key}: {error}");
        }
        if let Some(root) = &passed.moved_to {
            return self.finish_move(key, root);
        }
        let commit = passed.pushed.clone().unwrap_or(passed.remote);
        self.finish_storage(passed.found, passed.pushed.as_deref())?;
        self.db.execute(
            "UPDATE sync_record SET synced=?2 WHERE repository=?1 AND path=''",
            params![key, commit.as_bytes()],
        )?;
        self.brains_now = true;
        Ok(())
    }
    /// Writes what a storage repository's rule sets pulled, unless the host wrote it meanwhile,
    /// records what they synced, paused and held, and takes pulled Space definitions and tracker
    /// configs into the store. `head`: the commit their push left on GitHub.
    pub(crate) fn finish_storage(&mut self, found: Found, head: Option<&str>) -> Result<()> {
        let pushed = head.is_some();
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
        let root = Root::new(&record.space.storage.root);
        let touched = (found.pulled.iter().map(|(path, ..)| path))
            .chain(found.merged.iter().map(|(path, ..)| path));
        let before = self.issues_before(storage, touched);
        let mut pulled = vec![];
        let mut paused = found.paused;
        // Merged with GitHub's: each written here, unless the host wrote it meanwhile, and
        // recorded as last synced in GitHub's version, so this Mac's own part changes push.
        // One the host wrote meanwhile keeps the record it had, even if its merge was pushed:
        // the merge's parts from GitHub never reached it, so the next pass merges again.
        let (mut merged, mut skipped) = (vec![], vec![]);
        for (path, here, kept, there) in found.merged {
            if fs::read(clone.join(&path)).ok() != here {
                skipped.push(path);
                continue;
            }
            if here.as_ref() != Some(&kept) {
                shared::write_atomically(&clone.join(&path), &kept)?;
            }
            merged.push((path, kept, there));
        }
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
                None => {
                    drop(fs::remove_file(clone.join(&path)));
                    tracker::remove_emptied(
                        &root.in_clone(&clone),
                        root.strip(&path).unwrap_or(&path),
                    );
                }
            }
            pulled.push((path, there));
        }
        let mut synced = found.alike;
        synced.extend(
            (found.committed.into_iter()).filter(|(path, _)| pushed && !skipped.contains(path)),
        );
        // A rule set whose compare failed keeps its notes until a pass compares it again.
        let ours = |path: &str| root.scanned(path) && !under(&found.failed_roots, path);
        let tx = self.db.unchecked_transaction()?;
        for (path, kept, there) in &merged {
            tx.execute(
                "INSERT INTO sync_record(repository,path,synced,written) VALUES (?1,?2,?3,?4) ON CONFLICT(repository,path) DO UPDATE SET synced=excluded.synced, written=excluded.written",
                params![key, path, there, kept],
            )?;
        }
        for path in &found.parts_compared {
            tx.execute(
                "DELETE FROM issue_parts WHERE repository=?1 AND path=?2",
                params![key, path],
            )?;
        }
        for part in &found.parts {
            tx.execute(
                "INSERT OR REPLACE INTO issue_parts(repository,path,part,theirs,by) VALUES (?1,?2,?3,?4,?5)",
                params![key, part.path, part.part, part.theirs, part.by],
            )?;
        }
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
                "INSERT INTO sync_held(repository,path,check_name,rule,reason,line) VALUES (?1,?2,?3,?4,?5,?6)",
                params![key, held.path, held.check, held.rule, held.reason, held.line],
            )?;
        }
        tx.commit()?;
        self.finish_entries(&key, found.entries, pushed)?;
        if let Some((landing, landed)) = found.landing {
            self.finish_landing(landing, landed, head, &clone)?;
        }
        for (path, content) in pulled {
            self.take_pulled(storage, &path, content.as_deref())?;
        }
        for (path, ..) in &merged {
            if let Some(id) = root.issue_of(path) {
                self.reindex_issue(storage, &id)?;
            }
        }
        self.note_issue_news(storage, before);
        if pushed {
            self.apply_numbers(storage, &found.numbered)?;
        }
        self.apply_numbers(storage, &found.adopted)?;
        // A file put back by hand since no longer holds back its issue's work.
        self.follow_pending_work(storage);
        // The pass compared every issue file, so an edit made by hand is indexed now too.
        self.refresh_issue_index(Some(&storage.key()))?;
        Ok(())
    }
    /// Takes a pulled `space.yaml` of a Space on this Mac, or the tracker's config, into the
    /// store, and a pulled issue's file into the index, by its path in the repository.
    pub(crate) fn take_pulled(
        &mut self,
        storage: &StorageRepository,
        path: &str,
        content: Option<&[u8]>,
    ) -> Result<()> {
        let root = self.root_of(storage)?;
        let Some(path) = root.strip(path) else {
            return Ok(());
        };
        if let Some(id) = tracker::issue_of(path) {
            return self.reindex_issue(storage, &id);
        }
        if let Some((folder, id)) = recipe_path(path) {
            let space = (self.spaces()?.into_iter()).find(|r| {
                &r.space.storage.repository == storage && r.space.storage.folder == folder
            });
            return match space {
                Some(record) => self.take_pulled_recipe(&record.space, id, content),
                None => Ok(()),
            };
        }
        let Some(content) = content else {
            return Ok(());
        };
        if path == TRACKER {
            let tracker = read_tracker(content)?;
            self.db.execute(
                "INSERT INTO trackers VALUES (?1,?2) ON CONFLICT(storage) DO UPDATE SET data=excluded.data",
                params![storage.key(), encode(&tracker)?],
            )?;
            let mut spaces = self.spaces()?.into_iter().map(|r| r.space);
            if let Some(space) = spaces.find(|s| &s.storage.repository == storage) {
                self.reindex_tracker(&space, &tracker.prefix)?;
            }
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
    /// What the Space page's Storage and issues box shows about the Space's storage repository.
    pub fn storage_status(&self, space_id: &SpaceId) -> Result<StorageStatus> {
        let space = self.space(space_id)?.space;
        let folder = self.storage_folder(&space.storage.repository);
        let prefix = self.tracker(&space).ok().map(|tracker| tracker.prefix);
        let path = folder.to_string_lossy().into_owned();
        self.refresh_issue_index(Some(&space.storage.repository.key()))?;
        let issues: usize = self.db.query_row(
            "SELECT count(*) FROM issue_index WHERE storage=?1",
            [space.storage.repository.key()],
            |r| r.get(0),
        )?;
        let Some(key) = Self::storage_key(&space.storage.repository) else {
            return Ok(StorageStatus {
                prefix,
                issues,
                readers: None,
                settings_repository: false,
                path,
                synced_at: None,
                error: None,
            });
        };
        let repository = self.stored_repository(&key)?;
        let settings_repository = self.settings_key().as_ref() == Some(&key);
        let (synced_at, error) = self.storage_sync_state(&key)?;
        Ok(StorageStatus {
            prefix,
            issues,
            readers: Some(repository.readers),
            settings_repository,
            path,
            synced_at,
            error,
        })
    }
    /// When a pass of the GitHub storage repository `key` last reached GitHub, and why the
    /// latest one failed: the settings sync's, when it is also the settings repository.
    pub(crate) fn storage_sync_state(&self, key: &str) -> Result<(Option<i64>, Option<String>)> {
        if self.settings_key().as_deref() == Some(key) {
            return Ok((self.sync_status()?).map_or((None, None), |s| (s.synced_at, s.error)));
        }
        Ok((self.storage_synced.get(key)).map_or((None, None), |s| (s.at, s.error.clone())))
    }
    /// The settings repository's record key, `None` without one. Reads only this Mac's settings
    /// file, as the shared folder's recorder asks while the shared settings are read.
    pub(crate) fn settings_key(&self) -> Option<String> {
        shared::chosen(&self.home).map(|r| repos::key(&r.name))
    }
    /// One note per file a scan held back from a push.
    pub(crate) fn held_notes(&self) -> Result<Vec<WorkspaceNote>> {
        let rows: Vec<(String, scan::Held)> = (self.db.prepare(
            "SELECT repository,path,check_name,rule,reason,line FROM sync_held ORDER BY repository,path",
        )?)
        .query_map([], |r| {
            let held = scan::Held {
                path: r.get(1)?,
                check: r.get(2)?,
                rule: r.get(3)?,
                reason: r.get(4)?,
                line: r.get(5)?,
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
                    line,
                } = held;
                let repository = (storage.iter())
                    .find(|r| repos::key(&r.name) == key)
                    .map_or(key, |r| r.name.clone());
                let at = line.map_or(String::new(), |line| format!(" on line {line}"));
                let why = match (&rule, &reason) {
                    (Some(rule), _) => format!("the {check} check found a possible match of its {rule} rule{at}"),
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
                    line,
                    text,
                })
            })
            .collect())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sync::tests::{GitHub, NAME};
    use crate::test_support::{git, scan_tools};
    use std::sync::Arc;

    pub(crate) fn host() -> (TempDir, Host) {
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
    pub(crate) fn sage(description: &str) -> String {
        let space = Space {
            id: SpaceId("sage-id".into()),
            name: "Sage".into(),
            description: description.into(),
            additions: "Name the ticket.".into(),
            behaviors: Default::default(),
            repository_settings: Default::default(),
            brain: None,
            storage: SpaceStorage {
                repository: github(NAME),
                folder: "sage".into(),
                root: String::new(),
            },
            repositories: vec!["Sage-Social/api".into()],
            models: SpaceModels::default(),
            reviews: Default::default(),
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
            additions: "",
            prefix,
        }
    }

    /// A storage repository on GitHub holding Sage and, with `prefix`, a tracker.
    pub(crate) fn shared_storage(folder: &Path, prefix: Option<&str>) -> Arc<GitHub> {
        let github = Arc::new(GitHub::new(folder, true));
        github.commit("spaces/sage/space.yaml", sage("Secure family messaging"));
        if let Some(prefix) = prefix {
            github.commit(TRACKER, format!("format: 1\nprefix: {prefix}\n"));
        }
        github
    }

    pub(crate) fn join_sage(host: &mut Host, github: &Arc<GitHub>) -> Result<Vec<SpaceRecord>> {
        join_sage_with(host, github, vec![])
    }

    /// Joins Sage with `clones` as the clones of its code repositories on this Mac.
    fn join_sage_with(
        host: &mut Host,
        github: &Arc<GitHub>,
        clones: Vec<String>,
    ) -> Result<Vec<SpaceRecord>> {
        let staged = stage(&host.home, NAME, &github.tools(), &[])?;
        let ticked = [JoinSpace {
            folder: "sage".into(),
            brain_path: None,
            clones,
        }];
        host.join_spaces(staged, &ticked)
    }

    /// Runs the pass of the storage repository `NAME`.
    pub(crate) fn sync(host: &mut Host, github: &Arc<GitHub>) {
        let key = repos::key(NAME);
        let pass = host.storage_pass(&key).unwrap().unwrap();
        host.finish_storage_pass(&key, self::pass(pass, &github.tools()))
            .unwrap();
    }

    pub(crate) fn space_named(host: &Host, name: &str) -> Space {
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
            [
                "wiffletree/spaces/woodworking/space.yaml",
                "wiffletree/wiffletree-issues.yaml"
            ]
        );
        assert_eq!(git(&folder, &["rev-list", "--count", "HEAD"]), "1");
        let in_folder = folder.join(DEFAULT_ROOT);
        let tracker = read_tracker(&fs::read(in_folder.join(TRACKER)).unwrap()).unwrap();
        assert_eq!(tracker.prefix, "WOOD");
        let file = fs::read(in_folder.join("spaces/woodworking/space.yaml")).unwrap();
        assert_eq!(read_space(&file, space.storage.clone()).unwrap(), space);
        // Personal, from the store's upgrade, gets its repository at start, once.
        let personal = host.storage_folder(&personal(&host).storage.repository);
        assert_eq!(
            committed(&personal),
            [
                "wiffletree/spaces/personal/space.yaml",
                "wiffletree/wiffletree-issues.yaml"
            ]
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
        let shown = "HEAD:wiffletree/spaces/personal/space.yaml";
        let committed = git(&folder, &["show", shown]);
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
  threads:
    provider: claude
    model: opus
    effort: high
    future_option: keep-model
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
            root: String::new(),
        };
        read_space(FROM_A_NEWER_MAC.as_bytes(), storage).unwrap()
    }

    fn written(space: &Space) -> serde_yaml::Value {
        serde_yaml::from_slice(&space_yaml(space).unwrap()).unwrap()
    }

    #[test]
    fn a_spaces_additions_are_stored_as_instructions_as_older_hosts_read_them() {
        let storage = SpaceStorage {
            repository: super::github(NAME),
            folder: "sage".into(),
            root: String::new(),
        };
        let old = "format: 1\nid: sage-id\nname: Sage\ninstructions: Never log names.\n";
        let space = read_space(old.as_bytes(), storage).unwrap();
        assert_eq!(space.additions, "Never log names.");
        let file = written(&space);
        assert_eq!(file["instructions"], "Never log names.");
        assert_eq!(file.get("additions"), None);
    }

    #[test]
    fn behaviors_and_repository_settings_keep_what_this_release_does_not_know() {
        let storage = SpaceStorage {
            repository: super::github(NAME),
            folder: "sage".into(),
            root: String::new(),
        };
        let newer = "\
format: 1
id: sage-id
name: Sage
repositories: [Sage-Social/api]
behaviors:
  review_depth: full
  future_behavior: often
repository_settings:
  Sage-Social/api:
    instructions: Verify webhook signatures.
    future_key: 3
";
        let space = Space {
            description: "Changed here".into(),
            ..read_space(newer.as_bytes(), storage).unwrap()
        };
        assert_eq!(space.behaviors["review_depth"], "full");
        let api = space.repository_settings("sage-social/API").unwrap();
        assert_eq!(api.additions, "Verify webhook signatures.");
        let file = written(&space);
        assert_eq!(file["behaviors"]["future_behavior"], "often");
        let api = &file["repository_settings"]["Sage-Social/api"];
        assert_eq!(
            (&api["future_key"], &api["instructions"]),
            (&3.into(), &"Verify webhook signatures.".into())
        );
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
        assert_eq!(file["models"]["threads"]["future_option"], "keep-model");
        assert_eq!(file["models"]["threads"]["model"], "opus");
        assert_eq!(file["sources"][0]["note"], "first");
        assert_eq!(file["sources"][1]["note"], "second");
        // Read back by this release, the file is the same Space.
        let again = read_space(&space_yaml(&space).unwrap(), space.storage.clone()).unwrap();
        assert_eq!(again, space);
    }

    #[test]
    fn a_local_brain_repository_keeps_its_unknown_keys() {
        let text = "format: 1\nid: home\nname: Home\nbrain:\n  repository:\n    kind: local\n    future_option: keep-repository\n  folder: second-brain\n";
        let storage = SpaceStorage {
            repository: super::github(NAME),
            folder: "home".into(),
            root: String::new(),
        };
        let space = Space {
            description: "Changed here".into(),
            ..read_space(text.as_bytes(), storage).unwrap()
        };
        let file = written(&space);
        assert_eq!(file["brain"]["repository"]["kind"], "local");
        assert_eq!(
            file["brain"]["repository"]["future_option"],
            "keep-repository"
        );
    }

    #[test]
    fn a_space_file_naming_a_storage_is_refused_and_paused_when_pulled() {
        let storage = SpaceStorage {
            repository: super::github(NAME),
            folder: "sage".into(),
            root: String::new(),
        };
        let named = sage("") + "storage: a teammate's note\n";
        let error = read_space(named.as_bytes(), storage).unwrap_err();
        assert!(
            format!("{error:#}").contains("names a storage"),
            "{error:#}"
        );
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        github.commit("spaces/sage/space.yaml", &named);
        sync(&mut host, &github);
        let notes = host.workspace_notes(&[]).unwrap();
        assert!(
            matches!(notes.as_slice(), [WorkspaceNote::Paused(p)]
                if matches!(p.paused, Paused::CannotBeRead { .. })),
            "{notes:?}"
        );
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(
            space_named(&host, "Sage").description,
            "Secure family messaging"
        );
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
        github.commit(TRACKER, "format: 1\nprefix: SAGE\ncolors: [teal]\n");
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.set_tracker_prefix(&sage.id, "SG").unwrap();
        sync(&mut host, &github);
        let tracker = github.show(TRACKER).unwrap();
        assert!(
            tracker.contains("prefix: SG") && tracker.contains("- teal"),
            "{tracker}"
        );
    }

    /// `wiffletree-issues.yaml` as this release writes a new tracker's.
    fn tracker_file(prefix: &str) -> String {
        String::from_utf8(tracker_yaml(&TrackerConfig::new(prefix.into())).unwrap()).unwrap()
    }

    #[test]
    fn renaming_a_local_prefix_rewrites_its_issue_ids_folders_and_references() {
        let (_home, mut host) = host();
        let space = personal(&host);
        let folder = host.root_folder(&space.storage.repository).unwrap();
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
        assert_eq!(github.show(TRACKER).unwrap(), tracker_file("SG"));
        let issue = repos::clone_folder(home.path(), NAME).join("issues/SG-1/issue.md");
        fs::create_dir_all(issue.parent().unwrap()).unwrap();
        fs::write(issue, "---\nid: SG-1\n---\n").unwrap();
        let error = host.set_tracker_prefix(&sage.id, "SAGE").unwrap_err();
        assert!(format!("{error:#}").contains("changes only while it has none"));
    }

    #[test]
    fn an_issue_another_mac_pushed_keeps_a_shared_trackers_prefix() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        github.commit("issues/SAGE-1/issue.md", "---\nid: SAGE-1\n---\n");
        sync(&mut host, &github);
        let clone = repos::clone_folder(home.path(), NAME);
        assert!(clone.join("issues/SAGE-1/issue.md").exists(), "pulled");
        let sage = space_named(&host, "Sage");
        let error = host.set_tracker_prefix(&sage.id, "SG").unwrap_err();
        assert!(format!("{error:#}").contains("changes only while it has none"));
        assert_eq!(github.show(TRACKER).unwrap(), "format: 1\nprefix: SAGE\n");
    }

    #[test]
    fn a_new_space_on_github_pushes_its_folder_and_the_new_trackers_config_a_commit_each() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (home, mut host) = host();
        let staged = stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        let new = NewSpace {
            additions: "Name the ticket.",
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
        let (tracker, file) = (
            "wiffletree/wiffletree-issues.yaml",
            "wiffletree/spaces/sage/space.yaml",
        );
        for commit in ["main", "main~"] {
            let changed = git(&github.bare, &["show", "--format=", "--name-only", commit]);
            assert!(changed == tracker || changed == file, "{changed}");
        }
        let pushed = github.show(file).unwrap();
        assert!(pushed.contains("instructions: Name the ticket."));
        assert_eq!(github.show(tracker).unwrap(), tracker_file("SAGE"));
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
        assert!(github.show("wiffletree/spaces/sage/space.yaml").is_some());
        assert_eq!(
            github.show("wiffletree/wiffletree-issues.yaml").unwrap(),
            tracker_file("SAGE")
        );
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
        host.finish_storage_pass(&repos::key(NAME), running.join().unwrap())
            .unwrap();
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
    fn a_space_whose_brain_is_its_own_storage_is_joined_and_taken_when_pulled() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (owner, name) = NAME.split_once('/').unwrap();
        let brain = format!(
            "brain:\n  repository:\n    kind: git_hub\n    owner: {owner}\n    name: {name}\n  folder: second-brain\n"
        );
        let own_brain = sage("Its storage as its brain") + &brain;
        github.commit("spaces/sage/space.yaml", &own_brain);
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        assert!(space_named(&host, "Sage").brain.is_some());
        github.commit("spaces/sage/space.yaml", sage("Fine"));
        sync(&mut host, &github);
        github.commit("spaces/sage/space.yaml", &own_brain);
        sync(&mut host, &github);
        assert!(space_named(&host, "Sage").brain.is_some());
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
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
    fn a_repository_named_as_a_brain_may_also_be_storage_and_settings() {
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
            brain: Some(brain),
            ..personal
        })
        .unwrap();
        (host.choose_settings_repository(NAME, None, &github.tools())).unwrap();
        join_sage(&mut host, &github).unwrap();
        assert_eq!(
            space_named(&host, "Sage").storage.repository,
            super::github(NAME)
        );
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
                root: String::new(),
            },
        )
        .unwrap();
        sage.models.jobs = Some(
            ModelProfile {
                provider: Provider::Claude,
                model: "opus".into(),
                effort: "high".into(),
            }
            .into(),
        );
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
            additions: format!("Text the parent at {number}."),
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
            additions: "Text the parent at (415) 555-0134.".into(),
            ..space.clone()
        })
        .unwrap();
        let folder = host.storage_folder(&space.storage.repository);
        let shown = "HEAD:wiffletree/spaces/personal/space.yaml";
        let committed = git(&folder, &["show", shown]);
        assert!(committed.contains("555-0134"));
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
    }

    /// A clone at `folder` of the GitHub repository `identity`, with one commit.
    fn clone_of(folder: &Path, identity: &str) -> String {
        fs::create_dir_all(folder).unwrap();
        git(folder, &["init", "-q", "-b", "main"]);
        let url = format!("https://github.com/{identity}.git");
        git(folder, &["remote", "add", "origin", &url]);
        git(folder, &["commit", "-q", "--allow-empty", "-m", "First"]);
        folder.to_str().unwrap().to_owned()
    }

    #[test]
    fn joining_takes_the_clones_of_the_spaces_repositories_here_and_leaves_the_rest_missing() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        let check = stage(&host.home, NAME, &github.tools(), &[]).unwrap().check;
        assert_eq!(check.spaces[0].repositories, ["Sage-Social/api"]);
        let elsewhere = clone_of(&folder.path().join("site"), "Vyttle-LLC/site");
        let error = format!(
            "{:#}",
            join_sage_with(&mut host, &github, vec![elsewhere]).unwrap_err()
        );
        assert!(
            error.contains("is a clone of Vyttle-LLC/site, which Sage does not list"),
            "{error}"
        );
        assert!(
            host.find_space(&SpaceId("sage-id".into()))
                .unwrap()
                .is_none()
        );
        assert!(
            host.repositories().unwrap().is_empty(),
            "nothing is written"
        );
        let api = clone_of(&folder.path().join("api"), "Sage-Social/api");
        join_sage_with(&mut host, &github, vec![api.clone()]).unwrap();
        let here = host.repositories().unwrap();
        let path = fs::canonicalize(&api).unwrap();
        assert_eq!(
            (here.iter().map(|r| Path::new(&r.path)).collect::<Vec<_>>()),
            [path.as_path()],
            "the clone joins the workspace, whose GitHub read then names it"
        );
        let (_home, mut other) = self::host();
        join_sage(&mut other, &github).unwrap();
        let sage = space_named(&other, "Sage");
        let missing = other.space_repositories(&sage.id).unwrap();
        assert!(missing.repository_ids.is_empty());
        assert_eq!(missing.not_cloned, ["Sage-Social/api"], "left missing");
    }

    #[test]
    fn the_storage_box_counts_issues_and_says_who_reads_them_and_when_they_synced() {
        let (_home, mut host) = host();
        let personal = personal(&host);
        let local = host.storage_folder(&personal.storage.repository);
        let prefix = host.tracker(&personal).unwrap().prefix;
        host.record_gh_login(Some("jlin".into()));
        for title in ["Renew the passport", "Book the dentist"] {
            let new = NewIssue {
                title: title.into(),
                ..NewIssue::default()
            };
            host.create_issue(&personal.id, new).unwrap();
        }
        fs::create_dir_all(local.join("issues/notes")).unwrap();
        let status = host.storage_status(&personal.id).unwrap();
        assert_eq!(status.prefix.as_deref(), Some(prefix.as_str()));
        assert_eq!((status.issues, status.readers), (2, None));
        assert_eq!(status.path, local.to_str().unwrap());
        assert_eq!(
            (status.synced_at, status.settings_repository),
            (None, false)
        );

        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), None);
        github.commit("issues/SAGE-1/issue.md", "---\nid: SAGE-1\n---\n");
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let status = host.storage_status(&sage.id).unwrap();
        assert_eq!(
            (status.prefix, status.issues),
            (None, 0),
            "Set up issues is offered"
        );
        assert_eq!(status.readers, Some(Readers::Invited));
        assert_eq!(status.synced_at, None);
        host.set_up_issues(&sage.id, "SAGE").unwrap();
        sync(&mut host, &github);
        let status = host.storage_status(&sage.id).unwrap();
        assert_eq!(status.prefix.as_deref(), Some("SAGE"));
        assert_eq!(
            status.issues, 1,
            "the clone's issue is indexed once it is a tracker"
        );
        assert!(status.synced_at.is_some() && status.error.is_none());
    }

    #[test]
    fn a_clone_the_workspace_would_refuse_fails_the_join_before_anything_is_written() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (home, mut host) = host();
        let api = clone_of(&folder.path().join("api"), "Sage-Social/api");
        let inside = Path::new(&api).join("src");
        fs::create_dir_all(&inside).unwrap();
        let error =
            join_sage_with(&mut host, &github, vec![inside.to_str().unwrap().into()]).unwrap_err();
        assert!(
            format!("{error:#}").contains("Expected a Git repository root"),
            "{error:#}"
        );
        assert!(
            host.find_space(&SpaceId("sage-id".into()))
                .unwrap()
                .is_none()
        );
        assert!(host.repositories().unwrap().is_empty());
        assert!(
            !repos::clone_folder(home.path(), NAME).exists(),
            "the storage clone is not placed"
        );
    }

    const SAGE_FILE: &str = "wiffletree/spaces/sage/space.yaml";

    /// A repository on GitHub with Sage and a tracker already in `wiffletree/`.
    fn in_a_folder(folder: &Path) -> Arc<GitHub> {
        let github = Arc::new(GitHub::new(folder, true));
        github.commit(SAGE_FILE, sage("Secure family messaging"));
        github.commit(
            "wiffletree/wiffletree-issues.yaml",
            "format: 1\nprefix: SAGE\n",
        );
        github
    }

    /// The human's Move into the folder, on this Mac.
    fn press_move(host: &mut Host, github: &Arc<GitHub>) {
        let sage = space_named(host, "Sage");
        let moving = host.start_move(&sage.id).unwrap();
        let root = moving.run(&github.tools()).unwrap();
        host.finish_move(&moving.key, &root).unwrap();
    }

    fn issue(id: &str) -> Vec<u8> {
        tracker::issue_md(&tracker::tests::blurry(id)).unwrap()
    }

    fn moves_on(github: &GitHub) -> usize {
        let subjects = github.log("%s");
        (subjects.iter())
            .filter(|subject| subject.starts_with("Move into wiffletree/"))
            .count()
    }

    #[test]
    fn a_repository_in_a_folder_joins_it_and_syncs_only_inside_it() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        assert_eq!(sage.storage.root, "wiffletree");
        github.commit("README.md", "Changed on GitHub.\n");
        let filed = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        host.create_issue(&sage.id, filed).unwrap();
        host.update_space(Space {
            description: "Changed here".into(),
            ..sage
        })
        .unwrap();
        sync(&mut host, &github);
        assert!(github.show("wiffletree/issues/SAGE-1/issue.md").is_some());
        assert!(github.show(SAGE_FILE).unwrap().contains("Changed here"));
        assert_eq!(github.show("issues/SAGE-1/issue.md"), None);
        let clone = repos::clone_folder(home.path(), NAME);
        let readme = fs::read_to_string(clone.join("README.md")).unwrap();
        assert_eq!(readme, "Changed on GitHub.\n", "pulled");
        let changed = git(
            &github.bare,
            &["log", "--format=", "--name-only", "main~2.."],
        );
        assert!(changed.lines().all(|path| path.starts_with("wiffletree/")));
        let (ids, paused) = (
            host.list_issues(None, None).unwrap(),
            host.workspace_notes(&[]).unwrap(),
        );
        assert_eq!(ids[0].id.to_string(), "SAGE-1");
        assert!(paused.is_empty(), "{paused:?}");
    }

    #[test]
    fn two_spaces_share_one_folder_and_its_tracker() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let watch = sage("A second Space").replace("sage-id", "watch-id");
        let watch = watch.replace("name: Sage", "name: Sage Watch");
        github.commit("wiffletree/spaces/watch/space.yaml", watch);
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let staged = stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        assert_eq!(staged.check.spaces.len(), 2);
        let tick = |folder: &str| JoinSpace {
            folder: folder.into(),
            brain_path: None,
            clones: vec![],
        };
        (host.join_spaces(staged, &[tick("sage"), tick("watch")])).unwrap();
        let (sage, watch) = (space_named(&host, "Sage"), space_named(&host, "Sage Watch"));
        assert_eq!(sage.storage.root, watch.storage.root);
        assert_eq!(host.tracker(&watch).unwrap().prefix, "SAGE");
        host.update_space(Space {
            description: "From the watch".into(),
            ..watch.clone()
        })
        .unwrap();
        let filed = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        host.create_issue(&watch.id, filed).unwrap();
        sync(&mut host, &github);
        let pushed = github.show("wiffletree/spaces/watch/space.yaml").unwrap();
        assert!(pushed.contains("From the watch"));
        assert!(github.show("wiffletree/issues/SAGE-1/issue.md").is_some());
        assert_eq!(host.list_issues(Some(&sage.id), None).unwrap().len(), 1);
    }

    #[test]
    fn a_settings_edit_commits_at_the_root_beside_an_issue_in_the_folder() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let tools = github.tools();
        host.choose_settings_repository(NAME, None, &tools).unwrap();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let filed = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        host.create_issue(&sage.id, filed).unwrap();
        host.set_check_ins(20, 200).unwrap();
        host.sync_now(&tools).unwrap();
        assert!(github.show("wiffletree/issues/SAGE-1/issue.md").is_some());
        for commit in ["main", "main~"] {
            let changed = git(&github.bare, &["show", "--format=", "--name-only", commit]);
            let inside = changed.lines().all(|path| path.starts_with("wiffletree/"));
            let outside = !changed.lines().any(|path| path.starts_with("wiffletree/"));
            assert!(inside || outside, "{changed}");
        }
        assert!(github.show("wiffletree.json").is_some());
    }

    #[test]
    fn a_repository_with_an_empty_root_keeps_todays_paths() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        assert_eq!(sage.storage.root, "");
        let filed = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        host.create_issue(&sage.id, filed).unwrap();
        sync(&mut host, &github);
        assert!(github.show("issues/SAGE-1/issue.md").is_some());
        assert_eq!(moves_on(&github), 0, "a GitHub repository waits for Move");
        assert_eq!(github.show(SAGE_FILE), None);
    }

    #[test]
    fn a_repository_of_other_files_gets_the_folder_and_one_that_is_someone_elses_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_home, host) = host();
        let staged = stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        assert_eq!(staged.root, Root::default_folder());
        github.commit("wiffletree/notes.md", "Someone's notes.\n");
        let error = match stage(&host.home, NAME, &github.tools(), &[]) {
            Err(error) => format!("{error:#}"),
            Ok(_) => panic!("refused"),
        };
        assert!(error.contains("must be Wiffletree's"), "{error}");
    }

    #[test]
    fn two_folders_in_a_repository_are_refused_with_nothing_written() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        github.commit("other/spaces/watch/space.yaml", sage("Another"));
        let (home, host) = host();
        let error = match stage(&host.home, NAME, &github.tools(), &[]) {
            Err(error) => format!("{error:#}"),
            Ok(_) => panic!("refused"),
        };
        assert!(error.contains("other/ and wiffletree/"), "{error}");
        assert!(!repos::clone_folder(home.path(), NAME).exists());
    }

    /// Puts a local repository back as an earlier release left it, with its Spaces' files at
    /// its root and a Space that records no folder.
    fn at_the_root_again(host: &Host) {
        let mut space = personal(host);
        let folder = host.storage_folder(&space.storage.repository);
        git(&folder, &["mv", "wiffletree/spaces", "spaces"]);
        git(
            &folder,
            &["mv", "wiffletree/wiffletree-issues.yaml", TRACKER],
        );
        if folder.join("wiffletree/issues").exists() {
            git(&folder, &["mv", "wiffletree/issues", "issues"]);
        }
        git(&folder, &["commit", "-qm", "As an earlier release left it"]);
        space.storage.root = String::new();
        (host.db)
            .execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![space.id.as_str(), encode(&space).unwrap()],
            )
            .unwrap();
    }

    #[test]
    fn a_local_repository_moves_once_at_first_start() {
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let space = personal(&host);
        let filed = NewIssue {
            title: "Renew the passport".into(),
            ..NewIssue::default()
        };
        let filed = host.create_issue(&space.id, filed).unwrap();
        at_the_root_again(&host);
        drop(host);
        let host = Host::open(home.path()).unwrap();
        let space = personal(&host);
        assert_eq!(space.storage.root, "wiffletree");
        let folder = host.storage_folder(&space.storage.repository);
        let files = committed(&folder);
        assert!(files.contains(&"wiffletree/spaces/personal/space.yaml".to_owned()));
        assert!(files.contains(&format!("wiffletree/issues/{}/issue.md", filed.id)));
        let stub = fs::read_to_string(folder.join("spaces/personal/space.yaml")).unwrap();
        assert!(stub.contains("moved_to: wiffletree"), "{stub}");
        assert!(!folder.join("issues").exists());
        assert_eq!(git(&folder, &["status", "--porcelain"]), "");
        assert_eq!(host.read_issue(&filed.id).unwrap().id, filed.id);
        let head = git(&folder, &["rev-parse", "HEAD"]);
        let subject = git(&folder, &["log", "-1", "--format=%s"]);
        assert_eq!(subject, "Move into wiffletree/");
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(
            git(&folder, &["rev-parse", "HEAD"]),
            head,
            "a second start moves nothing"
        );
        assert_eq!(personal(&host).storage.root, "wiffletree");
    }

    #[test]
    fn a_local_repository_moves_at_first_start_on_a_store_from_before_entry_files() {
        let (home, host) = host();
        at_the_root_again(&host);
        drop(host);
        let db = Connection::open(home.path().join(STORE_FILE)).unwrap();
        db.execute_batch("DROP TABLE entry_files; ALTER TABLE sync_held DROP COLUMN line;")
            .unwrap();
        drop(db);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(personal(&host).storage.root, "wiffletree");
    }

    #[test]
    fn a_move_a_start_cut_short_is_finished_at_the_next_start() {
        let (home, host) = host();
        at_the_root_again(&host);
        let space = personal(&host);
        let folder = host.storage_folder(&space.storage.repository);
        // The commit landed, and the start stopped before anything else.
        let head = git(&folder, &["rev-parse", "HEAD"]);
        folder::stage_move(&folder, &head, &Root::default_folder()).unwrap();
        git(&folder, &["commit", "-qm", "Move into wiffletree/"]);
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(personal(&host).storage.root, "wiffletree");
        assert_eq!(git(&folder, &["rev-list", "--count", "HEAD~.."]), "1");
        assert_eq!(git(&folder, &["status", "--porcelain"]), "");
        assert!(
            folder
                .join("wiffletree/spaces/personal/space.yaml")
                .exists()
        );
    }

    #[test]
    fn a_github_repository_moves_by_the_host_call_with_a_stub_for_each_space_and_the_tracker() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", issue("SAGE-1"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        press_move(&mut host, &github);
        assert_eq!(moves_on(&github), 1);
        assert!(github.show(SAGE_FILE).unwrap().contains("Secure family"));
        assert!(github.show("wiffletree/issues/SAGE-1/issue.md").is_some());
        assert_eq!(github.show("issues/SAGE-1/issue.md"), None);
        let stub = github.show("spaces/sage/space.yaml").unwrap();
        assert!(stub.contains("format: 2") && stub.contains("moved_to: wiffletree"));
        assert!(
            github
                .show(TRACKER)
                .unwrap()
                .contains("moved_to: wiffletree")
        );
        assert!(github.show("README.md").is_some());
        assert_eq!(space_named(&host, "Sage").storage.root, "wiffletree");
        sync(&mut host, &github);
        assert_eq!(
            github.log("%s").len(),
            5,
            "a pass after the move adds nothing"
        );
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
        let clone = repos::clone_folder(home.path(), NAME);
        let stub = fs::read_to_string(clone.join("spaces/sage/space.yaml")).unwrap();
        assert!(stub.contains("moved_to"), "{stub}");
        assert!(!clone.join("issues").exists());
        assert_eq!(host.list_issues(None, None).unwrap().len(), 1);
        let sage = space_named(&host, "Sage");
        let error = host.start_move(&sage.id).err().unwrap();
        assert!(format!("{error:#}").contains("already in wiffletree/"));
    }

    #[test]
    fn a_local_repository_has_no_move_to_ask_for() {
        let (_home, host) = host();
        let error = host.start_move(&personal(&host).id).err().unwrap();
        assert!(format!("{error:#}").contains("at start"), "{error:#}");
    }

    #[test]
    fn two_macs_moving_at_once_make_one_commit() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_first_home, mut first) = host();
        let (_second_home, mut second) = host();
        join_sage(&mut first, &github).unwrap();
        join_sage(&mut second, &github).unwrap();
        // The second Mac's move lands just before the first Mac's push.
        let (bare, work) = (github.bare.clone(), tempfile::tempdir().unwrap());
        *github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || {
                git(work.path(), &["clone", "-q", bare.to_str().unwrap(), "."]);
                let head = git(work.path(), &["rev-parse", "HEAD"]);
                folder::stage_move(work.path(), &head, &Root::default_folder()).unwrap();
                git(
                    work.path(),
                    &["commit", "-qm", "Move into wiffletree/ · Other"],
                );
                git(work.path(), &["push", "-q", "origin", "main"]);
            }),
        ));
        press_move(&mut first, &github);
        assert_eq!(moves_on(&github), 1);
        assert_eq!(space_named(&first, "Sage").storage.root, "wiffletree");
        sync(&mut first, &github);
        sync(&mut second, &github);
        sync(&mut second, &github);
        assert_eq!(space_named(&second, "Sage").storage.root, "wiffletree");
        assert_eq!(moves_on(&github), 1);
    }

    #[test]
    fn a_move_refused_because_a_teammate_pushed_is_made_again_on_the_new_head() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let teammate = github.clone();
        *github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || teammate.commit("issues/SAGE-1/issue.md", issue("SAGE-1"))),
        ));
        press_move(&mut host, &github);
        assert_eq!(moves_on(&github), 1);
        assert!(github.show("wiffletree/issues/SAGE-1/issue.md").is_some());
        assert_eq!(github.show("issues/SAGE-1/issue.md"), None);
    }

    #[test]
    fn a_stray_tracker_beside_the_layout_does_not_stop_a_mac_adopting_a_teammates_move() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", issue("SAGE-1"));
        let (home, mut host) = host();
        let (_teammate_home, mut teammate) = self::host();
        join_sage(&mut host, &github).unwrap();
        join_sage(&mut teammate, &github).unwrap();
        let stray = "archive/wiffletree-issues.yaml";
        github.commit(stray, "format: 1\nprefix: OLD\n");
        press_move(&mut teammate, &github);
        assert_eq!(moves_on(&github), 1);
        let commits = github.log("%s").len();
        sync(&mut host, &github);
        assert_eq!(space_named(&host, "Sage").storage.root, "wiffletree");
        sync(&mut host, &github);
        assert_eq!(github.log("%s").len(), commits, "this Mac pushes nothing");
        assert!(github.show("wiffletree/issues/SAGE-1/issue.md").is_some());
        assert!(github.show(stray).is_some());
        let clone = repos::clone_folder(home.path(), NAME);
        assert!(clone.join("wiffletree/issues/SAGE-1/issue.md").exists());
        assert_eq!(host.list_issues(None, None).unwrap().len(), 1);
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
    }

    #[test]
    fn an_unpushed_comment_survives_adopting_a_teammates_move_and_is_pushed_under_the_folder() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", issue("SAGE-1"));
        let (home, mut host) = host();
        let (_teammate_home, mut teammate) = self::host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        join_sage(&mut teammate, &github).unwrap();
        let id = tracker::tests::id("SAGE-1");
        let comment = host.comment_on_issue(&id, "Seen on two watches.").unwrap();
        press_move(&mut teammate, &github);
        sync(&mut host, &github);
        assert_eq!(space_named(&host, "Sage").storage.root, "wiffletree");
        let clone = repos::clone_folder(home.path(), NAME);
        let moved = format!("wiffletree/issues/SAGE-1/comments/{}", comment.file);
        assert!(clone.join(&moved).exists(), "it moved with its issue");
        assert!(!clone.join("issues").exists());
        assert!(host.storage_written.borrow().contains(&repos::key(NAME)));
        sync(&mut host, &github);
        assert!(github.show(&moved).is_some());
        assert_eq!(moves_on(&github), 1);
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
        let stub = fs::read_to_string(clone.join(TRACKER)).unwrap();
        assert!(stub.contains("moved_to"), "{stub}");
        assert_eq!(host.read_issue(&id).unwrap().comments.len(), 1);
    }

    #[test]
    fn a_pause_and_a_hold_follow_their_file_through_the_move() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", issue("SAGE-1"));
        let (home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let file = repos::clone_folder(home.path(), NAME).join("issues/SAGE-1/issue.md");
        let edited = String::from_utf8(issue("SAGE-1")).unwrap() + "Edited by a shell.\n";
        fs::write(&file, &edited).unwrap();
        sync(&mut host, &github);
        let paused = |host: &Host| {
            host.read_issue(&tracker::tests::id("SAGE-1"))
                .unwrap()
                .paused
        };
        assert_eq!(paused(&host).len(), 1);
        press_move(&mut host, &github);
        sync(&mut host, &github);
        let moved = paused(&host);
        assert_eq!(moved[0].path, "wiffletree/issues/SAGE-1/issue.md");
        assert!(moved[0].why == Paused::OutsideWiffletree);
        let there = github.show("wiffletree/issues/SAGE-1/issue.md").unwrap();
        assert!(!there.contains("Edited by a shell"), "still not pushed");
    }

    #[test]
    fn settings_stay_at_the_root_through_the_move() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        let tools = github.tools();
        host.choose_settings_repository(NAME, None, &tools).unwrap();
        join_sage(&mut host, &github).unwrap();
        let settings = github.show("wiffletree.json").unwrap();
        press_move(&mut host, &github);
        assert_eq!(github.show("wiffletree.json").as_deref(), Some(&*settings));
        assert_eq!(github.show("wiffletree/wiffletree.json"), None);
        host.sync_now(&tools).unwrap();
        host.set_check_ins(20, 200).unwrap();
        let sage = space_named(&host, "Sage");
        host.update_space(Space {
            description: "Changed here".into(),
            ..sage
        })
        .unwrap();
        host.sync_now(&tools).unwrap();
        assert!(github.show(SAGE_FILE).unwrap().contains("Changed here"));
        let args = [
            "log",
            "--grep=Check-ins",
            "--format=",
            "--name-only",
            "main",
        ];
        let changed = git(&github.bare, &args);
        assert!(changed.starts_with("settings/"), "{changed}");
        let stub = github.show("spaces/sage/space.yaml").unwrap();
        assert!(stub.contains("moved_to"), "the stub never changes: {stub}");
    }

    #[test]
    fn a_settings_pass_stops_at_a_teammates_move_and_adopts_it() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        let (_teammate_home, mut teammate) = self::host();
        let tools = github.tools();
        host.choose_settings_repository(NAME, None, &tools).unwrap();
        join_sage(&mut host, &github).unwrap();
        join_sage(&mut teammate, &github).unwrap();
        press_move(&mut teammate, &github);
        host.sync_now(&tools).unwrap();
        assert_eq!(space_named(&host, "Sage").storage.root, "wiffletree");
        host.sync_now(&tools).unwrap();
        assert!(host.workspace_notes(&[]).unwrap().is_empty());
        assert_eq!(moves_on(&github), 1);
    }

    #[test]
    fn a_number_deleted_before_the_move_is_not_reused() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", issue("SAGE-1"));
        github.commit("issues/SAGE-2/issue.md", issue("SAGE-2"));
        let work = tempfile::tempdir().unwrap();
        let bare = github.bare.to_str().unwrap();
        git(work.path(), &["clone", "-q", bare, "."]);
        git(work.path(), &["rm", "-rq", "issues/SAGE-2"]);
        git(work.path(), &["commit", "-qm", "Remove an issue"]);
        git(work.path(), &["push", "-q", "origin", "main"]);
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        press_move(&mut host, &github);
        sync(&mut host, &github);
        let sage = space_named(&host, "Sage");
        let filed = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        host.create_issue(&sage.id, filed).unwrap();
        sync(&mut host, &github);
        assert!(github.show("wiffletree/issues/SAGE-3/issue.md").is_some());
        assert_eq!(github.show("wiffletree/issues/SAGE-2/issue.md"), None);
    }

    #[test]
    fn a_trackers_compile_time_and_allowed_domains_are_kept_with_keys_this_release_does_not_know() {
        let text = "format: 1
prefix: SAGE
compile:
  at: 06:00
  timezone: America/Chicago
allowed_email_domains:
- sagehaven.ai
future_key: 3
";
        let tracker = read_tracker(text.as_bytes()).unwrap();
        assert_eq!(tracker.compile.as_ref().unwrap().at, "06:00");
        assert_eq!(tracker.allowed_email_domains, ["sagehaven.ai"]);
        let written = String::from_utf8(tracker_yaml(&tracker).unwrap()).unwrap();
        assert_eq!(read_tracker(written.as_bytes()).unwrap(), tracker);
        assert!(written.contains("future_key: 3"), "{written}");
        let plain = tracker_yaml(&TrackerConfig::new("SAGE".into())).unwrap();
        let plain = String::from_utf8(plain).unwrap();
        assert!(!plain.contains("compile") && !plain.contains("allowed_email_domains"));
    }

    fn held_paths(host: &Host) -> Vec<String> {
        (host.held_notes().unwrap().into_iter())
            .map(|note| match note {
                WorkspaceNote::Held(held) => held.path,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    const PHONE: &str = "Text the parent at (415) 555-0134.";

    #[test]
    fn a_file_in_a_folder_is_scanned_and_held_before_a_push() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.update_space(Space {
            additions: PHONE.into(),
            ..sage
        })
        .unwrap();
        sync(&mut host, &github);
        assert!(!github.show(SAGE_FILE).unwrap().contains("555-0134"));
        assert_eq!(held_paths(&host), [SAGE_FILE]);
    }

    #[test]
    fn a_file_in_a_folder_is_scanned_in_a_repository_that_is_also_the_settings_repository() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let (_home, mut host) = host();
        let tools = github.tools();
        host.choose_settings_repository(NAME, None, &tools).unwrap();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.update_space(Space {
            additions: PHONE.into(),
            ..sage
        })
        .unwrap();
        host.sync_now(&tools).unwrap();
        assert!(!github.show(SAGE_FILE).unwrap().contains("555-0134"));
        assert_eq!(held_paths(&host), [SAGE_FILE]);
    }

    #[test]
    fn a_file_held_at_the_root_stays_held_after_the_move() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.update_space(Space {
            additions: PHONE.into(),
            ..sage
        })
        .unwrap();
        sync(&mut host, &github);
        assert_eq!(held_paths(&host), ["spaces/sage/space.yaml"]);
        press_move(&mut host, &github);
        assert_eq!(held_paths(&host), [SAGE_FILE], "its hold moved with it");
        sync(&mut host, &github);
        assert!(!github.show(SAGE_FILE).unwrap().contains("555-0134"));
        assert_eq!(held_paths(&host), [SAGE_FILE]);
    }

    #[test]
    fn a_boards_file_in_a_folder_is_pushed_by_the_boards_rule_set() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let file = "spaces/sage/boards/release/release.json".to_owned();
        let files = [(file, Some(&b"{}\n"[..]))];
        host.write_storage_files(&sage, &files, "Saved").unwrap();
        sync(&mut host, &github);
        let pushed = "wiffletree/spaces/sage/boards/release/release.json";
        assert!(github.show(pushed).is_some());
        let machine = sync::machine_name();
        assert_eq!(
            github.log("%s")[0],
            format!("Sage: saved outputs · {machine}")
        );
    }

    #[test]
    fn each_mac_writes_its_own_compiler_file_and_the_space_rule_set_pushes_it() {
        let folder = tempfile::tempdir().unwrap();
        let github = in_a_folder(folder.path());
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let file = format!("wiffletree/compilers/{}.yaml", host.host_id().unwrap());
        host.write_compiler_file(&sage, true).unwrap();
        sync(&mut host, &github);
        let pushed = github.show(&file).unwrap();
        assert!(pushed.contains("login: jlin"), "{pushed}");
        host.write_compiler_file(&sage, false).unwrap();
        sync(&mut host, &github);
        assert_eq!(github.show(&file), None);
    }

    #[test]
    fn a_stub_pauses_on_an_older_host_as_written_by_a_newer_wiffletree() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        press_move(&mut host, &github);
        let space = github.show("spaces/sage/space.yaml").unwrap();
        let tracker = github.show(TRACKER).unwrap();
        let storage = SpaceStorage {
            repository: super::github(NAME),
            folder: "sage".into(),
            root: String::new(),
        };
        // `read_space` and `read_tracker` are what an older host runs on a pulled file.
        let space = read_space(space.as_bytes(), storage)
            .unwrap_err()
            .to_string();
        let tracker = read_tracker(tracker.as_bytes()).unwrap_err().to_string();
        for (read, file) in [(space, "Space"), (tracker, "tracker")] {
            assert_eq!(
                read,
                format!("This {file} was written by a newer Wiffletree. Update to use it.")
            );
        }
    }

    #[test]
    fn a_space_yaml_at_a_repositorys_root_keeps_what_older_hosts_read() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        host.update_space(Space {
            additions: "Name the issue.".into(),
            ..sage
        })
        .unwrap();
        sync(&mut host, &github);
        let file = github.show("spaces/sage/space.yaml").unwrap();
        let file: serde_yaml::Value = serde_yaml::from_str(&file).unwrap();
        assert_eq!(file["format"], 1);
        assert_eq!(file["instructions"], "Name the issue.");
        assert_eq!(file.get("root"), None, "the folder stays on this Mac");
    }
}
