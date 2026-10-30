//! The settings repository: checking a candidate, setting one up from this machine, joining one
//! set up elsewhere, and the one push sync makes. Git and `gh` run through `Tools`, off the
//! host's command thread; only putting a prepared clone in place runs on it.
use crate::shared::SharedStore;
use crate::skills::{Change, Line, SkillStore};
use crate::*;
use publish::{GitTool, Tools};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// The only paths a sync commit may change, as decision 2 lays them out.
const SHARED_PATHS: [&str; 3] = ["wiffletree.json", "settings", "skills"];

fn is_shared(path: &str) -> bool {
    path == SHARED_PATHS[0] || path.starts_with("settings/") || path.starts_with("skills/")
}

/// GitHub's owner and name, and the URLs to try reaching it by, in order: the URL named, or for
/// `owner/name` the URL of gh's Git `protocol` first and then the other's.
fn candidates(input: &str, protocol: &str) -> Result<(String, String, Vec<String>)> {
    let input = input.trim();
    let named = input.contains(':');
    let url = match named {
        true => input.to_owned(),
        false => format!("https://github.com/{input}.git"),
    };
    let (owner, name) = pr_watch::github_repository(&url)
        .context("Name a repository on github.com as owner/name or by its URL")?;
    let ssh = format!("git@github.com:{owner}/{name}.git");
    let urls = match (named, protocol) {
        (true, _) => vec![url],
        (false, "ssh") => vec![ssh, url],
        (false, _) => vec![url, ssh],
    };
    Ok((owner, name, urls))
}

/// The Git protocol gh uses for github.com: `ssh`, or `https`, gh's default.
fn gh_protocol(tools: &Tools, home: &Path) -> String {
    let args = [
        "config",
        "get",
        "git_protocol",
        "-h",
        repositories::GITHUB_HOST,
    ];
    match (tools.gh)(home, &args) {
        Ok(answer) if answer.status.success() => answer.output.trim().to_owned(),
        _ => "https".into(),
    }
}

/// Git options that give Git gh's GitHub login for one command over HTTPS, in place of any
/// credential helper Git has, so no token is stored or named on a command line.
fn gh_login(url: &str) -> &'static [&'static str] {
    match url.starts_with("https://github.com/") {
        true => &[
            "-c",
            "credential.https://github.com.helper=",
            "-c",
            "credential.https://github.com.helper=!gh auth git-credential",
        ],
        false => &[],
    }
}

/// Runs Git through `git`, refusing a failed command with Git's reason.
fn run(git: &GitTool, path: &Path, args: &[&str]) -> Result<String> {
    let answer = git(path, args)?;
    ensure!(
        answer.status.success(),
        "git {} failed{}",
        publish::subcommand(args),
        answer.reason()
    );
    Ok(answer.output)
}

/// `run` for a command that reaches the repository at `url`: over HTTPS with gh's login.
fn run_at(git: &GitTool, path: &Path, url: &str, args: &[&str]) -> Result<String> {
    run(git, path, &[gh_login(url), args].concat())
}

fn gh_json(tools: &Tools, home: &Path, endpoint: &str) -> Result<Value> {
    let args = ["api", "--hostname", repositories::GITHUB_HOST, endpoint];
    let answer = (tools.gh)(home, &args)?;
    ensure!(
        answer.status.success(),
        "GitHub could not be read: {}",
        answer.error
    );
    Ok(serde_json::from_str(&answer.output)?)
}

/// Removes the staging clones a host that stopped while preparing one left in the data folder.
pub(crate) fn remove_staging(home: &Path) -> Result<()> {
    for entry in fs::read_dir(home)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("shared-staging-")
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// This computer's name, which sync's commits name the machine by.
pub(crate) fn machine_name() -> String {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(read_machine_name).clone()
}
fn read_machine_name() -> String {
    std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Unnamed machine".into())
}

/// The branch's head at `url`, or `None` while the repository has no commit on it. Reading it
/// is how this machine's access is checked.
fn remote_head(path: &Path, url: &str, branch: &str, git: &GitTool) -> Result<Option<String>> {
    let reference = format!("refs/heads/{branch}");
    let heads = run_at(git, path, url, &["ls-remote", url, &reference])?;
    Ok(heads.split_whitespace().next().map(str::to_owned))
}

/// The first of `urls` this machine can read `branch` at, with the branch's head there, or
/// every URL tried and why it could not be read.
fn first_readable(
    path: &Path,
    urls: Vec<String>,
    branch: &str,
    git: &GitTool,
) -> Result<(String, Option<String>)> {
    let mut reasons = vec![];
    for url in urls {
        match remote_head(path, &url, branch, git) {
            Ok(head) => return Ok((url, head)),
            Err(error) => reasons.push(format!("{url}: {error:#}")),
        }
    }
    bail!(
        "This machine cannot read the repository. Tried {}. Add an SSH key to GitHub, sign gh in with gh auth login, or check that the repository exists and is yours to read.",
        reasons.join("; tried ")
    )
}

/// What GitHub and Git say about the repository, with a clone of its branch in a new folder of
/// the data folder that is removed when dropped.
fn read(home: &Path, input: &str, tools: &Tools) -> Result<(SettingsRepositoryCheck, TempDir)> {
    let (owner, name, urls) = candidates(input, &gh_protocol(tools, home))?;
    let facts = gh_json(tools, home, &format!("repos/{owner}/{name}"))?;
    ensure!(
        facts["private"] == true,
        "Choose a private repository. Your skills and settings would be public."
    );
    let organization = facts["owner"]["type"] == "Organization";
    let readers = match organization {
        _ if facts["visibility"] == "internal" => Readers::EnterpriseMembers,
        false => Readers::Invited,
        // GitHub reports an organization's base permission only to some of its members.
        true => match gh_json(tools, home, &format!("orgs/{owner}"))
            .ok()
            .and_then(|org| {
                org["default_repository_permission"]
                    .as_str()
                    .map(str::to_owned)
            })
            .as_deref()
        {
            None => Readers::MaybeMembers,
            Some("none") => Readers::Invited,
            Some(_) => Readers::EveryMember,
        },
    };
    let field = |key: &str| facts[key].as_str().map(str::to_owned);
    let branch = field("default_branch").context("GitHub named no default branch")?;
    let (push_url, head) = first_readable(home, urls, &branch, &tools.git)?;
    let repository = SettingsRepository {
        name: field("full_name").context("GitHub named no repository")?,
        push_url,
        branch,
        readers,
    };
    let staging = tempfile::Builder::new()
        .prefix("shared-staging-")
        .tempdir_in(home)?;
    let into = staging
        .path()
        .to_str()
        .context("The data folder's path is not UTF-8")?;
    let mut clone = vec!["clone", "--quiet", "--no-tags", "--single-branch"];
    if head.is_some() {
        clone.extend(["--branch", &repository.branch]);
    }
    clone.extend([repository.push_url.as_str(), into]);
    run_at(&tools.git, home, &repository.push_url, &clone)?;
    if head.is_none() {
        let branch = format!("refs/heads/{}", repository.branch);
        run(
            &tools.git,
            staging.path(),
            &["symbolic-ref", "HEAD", &branch],
        )?;
    }
    for path in SHARED_PATHS {
        refuse_links(staging.path(), Path::new(path))?;
    }
    let held = SharedStore::at(staging.path().into());
    let joining = match held.holds_settings() {
        true => Some(joining(&held, &SharedStore::of(home), staging.path())?),
        false => None,
    };
    let check = SettingsRepositoryCheck {
        repository,
        organization,
        joining,
    };
    Ok((check, staging))
}

/// Checks a candidate repository and writes nothing that outlasts the check.
pub(crate) fn check(home: &Path, input: &str, tools: &Tools) -> Result<SettingsRepositoryCheck> {
    Ok(read(home, input, tools)?.0)
}

fn joining(held: &SharedStore, mine: &SharedStore, clone: &Path) -> Result<Joining> {
    let committed = runtime::git_output(clone, &["log", "-1", "--format=%ct", "--", shared::GUIDE]);
    let committed = (committed.ok()).and_then(|at| Some(at.trim().parse::<i64>().ok()? * 1000));
    let same_guide = held.guide() == mine.guide();
    let saved = if same_guide {
        committed
    } else {
        mine.guide_saved_at()
    };
    let guide = |store: &SharedStore, at: Option<i64>, format: &str| {
        let at = at.and_then(chrono::DateTime::from_timestamp_millis);
        let edited = at.map(|at| at.with_timezone(&chrono::Local).format(format).to_string());
        guide_summary(store.guide(), edited)
    };
    // Two guides edited on one day read apart by the time.
    let day = "Edited %-d %b";
    let format = match !same_guide && guide(held, committed, day) == guide(mine, saved, day) {
        true => "Edited %-d %b %H:%M",
        false => day,
    };
    let settings = summaries(held, guide(held, committed, format))
        .into_iter()
        .zip(summaries(mine, guide(mine, saved, format)))
        .map(
            |((setting, repository), (_, this_machine))| SettingComparison {
                setting,
                repository,
                this_machine,
            },
        )
        .collect();
    let last = runtime::git_output(clone, &["log", "-1", "--format=%ct%x00%an%x00%s"])?;
    let mut fields = last.trim_end().splitn(3, '\0');
    let at = fields.next().unwrap_or_default().parse::<i64>()? * 1000;
    let author = fields.next().unwrap_or_default();
    Ok(Joining {
        settings,
        last_machine: named(author, fields.next().unwrap_or_default()).to_owned(),
        last_changed_at: at,
    })
}

/// A guide other than a starter one reads as when it was `edited`.
fn guide_summary(guide: Option<String>, edited: Option<String>) -> String {
    let starter =
        |guide: &str| guide == STARTER_GUIDE || retired_starter_guides().iter().any(|g| g == guide);
    match guide {
        Some(guide) if !starter(&guide) => edited.unwrap_or_else(|| "Edited".into()),
        _ => "Starter guide".into(),
    }
}

/// Each shared setting in a word or two, in the order the Settings window lists them, with the
/// guide as `guide_summary` gives it.
fn summaries(store: &SharedStore, guide: String) -> Vec<(String, String)> {
    let levels: Vec<String> = store.review().levels.into_iter().map(|l| l.name).collect();
    let (child, human) = store.check_ins();
    let keeper = store.keeper();
    let triggers = [
        (keeper.rebase, "rebase"),
        (keeper.comments, "comments"),
        (keeper.conflicts, "conflicts"),
        (keeper.checks, "failed checks"),
    ];
    let on: Vec<&str> = triggers.iter().filter(|t| t.0).map(|t| t.1).collect();
    let mut rows = vec![
        ("Review".into(), levels.join(", ")),
        ("Model guide".into(), guide),
        (
            "Check-ins".into(),
            format!("every {child} and {human} turns"),
        ),
        (
            "PR keeper".into(),
            match on.is_empty() {
                true => "every trigger off".to_owned(),
                false => format!("on: {}", on.join(", ")),
            },
        ),
    ];
    let skills = SkillStore::new(store.skills_folder());
    rows.extend(Skill::ALL.map(|skill| {
        let state = skills.state(skill);
        let count = state.rules.lines().filter(|l| !l.trim().is_empty()).count();
        let held = match (state.rewrite, count) {
            (Some(_), _) => "rewritten".to_owned(),
            (None, 0) => "stock".to_owned(),
            (None, n) => format!("{n} of your rules"),
        };
        (skill.name().to_owned(), held)
    }));
    rows
}

/// Refuses a symbolic link at or under `path` in the clone, so sync never follows one.
fn refuse_links(clone: &Path, path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(clone.join(path)) else {
        return Ok(());
    };
    ensure!(
        !metadata.is_symlink(),
        "The repository's {} is a symbolic link, which sync never follows",
        path.display()
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(clone.join(path))? {
            refuse_links(clone, &path.join(entry?.file_name()))?;
        }
    }
    Ok(())
}

/// Copies `from` to `to`, refusing a symbolic link; with `kept`, the clone's root, a file
/// already in the clone is refused.
fn copy_tree(from: &Path, to: &Path, kept: Option<&Path>) -> Result<()> {
    let metadata = fs::symlink_metadata(from)?;
    ensure!(
        !metadata.is_symlink(),
        "{} is a symbolic link, which sync never follows",
        from.display()
    );
    if metadata.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), kept)?;
        }
    } else {
        if let Some(clone) = kept.filter(|_| to.exists()) {
            let file = to.strip_prefix(clone).unwrap_or(to).display();
            bail!("The repository already has {file}: set up a repository without it");
        }
        fs::copy(from, to)?;
    }
    Ok(())
}

/// Puts this machine's shared paths in the clone. Replacing removes the clone's first; setting
/// up keeps every file there and refuses to overwrite one.
fn put_this_machines(clone: &Path, mine: &Path, replace: bool) -> Result<()> {
    for path in SHARED_PATHS {
        let (from, to) = (mine.join(path), clone.join(path));
        match to.is_dir() {
            true if replace => fs::remove_dir_all(&to)?,
            false if replace && to.exists() => fs::remove_file(&to)?,
            _ => {}
        }
        if fs::symlink_metadata(&from).is_ok() {
            copy_tree(&from, &to, (!replace).then_some(clone))?;
        }
    }
    Ok(())
}

/// Every file under `folder` but Git's own, with its content.
fn contents(folder: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let (path, kind) = (entry.path(), entry.file_type()?);
        if kind.is_dir() && !path.ends_with(".git") {
            files.extend(contents(&path)?);
        } else if kind.is_file() {
            files.insert(path.clone(), fs::read(&path)?);
        }
    }
    Ok(files)
}

/// Adds this machine's skill history entries that the clone lacks, so both histories are kept.
fn add_missing_history(clone: &Path, mine: &Path) -> Result<()> {
    for skill in Skill::ALL {
        let history = Path::new("skills").join(skill.folder()).join("history");
        let Ok(entries) = fs::read_dir(mine.join(&history)) else {
            continue;
        };
        fs::create_dir_all(clone.join(&history))?;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let from = entry.path();
            let entry = fs::read(&from)?;
            let stem = from.file_stem().context("A history entry has a name")?;
            // Two machines can write an entry in the same millisecond; both are kept.
            for n in 1.. {
                let name = match n {
                    1 => format!("{}.json", stem.display()),
                    n => format!("{}-{n}.json", stem.display()),
                };
                let to = clone.join(&history).join(name);
                match fs::read(&to) {
                    Ok(held) if held == entry => break,
                    Ok(_) => continue,
                    Err(_) => {
                        fs::write(&to, &entry)?;
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

/// The shared paths the clone has or tracks.
fn shared_paths(clone: &Path) -> Result<Vec<String>> {
    let tracked = runtime::git_output(clone, &["ls-files", "-z", "--", "settings", "skills"])?;
    Ok(SHARED_PATHS
        .into_iter()
        .filter(|path| {
            clone.join(path).exists() || tracked.split('\0').any(|f| f.starts_with(path))
        })
        .map(str::to_owned)
        .collect())
}

/// Commits `paths` as the clone holds them, with the human's Git identity; `false` when nothing
/// changed.
fn commit(clone: &Path, paths: &[String], message: &str, git: &GitTool) -> Result<bool> {
    let literal: Vec<String> = paths.iter().map(|p| format!(":(literal){p}")).collect();
    let literal: Vec<&str> = literal.iter().map(String::as_str).collect();
    run(
        git,
        clone,
        &[&["add", "--all", "--"], &literal[..]].concat(),
    )?;
    commit_staged(clone, message, git)
}

/// Commits what is staged, with the human's Git identity; `false` when nothing is.
fn commit_staged(clone: &Path, message: &str, git: &GitTool) -> Result<bool> {
    let identity = ["user.name", "user.email"].map(|key| run(git, clone, &["config", key]));
    ensure!(
        identity
            .iter()
            .all(|value| value.as_ref().is_ok_and(|v| !v.trim().is_empty())),
        "Git has no name and email set. Set user.name and user.email with git config --global, then try again."
    );
    if runtime::git_answer(clone, &["diff", "--cached", "--quiet"])?
        .status
        .success()
    {
        return Ok(false);
    }
    run(git, clone, &["commit", "--quiet", "-m", message])?;
    Ok(true)
}

/// Pushes the clone's HEAD to the recorded branch at the recorded push URL, with no tags or
/// submodules and never forced: only a commit on top of the branch's head on GitHub, whose
/// commits change nothing outside the shared paths.
pub(crate) fn push(clone: &Path, repository: &SettingsRepository, git: &GitTool) -> Result<()> {
    let head = runtime::git_output(clone, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    let remote = remote_head(clone, &repository.push_url, &repository.branch, git)
        .context("This machine cannot read the repository")?;
    let range = match remote {
        Some(remote) => {
            ensure!(
                publish::is_ancestor(clone, &remote, &head),
                "{} moved on GitHub since this machine read it",
                repository.branch
            );
            format!("{remote}..{head}")
        }
        None => head.clone(),
    };
    let args = [
        "log",
        "--format=",
        "--name-only",
        "--no-renames",
        "-m",
        "-z",
        &range,
    ];
    let changed = runtime::git_output(clone, &args)?;
    if let Some(outside) = changed
        .split('\0')
        .find(|path| !path.is_empty() && !is_shared(path))
    {
        bail!(
            "Sync changes only wiffletree.json, settings/ and skills/, and a commit changes {outside}"
        );
    }
    let refspec = format!("{head}:refs/heads/{}", repository.branch);
    let args = ["push", "--no-follow-tags", "--recurse-submodules=no"];
    let url = repository.push_url.as_str();
    run_at(git, clone, url, &[&args[..], &[url, &refspec]].concat())?;
    Ok(())
}

/// Five minutes between passes, backing off to thirty while GitHub cannot be reached, and a
/// push five seconds after the last shared write, so a burst of saves is one commit.
const PULL_MS: i64 = 300_000;
const MAX_BACKOFF_MS: i64 = 1_800_000;
const PUSH_DELAY_MS: i64 = 5_000;

/// When sync's next pass is due. One runs at a time.
#[derive(Default)]
pub(crate) struct Pace {
    pull_at: i64,
    push_at: Option<i64>,
    running: bool,
    /// Sync now was chosen while a pass ran.
    again: bool,
    failures: u32,
}
impl Pace {
    /// While GitHub cannot be reached, a save waits for the next try.
    pub(crate) fn wrote(&mut self, at: i64) {
        self.push_at = Some(at + PUSH_DELAY_MS);
    }
    pub(crate) fn sync_now(&mut self) {
        self.pull_at = 0;
        self.again = self.running;
    }
    /// Marks a pass running when one is due at `at` and none runs.
    pub(crate) fn start(&mut self, at: i64) -> bool {
        let due = self.next_due().is_some_and(|due| due <= at);
        if due {
            self.running = true;
            self.push_at = None;
        }
        due
    }
    /// `reached`: the pass read GitHub's head.
    pub(crate) fn finish(&mut self, at: i64, reached: bool) {
        self.running = false;
        self.failures = if reached { 0 } else { self.failures + 1 };
        let wait = PULL_MS << self.failures.min(3);
        self.pull_at = match std::mem::take(&mut self.again) {
            true => at,
            false => at + wait.min(MAX_BACKOFF_MS),
        };
    }
    pub(crate) fn running(&self) -> bool {
        self.running
    }
    pub(crate) fn next_due(&self) -> Option<i64> {
        let push = self.push_at.filter(|_| self.failures == 0);
        (!self.running).then(|| push.map_or(self.pull_at, |push| push.min(self.pull_at)))
    }
}

/// What the latest pass did, for the sync status.
#[derive(Default)]
pub(crate) struct Synced {
    at: Option<i64>,
    pulled: usize,
    pushed: usize,
    error: Option<String>,
    failed_at: Option<i64>,
    failing_since: Option<i64>,
    stopped: bool,
    pub(crate) next_pull_at: Option<i64>,
    pub(crate) syncing: bool,
}
impl Synced {
    fn failed(&mut self, at: i64, error: String) {
        self.failing_since.get_or_insert(at);
        self.failed_at = Some(at);
        self.error = Some(error);
    }
}

/// Why sync pulls and pushes nothing until the repository's `wiffletree.json` can be read.
#[derive(Debug)]
struct Stopped(String);
impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Stopped {}

/// A file's content, `None` for no file.
type Content = Option<Vec<u8>>;

/// A pass's input from the host thread: the clone, its repository, sync's record, and the
/// host's writes, counted at `since`.
pub(crate) struct Pass {
    clone: PathBuf,
    repository: SettingsRepository,
    commit: String,
    synced: BTreeMap<String, Content>,
    written: BTreeMap<String, Content>,
    /// The files paused as changed on two machines, which stay paused until settled or alike.
    two_machines: Vec<String>,
    writes: shared::SharedWrites,
    since: u64,
    running: Arc<Mutex<()>>,
}

/// What a pass did, for the host thread to write and record.
pub(crate) struct Passed {
    repository: SettingsRepository,
    /// GitHub's head the pass read, and the commit pushed on it, if any.
    remote: String,
    pushed: Option<String>,
    /// Each file changed only on GitHub: this machine's content as read, then GitHub's.
    pulled: Vec<(String, Content, Content)>,
    /// Each file both sides now hold alike: changed the same on both, or committed here.
    alike: Vec<(String, Content)>,
    committed: Vec<(String, Content)>,
    /// Each file left alone, with why and GitHub's content.
    paused: Vec<(String, Paused, Content)>,
    /// Why committing or pushing failed; what was pulled still applies.
    error: Option<String>,
    since: u64,
}

/// Sync's files: the shared paths, without hidden files such as `.DS_Store` or a write's
/// temporary file.
fn is_synced(path: &str) -> bool {
    is_shared(path) && !path.split('/').any(|part| part.starts_with('.'))
}

/// This machine's synced files, read whole.
fn files_here(clone: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for path in SHARED_PATHS {
        refuse_links(clone, Path::new(path))?;
        let at = clone.join(path);
        let found = match at.is_dir() {
            true => contents(&at)?,
            false => fs::read(&at).map_or_else(|_| BTreeMap::new(), |c| BTreeMap::from([(at, c)])),
        };
        for (file, content) in found {
            let name = file.strip_prefix(clone)?.to_str();
            let name = name.context("A shared file's path is not UTF-8")?;
            if is_synced(name) {
                files.insert(name.to_owned(), content);
            }
        }
    }
    Ok(files)
}

/// GitHub's synced files at `commit`, read whole.
fn files_at(clone: &Path, commit: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let listing = runtime::git_output(clone, &["ls-tree", "-r", "-z", commit])?;
    let mut blobs = vec![];
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (kind, path) = entry
            .split_once('\t')
            .context("Git listed an entry with no path")?;
        if is_synced(path) {
            let mut kind = kind.split(' ');
            ensure!(
                matches!(kind.next(), Some("100644" | "100755")),
                "The repository's {path} is not a plain file, which sync never takes"
            );
            blobs.push((
                path.to_owned(),
                kind.nth(1).context("Git listed no object")?,
            ));
        }
    }
    let ids: String = blobs.iter().map(|(_, id)| format!("{id}\n")).collect();
    let batch = runtime::git_bytes(clone, &["cat-file", "--batch"], ids.as_bytes())?;
    let mut rest = batch.as_slice();
    let mut files = BTreeMap::new();
    for (path, _) in blobs {
        let header = rest
            .iter()
            .position(|&b| b == b'\n')
            .context("Git sent no header")?;
        let size: usize = std::str::from_utf8(&rest[..header])?
            .rsplit(' ')
            .next()
            .unwrap_or_default()
            .parse()?;
        let content = rest
            .get(header + 1..header + 1 + size)
            .context("Git sent too little")?;
        files.insert(path, content.to_vec());
        rest = rest.get(header + size + 2..).unwrap_or_default();
    }
    Ok(files)
}

/// Brings files outside the shared paths to GitHub's `remote`, as they were at `previous`.
fn take_other_files(clone: &Path, previous: &str, remote: &str) -> Result<()> {
    let diff = ["diff", "--name-status", "-z", "--no-renames"];
    let changed = runtime::git_output(clone, &[&diff[..], &[previous, remote]].concat())?;
    let mut fields = changed.split('\0');
    let mut taken = vec![];
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        match status {
            _ if is_shared(path) => {}
            "D" => match fs::remove_file(clone.join(path)) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error)?,
                _ => {}
            },
            _ => taken.push(format!(":(literal){path}")),
        }
    }
    if !taken.is_empty() {
        let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
        runtime::git_output(clone, &[&["checkout", remote, "--"], &taken[..]].concat())?;
    }
    Ok(())
}

/// A paused file, with GitHub's content and the content last synced.
type PausedFile = (String, Content, Content);

/// The files paused and settled together: a skill's rules, rewrite and base, as its folder, or
/// one file.
fn unit(path: &str) -> &str {
    let mut parts = path.splitn(3, '/');
    match [(); 3].map(|()| parts.next()) {
        [Some("skills"), Some(skill), Some(file)] if skills::CURRENT.contains(&file) => {
            &path[..skill.len() + "skills//".len()]
        }
        _ => path,
    }
}

/// Stages exactly the content a pass read, so no file written since reaches the commit.
fn stage(clone: &Path, files: &[(String, Content)]) -> Result<()> {
    let mut index = vec![];
    for (path, content) in files {
        let entry = match content {
            Some(content) => {
                let id = runtime::git_bytes(clone, &["hash-object", "-w", "--stdin"], content)?;
                format!("100644 {}\t{path}\0", String::from_utf8(id)?.trim())
            }
            None => format!("0 {}\t{path}\0", "0".repeat(40)),
        };
        index.extend(entry.into_bytes());
    }
    runtime::git_bytes(clone, &["update-index", "-z", "--index-info"], &index)?;
    Ok(())
}

/// The machine a commit names after its " · ", or else its author.
fn named<'a>(author: &'a str, subject: &'a str) -> &'a str {
    subject
        .rsplit_once(" · ")
        .map_or(author, |(_, machine)| machine)
}

/// The machine that last changed `paths` on GitHub.
fn changed_by(clone: &Path, remote: &str, paths: &[&String]) -> String {
    let literal: Vec<String> = paths.iter().map(|p| format!(":(literal){p}")).collect();
    let mut args = vec!["log", "-1", "--format=%an%x00%s", remote, "--"];
    args.extend(literal.iter().map(String::as_str));
    let line = runtime::git_output(clone, &args).unwrap_or_default();
    match line.trim_end().split_once('\0') {
        Some((author, subject)) => named(author, subject).to_owned(),
        None => "another machine".into(),
    }
}

/// The settings `paths` hold, each once, as the Settings window names them.
fn names(paths: &[String]) -> Vec<String> {
    let mut names: Vec<&str> = vec![];
    for path in paths {
        let name = match path.as_str() {
            shared::REVIEW => "Review",
            shared::GUIDE => "Model guide",
            shared::CHECK_INS => "Check-ins",
            shared::KEEPER => "PR keeper",
            shared::MODELS => "Switch threshold",
            path => Skill::ALL
                .into_iter()
                .find(|skill| path.starts_with(&format!("skills/{}/", skill.folder())))
                .map_or(path, |skill| skill.name()),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.into_iter().map(str::to_owned).collect()
}

/// The skill and stock version of a "Merged" history entry at `path`.
fn merge_in(path: &str, content: &Content) -> Option<(Skill, String)> {
    let skill = Skill::ALL
        .into_iter()
        .find(|skill| path.starts_with(&format!("skills/{}/history/", skill.folder())))?;
    match serde_json::from_slice::<skills::Entry>(content.as_deref()?)
        .ok()?
        .change
    {
        Change::Merged { to, .. } => Some((skill, to)),
        _ => None,
    }
}

/// Whether this machine's only changes to the skill `unit` holds since it last synced are its
/// own automatic merges: each history entry written here since is one, or a new stock version.
/// Never for a unit that is not a skill, which has no history.
fn only_own_merges(
    unit: &str,
    here: &BTreeMap<String, Vec<u8>>,
    synced: &BTreeMap<String, Content>,
) -> bool {
    let history = format!("{unit}history/");
    let written_since = (here.iter()).filter(|(path, _)| {
        path.starts_with(&history) && synced.get(*path).is_none_or(Option::is_none)
    });
    let mut merged = false;
    for (_, content) in written_since {
        match serde_json::from_slice::<skills::Entry>(content).map(|entry| entry.change) {
            Ok(Change::Merged {
                automatic: true, ..
            }) => merged = true,
            Ok(Change::NewStockVersion { .. }) => {}
            _ => return false,
        }
    }
    merged
}

/// Whether this machine undid a merge of the skill `unit` holds, or restored its rewrite from
/// before one, since it last synced. That is a change to the skill even when its files are back
/// to the version last synced. Never for a unit that is not a skill.
fn went_back_here(
    unit: &str,
    here: &BTreeMap<String, Vec<u8>>,
    synced: &BTreeMap<String, Content>,
) -> bool {
    let folder = format!("{unit}history/");
    let unsynced = |path: &str| synced.get(path).is_none_or(Option::is_none);
    if !(here.keys()).any(|path| path.starts_with(&folder) && unsynced(path)) {
        return false;
    }
    // By name, the time each was written, as the paths sort.
    let history: Vec<(String, skills::Entry)> = (here.iter())
        .filter_map(|(path, content)| {
            let name = path.strip_prefix(&folder)?.strip_suffix(".json")?;
            Some((name.to_owned(), serde_json::from_slice(content).ok()?))
        })
        .collect();
    (history.iter())
        .filter(|(name, _)| unsynced(&format!("{folder}{name}.json")))
        .any(|named| skills::stops(&history, named).is_some())
}

/// What a commit of `committed` changes, as the Settings window names it, and a skill's merge
/// as "Coordinating: merged v2 into your rewrite".
fn subject(committed: &[(String, Content)]) -> String {
    let paths: Vec<String> = committed.iter().map(|(p, _)| p.clone()).collect();
    let merges: Vec<_> = (committed.iter())
        .filter_map(|(path, content)| merge_in(path, content))
        .collect();
    let named = |name: String| match merges.iter().rfind(|(skill, _)| skill.name() == name) {
        Some((_, to)) => format!("{name}: merged {to} into your rewrite"),
        None => name,
    };
    names(&paths)
        .into_iter()
        .map(named)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Fetches the branch, compares each file whole with the version last synced, and commits this
/// machine's changes on GitHub's head and pushes them. A skill's rules, rewrite and base are
/// compared as one. What changed on both sides, changed outside Wiffletree, or arrived in a
/// form this machine refuses is paused and kept here, except a skill whose only change here is
/// this machine's own automatic merge, which takes GitHub's version. What the host wrote while
/// the pass ran is left for the next one. Files to pull are returned for the host thread to
/// write.
pub(crate) fn pass(pass: Pass, tools: &Tools) -> Result<Passed> {
    let Pass {
        clone,
        repository,
        commit: previous,
        synced,
        written,
        two_machines,
        writes,
        since,
        running,
    } = pass;
    // Held for the whole pass, so no clone is adopted in its place until the pass is done. A
    // pass the host started before an adoption changes nothing, and the host drops it.
    let _running = running
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut passed = Passed {
        repository,
        remote: previous.clone(),
        pushed: None,
        pulled: vec![],
        alike: vec![],
        committed: vec![],
        paused: vec![],
        error: None,
        since,
    };
    if shared::lock(&writes).touched("", since) {
        return Ok(passed);
    }
    let repository = &passed.repository;
    let branch = format!("refs/heads/{}", repository.branch);
    let fetch = ["fetch", "--quiet", "--no-tags", "--recurse-submodules=no"];
    let url = repository.push_url.as_str();
    run_at(
        &tools.git,
        &clone,
        url,
        &[&fetch[..], &[url, &branch]].concat(),
    )?;
    let remote = runtime::git_output(&clone, &["rev-parse", "FETCH_HEAD"])?
        .trim()
        .to_owned();
    let (here, there) = (files_here(&clone)?, files_at(&clone, &remote)?);
    if let Some(format) = there.get(shared::FORMAT) {
        match shared::format(format) {
            Ok(1) => {}
            Ok(_) => Err(Stopped(
                "This repository was written by a newer Wiffletree. Update to keep syncing.".into(),
            ))?,
            Err(error) => Err(Stopped(format!(
                "The repository's wiffletree.json can't be read: {error:#}. Sync waits until it is fixed on GitHub."
            )))?,
        }
    }
    passed.remote = remote.clone();
    let mut units: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for path in synced.keys().chain(here.keys()).chain(there.keys()) {
        let paths = units.entry(unit(path)).or_default();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let version = |map: &BTreeMap<String, Content>, path: &String| map.get(path).cloned().flatten();
    for (unit, paths) in units {
        let files: Vec<_> = (paths.iter())
            .map(|&path| {
                let (base, mine) = (version(&synced, path), here.get(path).cloned());
                (path, base, mine, there.get(path).cloned())
            })
            .collect();
        let outside = (files.iter())
            .any(|(path, base, mine, _)| mine != base && *mine != version(&written, path));
        let refused = (files.iter())
            .filter(|(_, base, _, theirs)| theirs != base)
            .find_map(|(path, _, _, theirs)| shared::check(path, theirs.as_ref()?).err());
        let changed_here = went_back_here(unit, &here, &synced)
            || files.iter().any(|(_, base, mine, _)| mine != base);
        let changed_there = files.iter().any(|(_, base, _, theirs)| theirs != base);
        let was_paused = paths.iter().any(|path| two_machines.contains(path));
        // GitHub's version supersedes a merge of the version both last synced.
        let own_merge = changed_there && only_own_merges(unit, &here, &synced);
        let paused = match () {
            _ if files.iter().all(|(_, _, mine, theirs)| mine == theirs) => None,
            _ if outside => Some(Paused::OutsideWiffletree),
            _ => match refused {
                Some(reason) => Some(Paused::CannotBeRead {
                    machine: changed_by(&clone, &remote, &paths),
                    reason: format!("{reason:#}"),
                }),
                None if changed_here && changed_there && !own_merge || was_paused => {
                    Some(Paused::TwoMachines {
                        machine: changed_by(&clone, &remote, &paths),
                    })
                }
                None => None,
            },
        };
        for (path, base, mine, theirs) in files {
            let path = path.clone();
            match &paused {
                Some(why) => passed.paused.push((path, why.clone(), theirs)),
                None if mine == theirs && mine != base => passed.alike.push((path, mine)),
                None if mine == theirs => {}
                None if mine == base || own_merge => passed.pulled.push((path, mine, theirs)),
                None => passed.committed.push((path, mine)),
            }
        }
    }
    take_other_files(&clone, &previous, &remote)?;
    runtime::git_output(&clone, &["reset", "--quiet", &remote])?;
    {
        let writes = shared::lock(&writes);
        let read = |path: &String| !writes.touched(path, since);
        passed.pulled.retain(|(path, ..)| read(path));
        passed.alike.retain(|(path, _)| read(path));
        passed.committed.retain(|(path, _)| read(path));
        passed.paused.retain(|(path, ..)| read(path));
    }
    let mut refused = None;
    passed.committed.retain(|(path, content)| {
        let Some(Err(error)) = content.as_deref().map(|c| shared::check(path, c)) else {
            return true;
        };
        refused = Some(format!(
            "{path} was not pushed: this Mac would refuse it: {error:#}"
        ));
        false
    });
    if !passed.committed.is_empty() {
        let message = format!("{} · {}", subject(&passed.committed), machine_name());
        let pushed = stage(&clone, &passed.committed)
            .and_then(|()| commit_staged(&clone, &message, &tools.git))
            .and_then(|made| {
                if made {
                    push(&clone, &passed.repository, &tools.git)?;
                }
                let head = runtime::git_output(&clone, &["rev-parse", "HEAD"])?;
                Ok(made.then(|| head.trim().to_owned()))
            });
        match pushed {
            Ok(head) => passed.pushed = head,
            Err(error) => passed.error = Some(format!("{error:#}")),
        }
    }
    passed.error = passed.error.or(refused);
    Ok(passed)
}

/// Holds off sync passes while the host changes what they sync with. Refused while one runs,
/// which can take minutes.
fn no_pass_running(passes: &Mutex<()>) -> Result<std::sync::MutexGuard<'_, ()>> {
    match passes.try_lock() {
        Ok(none) => Ok(none),
        Err(std::sync::TryLockError::Poisoned(none)) => Ok(none.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => {
            bail!("Sync is running. Try again in a moment.")
        }
    }
}

/// A clone holding the shared settings this machine will use, committed and pushed.
pub(crate) struct Prepared {
    staging: TempDir,
    repository: SettingsRepository,
    /// This machine's shared folder as it was read, so a save made since is never undone.
    read_from: BTreeMap<PathBuf, Vec<u8>>,
}

/// Sets up the repository from this machine, with no `with`, or joins it keeping `with`'s.
pub(crate) fn prepare(
    home: &Path,
    input: &str,
    with: Option<JoinWith>,
    tools: &Tools,
) -> Result<Prepared> {
    let (check, staging) = read(home, input, tools)?;
    let (clone, mine) = (staging.path(), home.join(shared::FOLDER));
    let read_from = contents(&mine)?;
    let change = match (check.joining.is_some(), with) {
        (false, None) => "Set up the shared settings",
        (true, Some(JoinWith::ThisMachine)) => "Replaced the shared settings",
        (true, Some(JoinWith::Repository)) => "Skills: kept this machine's history",
        (false, Some(_)) => bail!("The repository holds no shared settings yet: set it up"),
        (true, None) => bail!("The repository already holds shared settings: join it"),
    };
    match with {
        Some(JoinWith::Repository) => add_missing_history(clone, &mine)?,
        with => put_this_machines(clone, &mine, with.is_some())?,
    }
    let message = format!("{change} · {}", machine_name());
    if commit(clone, &shared_paths(clone)?, &message, &tools.git)? {
        push(clone, &check.repository, &tools.git)?;
    }
    Ok(Prepared {
        staging,
        repository: check.repository,
        read_from,
    })
}

impl Host {
    /// `prepare` and `adopt_settings_repository` in turn, on this thread.
    pub(crate) fn choose_settings_repository(
        &mut self,
        input: &str,
        with: Option<JoinWith>,
        tools: &Tools,
    ) -> Result<HostSettings> {
        let prepared = prepare(&self.home, input, with, tools)?;
        self.adopt_settings_repository(prepared)
    }
    /// Puts a prepared clone in place of the shared folder, moving the folder to `backups/`, and
    /// records the repository in this machine's settings. A shared setting saved since the
    /// clone was prepared refuses it, and a failure puts the folder back.
    pub(crate) fn adopt_settings_repository(&mut self, prepared: Prepared) -> Result<HostSettings> {
        // One that ran before is dropped, not recorded against the new clone.
        let passes = self.sync_passes.clone();
        let _no_pass = no_pass_running(&passes)?;
        let _writing = shared::Writing::start(&self.shared_writes, "");
        let shared = self.home.join(shared::FOLDER);
        ensure!(
            contents(&shared)? == prepared.read_from,
            "A shared setting changed meanwhile, and stays as you saved it here. The repository has the earlier settings: join it with this Mac's to push yours."
        );
        let backups = self.home.join("backups");
        fs::create_dir_all(&backups)?;
        let at = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S%.3fZ");
        let backup = backups.join(format!("shared-{at}"));
        fs::rename(&shared, &backup)?;
        let settings = HostSettings {
            settings_repository: Some(prepared.repository.clone()),
            ..self.settings()
        };
        let adopted = fs::rename(prepared.staging.path(), &shared)
            .map_err(anyhow::Error::from)
            .and_then(|()| {
                self.save_settings(&settings).inspect_err(|_| {
                    let _ = fs::rename(&shared, prepared.staging.path());
                })
            });
        if let Err(error) = adopted {
            fs::rename(&backup, &shared)?;
            return Err(error);
        }
        self.record_clone()?;
        self.recall_skill_histories();
        Ok(self.settings())
    }
    /// Stops syncing on this machine and forgets the repository chosen. The shared folder stays
    /// as it is and in use, the notes about paused files close, and GitHub is left unchanged.
    pub(crate) fn stop_settings_sync(&mut self) -> Result<HostSettings> {
        let passes = self.sync_passes.clone();
        let _no_pass = no_pass_running(&passes)?;
        // A pass started before this, still to take the lock, then reads nothing and pushes nothing.
        let _writing = shared::Writing::start(&self.shared_writes, "");
        let settings = HostSettings {
            settings_repository: None,
            ..self.settings()
        };
        self.save_settings(&settings)?;
        self.db.execute("DELETE FROM sync_paused", [])?;
        self.sync = Synced::default();
        Ok(self.settings())
    }
    /// The pass sync runs next, or `None` while no settings repository is chosen.
    pub(crate) fn sync_pass(&mut self) -> Result<Option<Pass>> {
        let Some(repository) = self.settings().settings_repository else {
            return Ok(None);
        };
        let recorded = "SELECT EXISTS(SELECT 1 FROM sync_record WHERE path='')";
        if !self.db.query_row(recorded, [], |r| r.get::<_, bool>(0))? {
            // A repository chosen before sync's record existed.
            self.record_clone()?;
        }
        let rows: Vec<(String, Content, Content)> = self
            .db
            .prepare("SELECT path,synced,written FROM sync_record")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut synced: BTreeMap<String, Content> = BTreeMap::new();
        let mut written = BTreeMap::new();
        for (path, content, wrote) in rows {
            written.insert(path.clone(), wrote);
            synced.insert(path, content);
        }
        let commit = synced.remove("").flatten().map(String::from_utf8);
        Ok(Some(Pass {
            clone: self.home.join(shared::FOLDER),
            repository,
            commit: commit.transpose()?.context("Sync's record has no commit")?,
            synced,
            written,
            two_machines: (self.db.prepare(
                "SELECT path FROM sync_paused WHERE json_extract(why,'$.kind')='two_machines'",
            )?)
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
            writes: self.shared_writes.clone(),
            since: shared::lock(&self.shared_writes).ended(),
            running: self.sync_passes.clone(),
        }))
    }
    /// Writes what a pass pulled and records what it synced and paused; `true` when the status
    /// or a setting changed. What the host wrote or settled during the pass is left for the
    /// next one, and a pass that started before a clone was adopted is dropped.
    pub(crate) fn finish_sync(&mut self, at: i64, passed: Result<Passed>) -> Result<bool> {
        let passed = match passed {
            Ok(passed) => passed,
            Err(error) => {
                self.sync.stopped = error.is::<Stopped>();
                self.sync.failed(at, format!("{error:#}"));
                self.merge_after_pass(&[]);
                return Ok(true);
            }
        };
        let writes = self.shared_writes.clone();
        let touched = |path: &str| shared::lock(&writes).touched(path, passed.since);
        if self.settings().settings_repository.as_ref() != Some(&passed.repository) || touched("") {
            return Ok(false);
        }
        let sorting = self.settings().keeper.comments;
        let shared = self.home.join(shared::FOLDER);
        // A skill's rules, rewrite and base are pulled together or not at all.
        let skipped: std::collections::BTreeSet<String> = (passed.pulled.iter())
            .filter(|(path, here, _)| fs::read(shared.join(path)).ok() != *here || touched(path))
            .map(|(path, ..)| unit(path).to_owned())
            .collect();
        let mut pulled = vec![];
        for (path, _, there) in passed.pulled {
            let file = shared.join(&path);
            if skipped.contains(unit(&path)) {
                continue;
            }
            match &there {
                Some(content) => shared::write_atomically(&file, content)?,
                None => fs::remove_file(&file)?,
            }
            pulled.push((path, there));
        }
        let pushed = passed.pushed.is_some().then_some(passed.committed.len());
        let mut synced = passed.alike;
        synced.extend(passed.committed.into_iter().filter(|_| pushed.is_some()));
        let commit = passed.pushed.unwrap_or(passed.remote);
        synced.push((String::new(), Some(commit.into_bytes())));
        let tx = self.db.transaction()?;
        // The host wrote what it pulled; what it pushed or found alike keeps its last write.
        for (rows, written) in [(&pulled, "excluded.written"), (&synced, "written")] {
            for (path, content) in rows {
                tx.execute(
                    &format!("INSERT INTO sync_record(path,synced,written) VALUES (?1,?2,?2) ON CONFLICT(path) DO UPDATE SET synced=excluded.synced, written={written}"),
                    params![path, content],
                )?;
            }
        }
        let paused: Vec<String> = (tx.prepare("SELECT path FROM sync_paused")?)
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for path in paused.iter().filter(|path| !touched(path)) {
            tx.execute("DELETE FROM sync_paused WHERE path=?1", [path])?;
        }
        for (path, why, theirs) in passed.paused.iter().filter(|(path, ..)| !touched(path)) {
            tx.execute(
                "INSERT OR REPLACE INTO sync_paused(path,why,theirs) VALUES (?1,?2,?3)",
                params![path, serde_json::to_string(why)?, theirs],
            )?;
        }
        tx.commit()?;
        if !sorting && self.settings().keeper.comments {
            self.restart_comments()?;
        }
        if pulled.iter().any(|(path, _)| path.contains("/history/")) {
            self.recall_skill_histories();
        }
        self.sync = Synced {
            // A pass that ended in an error is not a sync: the last one that reached GitHub stays.
            at: if passed.error.is_none() {
                Some(at)
            } else {
                self.sync.at
            },
            pulled: pulled.len(),
            pushed: pushed.unwrap_or_default(),
            failing_since: self.sync.failing_since,
            next_pull_at: self.sync.next_pull_at,
            syncing: self.sync.syncing,
            ..Synced::default()
        };
        match passed.error {
            Some(error) => self.sync.failed(at, error),
            None => self.sync.failing_since = None,
        }
        self.merge_after_pass(&pulled);
        Ok(true)
    }
    /// Once a pass ended, reached GitHub or not: the merges held for this launch's first pass,
    /// and a merge of each skill whose files it pulled.
    fn merge_after_pass(&mut self, pulled: &[(String, Content)]) {
        let first = std::mem::take(&mut self.merges_wait_for_sync);
        let pulled = |skill: &Skill| {
            let unit = format!("skills/{}/", skill.folder());
            pulled.iter().any(|(path, _)| self::unit(path) == unit)
        };
        self.merge_due_skills(
            Skill::ALL
                .into_iter()
                .filter(|skill| first || pulled(skill)),
        );
    }
    /// Runs a pass on this thread, as a host with no service does for Sync now.
    pub(crate) fn sync_now(&mut self, tools: &Tools) -> Result<()> {
        if let Some(pass) = self.sync_pass()? {
            self.finish_sync(now(), self::pass(pass, tools))?;
        }
        Ok(())
    }
    /// Whether, with a settings repository, any of the shared `paths` holds what the host
    /// neither last wrote nor last synced, which the next pass pauses.
    pub(crate) fn changed_outside_wiffletree(&self, paths: &[String]) -> Result<bool> {
        if self.settings().settings_repository.is_none() {
            return Ok(false);
        }
        let shared = self.home.join(shared::FOLDER);
        for path in paths {
            let recorded = "SELECT synced,written FROM sync_record WHERE path=?1";
            let (synced, written): (Content, Content) = (self.db)
                .query_row(recorded, [path], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?
                .unwrap_or_default();
            let mine = fs::read(shared.join(path)).ok();
            if mine != synced && mine != written {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(crate) fn sync_status(&self) -> Result<Option<SyncStatus>> {
        let Some(repository) = self.settings().settings_repository else {
            return Ok(None);
        };
        let waiting: Vec<String> = (self.db.prepare(
            "SELECT path FROM sync_record WHERE path<>'' AND written IS NOT synced AND path NOT IN (SELECT path FROM sync_paused) ORDER BY path",
        )?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        Ok(Some(SyncStatus {
            repository,
            synced_at: self.sync.at,
            pulled: self.sync.pulled,
            pushed: self.sync.pushed,
            next_pull_at: self.sync.next_pull_at,
            error: self.sync.error.clone(),
            failed_at: self.sync.failed_at,
            failing_since: self.sync.failing_since,
            stopped: self.sync.stopped,
            syncing: self.sync.syncing,
            waiting: names(&waiting),
            machine: machine_name(),
        }))
    }
    /// One note per paused setting, a skill's rules, rewrite and base being one: two machines'
    /// changes first, then edits made outside Wiffletree, then what this machine cannot read.
    /// Then one per skill question, from `skills`, which are the skill states.
    pub(crate) fn workspace_notes(&self, skills: &[SkillState]) -> Result<Vec<WorkspaceNote>> {
        let rows: Vec<(String, String)> = (self.db.prepare("SELECT path,why FROM sync_paused")?)
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut notes = BTreeMap::new();
        for (path, why) in rows {
            let path = unit(&path).to_owned();
            let paused: Paused = serde_json::from_str(&why)?;
            let setting = self.setting(&path);
            // "rules" or "rewrite" for a skill.
            let own = setting
                .rsplit_once(" · your ")
                .map_or("version", |(_, own)| own);
            let text = match &paused {
                Paused::TwoMachines { machine } => format!(
                    "{setting} changed on this Mac and on {machine} since they last synced. This Mac keeps using its own {own}. Other settings keep syncing."
                ),
                Paused::OutsideWiffletree => format!(
                    "{setting} was changed outside Wiffletree on this Mac, by an editor, a script or an agent’s shell. It isn’t sent to your other Macs until you choose. This Mac uses it as it is now."
                ),
                Paused::CannotBeRead { machine, reason } => format!(
                    "{setting} from {machine} can’t be used here: {reason}. This Mac keeps its own {own}."
                ),
            };
            let note = PausedSetting {
                path: path.clone(),
                setting,
                paused,
                text,
            };
            notes.insert(path, note);
        }
        let mut notes: Vec<PausedSetting> = notes.into_values().collect();
        notes.sort_by_key(|note| match note.paused {
            Paused::TwoMachines { .. } => 0,
            Paused::OutsideWiffletree => 1,
            Paused::CannotBeRead { .. } => 2,
        });
        let questions = (skills.iter()).filter_map(|state| state.question.clone());
        let notes = notes.into_iter().map(WorkspaceNote::Paused);
        Ok(notes
            .chain(questions.map(WorkspaceNote::SkillQuestion))
            .collect())
    }
    /// A paused file as the app names it, such as "Implementing · your rules".
    fn setting(&self, path: &str) -> String {
        let name = names(&[path.to_owned()]).join("");
        let rewritten = self.home.join(shared::FOLDER).join(path).join("rewrite.md");
        match path {
            _ if path.contains("/history/") => format!("{name} · a history entry"),
            _ if !path.ends_with('/') => name,
            _ if rewritten.exists() => format!("{name} · your rewrite"),
            _ => format!("{name} · your rules"),
        }
    }
    /// Why the file, or a skill's folder, a note names is paused, and each of its files.
    fn paused_unit(&self, path: &str) -> Result<(Paused, Vec<PausedFile>)> {
        let mut rows: Vec<(String, String, Content, Content)> = (self.db.prepare(
            "SELECT p.path,p.why,p.theirs,r.synced FROM sync_paused p LEFT JOIN sync_record r USING(path)",
        )?)
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
        rows.retain(|(file, ..)| unit(file) == path);
        let (_, why, ..) = rows.first().context("That setting is not paused")?;
        let paused = serde_json::from_str(why)?;
        let files = rows
            .into_iter()
            .map(|(file, _, theirs, synced)| (file, theirs, synced));
        Ok((paused, files.collect()))
    }
    /// This machine's version beside the other, line by line, as a note's Compare shows them.
    pub(crate) fn compare_paused(&self, path: &str) -> Result<Vec<Line>> {
        let (paused, files) = self.paused_unit(path)?;
        let shared = self.home.join(shared::FOLDER);
        // A skill's agents get its rewrite, or else its rules; `base.md` is never theirs to read.
        let text = |version: &dyn Fn(&PausedFile) -> Content| {
            let read = |rewrite: bool| {
                let file = files.iter().find(|(file, ..)| {
                    file.ends_with("/rewrite.md") == rewrite && !file.ends_with("/base.md")
                });
                file.and_then(version)
            };
            let content = read(true).or_else(|| read(false)).unwrap_or_default();
            String::from_utf8_lossy(&content).into_owned()
        };
        let mine = text(&|(file, ..)| fs::read(shared.join(file)).ok());
        let other = text(&|(_, theirs, synced)| match paused {
            Paused::OutsideWiffletree => synced.clone(),
            _ => theirs.clone(),
        });
        Ok(skills::line_diff(&mine, &other))
    }
    /// Settles a paused file, or a skill's rules, rewrite and base together, as its note offers.
    /// The next pass, due five seconds later, pushes or pulls what the choice leaves.
    pub(crate) fn settle_paused(&self, path: &str, settle: Settle) -> Result<()> {
        let (paused, mut rows) = self.paused_unit(path)?;
        let offered = match paused {
            Paused::TwoMachines { .. } => [Settle::KeepThisMachines, Settle::UseTheOthers],
            Paused::OutsideWiffletree => [Settle::SyncThisVersion, Settle::PutBackSynced],
            Paused::CannotBeRead { .. } => [Settle::KeepThisMachines; 2],
        };
        ensure!(
            offered.contains(&settle),
            "That note does not offer this choice"
        );
        let _writing = shared::Writing::start(&self.shared_writes, path);
        let shared = self.home.join(shared::FOLDER);
        let mine = |file: &str| fs::read(shared.join(file)).ok();
        if settle == Settle::SyncThisVersion {
            for (file, ..) in &rows {
                if let Some(content) = mine(file) {
                    shared::check(file, &content)
                        .with_context(|| format!("{file} can't be synced"))?;
                }
            }
        }
        let recorder = self.recorder();
        // `rewrite.md` decides a skill's mode, so it changes last.
        rows.sort_by_key(|(file, ..)| file.ends_with("/rewrite.md"));
        for (file, theirs, synced) in rows {
            let at = shared.join(&file);
            let (written, synced) = match settle {
                Settle::KeepThisMachines => (None, Some(theirs)),
                Settle::UseTheOthers => (Some(theirs.clone()), Some(theirs)),
                Settle::SyncThisVersion => (Some(mine(&file)), Some(theirs)),
                Settle::PutBackSynced => (Some(synced), None),
            };
            if let Some(content) = written {
                if settle != Settle::SyncThisVersion {
                    match &content {
                        Some(content) => shared::write_atomically(&at, content)?,
                        None => drop(fs::remove_file(&at)),
                    }
                }
                recorder.wrote(&at, content.as_deref())?;
            }
            if let Some(synced) = synced {
                self.db.execute(
                    "INSERT INTO sync_record(path,synced) VALUES (?1,?2) ON CONFLICT(path) DO UPDATE SET synced=excluded.synced",
                    params![file, synced],
                )?;
            }
            self.db
                .execute("DELETE FROM sync_paused WHERE path=?1", [&file])?;
        }
        self.shared_written.set(true);
        Ok(())
    }
    /// Starts sync's record afresh from the clone: each file as its head holds it is synced, and
    /// as the folder holds it written, so a change not yet committed waits to be pushed.
    fn record_clone(&mut self) -> Result<()> {
        let shared = self.home.join(shared::FOLDER);
        let head = runtime::git_output(&shared, &["rev-parse", "HEAD"])?;
        let head = head.trim();
        let (synced, written) = (files_at(&shared, head)?, files_here(&shared)?);
        let paths: std::collections::BTreeSet<&String> =
            synced.keys().chain(written.keys()).collect();
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM sync_record", [])?;
        tx.execute("DELETE FROM sync_paused", [])?;
        tx.execute(
            "INSERT INTO sync_record(path,synced) VALUES ('',?1)",
            [head.as_bytes()],
        )?;
        for path in paths {
            tx.execute(
                "INSERT INTO sync_record(path,synced,written) VALUES (?1,?2,?3)",
                params![path, synced.get(path), written.get(path)],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// What this machine lacks to run the settings it shares through a repository, one line each.
    /// Every line is about this machine's models, which Models chooses.
    pub(crate) fn still_needs(&self) -> Result<Vec<StillNeed>> {
        let settings = self.settings();
        let mut needs = vec![];
        if settings.settings_repository.is_none() {
            return Ok(vec![]);
        }
        let selection = self.model_selection()?;
        needs.extend(self.keeper_held());
        for focus in &selection.copied {
            needs.push(format!(
                "The {focus} verifier has no row in Who runs what on this machine; it uses a copy of another row until you check it and save"
            ));
        }
        let page = WorkspacePage::Models;
        Ok((needs.into_iter())
            .map(|text| StillNeed { text, page })
            .collect())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::publish::tests::{exit, git};
    use crate::test_support::without_background_maintenance;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub(crate) const NAME: &str = "MHohlios/wiffletree-storage";
    const README: &str = "# My Wiffletree storage\n";

    /// A local bare repository standing in for GitHub, the answers a fake `gh` gives, and every
    /// command each tool was sent. No test reaches GitHub.
    type Hook = (&'static str, Box<dyn FnOnce() + Send>);

    pub(crate) struct GitHub {
        bare: PathBuf,
        repository: Value,
        organization: Option<Value>,
        identity: (&'static str, &'static str),
        gh: Mutex<Vec<Vec<String>>>,
        git: Mutex<Vec<Vec<String>>>,
        /// Git cannot reach GitHub.
        offline: AtomicBool,
        /// The Git protocol gh is configured with.
        protocol: Mutex<&'static str>,
        /// The beginnings of URLs Git cannot read the repository by, as without an SSH key or
        /// HTTPS login.
        unreadable: Mutex<Vec<&'static str>>,
        /// Runs before the next Git command of that name, as another machine's push or a
        /// save made meanwhile would.
        before: Mutex<Option<Hook>>,
    }
    impl GitHub {
        pub(crate) fn new(folder: &Path, readme: bool) -> Self {
            let bare = folder.join("github.git");
            git(
                folder,
                &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
            );
            if readme {
                let work = folder.join("created-on-github");
                git(
                    folder,
                    &["init", "-q", "-b", "main", work.to_str().unwrap()],
                );
                fs::write(work.join("README.md"), README).unwrap();
                git(&work, &["add", "."]);
                git(&work, &["commit", "-q", "-m", "Initial commit"]);
                git(&work, &["push", "-q", bare.to_str().unwrap(), "main"]);
            }
            Self {
                bare,
                repository: json!({"full_name": NAME, "private": true, "default_branch": "main",
                    "owner": {"login": "MHohlios", "type": "User"}}),
                organization: None,
                identity: ("F", "f@example.invalid"),
                gh: Mutex::default(),
                git: Mutex::default(),
                offline: AtomicBool::new(false),
                protocol: Mutex::new("https"),
                unreadable: Mutex::default(),
                before: Mutex::default(),
            }
        }
        pub(crate) fn tools(self: &Arc<Self>) -> Tools {
            let github = self.clone();
            let gh = move |_: &Path, args: &[&str]| -> Result<runtime::GhAnswer> {
                github
                    .gh
                    .lock()
                    .unwrap()
                    .push(args.iter().map(|a| a.to_string()).collect());
                if args[0] == "config" {
                    assert_eq!(args, ["config", "get", "git_protocol", "-h", "github.com"]);
                    return Ok(runtime::GhAnswer {
                        status: exit(0),
                        output: format!("{}\n", github.protocol.lock().unwrap()),
                        error: String::new(),
                    });
                }
                assert_eq!(args[..3], ["api", "--hostname", "github.com"], "reads only");
                assert_eq!(args.len(), 4, "a GET with no fields: {args:?}");
                let answer = match args[3].split_once('/') {
                    Some(("repos", _)) => Some(&github.repository),
                    _ => github.organization.as_ref(),
                };
                Ok(runtime::GhAnswer {
                    status: exit(if answer.is_some() { 0 } else { 1 }),
                    output: answer.map(Value::to_string).unwrap_or_default(),
                    error: answer.map_or("HTTP 404: Not Found", |_| "").into(),
                })
            };
            let github = self.clone();
            let git = move |path: &Path, args: &[&str]| -> Result<runtime::GitAnswer> {
                github
                    .git
                    .lock()
                    .unwrap()
                    .push(args.iter().map(|a| a.to_string()).collect());
                let bare = github.bare.to_str().unwrap();
                let (name, email) = github.identity;
                let (name, email) = (format!("user.name={name}"), format!("user.email={email}"));
                let mut local = vec!["-c", &name, "-c", &email];
                let is_url = |a: &str| {
                    ["https://github.com/", "git@github.com:"]
                        .iter()
                        .any(|p| a.starts_with(p))
                };
                local.extend(args.iter().map(|&a| if is_url(a) { bare } else { a }));
                let url = args.iter().find(|a| is_url(a));
                if github.offline.load(Ordering::Relaxed) && url.is_some() {
                    return Ok(runtime::GitAnswer {
                        status: exit(128),
                        output: String::new(),
                        error: "fatal: unable to access 'https://github.com/MHohlios/wiffletree-storage.git/': Could not resolve host: github.com".into(),
                    });
                }
                let unreadable = github.unreadable.lock().unwrap().clone();
                if let Some(url) = url.filter(|url| unreadable.iter().any(|p| url.starts_with(p))) {
                    let error = match url.starts_with("git@") {
                        true => "git@github.com: Permission denied (publickey).",
                        false => {
                            "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
                        }
                    };
                    return Ok(runtime::GitAnswer {
                        status: exit(128),
                        output: String::new(),
                        error: error.into(),
                    });
                }
                let command = publish::subcommand(args);
                let mut before = github.before.lock().unwrap();
                if before.as_ref().is_some_and(|(c, _)| *c == command) {
                    before.take().unwrap().1();
                }
                drop(before);
                let answer = match command {
                    "push" => runtime::git_push(path, &local),
                    _ => runtime::git_answer(path, &local),
                };
                if command == "clone" && answer.as_ref().is_ok_and(|a| a.status.success()) {
                    without_background_maintenance(&path.join(args.last().unwrap()));
                }
                answer
            };
            Tools {
                gh: Arc::new(gh),
                git: Arc::new(git),
            }
        }
        pub(crate) fn log(&self, format: &str) -> Vec<String> {
            let log = git(&self.bare, &["log", &format!("--format={format}"), "main"]);
            log.lines().map(str::to_owned).collect()
        }
        fn show(&self, file: &str) -> Option<String> {
            let output = std::process::Command::new("git")
                .current_dir(&self.bare)
                .args(["show", &format!("main:{file}")])
                .output()
                .unwrap();
            output
                .status
                .success()
                .then(|| String::from_utf8(output.stdout).unwrap())
        }
        /// A commit made on GitHub, as the human would in its web editor.
        fn commit(&self, file: &str, content: impl AsRef<[u8]>) {
            commit_on(&self.bare, file, content);
        }
        /// Every Git command of that name sent, with the options before it.
        fn calls(&self, command: &str) -> Vec<Vec<String>> {
            let named = |args: &&Vec<String>| {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                publish::subcommand(&args) == command
            };
            self.git
                .lock()
                .unwrap()
                .iter()
                .filter(named)
                .cloned()
                .collect()
        }
        fn pushes(&self) -> usize {
            self.calls("push").len()
        }
    }

    fn commit_on(bare: &Path, file: &str, content: impl AsRef<[u8]>) {
        let work = tempfile::tempdir().unwrap();
        git(work.path(), &["clone", "-q", bare.to_str().unwrap(), "."]);
        let path = work.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        git(work.path(), &["add", "--all"]);
        git(work.path(), &["commit", "-q", "-m", file]);
        git(work.path(), &["push", "-q", "origin", "main"]);
    }

    fn host() -> (tempfile::TempDir, Host) {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open_set_up(home.path()).unwrap();
        (home, host)
    }

    fn listing(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with("store.sqlite"))
            .collect();
        names.sort();
        names
    }

    fn save_rules(host: &mut Host, skill: Skill, rules: &str) {
        let rules = rules.into();
        host.execute(Command::SaveSkillRules { skill, rules })
            .unwrap();
    }

    #[test]
    fn a_repository_is_named_as_owner_and_name_tried_by_ghs_protocol_first_or_by_its_url() {
        let https = "https://github.com/MHohlios/wiffletree-storage.git";
        let ssh = "git@github.com:MHohlios/wiffletree-storage.git";
        for (input, protocol, urls) in [
            (NAME, "https", vec![https, ssh]),
            (" MHohlios/wiffletree-storage ", "ssh", vec![ssh, https]),
            (NAME, "", vec![https, ssh]),
            (https, "ssh", vec![https]),
            (ssh, "https", vec![ssh]),
        ] {
            let urls: Vec<String> = urls.into_iter().map(str::to_owned).collect();
            assert_eq!(
                candidates(input, protocol).unwrap(),
                ("MHohlios".into(), "wiffletree-storage".into(), urls)
            );
        }
        for input in [
            "gitlab.com:me/storage",
            "https://gitlab.com/me/storage.git",
            "me",
            "/tmp/storage.git",
        ] {
            assert!(candidates(input, "https").is_err(), "{input}");
        }
    }

    #[test]
    fn only_https_on_github_is_given_ghs_login_and_never_a_token() {
        let login = gh_login("https://github.com/MHohlios/wiffletree-storage.git");
        assert_eq!(
            login,
            [
                "-c",
                "credential.https://github.com.helper=",
                "-c",
                "credential.https://github.com.helper=!gh auth git-credential"
            ]
        );
        for url in [
            "git@github.com:MHohlios/wiffletree-storage.git",
            "ssh://git@github.com/MHohlios/wiffletree-storage.git",
        ] {
            assert!(gh_login(url).is_empty(), "{url}");
        }
        assert_eq!(
            publish::subcommand(&[login, &["ls-remote"]].concat()),
            "ls-remote"
        );
    }

    #[test]
    fn with_ghs_ssh_protocol_set_up_and_sync_reach_github_over_ssh_without_ghs_login() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        *github.protocol.lock().unwrap() = "ssh";
        *github.unreadable.lock().unwrap() = vec!["https://"];
        let ssh = "git@github.com:MHohlios/wiffletree-storage.git";
        let check = check(&host.home, NAME, &github.tools()).unwrap();
        assert_eq!(check.repository.push_url, ssh);
        host.set_check_ins(10, 300).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        github.commit("notes.md", "From GitHub.\n");
        host.set_check_ins(20, 300).unwrap();
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(status(&host).error, None);
        assert!(github.show(shared::CHECK_INS).unwrap().contains("20"));
        assert!(home.path().join("shared/notes.md").exists());
        for command in ["ls-remote", "clone", "fetch", "push"] {
            let calls = github.calls(command);
            assert!(!calls.is_empty(), "{command}");
            for call in calls {
                assert!(
                    call.contains(&ssh.to_owned()) && call[0] == command,
                    "{call:?}"
                );
            }
        }
    }

    #[test]
    fn without_an_ssh_key_set_up_and_sync_reach_github_over_https_with_ghs_login() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        *github.protocol.lock().unwrap() = "ssh";
        *github.unreadable.lock().unwrap() = vec!["git@"];
        let https = "https://github.com/MHohlios/wiffletree-storage.git";
        host.set_check_ins(10, 300).unwrap();
        let settings = host
            .choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        assert_eq!(settings.settings_repository.unwrap().push_url, https);
        github.commit("notes.md", "From GitHub.\n");
        host.set_check_ins(20, 300).unwrap();
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(status(&host).error, None);
        assert!(github.show(shared::CHECK_INS).unwrap().contains("20"));
        assert!(home.path().join("shared/notes.md").exists());
        let ls_remote = github.calls("ls-remote");
        assert!(
            ls_remote[0].contains(&"git@github.com:MHohlios/wiffletree-storage.git".to_owned())
        );
        for command in ["ls-remote", "clone", "fetch", "push"] {
            for call in github
                .calls(command)
                .into_iter()
                .filter(|c| c.contains(&https.to_owned()))
            {
                assert_eq!(call[..4], *gh_login(https), "{call:?}");
            }
        }
        assert_eq!(github.calls("fetch").len(), 1);
        assert_eq!(github.calls("push").len(), 2);
    }

    #[test]
    fn a_url_the_human_names_is_the_only_one_tried() {
        let (home, host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        *github.protocol.lock().unwrap() = "ssh";
        let https = "https://github.com/MHohlios/wiffletree-storage.git";
        let check = check(&host.home, https, &github.tools()).unwrap();
        assert_eq!(check.repository.push_url, https);
        let ls_remote = github.calls("ls-remote");
        assert_eq!(ls_remote.len(), 1);
        assert_eq!(ls_remote[0][..4], *gh_login(https));
        *github.unreadable.lock().unwrap() = vec!["git@"];
        let ssh = "git@github.com:MHohlios/wiffletree-storage.git";
        let error = format!(
            "{:#}",
            super::check(&host.home, ssh, &github.tools()).unwrap_err()
        );
        assert!(error.contains(ssh) && !error.contains(https), "{error}");
    }

    #[test]
    fn when_no_way_reads_the_repository_each_url_and_its_reason_is_named() {
        let (home, host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        *github.protocol.lock().unwrap() = "ssh";
        *github.unreadable.lock().unwrap() = vec!["git@", "https://"];
        let before = listing(home.path());
        let error = format!(
            "{:#}",
            check(&host.home, NAME, &github.tools()).unwrap_err()
        );
        assert_eq!(
            error,
            "This machine cannot read the repository. \
             Tried git@github.com:MHohlios/wiffletree-storage.git: git ls-remote failed: git@github.com: Permission denied (publickey).; \
             tried https://github.com/MHohlios/wiffletree-storage.git: git ls-remote failed: fatal: could not read Username for 'https://github.com': terminal prompts disabled. \
             Add an SSH key to GitHub, sign gh in with gh auth login, or check that the repository exists and is yours to read."
        );
        assert_eq!(listing(home.path()), before);
        assert!(github.calls("clone").is_empty());
    }

    #[test]
    fn a_users_private_repository_is_read_by_its_owner_and_checking_writes_nothing() {
        let (home, host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        let before = (listing(home.path()), listing(&home.path().join("shared")));
        let check = check(&host.home, NAME, &github.tools()).unwrap();
        assert_eq!(
            check,
            SettingsRepositoryCheck {
                repository: SettingsRepository {
                    name: NAME.into(),
                    push_url: "https://github.com/MHohlios/wiffletree-storage.git".into(),
                    branch: "main".into(),
                    readers: Readers::Invited,
                },
                organization: false,
                joining: None,
            }
        );
        assert_eq!(
            (listing(home.path()), listing(&home.path().join("shared"))),
            before
        );
        assert_eq!(github.log("%s"), ["Initial commit"]);
        assert_eq!(github.pushes(), 0);
        assert_eq!(host.settings().settings_repository, None);
    }

    #[test]
    fn an_organizations_base_permission_says_who_else_can_read_it() {
        let (_home, host) = host();
        for (permission, readers) in [
            (
                Some(json!({"default_repository_permission": "read"})),
                Readers::EveryMember,
            ),
            (
                Some(json!({"default_repository_permission": "none"})),
                Readers::Invited,
            ),
            (Some(json!({"login": "Sage-Social"})), Readers::MaybeMembers),
            (None, Readers::MaybeMembers),
        ] {
            let folder = tempfile::tempdir().unwrap();
            let mut github = GitHub::new(folder.path(), true);
            github.repository["owner"] = json!({"login": "Sage-Social", "type": "Organization"});
            github.organization = permission;
            let github = Arc::new(github);
            let check = check(
                &host.home,
                "Sage-Social/wiffletree-storage",
                &github.tools(),
            )
            .unwrap();
            assert!(check.organization);
            assert_eq!(check.repository.readers, readers);
            let calls = github.gh.lock().unwrap().clone();
            assert_eq!(calls[2][3], "orgs/Sage-Social");
        }
    }

    #[test]
    fn a_public_repository_is_refused_and_nothing_is_written() {
        let (home, host) = host();
        let mut github = GitHub::new(home.path(), true);
        github.repository["private"] = json!(false);
        let github = Arc::new(github);
        let before = listing(home.path());
        for result in [
            check(&host.home, NAME, &github.tools()).map(drop),
            prepare(&host.home, NAME, None, &github.tools()).map(drop),
        ] {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("Your skills and settings would be public"),
                "{error}"
            );
        }
        assert_eq!(listing(home.path()), before);
        assert!(github.git.lock().unwrap().is_empty());
    }

    #[test]
    fn setting_up_beside_a_readme_adds_the_shared_settings_in_one_pushed_commit() {
        for readme in [true, false] {
            let (home, mut host) = host();
            let github = Arc::new(GitHub::new(home.path(), readme));
            host.set_check_ins(10, 300).unwrap();
            save_rules(&mut host, Skill::Implementing, "Run the linter.");
            let settings = host
                .choose_settings_repository(NAME, None, &github.tools())
                .unwrap();
            let commits = github.log("%s");
            assert_eq!(commits.len(), 1 + readme as usize);
            assert_eq!(
                commits[0],
                format!("Set up the shared settings · {}", machine_name())
            );
            let changed = git(&github.bare, &["show", "--format=", "--name-only", "main"]);
            assert!(changed.lines().all(is_shared), "{changed}");
            assert!(
                changed.contains("settings/check-ins.json") && changed.contains("wiffletree.json")
            );
            assert_eq!(
                github.show("skills/implementing/additions.md").unwrap(),
                "Run the linter."
            );
            assert_eq!(
                github.show("README.md").as_deref(),
                readme.then_some(README)
            );
            // The push named one branch at the push URL, sent no tags and was not forced.
            let push = github.calls("push").remove(0);
            assert_eq!(push[..4], *gh_login(&push[7]), "gh's login over HTTPS");
            let push = &push[4..];
            assert_eq!(
                push[..3],
                ["push", "--no-follow-tags", "--recurse-submodules=no"]
            );
            assert_eq!(
                push[3],
                "https://github.com/MHohlios/wiffletree-storage.git"
            );
            assert!(
                push[4].ends_with(":refs/heads/main") && push.len() == 5,
                "{push:?}"
            );
            // The shared folder is the clone, the old one is kept, and the choice stays here.
            let shared = home.path().join("shared");
            assert_eq!(git(&shared, &["rev-parse", "HEAD"]), github.log("%H")[0]);
            assert_eq!(listing(&home.path().join("backups")).len(), 1);
            assert_eq!(host.settings(), settings);
            assert_eq!(settings.settings_repository.unwrap().branch, "main");
            assert_eq!(
                (settings.checkin_child_turns, settings.checkin_human_turns),
                (10, 300)
            );
            let saved: Value =
                serde_json::from_slice(&fs::read(home.path().join("settings.json")).unwrap())
                    .unwrap();
            assert_eq!(saved["settings_repository"]["name"], NAME);
            assert!(!shared.join("settings.json").exists());
            assert!(
                listing(home.path())
                    .iter()
                    .all(|name| !name.starts_with("shared-staging"))
            );
            // A repository set up once is joined, not set up again.
            let again = host.choose_settings_repository(NAME, None, &github.tools());
            assert!(format!("{:#}", again.unwrap_err()).contains("join it"));
        }
    }

    #[test]
    fn setting_up_keeps_the_repositorys_own_files_in_settings_and_skills_and_overwrites_none() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        github.commit("settings/notes.txt", "Mine.\n");
        github.commit("skills/notes.txt", "Also mine.\n");
        host.set_check_ins(10, 300).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        assert_eq!(github.show("settings/notes.txt").unwrap(), "Mine.\n");
        assert_eq!(github.show("skills/notes.txt").unwrap(), "Also mine.\n");
        assert!(github.show("settings/check-ins.json").is_some());
        // A file where this machine's would go is refused, and nothing is pushed.
        let (home, mut host) = self::host();
        let github = Arc::new(GitHub::new(home.path(), true));
        github.commit("settings/check-ins.json", "{}\n");
        host.set_check_ins(10, 300).unwrap();
        let error = host
            .choose_settings_repository(NAME, None, &github.tools())
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("settings/check-ins.json"),
            "{error:#}"
        );
        assert_eq!(
            (
                github.pushes(),
                github.show("settings/check-ins.json").unwrap()
            ),
            (0, "{}\n".into())
        );
        assert!(!home.path().join("shared/.git").exists());
    }

    #[test]
    fn a_symbolic_link_in_the_shared_paths_is_refused_and_never_followed() {
        for (link, target) in [("settings", "other"), ("skills/implementing", "../other")] {
            let (home, mut host) = host();
            let github = Arc::new(GitHub::new(home.path(), true));
            github.commit("other/README.md", "Unrelated.\n");
            let work = tempfile::tempdir().unwrap();
            git(
                work.path(),
                &["clone", "-q", github.bare.to_str().unwrap(), "."],
            );
            let at = work.path().join(link);
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, &at).unwrap();
            git(work.path(), &["add", "--all"]);
            git(work.path(), &["commit", "-q", "-m", "Link"]);
            git(work.path(), &["push", "-q", "origin", "main"]);
            host.set_check_ins(10, 300).unwrap();
            for result in [
                check(&host.home, NAME, &github.tools()).map(drop),
                host.choose_settings_repository(NAME, None, &github.tools())
                    .map(drop),
            ] {
                let error = format!("{:#}", result.unwrap_err());
                assert!(
                    error.contains(&format!("{link} is a symbolic link")),
                    "{error}"
                );
            }
            assert_eq!(
                (github.pushes(), github.show("other/check-ins.json")),
                (0, None)
            );
            assert!(!home.path().join("shared/.git").exists());
        }
        // A link in this machine's shared folder is refused, not followed.
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        let outside = home.path().join("outside.md");
        fs::write(&outside, "Not a setting.").unwrap();
        let guide = home.path().join("shared/settings/model-guide.md");
        fs::create_dir_all(guide.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&guide);
        std::os::unix::fs::symlink(&outside, &guide).unwrap();
        let error = host
            .choose_settings_repository(NAME, None, &github.tools())
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("is a symbolic link"),
            "{error:#}"
        );
        assert_eq!(github.pushes(), 0);
    }

    #[test]
    fn an_internal_repository_is_readable_by_every_member_of_its_enterprise() {
        let (home, host) = host();
        let mut github = GitHub::new(home.path(), true);
        github.repository["visibility"] = json!("internal");
        github.repository["owner"] = json!({"login": "Sage-Social", "type": "Organization"});
        github.organization = Some(json!({"default_repository_permission": "none"}));
        let check = check(&host.home, NAME, &Arc::new(github).tools()).unwrap();
        assert_eq!(check.repository.readers, Readers::EnterpriseMembers);
    }

    #[test]
    fn a_save_made_while_setting_up_is_kept_and_the_clone_refused() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.set_check_ins(10, 300).unwrap();
        let prepared = prepare(&host.home, NAME, None, &github.tools()).unwrap();
        host.set_check_ins(20, 200).unwrap();
        let error = host.adopt_settings_repository(prepared).unwrap_err();
        assert!(
            format!("{error:#}").contains("changed meanwhile"),
            "{error:#}"
        );
        let settings = host.settings();
        assert_eq!(
            (settings.checkin_child_turns, settings.checkin_human_turns),
            (20, 200)
        );
        assert_eq!(settings.settings_repository, None);
        assert!(!home.path().join("shared/.git").exists());
    }

    #[test]
    fn a_choice_that_cannot_be_saved_puts_the_shared_folder_back() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first, _) = set_up_elsewhere(&github);
        let (home, mut host) = host();
        let mut selection = host.model_selection().unwrap();
        selection.guide = "Local guide.".into();
        host.set_model_selection(&selection).unwrap();
        let prepared = prepare(
            &host.home,
            NAME,
            Some(JoinWith::Repository),
            &github.tools(),
        )
        .unwrap();
        let settings_file = home.path().join("settings.json");
        let _ = fs::remove_file(&settings_file);
        fs::create_dir(&settings_file).unwrap();
        assert!(host.adopt_settings_repository(prepared).is_err());
        assert_eq!(host.model_selection().unwrap().guide, "Local guide.");
        assert!(!home.path().join("shared/.git").exists());
        assert!(listing(&home.path().join("backups")).is_empty());
    }

    #[test]
    fn history_entries_with_one_name_from_two_machines_are_both_kept() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first, first) = set_up_elsewhere(&github);
        let (theirs_name, theirs) = first.skills().history(Skill::Coordinating).remove(0);
        let (home, host) = host();
        let mine = skills::Entry {
            edits: skills::Edits {
                rules: "From this machine.".into(),
                rewrite: None,
            },
            ..theirs.clone()
        };
        let history = home.path().join("shared/skills/coordinating/history");
        fs::create_dir_all(&history).unwrap();
        fs::write(
            history.join(format!("{theirs_name}.json")),
            serde_json::to_vec(&mine).unwrap(),
        )
        .unwrap();
        let mut host = host;
        host.choose_settings_repository(NAME, Some(JoinWith::Repository), &github.tools())
            .unwrap();
        let kept: Vec<skills::Entry> = host
            .skills()
            .history(Skill::Coordinating)
            .into_iter()
            .map(|(_, e)| e)
            .collect();
        assert_eq!(kept, [theirs, mine]);
        assert!(
            github
                .show(&format!("skills/coordinating/history/{theirs_name}-2.json"))
                .is_some()
        );
    }

    #[test]
    fn an_owner_allowed_for_pull_requests_neither_allows_nor_is_needed_for_sync() {
        let (home, mut host) = host();
        host.set_owner_allowed("MHohlios", true).unwrap();
        host.set_check_ins(10, 300).unwrap();
        save_rules(&mut host, Skill::Implementing, "Run the linter.");
        assert_eq!(host.settings().settings_repository, None);
        assert!(!home.path().join("shared/.git").exists());
        host.set_owner_allowed("MHohlios", false).unwrap();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        assert_eq!(github.pushes(), 1);
    }

    #[test]
    fn a_push_carries_only_shared_paths_on_top_of_the_remote_head() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        let repository = host
            .choose_settings_repository(NAME, None, &github.tools())
            .unwrap()
            .settings_repository
            .unwrap();
        let clone = home.path().join("shared");
        let tip = github.log("%H")[0].clone();
        // A commit that touches README.md is refused before anything is pushed.
        fs::write(clone.join("README.md"), "Changed here.\n").unwrap();
        fs::write(clone.join("settings/check-ins.json"), "{}\n").unwrap();
        git(&clone, &["add", "--all"]);
        git(&clone, &["commit", "-q", "-m", "Both"]);
        let error = push(&clone, &repository, &github.tools().git).unwrap_err();
        assert!(format!("{error:#}").contains("README.md"), "{error:#}");
        assert_eq!((github.pushes(), &github.log("%H")[0]), (1, &tip));
        git(&clone, &["reset", "-q", "--hard", "HEAD~"]);
        // A commit on a head GitHub has moved past is refused, and nothing is overwritten.
        let elsewhere = home.path().join("elsewhere");
        git(
            home.path(),
            &[
                "clone",
                "-q",
                github.bare.to_str().unwrap(),
                elsewhere.to_str().unwrap(),
            ],
        );
        git(
            &elsewhere,
            &["commit", "-q", "--allow-empty", "-m", "Another machine"],
        );
        git(&elsewhere, &["push", "-q", "origin", "main"]);
        fs::write(clone.join("settings/check-ins.json"), "{}\n").unwrap();
        git(&clone, &["add", "--all"]);
        git(&clone, &["commit", "-q", "-m", "Check-ins"]);
        assert!(
            format!(
                "{:#}",
                push(&clone, &repository, &github.tools().git).unwrap_err()
            )
            .contains("moved")
        );
        assert_eq!(github.log("%s")[0], "Another machine");
    }

    #[test]
    fn without_a_git_identity_nothing_is_pushed_and_the_shared_folder_stays() {
        let (home, mut host) = host();
        let mut github = GitHub::new(home.path(), true);
        github.identity = ("", "");
        let github = Arc::new(github);
        let error = host
            .choose_settings_repository(NAME, None, &github.tools())
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("Git has no name and email set"),
            "{error:#}"
        );
        assert_eq!(github.log("%s"), ["Initial commit"]);
        assert!(!home.path().join("shared/.git").exists());
        assert_eq!(host.settings().settings_repository, None);
    }

    /// The first machine's repository: Implementing with 3 rules, Coordinating with a history
    /// entry, Reviewing rewritten, check-ins 10 and 300, Sage-Social allowed and Codex enabled.
    fn set_up_elsewhere(github: &Arc<GitHub>) -> (tempfile::TempDir, Host) {
        let (home, mut host) = host();
        host.set_owner_allowed("Sage-Social", true).unwrap();
        host.set_check_ins(10, 300).unwrap();
        save_rules(
            &mut host,
            Skill::Implementing,
            "- Run the linter.\n- Keep commits small.\n- Name the ticket.\n",
        );
        save_rules(&mut host, Skill::Coordinating, "From the first machine.");
        let skill = Skill::Reviewing;
        host.execute(Command::StartSkillRewrite { skill }).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        (home, host)
    }

    #[test]
    fn joining_with_the_repositorys_settings_keeps_both_histories_and_backs_up_the_folder() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first, mut first) = set_up_elsewhere(&github);
        first
            .shared()
            .set_guide("Prefer Opus for migrations.\n")
            .unwrap();
        first.sync_now(&github.tools()).unwrap();
        let (home, mut host) = host();
        host.set_workspaces_dir(home.path().join("work").to_str().unwrap())
            .unwrap();
        save_rules(&mut host, Skill::Coordinating, "From this machine.");
        let own = host.skills().history(Skill::Coordinating);
        let selection = host.model_selection().unwrap();
        let check = check(&host.home, NAME, &github.tools()).unwrap();
        let joining = check.joining.unwrap();
        assert_eq!(joining.last_machine, machine_name());
        assert!(now() - joining.last_changed_at < 60_000);
        let implementing = joining
            .settings
            .iter()
            .find(|row| row.setting == "Implementing")
            .unwrap();
        assert_eq!(
            (&*implementing.repository, &*implementing.this_machine),
            ("3 of your rules", "stock")
        );
        let reviewing = joining
            .settings
            .iter()
            .find(|row| row.setting == "Reviewing")
            .unwrap();
        assert_eq!(
            (&*reviewing.repository, &*reviewing.this_machine),
            ("rewritten", "stock")
        );
        let guide = (joining.settings.iter())
            .find(|row| row.setting == "Model guide")
            .unwrap();
        let today = chrono::Local::now().format("Edited %-d %b").to_string();
        assert_eq!(
            (&*guide.repository, &*guide.this_machine),
            (&*today, "Starter guide")
        );
        let check_ins = joining
            .settings
            .iter()
            .find(|row| row.setting == "Check-ins")
            .unwrap();
        assert_eq!(check_ins.repository, "every 10 and 300 turns");
        // Using the repository's settings.
        let before = host.settings();
        let settings = host
            .choose_settings_repository(NAME, Some(JoinWith::Repository), &github.tools())
            .unwrap();
        assert!(
            host.instructions(Role::Implementer)
                .contains("- Keep commits small.")
        );
        assert_eq!(
            (settings.checkin_child_turns, settings.checkin_human_turns),
            (10, 300)
        );
        assert_eq!(settings.verification, first.settings().verification);
        assert_eq!(settings.keeper, first.settings().keeper);
        assert_eq!(
            host.model_selection().unwrap().guide,
            first.model_selection().unwrap().guide
        );
        assert_eq!(
            (&settings.allowed_owners, &settings.workspaces_dir),
            (&before.allowed_owners, &before.workspaces_dir)
        );
        assert!(settings.allowed_owners.is_empty());
        assert_eq!(
            ModelSelection {
                guide: String::new(),
                ..host.model_selection().unwrap()
            },
            ModelSelection {
                guide: String::new(),
                ..selection
            }
        );
        // Both machines' Coordinating entries, here and on GitHub; this machine's old folder in backups.
        let history = host.skills().history(Skill::Coordinating);
        assert_eq!(history.len(), 2);
        assert!(own.iter().all(|entry| history.contains(entry)));
        assert_eq!(
            github.log("%s")[0],
            format!("Skills: kept this machine's history · {}", machine_name())
        );
        let backup = home
            .path()
            .join("backups")
            .join(&listing(&home.path().join("backups"))[0]);
        assert_eq!(
            fs::read_to_string(backup.join("skills/coordinating/additions.md")).unwrap(),
            "From this machine."
        );
        assert_eq!(host.settings().settings_repository.unwrap().name, NAME);
    }

    #[test]
    fn replacing_the_repositorys_settings_pushes_this_machines_in_one_commit() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first, _) = set_up_elsewhere(&github);
        let (_home, mut host) = host();
        save_rules(&mut host, Skill::Reviewing, "Read every test.");
        let commits = github.log("%H").len();
        host.choose_settings_repository(NAME, Some(JoinWith::ThisMachine), &github.tools())
            .unwrap();
        assert_eq!(github.log("%H").len(), commits + 1);
        assert_eq!(
            github.show("skills/reviewing/additions.md").unwrap(),
            "Read every test."
        );
        assert_eq!(github.show("skills/implementing/additions.md"), None);
        assert_eq!(github.show("settings/check-ins.json"), None);
        assert_eq!(github.show("README.md").unwrap(), README);
        assert_eq!(host.settings().checkin_child_turns, 25);
    }

    #[test]
    fn after_joining_this_machine_lists_what_it_still_needs() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (first_home, first) = set_up_elsewhere(&github);
        // Triggers the first machine runs, with its keeper model and a re-check level.
        let keeper =
            r#"{"rebase": true, "conflicts": true, "quiet_minutes": 2, "review_level": "Light"}"#;
        let clone = first_home.path().join("shared");
        fs::write(clone.join("settings/pr-keeper.json"), keeper).unwrap();
        let mut review = first.settings().verification;
        review.verifiers.push(VerifierConfig {
            role: Role::Reviewer,
            focus: "Security".into(),
            instruction: None,
        });
        fs::write(
            clone.join("settings/review.json"),
            serde_json::to_string(&review).unwrap(),
        )
        .unwrap();
        git(&clone, &["add", "--all"]);
        git(&clone, &["commit", "-q", "-m", "Keeper · First"]);
        git(
            &clone,
            &["push", "-q", github.bare.to_str().unwrap(), "main"],
        );
        // A machine with a saved setup and no PR keeper model, which has no row for Security.
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        assert!(host.still_needs().unwrap().is_empty());
        host.choose_settings_repository(NAME, Some(JoinWith::Repository), &github.tools())
            .unwrap();
        let needs = host.snapshot().unwrap().still_needs;
        assert!(
            needs[0].text.contains("needs a PR keeper model"),
            "{needs:?}"
        );
        assert!(
            needs[1]
                .text
                .starts_with("The Security verifier has no row in Who runs what"),
            "{needs:?}"
        );
        assert_eq!(needs.len(), 2);
        assert!(needs.iter().all(|n| n.page == WorkspacePage::Models));
    }

    #[test]
    fn an_agent_cannot_choose_a_repository_or_reach_the_shared_folder() {
        let (home, mut host) = host();
        host.create_project("Site").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let shared = home.path().join("shared");
        let before = listing(&shared);
        for tool in mcp::tools().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let text = tool.to_string().to_lowercase();
            for word in [
                "shared",
                "settings repository",
                "settings_repository",
                "wiffletree.json",
            ] {
                assert!(!text.contains(word), "{name} names {word}");
            }
            let _ = host.agent_tool(
                &coordinator.id,
                name,
                json!({"repository": NAME, "with": "this_machine", "path": shared,
                    "settings_repository": {"name": NAME, "push_url": NAME, "branch": "main"}}),
            );
        }
        assert_eq!(listing(&shared), before);
        assert_eq!(host.settings().settings_repository, None);
    }

    /// A second machine that joined the first's repository with its settings.
    fn joined(github: &Arc<GitHub>) -> (tempfile::TempDir, Host) {
        let (home, mut host) = host();
        host.choose_settings_repository(NAME, Some(JoinWith::Repository), &github.tools())
            .unwrap();
        (home, host)
    }

    fn status(host: &Host) -> SyncStatus {
        host.sync_status().unwrap().unwrap()
    }

    #[test]
    fn the_record_holds_what_the_host_last_wrote_to_each_shared_file() {
        let (home, mut host) = host();
        host.shared_written.take();
        host.set_check_ins(40, 400).unwrap();
        save_rules(&mut host, Skill::Implementing, "Run the linter.");
        host.execute(Command::ResetSkill {
            skill: Skill::Implementing,
        })
        .unwrap();
        assert!(host.shared_written.take());
        let written: BTreeMap<String, Option<Vec<u8>>> = host
            .db
            .prepare("SELECT path,written FROM sync_record")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let shared = home.path().join("shared");
        let check_ins = &written["settings/check-ins.json"];
        assert_eq!(
            check_ins,
            &fs::read(shared.join("settings/check-ins.json")).ok()
        );
        assert_eq!(written["skills/implementing/additions.md"], None);
        let history: Vec<_> = written
            .iter()
            .filter(|(path, _)| path.starts_with("skills/implementing/history/"))
            .collect();
        assert_eq!(history.len(), 2);
        for (path, content) in history {
            assert_eq!(content, &fs::read(shared.join(path)).ok());
        }
    }

    #[test]
    fn different_settings_on_two_machines_reach_both_and_nothing_is_paused() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        let mut review = first.settings().verification;
        review.levels[0].use_when = "Every ticket.".into();
        first.set_verification(review.clone()).unwrap();
        second.set_check_ins(40, 400).unwrap();
        assert_eq!(status(&second).waiting.len(), 1);
        first.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        first.sync_now(&tools).unwrap();
        for host in [&first, &second] {
            let settings = host.settings();
            assert_eq!(settings.verification, review);
            assert_eq!(
                (settings.checkin_child_turns, settings.checkin_human_turns),
                (40, 400)
            );
            let status = status(host);
            assert_eq!((status.error, status.waiting.len()), (None, 0));
        }
        let machine = machine_name();
        assert_eq!(
            github.log("%s")[..2],
            [
                format!("Check-ins · {machine}"),
                format!("Review · {machine}")
            ]
        );
        let status = status(&first);
        assert_eq!(
            (
                status.repository.name.as_str(),
                status.repository.branch.as_str()
            ),
            (NAME, "main")
        );
        assert_eq!(status.repository.readers, Readers::Invited);
        assert!(now() - status.synced_at.unwrap() < 60_000);
        assert_eq!((status.pulled, status.pushed), (1, 0));
        assert_eq!(self::status(&second).pushed, 1);
    }

    #[test]
    fn other_files_in_the_repository_are_pulled_and_never_committed() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        github.commit("README.md", "Edited on GitHub.\n");
        github.commit("docs/notes.md", "New.\n");
        let shared = home.path().join("shared");
        fs::write(shared.join("README.md"), "Changed here.\n").unwrap();
        host.set_check_ins(40, 400).unwrap();
        host.sync_now(&github.tools()).unwrap();
        let changed = git(&github.bare, &["show", "--format=", "--name-only", "main"]);
        assert_eq!(changed.trim(), "settings/check-ins.json");
        assert_eq!(github.show("README.md").unwrap(), "Edited on GitHub.\n");
        for (file, content) in [
            ("README.md", "Edited on GitHub.\n"),
            ("docs/notes.md", "New.\n"),
        ] {
            assert_eq!(fs::read_to_string(shared.join(file)).unwrap(), content);
        }
        assert_eq!(status(&host).error, None);
    }

    fn notes(host: &Host) -> Vec<PausedSetting> {
        host.snapshot()
            .unwrap()
            .paused_settings()
            .cloned()
            .collect()
    }

    fn compare(host: &mut Host, path: &str) -> Vec<Line> {
        let path = path.into();
        let lines = host.execute(Command::ComparePausedFile { path }).unwrap();
        serde_json::from_value(lines).unwrap()
    }

    fn settle(host: &mut Host, path: &str, settle: Settle) {
        let path = path.into();
        host.execute(Command::SettlePausedFile { path, settle })
            .unwrap();
    }

    const ADDITIONS: &str = "skills/implementing/additions.md";

    #[test]
    fn both_machines_edit_implementings_rules_and_keeping_this_macs_pushes_it() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        save_rules(&mut first, Skill::Implementing, "From the first.");
        save_rules(&mut second, Skill::Implementing, "From the second.");
        second.set_check_ins(40, 400).unwrap();
        first.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        let rules = |host: &Host| host.skills().state(Skill::Implementing).rules;
        assert_eq!(rules(&second), "From the second.");
        assert_eq!(github.show(ADDITIONS).unwrap(), "From the first.");
        assert!(github.show(shared::CHECK_INS).unwrap().contains("400"));
        let machine = machine_name();
        assert_eq!(
            notes(&second),
            [PausedSetting {
                path: "skills/implementing/".into(),
                setting: "Implementing · your rules".into(),
                paused: Paused::TwoMachines {
                    machine: machine.clone()
                },
                text: format!(
                    "Implementing · your rules changed on this Mac and on {machine} since they last synced. This Mac keeps using its own rules. Other settings keep syncing."
                ),
            }]
        );
        assert_eq!(status(&second).waiting.len(), 0);
        assert_eq!(second.skills().history(Skill::Implementing).len(), 3);
        save_rules(&mut second, Skill::Implementing, "From the second, again.");
        second.sync_now(&tools).unwrap();
        assert_eq!(github.show(ADDITIONS).unwrap(), "From the first.");
        assert_eq!(notes(&second).len(), 1);
        assert_eq!(
            compare(&mut second, "skills/implementing/"),
            [
                Line::Removed("From the second, again.".into()),
                Line::Added("From the first.".into())
            ]
        );
        first.sync_now(&tools).unwrap();
        assert_eq!(first.settings().checkin_human_turns, 400);
        assert!(notes(&first).is_empty());
        settle(
            &mut second,
            "skills/implementing/",
            Settle::KeepThisMachines,
        );
        assert!(notes(&second).is_empty());
        second.sync_now(&tools).unwrap();
        assert_eq!(github.show(ADDITIONS).unwrap(), "From the second, again.");
        first.sync_now(&tools).unwrap();
        assert_eq!(rules(&first), "From the second, again.");
        let earlier = git(
            &github.bare,
            &["log", "--format=%H", "main", "--", ADDITIONS],
        );
        let earlier = earlier.lines().nth(1).unwrap();
        let kept = git(&github.bare, &["show", &format!("{earlier}:{ADDITIONS}")]);
        assert_eq!(kept, "From the first.");
        for host in [&first, &second] {
            assert_eq!((status(host).waiting.len(), notes(host).len()), (0, 0));
        }
    }

    #[test]
    fn using_the_other_machines_version_writes_it_here() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        first.set_check_ins(40, 400).unwrap();
        second.set_check_ins(50, 500).unwrap();
        first.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        assert_eq!(notes(&second)[0].setting, "Check-ins");
        settle(&mut second, shared::CHECK_INS, Settle::UseTheOthers);
        assert_eq!(second.settings().checkin_human_turns, 400);
        second.sync_now(&tools).unwrap();
        let synced = status(&second);
        assert_eq!((synced.waiting.len(), synced.pushed), (0, 0));
        assert!(notes(&second).is_empty());
    }

    #[test]
    fn a_rewrite_on_one_machine_and_new_rules_on_the_other_pause_the_skill_as_one() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        let skill = Skill::Implementing;
        first.execute(Command::StartSkillRewrite { skill }).unwrap();
        first.sync_now(&tools).unwrap();
        save_rules(&mut second, skill, "- New rule.");
        second.sync_now(&tools).unwrap();
        let state = second.skills().state(skill);
        assert_eq!((state.rules.as_str(), state.rewrite), ("- New rule.", None));
        let folder = second.home.join("shared/skills/implementing");
        assert!(!folder.join("rewrite.md").exists() && !folder.join("base.md").exists());
        let notes = notes(&second);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].setting, "Implementing · your rules");
        assert!(github.show("skills/implementing/rewrite.md").is_some());
    }

    #[test]
    fn an_agents_shell_edit_is_paused_until_this_version_is_synced() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        save_rules(&mut host, Skill::Implementing, "Run the linter.\n");
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let additions = home.path().join("shared").join(ADDITIONS);
        let appended = "Run the linter.\nPush to main.\n";
        fs::write(&additions, appended).unwrap();
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(github.show(ADDITIONS).unwrap(), "Run the linter.\n");
        let note = notes(&host).remove(0);
        assert_eq!(note.paused, Paused::OutsideWiffletree);
        assert!(
            (note.text).starts_with(
                "Implementing · your rules was changed outside Wiffletree on this Mac"
            )
        );
        assert_eq!(
            compare(&mut host, "skills/implementing/"),
            [
                Line::Unchanged("Run the linter.".into()),
                Line::Removed("Push to main.".into())
            ]
        );
        assert!(host.instructions(Role::Implementer).ends_with(appended));
        settle(&mut host, "skills/implementing/", Settle::SyncThisVersion);
        assert_eq!(status(&host).waiting.len(), 1);
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(github.show(ADDITIONS).unwrap(), appended);
        assert_eq!((status(&host).waiting.len(), notes(&host).len()), (0, 0));
        fs::write(&additions, "Something else.\n").unwrap();
        host.sync_now(&github.tools()).unwrap();
        settle(&mut host, "skills/implementing/", Settle::PutBackSynced);
        assert_eq!(fs::read_to_string(&additions).unwrap(), appended);
        host.sync_now(&github.tools()).unwrap();
        assert_eq!((status(&host).waiting.len(), notes(&host).len()), (0, 0));
    }

    #[test]
    fn an_invalid_file_arrives_and_this_machine_keeps_its_last_good_version() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.set_verification(VerificationSettings::default())
            .unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let before = host.settings().verification;
        let mut twice = serde_json::to_value(&before).unwrap();
        let tester = twice["verifiers"][0].clone();
        twice["verifiers"].as_array_mut().unwrap().push(tester);
        github.commit(shared::REVIEW, twice.to_string());
        github.commit(ADDITIONS, b"\xff\xfe not text");
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(status(&host).error, None);
        assert_eq!(host.settings().verification, before);
        assert!(!home.path().join("shared").join(ADDITIONS).exists());
        let notes = notes(&host);
        let reasons: Vec<&Paused> = notes.iter().map(|n| &n.paused).collect();
        let machine = "F".to_owned();
        let reason = format!(
            "Two verifiers share the focus \"{}\"",
            before.verifiers[0].focus
        );
        assert_eq!(
            reasons,
            [
                &Paused::CannotBeRead {
                    machine: machine.clone(),
                    reason
                },
                &Paused::CannotBeRead {
                    machine,
                    reason: "it is not UTF-8 text".into()
                },
            ]
        );
        assert!(
            notes[0]
                .text
                .starts_with("Review from F can’t be used here")
        );
        settle(&mut host, shared::REVIEW, Settle::KeepThisMachines);
        host.sync_now(&github.tools()).unwrap();
        let pushed: VerificationSettings =
            serde_json::from_str(&github.show(shared::REVIEW).unwrap()).unwrap();
        assert_eq!(pushed, before);
        github.commit(ADDITIONS, "Readable.");
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(host.skills().state(Skill::Implementing).rules, "Readable.");
        assert!(self::notes(&host).is_empty());
    }

    #[test]
    fn a_newer_format_stops_sync_and_nothing_is_pulled_or_pushed() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let guide = host.shared().guide();
        github.commit(shared::GUIDE, "From a newer release.");
        github.commit(shared::FORMAT, "{\"format\": 2}\n");
        host.set_check_ins(40, 400).unwrap();
        let commits = github.log("%H");
        host.sync_now(&github.tools()).unwrap();
        let error = status(&host).error.unwrap();
        assert!(
            error.contains("written by a newer Wiffletree. Update"),
            "{error}"
        );
        assert_eq!(github.log("%H"), commits);
        assert_eq!(host.shared().guide(), guide);
        assert_eq!(status(&host).waiting.len(), 1);
        assert!(status(&host).stopped);
    }

    #[test]
    fn a_format_file_on_github_that_cannot_be_read_stops_sync_saying_so() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        for (content, why) in [("not json", "expected"), ("{}", "it names no format")] {
            github.commit(shared::FORMAT, content);
            let commits = github.log("%H");
            host.sync_now(&github.tools()).unwrap();
            let stopped = status(&host);
            let error = stopped.error.unwrap();
            assert!(
                error.starts_with("The repository's wiffletree.json can't be read: ")
                    && error.contains(why)
                    && !error.contains("newer"),
                "{error}"
            );
            assert!(stopped.stopped);
            assert_eq!(github.log("%H"), commits);
        }
        github.commit(shared::FORMAT, "{\"format\": 1}\n");
        host.sync_now(&github.tools()).unwrap();
        let synced = status(&host);
        assert_eq!((synced.error, synced.stopped), (None, false));
    }

    #[test]
    fn stopping_sync_keeps_every_setting_closes_the_notes_and_changes_nothing_on_github() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.set_check_ins(40, 400).unwrap();
        save_rules(&mut host, Skill::Implementing, "Run the linter.\n");
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let additions = home.path().join("shared").join(ADDITIONS);
        fs::write(&additions, "Push to main.\n").unwrap();
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(notes(&host).len(), 1);
        let commits = github.log("%H");
        let settings = host.execute(Command::StopSettingsSync).unwrap();
        let settings: HostSettings = serde_json::from_value(settings).unwrap();
        assert_eq!(settings.settings_repository, None);
        assert_eq!(
            (settings.checkin_child_turns, settings.checkin_human_turns),
            (40, 400)
        );
        let snapshot = host.snapshot().unwrap();
        assert!(snapshot.workspace_notes.is_empty() && snapshot.settings_sync.is_none());
        assert_eq!(fs::read_to_string(&additions).unwrap(), "Push to main.\n");
        host.set_check_ins(50, 500).unwrap();
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(github.log("%H"), commits);
        assert_eq!(github.pushes(), 1);
        assert_eq!(host.settings().checkin_child_turns, 50);
    }

    #[test]
    fn a_newer_stock_version_pulled_from_another_machine_is_shown_without_a_restart() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, _first) = set_up_elsewhere(&github);
        let (_home, mut second) = joined(&github);
        assert!(
            second
                .skill_states()
                .unwrap()
                .iter()
                .all(|s| s.newer_stock_version.is_none())
        );
        let (name, _) = second.skills().history(Skill::Coordinating).remove(0);
        let history = "skills/coordinating/history";
        let entry = github.show(&format!("{history}/{name}.json")).unwrap();
        let entry = entry.replace("\"v4\"", "\"v5\"");
        github.commit(&format!("{history}/2099-01-01T00-00-00.000Z.json"), entry);
        second.sync_now(&github.tools()).unwrap();
        let states = second.skill_states().unwrap();
        assert_eq!(states[0].newer_stock_version.as_deref(), Some("v5"));
    }

    #[test]
    fn a_save_made_during_a_pass_waits_for_the_next() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        first.set_check_ins(30, 300).unwrap();
        let pass = first.sync_pass().unwrap().unwrap();
        first.set_check_ins(40, 400).unwrap();
        let commits = github.log("%H");
        let passed = self::pass(pass, &tools);
        first.finish_sync(now(), passed).unwrap();
        assert_eq!(github.log("%H"), commits);
        assert_eq!((status(&first).waiting.len(), notes(&first).len()), (1, 0));
        first.sync_now(&tools).unwrap();
        assert!(github.show(shared::CHECK_INS).unwrap().contains("400"));
        assert_eq!(status(&first).waiting.len(), 0);
        // A later change made only on the other machine is pulled, not taken for a conflict.
        second.sync_now(&tools).unwrap();
        second.set_check_ins(50, 500).unwrap();
        second.sync_now(&tools).unwrap();
        first.sync_now(&tools).unwrap();
        assert_eq!(first.settings().checkin_human_turns, 500);
        assert_eq!(status(&first).pulled, 1);
    }

    #[test]
    fn a_skill_caught_mid_change_by_a_pass_is_left_for_the_next() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        save_rules(&mut host, Skill::Implementing, "Rule one.");
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        save_rules(&mut host, Skill::Implementing, "Rule two.");
        // Mid-pass, a change has moved the rules aside and not yet put its new file in place.
        let additions = home.path().join("shared").join(ADDITIONS);
        let aside = home.path().join("aside.md");
        let changing = Arc::new(Mutex::new(None));
        let (writes, slot) = (host.shared_writes.clone(), changing.clone());
        let (from, to) = (additions.clone(), aside.clone());
        *github.before.lock().unwrap() = Some((
            "fetch",
            Box::new(move || {
                let writing = shared::Writing::start(&writes, "skills/implementing/");
                fs::rename(from, to).unwrap();
                *slot.lock().unwrap() = Some(writing);
            }),
        ));
        let pass = host.sync_pass().unwrap().unwrap();
        let passed = self::pass(pass, &github.tools());
        fs::rename(&aside, &additions).unwrap();
        changing.lock().unwrap().take();
        assert!(host.finish_sync(now(), passed).unwrap());
        assert_eq!(github.show(ADDITIONS).unwrap(), "Rule one.");
        assert!(notes(&host).is_empty());
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(github.show(ADDITIONS).unwrap(), "Rule two.");
        assert_eq!((status(&host).waiting.len(), notes(&host).len()), (0, 0));
    }

    #[test]
    fn a_pass_that_started_before_a_clone_was_adopted_is_dropped_unrecorded() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, _first) = set_up_elsewhere(&github);
        let (_home, mut host) = joined(&github);
        host.set_check_ins(40, 400).unwrap();
        let pass = host.sync_pass().unwrap().unwrap();
        let commits = github.log("%H");
        host.choose_settings_repository(NAME, Some(JoinWith::ThisMachine), &github.tools())
            .unwrap();
        let rejoined = github.log("%H");
        assert_eq!(rejoined.len(), commits.len() + 1);
        // GitHub moves on, removing a file the adopted clone holds.
        let work = tempfile::tempdir().unwrap();
        git(
            work.path(),
            &["clone", "-q", github.bare.to_str().unwrap(), "."],
        );
        git(work.path(), &["rm", "-q", "README.md"]);
        git(work.path(), &["commit", "-q", "-m", "No README"]);
        git(work.path(), &["push", "-q", "origin", "main"]);
        let passed = self::pass(pass, &github.tools());
        assert!(!host.finish_sync(now(), passed).unwrap());
        assert_eq!(github.log("%H")[1..], rejoined);
        let clone = host.home.join("shared");
        assert!(clone.join("README.md").exists());
        assert_eq!(git(&clone, &["rev-parse", "HEAD"]), rejoined[0]);
        let status = status(&host);
        assert_eq!((status.synced_at, status.waiting.len()), (None, 0));
    }

    #[test]
    fn a_pass_that_started_before_sync_stopped_pushes_nothing() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        host.set_check_ins(40, 400).unwrap();
        let pass = host.sync_pass().unwrap().unwrap();
        let commits = github.log("%H");
        host.execute(Command::StopSettingsSync).unwrap();
        let passed = self::pass(pass, &github.tools());
        assert!(!host.finish_sync(now(), passed).unwrap());
        assert_eq!(github.log("%H"), commits);
        assert_eq!(github.pushes(), 1);
    }

    #[test]
    fn a_trigger_pulled_without_a_keeper_model_here_is_held_and_a_first_setup_saves() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, _first) = set_up_elsewhere(&github);
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        host.choose_settings_repository(NAME, Some(JoinWith::Repository), &github.tools())
            .unwrap();
        let keeper = r#"{"checks": true, "rebase": true, "review_level": "Light"}"#;
        github.commit(shared::KEEPER, keeper);
        host.sync_now(&github.tools()).unwrap();
        assert!(host.settings().keeper.rebase && !host.settings().keeper.checks);
        let held = host.snapshot().unwrap().keeper_held;
        assert!(
            held[0].contains(&format!("\"{KEEPER_TRIES}\" needs")),
            "{held:?}"
        );
        let file = home.path().join("shared").join(shared::KEEPER);
        assert_eq!(fs::read_to_string(&file).unwrap(), keeper);
        let suggested = Host::open_set_up(folder.path().join("suggested")).unwrap();
        let selection = ModelSelection {
            guide: host.model_selection().unwrap().guide,
            ..suggested.model_selection().unwrap()
        };
        assert_eq!(selection.keeper(), None);
        let review = host.settings().verification;
        host.set_setup(&selection, Some(review)).unwrap();
        assert!(host.model_selection().unwrap().is_saved());
        assert_eq!(fs::read_to_string(&file).unwrap(), keeper);
    }

    #[test]
    fn syncing_an_outside_edit_pushes_it_over_a_remote_change() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.set_check_ins(30, 300).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let shared = home.path().join("shared");
        let outside = "{\n  \"checkin_child_turns\": 40,\n  \"checkin_human_turns\": 400\n}\n";
        fs::write(shared.join(shared::CHECK_INS), outside).unwrap();
        github.commit(shared::CHECK_INS, outside.replace('4', "5"));
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(notes(&host)[0].paused, Paused::OutsideWiffletree);
        settle(&mut host, shared::CHECK_INS, Settle::SyncThisVersion);
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(github.show(shared::CHECK_INS).unwrap(), outside);
        assert_eq!((status(&host).waiting.len(), notes(&host).len()), (0, 0));
    }

    #[test]
    fn a_format_file_this_release_would_refuse_is_never_synced() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        fs::write(home.path().join("shared").join(shared::FORMAT), "not json").unwrap();
        host.sync_now(&github.tools()).unwrap();
        let settle = Command::SettlePausedFile {
            path: shared::FORMAT.into(),
            settle: Settle::SyncThisVersion,
        };
        let refused = format!("{:#}", host.execute(settle).unwrap_err());
        assert!(
            refused.contains("wiffletree.json can't be synced"),
            "{refused}"
        );
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(
            github.show(shared::FORMAT).unwrap(),
            "{\n  \"format\": 1\n}\n"
        );
        assert_eq!((status(&host).error, notes(&host).len()), (None, 1));
    }

    #[test]
    fn a_file_paused_on_two_machines_stays_paused_when_the_remote_reverts() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.set_check_ins(30, 300).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let synced = github.show(shared::CHECK_INS).unwrap();
        host.set_check_ins(40, 400).unwrap();
        github.commit(shared::CHECK_INS, synced.replace('3', "5"));
        host.sync_now(&github.tools()).unwrap();
        assert!(matches!(notes(&host)[0].paused, Paused::TwoMachines { .. }));
        github.commit(shared::CHECK_INS, &synced);
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(github.show(shared::CHECK_INS).unwrap(), synced);
        assert!(matches!(notes(&host)[0].paused, Paused::TwoMachines { .. }));
        assert_eq!(host.settings().checkin_human_turns, 400);
    }

    #[test]
    fn joining_another_repository_is_refused_while_a_pass_runs_and_leaves_the_first_alone() {
        let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let github = Arc::new(GitHub::new(one.path(), true));
        let other = Arc::new(GitHub::new(two.path(), true));
        let (_other_home, _other_host) = set_up_elsewhere(&other);
        let (home, mut host) = host();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        host.set_check_ins(40, 400).unwrap();
        let before = github.log("%H");
        let (at_push, reached) = std::sync::mpsc::channel();
        let (go, gate) = std::sync::mpsc::channel::<()>();
        *github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || {
                at_push.send(()).unwrap();
                gate.recv().unwrap();
            }),
        ));
        let pass = host.sync_pass().unwrap().unwrap();
        let tools = github.tools();
        let running = std::thread::spawn(move || self::pass(pass, &tools));
        reached.recv().unwrap();
        let data = host.home.clone();
        let join = || prepare(&data, NAME, Some(JoinWith::Repository), &other.tools());
        let refused = host.adopt_settings_repository(join().unwrap());
        assert_eq!(
            format!("{:#}", refused.unwrap_err()),
            "Sync is running. Try again in a moment."
        );
        go.send(()).unwrap();
        let passed = running.join().unwrap();
        assert_eq!(passed.as_ref().map(|p| p.error.clone()).unwrap(), None);
        let pushed = github.log("%H");
        assert_eq!(pushed[1..], before);
        let changed = git(&github.bare, &["show", "--format=", "--name-only", "main"]);
        assert_eq!(changed, shared::CHECK_INS);
        assert!(host.finish_sync(now(), passed).unwrap());
        let joined = host.adopt_settings_repository(join().unwrap()).unwrap();
        assert!(joined.settings_repository.is_some());
        let shared = home.path().join("shared");
        assert_eq!(git(&shared, &["rev-parse", "HEAD"]), other.log("%H")[0]);
        assert_eq!(github.log("%H"), pushed);
    }

    #[test]
    fn an_adoption_refused_while_a_pass_runs_changes_nothing() {
        let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let github = Arc::new(GitHub::new(one.path(), true));
        let other = Arc::new(GitHub::new(two.path(), true));
        let (_other_home, _other_host) = set_up_elsewhere(&other);
        let (home, mut host) = host();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let shared = home.path().join("shared");
        let before = (listing(&shared), git(&shared, &["rev-parse", "HEAD"]));
        let (settings, record) = (host.settings(), status(&host));
        let joining = prepare(&host.home, NAME, Some(JoinWith::Repository), &other.tools());
        let passes = host.sync_passes.clone();
        let running = passes.lock().unwrap();
        assert!(host.adopt_settings_repository(joining.unwrap()).is_err());
        drop(running);
        assert_eq!(
            (listing(&shared), git(&shared, &["rev-parse", "HEAD"])),
            before
        );
        assert_eq!((host.settings(), status(&host)), (settings, record));
        let left = listing(home.path());
        assert!(
            !left.iter().any(|name| name.starts_with("shared-staging-")),
            "{left:?}"
        );
        assert_eq!(listing(&home.path().join("backups")).len(), 1);
    }

    #[test]
    fn a_skills_rules_rewrite_and_base_are_pulled_together_or_not_at_all() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        let skill = Skill::Implementing;
        first.execute(Command::StartSkillRewrite { skill }).unwrap();
        first.sync_now(&tools).unwrap();
        let pass = second.sync_pass().unwrap().unwrap();
        let passed = self::pass(pass, &tools);
        let additions = second.home.join("shared").join(ADDITIONS);
        let edited = "Edited while the pass ran.\n";
        fs::write(&additions, edited).unwrap();
        second.finish_sync(now(), passed).unwrap();
        let folder = second.home.join("shared/skills/implementing");
        assert!(!folder.join("rewrite.md").exists() && !folder.join("base.md").exists());
        assert_eq!(second.skills().state(skill).rules, edited);
    }

    #[test]
    fn joining_another_repository_closes_the_notes_about_the_old_one() {
        let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let github = Arc::new(GitHub::new(one.path(), true));
        let other = Arc::new(GitHub::new(two.path(), true));
        let (_other_home, _other_host) = set_up_elsewhere(&other);
        let (_home, mut host) = host();
        host.set_check_ins(30, 300).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let synced = github.show(shared::CHECK_INS).unwrap();
        host.set_check_ins(40, 400).unwrap();
        github.commit(shared::CHECK_INS, synced.replace('3', "5"));
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(notes(&host).len(), 1);
        host.choose_settings_repository(NAME, Some(JoinWith::Repository), &other.tools())
            .unwrap();
        assert!(notes(&host).is_empty());
        let settle = Command::SettlePausedFile {
            path: shared::CHECK_INS.into(),
            settle: Settle::UseTheOthers,
        };
        let refused = format!("{:#}", host.execute(settle).unwrap_err());
        assert_eq!(refused, "That setting is not paused");
        assert_eq!(host.settings().checkin_human_turns, 300);
    }

    #[test]
    fn a_rule_saved_twice_within_five_seconds_is_one_commit_the_other_machine_pulls() {
        let mut pace = Pace::default();
        assert!(pace.start(0));
        pace.finish(0, true);
        pace.wrote(1_000);
        pace.wrote(4_000);
        assert!(!pace.start(8_999));
        assert!(pace.start(9_000));
        pace.finish(9_000, true);
        assert_eq!(pace.next_due(), Some(9_000 + PULL_MS));
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        let commits = github.log("%H").len();
        save_rules(&mut first, Skill::Implementing, "- Run the linter.");
        save_rules(
            &mut first,
            Skill::Implementing,
            "- Run the linter.\n- Name the ticket.",
        );
        first.sync_now(&tools).unwrap();
        assert_eq!(github.log("%H").len(), commits + 1);
        assert_eq!(
            github.log("%s")[0],
            format!("Implementing · {}", machine_name())
        );
        second.sync_now(&tools).unwrap();
        assert!(
            second
                .instructions(Role::Implementer)
                .contains("- Name the ticket.")
        );
        assert_eq!(second.skills().history(Skill::Implementing).len(), 3);
    }

    #[test]
    fn offline_a_change_applies_here_waits_and_is_pushed_at_the_first_pass_that_reaches_github() {
        let (_home, mut host) = host();
        let github = Arc::new(GitHub::new(_home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        github.offline.store(true, Ordering::Relaxed);
        host.set_check_ins(40, 400).unwrap();
        assert_eq!(host.settings().checkin_child_turns, 40);
        host.sync_now(&github.tools()).unwrap();
        let offline = status(&host);
        assert_eq!(offline.waiting, ["Check-ins"]);
        let error = offline.error.unwrap();
        assert!(
            error.contains("Could not resolve host: github.com"),
            "{error}"
        );
        assert_eq!(github.show("settings/check-ins.json"), None);
        let failing_since = offline.failing_since.unwrap();
        assert_eq!(offline.failed_at, Some(failing_since));
        std::thread::sleep(std::time::Duration::from_millis(2));
        host.sync_now(&github.tools()).unwrap();
        let again = status(&host);
        assert_eq!(again.failing_since, Some(failing_since));
        assert!(again.failed_at > Some(failing_since) && !again.stopped);
        // Passes back off to thirty minutes; a save waits for the next try, Sync now does not.
        let mut pace = Pace::default();
        let mut at = 0;
        for minutes in [10, 20, 30, 30] {
            assert!(pace.start(at));
            pace.finish(at, false);
            assert_eq!(pace.next_due(), Some(at + minutes * 60_000));
            at += minutes * 60_000;
        }
        pace.wrote(at - 60_000);
        assert!(!pace.start(at - 60_000 + PUSH_DELAY_MS));
        pace.sync_now();
        assert!(pace.start(at - 60_000 + PUSH_DELAY_MS));
        github.offline.store(false, Ordering::Relaxed);
        host.sync_now(&github.tools()).unwrap();
        let online = status(&host);
        assert_eq!(
            (online.error, online.waiting.len(), online.pushed),
            (None, 0, 1)
        );
        assert_eq!((online.failed_at, online.failing_since), (None, None));
        assert!(
            github
                .show("settings/check-ins.json")
                .unwrap()
                .contains("400")
        );
    }

    #[test]
    fn when_the_remote_moved_first_nothing_is_overwritten_and_the_next_pass_pulls_before_pushing() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        host.sync_now(&github.tools()).unwrap();
        let reached = status(&host).synced_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        host.set_check_ins(40, 400).unwrap();
        let bare = github.bare.clone();
        *github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || commit_on(&bare, "skills/reviewing/additions.md", "Read every test.")),
        ));
        host.sync_now(&github.tools()).unwrap();
        let refused = status(&host);
        assert!(refused.error.unwrap().contains("git push failed"));
        assert_eq!(refused.waiting.len(), 1);
        assert_eq!(refused.synced_at, reached, "a refused push is not a sync");
        assert_eq!(github.log("%s")[0], "skills/reviewing/additions.md");
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(
            host.skills().state(Skill::Reviewing).rules,
            "Read every test."
        );
        assert_eq!(
            github.log("%s")[..2],
            [
                format!("Check-ins · {}", machine_name()),
                "skills/reviewing/additions.md".into()
            ]
        );
        let synced = status(&host);
        assert!(synced.synced_at > reached);
        assert_eq!((synced.error, synced.waiting.len()), (None, 0));
    }

    #[test]
    fn a_push_takes_a_non_ascii_path_inside_the_shared_paths_as_shared() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        let repository = host
            .choose_settings_repository(NAME, None, &github.tools())
            .unwrap()
            .settings_repository
            .unwrap();
        let clone = home.path().join("shared");
        fs::create_dir_all(clone.join("skills/implementing")).unwrap();
        fs::write(clone.join("skills/implementing/notes-é.md"), "Noted.\n").unwrap();
        git(&clone, &["add", "--all"]);
        git(&clone, &["commit", "-q", "-m", "Notes"]);
        push(&clone, &repository, &github.tools().git).unwrap();
        assert_eq!(
            github.show("skills/implementing/notes-é.md").unwrap(),
            "Noted.\n"
        );
    }

    #[test]
    fn a_staging_clone_left_by_a_stopped_host_is_removed_at_start() {
        let home = tempfile::tempdir().unwrap();
        drop(Host::open(home.path()).unwrap());
        let staging = home.path().join("shared-staging-AbC123");
        fs::create_dir_all(staging.join(".git")).unwrap();
        fs::write(staging.join("README.md"), README).unwrap();
        Host::open(home.path()).unwrap();
        assert!(!staging.exists());
        assert!(home.path().join("shared/wiffletree.json").exists());
    }

    #[test]
    fn a_repository_chosen_before_the_record_existed_syncs_from_its_committed_head() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        host.set_check_ins(25, 250).unwrap();
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        // As slice 2 left it: no record, and a change made since in the clone.
        host.db.execute("DELETE FROM sync_record", []).unwrap();
        SharedStore::of(home.path()).set_check_ins(40, 400).unwrap();
        host.sync_now(&github.tools()).unwrap();
        assert!(
            github
                .show("settings/check-ins.json")
                .unwrap()
                .contains("400")
        );
        let synced = status(&host);
        assert_eq!(
            (synced.pushed, synced.waiting.len(), synced.error),
            (1, 0, None)
        );
        github.commit(
            "settings/check-ins.json",
            "{\n  \"checkin_child_turns\": 50,\n  \"checkin_human_turns\": 500\n}\n",
        );
        host.sync_now(&github.tools()).unwrap();
        assert_eq!(host.settings().checkin_human_turns, 500);
    }

    #[test]
    fn a_rewrite_saved_through_the_host_is_pushed_by_a_pass() {
        let (home, mut host) = host();
        let github = Arc::new(GitHub::new(home.path(), true));
        save_rules(&mut host, Skill::Implementing, "Run the linter.");
        host.choose_settings_repository(NAME, None, &github.tools())
            .unwrap();
        let skill = Skill::Implementing;
        host.execute(Command::StartSkillRewrite { skill }).unwrap();
        let text = "My own way of implementing.".to_owned();
        host.execute(Command::SaveSkillRewrite { skill, text })
            .unwrap();
        assert_eq!(status(&host).waiting, ["Implementing"]);
        host.sync_now(&github.tools()).unwrap();
        let folder = "skills/implementing";
        assert_eq!(
            github.show(&format!("{folder}/rewrite.md")).unwrap(),
            "My own way of implementing."
        );
        assert_eq!(
            github.show(&format!("{folder}/base.md")).unwrap(),
            skills::stock(skill)
        );
        assert_eq!(github.show(&format!("{folder}/additions.md")), None);
        for (name, _) in host.skills().history(skill) {
            assert!(
                github
                    .show(&format!("{folder}/history/{name}.json"))
                    .is_some()
            );
        }
        assert_eq!(
            github.log("%s")[0],
            format!("Implementing · {}", machine_name())
        );
        let synced = status(&host);
        assert_eq!((synced.waiting.len(), synced.error), (0, None));
    }

    const COORDINATING: &str = "skills/coordinating";

    fn coordinating(host: &Host) -> skills::Rewritten {
        let state = host.skills().state(Skill::Coordinating).rewrite.unwrap();
        skills::Rewritten {
            text: state.text,
            base: state.base,
        }
    }

    /// The "Merged" entries in Coordinating's history, by name.
    fn merges(host: &Host) -> Vec<String> {
        (host.skills().history(Skill::Coordinating).into_iter())
            .filter(|(_, entry)| matches!(entry.change, Change::Merged { .. }))
            .map(|(name, _)| name)
            .collect()
    }

    /// Two Macs whose synced Coordinating is a rewrite of a stand-in v0, each with its data
    /// folder closed, to be started on this release. Answers what this release's plain merge
    /// makes of it.
    fn rewritten_on_two_macs(
        github: &Arc<GitHub>,
    ) -> (tempfile::TempDir, tempfile::TempDir, String) {
        let (first_home, first) = host();
        let stock = skills::stock(Skill::Coordinating);
        let base = (stock.replacen("— v4", "— v0", 1)).replacen(
            "Ask the human instead when a waiver",
            "Ask the human when",
            1,
        );
        let text = base.replacen("Start by clarifying", "Start by restating", 1);
        first.skills().start_rewrite(Skill::Coordinating).unwrap();
        (first.skills().save_rewrite(Skill::Coordinating, &text)).unwrap();
        let folder = first_home.path().join("shared").join(COORDINATING);
        fs::write(folder.join("base.md"), base).unwrap();
        let mut first = first;
        (first.choose_settings_repository(NAME, None, &github.tools())).unwrap();
        let (second_home, _second) = joined(github);
        let merged = stock.replacen("Start by clarifying", "Start by restating", 1);
        (first_home, second_home, merged)
    }

    /// Pushes Coordinating's rewrite `text` on `base`, as `machine` would after merging `from`
    /// into it, with its "Merged" entry.
    fn push_merge(github: &GitHub, from: &str, base: &str, text: &str, machine: &str) {
        let rewrite = skills::Rewritten {
            text: text.into(),
            base: base.into(),
        };
        let entry = skills::Entry {
            change: Change::Merged {
                from: from.into(),
                to: skills::version(base).into(),
                before: rewrite.clone(),
                changes: vec![],
                model: None,
                machine: machine.into(),
                automatic: true,
            },
            stock_version: skills::version(base).into(),
            edits: skills::Edits {
                rules: String::new(),
                rewrite: Some(rewrite),
            },
        };
        github.commit(&format!("{COORDINATING}/base.md"), base);
        github.commit(&format!("{COORDINATING}/rewrite.md"), text);
        let to = skills::version(base).trim_start_matches('v');
        let history = format!("{COORDINATING}/history/2090-01-01T00-00-0{to}.000Z.json");
        github.commit(&history, serde_json::to_vec(&entry).unwrap());
    }

    #[test]
    fn one_mac_merges_first_and_the_other_takes_that_merge_at_its_first_pass() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (first_home, second_home, merged) = rewritten_on_two_macs(&github);
        let stock = skills::stock(Skill::Coordinating);

        let mut first = Host::open(first_home.path()).unwrap();
        let waiting = first.skill_state(Skill::Coordinating).unwrap();
        assert_eq!(waiting.merge_standing, Some(MergeStanding::WaitingForSync));
        assert!(merges(&first).is_empty());
        first.sync_now(&tools).unwrap();
        assert_eq!(coordinating(&first).text, merged);
        assert_eq!(coordinating(&first).base, stock);
        first.sync_now(&tools).unwrap();
        assert_eq!(
            github.log("%s")[0],
            format!(
                "Coordinating: merged v4 into your rewrite · {}",
                machine_name()
            )
        );

        let mut second = Host::open(second_home.path()).unwrap();
        let waiting = second.skill_state(Skill::Coordinating).unwrap();
        assert_eq!(waiting.merge_standing, Some(MergeStanding::WaitingForSync));
        let pushes = github.pushes();
        second.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        assert_eq!(github.pushes(), pushes);
        assert_eq!(coordinating(&second).text, merged);
        assert_eq!(merges(&second), merges(&first));
        assert_eq!(merges(&second).len(), 1);
        for host in [&first, &second] {
            let state = host.skill_state(Skill::Coordinating).unwrap();
            assert_eq!(state.merge_note.unwrap().entry, merges(&first)[0]);
            assert_eq!(state.merge_standing, None);
        }
    }

    #[test]
    fn a_question_asked_on_one_mac_is_shown_on_both_and_answered_once_for_both() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (first_home, second_home, _) = rewritten_on_two_macs(&github);
        let skill = Skill::Coordinating;
        let with_a_claude_keeper = |home: &tempfile::TempDir| {
            let mut host = Host::open(home.path()).unwrap();
            let mut selection = host.model_selection().unwrap();
            selection.set_keeper(Some(crate::keeper::tests::keeper_line(Provider::Claude)));
            host.set_model_selection(&selection).unwrap();
            host
        };
        let mut first = with_a_claude_keeper(&first_home);
        let mut second = with_a_claude_keeper(&second_home);
        let text = coordinating(&first)
            .text
            .replacen("Ask the human when", "Ask me when", 1);
        first.skills().save_rewrite(skill, &text).unwrap();
        first.sync_now(&tools).unwrap();
        let mut runs = first.take_merge_runs();
        assert_eq!(runs.len(), 1);
        let answer =
            json!({"passages": [{"id": "1", "question": "v1 asks the human; yours asks you."}]});
        first.finish_merge_run(
            runs.remove(0),
            Ok(provider::MergeAnswer { answer, tokens: 1 }),
        );
        first.sync_now(&tools).unwrap();

        second.sync_now(&tools).unwrap();
        assert!(second.take_merge_runs().is_empty());
        assert_eq!(coordinating(&second).text, text);
        for host in [&first, &second] {
            let notes = host.snapshot().unwrap().workspace_notes;
            assert!(
                matches!(notes[..], [WorkspaceNote::SkillQuestion(_)]),
                "{notes:?}"
            );
        }

        let keep = Command::AnswerSkillQuestion {
            skill,
            wordings: vec![Wording::Yours],
        };
        first.execute(keep).unwrap();
        first.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        let merged = text.replacen("— v0", "— v4", 1);
        for host in [&first, &second] {
            assert_eq!(coordinating(host).text, merged);
            assert_eq!(coordinating(host).base, skills::stock(skill));
            assert_eq!(host.snapshot().unwrap().workspace_notes, []);
            assert_eq!(host.skill_state(skill).unwrap().question, None);
        }
        assert!(second.take_merge_runs().is_empty());
    }

    #[test]
    fn both_macs_merge_at_once_and_the_second_to_push_keeps_both_entries_without_a_pause() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (first_home, second_home, merged) = rewritten_on_two_macs(&github);
        let mut first = Host::open(first_home.path()).unwrap();
        let mut second = Host::open(second_home.path()).unwrap();
        github.offline.store(true, Ordering::Relaxed);
        first.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        github.offline.store(false, Ordering::Relaxed);
        first.sync_now(&tools).unwrap();
        second.sync_now(&tools).unwrap();
        first.sync_now(&tools).unwrap();
        for host in [&first, &second] {
            assert_eq!(coordinating(host).text, merged);
            assert_eq!(merges(host).len(), 2);
            assert_eq!((notes(host).len(), status(host).waiting.len()), (0, 0));
        }
    }

    #[test]
    fn offline_at_start_a_mac_merges_and_takes_another_macs_merge_without_a_pause() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (home, _second_home, merged) = rewritten_on_two_macs(&github);
        let stock = skills::stock(Skill::Coordinating);
        let mut host = Host::open(home.path()).unwrap();
        github.offline.store(true, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        assert!(status(&host).error.is_some());
        assert_eq!(coordinating(&host).text, merged);
        let own = merges(&host).remove(0);

        let theirs = merged.replacen("Start by restating", "Start by restating briefly", 1);
        push_merge(&github, "v0", stock, &theirs, "Mike's MacBook Air");
        github.offline.store(false, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        assert_eq!(coordinating(&host).text, theirs);
        assert_eq!((notes(&host).len(), status(&host).waiting.len()), (0, 0));
        let paused = "SELECT COUNT(*) FROM sync_paused";
        assert_eq!(
            host.db
                .query_row(paused, [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        // Taken, checked again and found up to date: nothing more is merged.
        assert_eq!(merges(&host).len(), 2);
        let entry = format!("{COORDINATING}/history/{own}.json");
        assert!(github.show(&entry).is_some());
        let state = host.skill_state(Skill::Coordinating).unwrap();
        let note = state.merge_note.unwrap();
        assert_eq!(note.machine.as_deref(), Some("Mike's MacBook Air"));
    }

    #[test]
    fn an_outside_edit_is_not_merged_offline_at_start_and_pauses_once_online() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (home, _second_home, _) = rewritten_on_two_macs(&github);
        let rewrite = home
            .path()
            .join("shared")
            .join(COORDINATING)
            .join("rewrite.md");
        let edited = format!(
            "{}\nAn agent's shell edit.\n",
            fs::read_to_string(&rewrite).unwrap()
        );
        fs::write(&rewrite, &edited).unwrap();
        let mut host = Host::open(home.path()).unwrap();
        github.offline.store(true, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        assert_eq!(coordinating(&host).text, edited);
        assert!(merges(&host).is_empty());
        github.offline.store(false, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        assert_eq!(coordinating(&host).text, edited);
        assert!(matches!(
            notes(&host)[..],
            [PausedSetting {
                paused: Paused::OutsideWiffletree,
                ..
            }]
        ));
    }

    #[test]
    fn an_edit_after_this_macs_merge_pauses_the_skill_beside_another_macs_merge() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (home, _second_home, merged) = rewritten_on_two_macs(&github);
        let mut host = Host::open(home.path()).unwrap();
        github.offline.store(true, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        let skill = Skill::Coordinating;
        let text = format!("{merged}\nMy own last line.\n");
        (host.execute(Command::SaveSkillRewrite {
            skill,
            text: text.clone(),
        }))
        .unwrap();
        let theirs = merged.replacen("Start by restating", "Start by restating briefly", 1);
        let stock = skills::stock(skill);
        push_merge(&github, "v0", stock, &theirs, "Mike's MacBook Air");
        github.offline.store(false, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        assert_eq!(coordinating(&host).text, text);
        let notes = notes(&host);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].setting, "Coordinating · your rewrite");
        assert!(matches!(notes[0].paused, Paused::TwoMachines { .. }));
    }

    #[test]
    fn an_undo_before_it_syncs_pauses_the_skill_beside_another_macs_merge() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (home, _second_home, merged) = rewritten_on_two_macs(&github);
        let mut host = Host::open(home.path()).unwrap();
        let synced = coordinating(&host);
        github.offline.store(true, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        let skill = Skill::Coordinating;
        let entry = merges(&host).remove(0);
        (host.execute(Command::UndoSkillMerge { skill, entry })).unwrap();
        assert_eq!(coordinating(&host), synced);
        let theirs = merged.replacen("Start by restating", "Start by restating briefly", 1);
        let stock = skills::stock(skill);
        push_merge(&github, "v0", stock, &theirs, "Mike's MacBook Air");
        github.offline.store(false, Ordering::Relaxed);
        host.sync_now(&tools).unwrap();
        assert_eq!(coordinating(&host), synced);
        let notes = notes(&host);
        assert_eq!(notes.len(), 1);
        assert!(matches!(notes[0].paused, Paused::TwoMachines { .. }));
    }

    #[test]
    fn a_restore_from_before_a_merge_is_a_change_here_until_it_syncs() {
        let rewrite = skills::Rewritten {
            text: "Mine".into(),
            base: "# Coordinating — v0\n".into(),
        };
        let entry = |change| skills::Entry {
            change,
            stock_version: "v1".into(),
            edits: skills::Edits {
                rules: String::new(),
                rewrite: Some(rewrite.clone()),
            },
        };
        let merged = Change::Merged {
            from: "v0".into(),
            to: "v1".into(),
            before: rewrite.clone(),
            changes: vec![],
            model: None,
            machine: machine_name(),
            automatic: true,
        };
        let restored = Change::Restored { entry: "1".into() };
        let path = |name: &str| format!("{COORDINATING}/history/{name}.json");
        let here: BTreeMap<_, _> = [("1", Change::SavedRewrite), ("2", merged), ("3", restored)]
            .map(|(name, change)| (path(name), serde_json::to_vec(&entry(change)).unwrap()))
            .into();
        let synced_up_to = |last: usize| {
            let synced = (here.iter().take(last)).map(|(p, c)| (p.clone(), Some(c.clone())));
            synced.collect::<BTreeMap<_, Content>>()
        };
        let unit = format!("{COORDINATING}/");
        assert!(went_back_here(&unit, &here, &synced_up_to(1)));
        assert!(went_back_here(&unit, &here, &synced_up_to(2)));
        assert!(!went_back_here(&unit, &here, &synced_up_to(3)));
        let mut merged_only = here.clone();
        merged_only.remove(&path("3"));
        assert!(!went_back_here(&unit, &merged_only, &synced_up_to(1)));
    }

    #[test]
    fn a_rewrite_merged_on_an_older_release_is_merged_here_and_one_from_a_newer_is_kept() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let tools = github.tools();
        let (home, _second_home, merged) = rewritten_on_two_macs(&github);
        let stock = skills::stock(Skill::Coordinating);
        let mut host = Host::open(home.path()).unwrap();
        host.sync_now(&tools).unwrap();
        host.sync_now(&tools).unwrap();

        // A Mac on v0 saves over the merge; this Mac on v4 merges v0 → v4 into it and pushes that.
        let older = (stock.replacen("— v4", "— v0", 1)).replacen(
            "Ask the human instead when a waiver",
            "Ask the human when",
            1,
        );
        let theirs = older.replacen("Start by clarifying", "Start by restating briefly", 1);
        push_merge(&github, "v-1", &older, &theirs, "An older Mac");
        host.sync_now(&tools).unwrap();
        let briefly = merged.replacen("Start by restating", "Start by restating briefly", 1);
        assert_eq!(coordinating(&host).text, briefly);
        host.sync_now(&tools).unwrap();
        assert_eq!(
            github.show(&format!("{COORDINATING}/base.md")).unwrap(),
            stock
        );
        assert_eq!(
            github.show(&format!("{COORDINATING}/rewrite.md")).unwrap(),
            briefly
        );

        // A Mac on v5 merges v5 in; this Mac sends that rewrite as written and merges nothing.
        let newer = stock.replacen("— v4", "— v5", 1);
        let text = "Coordinating, merged with v5.";
        push_merge(&github, "v4", &newer, text, "A newer Mac");
        let count = merges(&host).len();
        host.sync_now(&tools).unwrap();
        let pushes = github.pushes();
        host.sync_now(&tools).unwrap();
        assert_eq!(github.pushes(), pushes);
        assert_eq!(merges(&host).len(), count + 1);
        let state = host.skill_state(Skill::Coordinating).unwrap();
        assert!(!state.rewrite.unwrap().out_of_date);
        assert_eq!(
            state.merge_standing,
            Some(MergeStanding::NewerBase {
                machine: Some("A newer Mac".into())
            })
        );
        assert!(host.instructions(Role::ProjectOrchestrator).ends_with(text));
    }
}
