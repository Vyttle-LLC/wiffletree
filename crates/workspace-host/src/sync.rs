//! The settings repository: checking a candidate, setting one up from this machine, joining one
//! set up elsewhere, and the one push sync makes. Git and `gh` run through `Tools`, off the
//! host's command thread; only putting a prepared clone in place runs on it.
use crate::shared::SharedStore;
use crate::skills::SkillStore;
use crate::*;
use publish::{GitTool, Tools};
use std::collections::BTreeMap;
use tempfile::TempDir;

/// The only paths a sync commit may change, as decision 2 lays them out.
const SHARED_PATHS: [&str; 3] = ["wiffletree.json", "settings", "skills"];

fn is_shared(path: &str) -> bool {
    path == SHARED_PATHS[0] || path.starts_with("settings/") || path.starts_with("skills/")
}

/// GitHub's owner and name, and the URL to push to: the URL named, or HTTPS for `owner/name`.
fn parse(input: &str) -> Result<(String, String, String)> {
    let input = input.trim();
    let url = match input.contains(':') {
        true => input.to_owned(),
        false => format!("https://github.com/{input}.git"),
    };
    let (owner, name) = pr_watch::github_repository(&url)
        .context("Name a repository on github.com as owner/name or by its URL")?;
    Ok((owner, name, url))
}

/// Runs Git through `git`, refusing a failed command with Git's reason.
fn run(git: &GitTool, path: &Path, args: &[&str]) -> Result<String> {
    let answer = git(path, args)?;
    ensure!(
        answer.status.success(),
        "git {} failed{}",
        args[0],
        answer.reason()
    );
    Ok(answer.output)
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

/// The branch's head on GitHub, or `None` while the repository has no commit on it. Reading it
/// is how this machine's access is checked.
fn remote_head(
    path: &Path,
    repository: &SettingsRepository,
    git: &GitTool,
) -> Result<Option<String>> {
    let reference = format!("refs/heads/{}", repository.branch);
    let heads = run(git, path, &["ls-remote", &repository.push_url, &reference])
        .context("This machine cannot read the repository")?;
    Ok(heads.split_whitespace().next().map(str::to_owned))
}

/// What GitHub and Git say about the repository, with a clone of its branch in a new folder of
/// the data folder that is removed when dropped.
fn read(home: &Path, input: &str, tools: &Tools) -> Result<(SettingsRepositoryCheck, TempDir)> {
    let (owner, name, push_url) = parse(input)?;
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
    let repository = SettingsRepository {
        name: field("full_name").context("GitHub named no repository")?,
        push_url,
        branch: field("default_branch").context("GitHub named no default branch")?,
        readers,
    };
    let head = remote_head(home, &repository, &tools.git)?;
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
    run(&tools.git, home, &clone)?;
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
    let settings = summaries(held)
        .into_iter()
        .zip(summaries(mine))
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
    let machine = fields.next().and_then(|subject| subject.rsplit_once(" · "));
    Ok(Joining {
        settings,
        last_machine: machine.map_or(author, |(_, machine)| machine).to_owned(),
        last_changed_at: at,
    })
}

/// Each shared setting in a word or two, in the order the Settings window lists them.
fn summaries(store: &SharedStore) -> Vec<(String, String)> {
    let levels: Vec<String> = store.review().levels.into_iter().map(|l| l.name).collect();
    let guide = match store.guide() {
        None => "none".to_owned(),
        Some(guide) if guide.trim().is_empty() => "empty".to_owned(),
        Some(guide) => format!("{} lines", guide.lines().count()),
    };
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
    let identity = ["user.name", "user.email"].map(|key| run(git, clone, &["config", key]));
    ensure!(
        identity
            .iter()
            .all(|value| value.as_ref().is_ok_and(|v| !v.trim().is_empty())),
        "Git has no name and email set. Set user.name and user.email with git config --global, then try again."
    );
    let literal: Vec<String> = paths.iter().map(|p| format!(":(literal){p}")).collect();
    let literal: Vec<&str> = literal.iter().map(String::as_str).collect();
    run(
        git,
        clone,
        &[&["add", "--all", "--"], &literal[..]].concat(),
    )?;
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
    let range = match remote_head(clone, repository, git)? {
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
    run(
        git,
        clone,
        &[&args[..], &[&repository.push_url, &refspec]].concat(),
    )?;
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
    pub(crate) next_pull_at: Option<i64>,
}

/// A file's content, `None` for no file.
type Content = Option<Vec<u8>>;

/// A pass's input from the host thread: the clone, its repository and sync's record.
pub(crate) struct Pass {
    clone: PathBuf,
    repository: SettingsRepository,
    commit: String,
    synced: BTreeMap<String, Content>,
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
    /// Why committing or pushing failed; what was pulled still applies.
    error: Option<String>,
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
    let batch = runtime::git_answer_with_input(clone, &["cat-file", "--batch"], &ids)?;
    ensure!(
        batch.status.success(),
        "git cat-file failed{}",
        batch.reason()
    );
    let mut rest = batch.output.as_bytes();
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

/// The settings `paths` hold, as the Settings window names them.
fn names(paths: &[String]) -> String {
    let mut names: Vec<&str> = vec![];
    for path in paths {
        let name = match path.as_str() {
            shared::REVIEW => "Review",
            shared::GUIDE => "Model guide",
            shared::CHECK_INS => "Check-ins",
            shared::KEEPER => "PR keeper",
            path => Skill::ALL
                .into_iter()
                .find(|skill| path.starts_with(&format!("skills/{}/", skill.folder())))
                .map_or(path, |skill| skill.name()),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(", ")
}

/// Fetches the branch, compares each file whole with the version last synced, and commits this
/// machine's changes on GitHub's head and pushes them. A file changed on both sides is left
/// out of the commit and kept here. Files to pull are returned for the host thread to write.
pub(crate) fn pass(pass: Pass, tools: &Tools) -> Result<Passed> {
    let Pass {
        clone,
        repository,
        commit: previous,
        synced,
    } = pass;
    let branch = format!("refs/heads/{}", repository.branch);
    let fetch = ["fetch", "--quiet", "--no-tags", "--recurse-submodules=no"];
    run(
        &tools.git,
        &clone,
        &[&fetch[..], &[&repository.push_url, &branch]].concat(),
    )?;
    let remote = runtime::git_output(&clone, &["rev-parse", "FETCH_HEAD"])?
        .trim()
        .to_owned();
    let (here, there) = (files_here(&clone)?, files_at(&clone, &remote)?);
    let mut passed = Passed {
        repository,
        remote: remote.clone(),
        pushed: None,
        pulled: vec![],
        alike: vec![],
        committed: vec![],
        error: None,
    };
    let paths: std::collections::BTreeSet<&String> = synced
        .keys()
        .chain(here.keys())
        .chain(there.keys())
        .collect();
    for path in paths {
        let base = synced.get(path).and_then(Option::as_ref);
        let (mine, theirs) = (here.get(path), there.get(path));
        if mine == theirs {
            if mine != base {
                passed.alike.push((path.clone(), mine.cloned()));
            }
        } else if mine == base {
            passed
                .pulled
                .push((path.clone(), mine.cloned(), theirs.cloned()));
        } else if theirs == base {
            passed.committed.push((path.clone(), mine.cloned()));
        }
    }
    take_other_files(&clone, &previous, &remote)?;
    runtime::git_output(&clone, &["reset", "--quiet", &remote])?;
    let paths: Vec<String> = passed.committed.iter().map(|(p, _)| p.clone()).collect();
    if !paths.is_empty() {
        let message = format!("{} · {}", names(&paths), machine_name());
        let pushed = commit(&clone, &paths, &message, &tools.git).and_then(|made| {
            if made {
                push(&clone, &passed.repository, &tools.git)?;
            }
            let head = runtime::git_output(&clone, &["rev-parse", "HEAD"])?;
            Ok(made.then(|| head.trim().to_owned()))
        });
        match pushed {
            Ok(Some(head)) => {
                // A save made during the pass can be what was committed: record that.
                let pushed = files_at(&clone, &head)?;
                for (path, content) in &mut passed.committed {
                    *content = pushed.get(path).cloned();
                }
                passed.pushed = Some(head);
            }
            Ok(None) => {}
            Err(error) => passed.error = Some(format!("{error:#}")),
        }
    }
    Ok(passed)
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
        let mut synced: BTreeMap<String, Content> = self
            .db
            .prepare("SELECT path,synced FROM sync_record")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let commit = synced.remove("").flatten().map(String::from_utf8);
        Ok(Some(Pass {
            clone: self.home.join(shared::FOLDER),
            repository,
            commit: commit.transpose()?.context("Sync's record has no commit")?,
            synced,
        }))
    }
    /// Writes what a pass pulled and records what it synced; `true` when the status or a
    /// setting changed. A file saved here during the pass is not overwritten: the next pass
    /// finds it changed on both sides.
    pub(crate) fn finish_sync(&mut self, at: i64, passed: Result<Passed>) -> Result<bool> {
        let passed = match passed {
            Ok(passed) => passed,
            Err(error) => {
                self.sync.error = Some(format!("{error:#}"));
                return Ok(true);
            }
        };
        if self.settings().settings_repository.as_ref() != Some(&passed.repository) {
            return Ok(false);
        }
        let shared = self.home.join(shared::FOLDER);
        let mut pulled = vec![];
        for (path, here, there) in passed.pulled {
            let file = shared.join(&path);
            if fs::read(&file).ok() != here {
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
        tx.commit()?;
        self.sync = Synced {
            at: Some(at),
            pulled: pulled.len(),
            pushed: pushed.unwrap_or_default(),
            error: passed.error,
            next_pull_at: self.sync.next_pull_at,
        };
        Ok(true)
    }
    /// Runs a pass on this thread, as a host with no service does for Sync now.
    pub(crate) fn sync_now(&mut self, tools: &Tools) -> Result<()> {
        if let Some(pass) = self.sync_pass()? {
            self.finish_sync(now(), self::pass(pass, tools))?;
        }
        Ok(())
    }
    pub(crate) fn sync_status(&self) -> Result<Option<SyncStatus>> {
        let Some(repository) = self.settings().settings_repository else {
            return Ok(None);
        };
        let waiting: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM sync_record WHERE path<>'' AND written IS NOT synced",
            [],
            |r| r.get(0),
        )?;
        Ok(Some(SyncStatus {
            repository,
            synced_at: self.sync.at,
            pulled: self.sync.pulled,
            pushed: self.sync.pushed,
            next_pull_at: self.sync.next_pull_at,
            error: self.sync.error.clone(),
            waiting: waiting as usize,
        }))
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
    pub(crate) fn still_needs(&self) -> Result<Vec<String>> {
        let settings = self.settings();
        let mut needs = vec![];
        if settings.settings_repository.is_none() {
            return Ok(needs);
        }
        let selection = self.model_selection()?;
        if let Err(keeper) =
            policies::check_keeper_needs(&settings.keeper, &selection, &settings.verification)
        {
            needs.push(format!("{keeper:#}"));
        }
        for verifier in &settings.verification.verifiers {
            if let Some(provider) = verifier.provider
                && !selection.is_enabled(provider)
            {
                needs.push(format!(
                    "The {} verifier runs on {}, which is not enabled on this machine",
                    verifier.focus,
                    provider.label()
                ));
            }
        }
        Ok(needs)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::publish::tests::{exit, git};
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
                local.extend(
                    args.iter()
                        .map(|a| if a.contains("github.com") { bare } else { a }),
                );
                if github.offline.load(Ordering::Relaxed)
                    && args.iter().any(|a| a.contains("github.com"))
                {
                    return Ok(runtime::GitAnswer {
                        status: exit(128),
                        output: String::new(),
                        error: "fatal: unable to access 'https://github.com/MHohlios/wiffletree-storage.git/': Could not resolve host: github.com".into(),
                    });
                }
                let mut before = github.before.lock().unwrap();
                if before
                    .as_ref()
                    .is_some_and(|(command, _)| *command == args[0])
                {
                    before.take().unwrap().1();
                }
                drop(before);
                match args[0] {
                    "push" => runtime::git_push(path, &local),
                    _ => runtime::git_answer(path, &local),
                }
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
        fn commit(&self, file: &str, content: &str) {
            commit_on(&self.bare, file, content);
        }
        fn pushes(&self) -> usize {
            self.git
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a[0] == "push")
                .count()
        }
    }

    fn commit_on(bare: &Path, file: &str, content: &str) {
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
    fn a_repository_is_named_as_owner_and_name_or_by_its_github_url() {
        let https = "https://github.com/MHohlios/wiffletree-storage.git";
        for (input, url) in [
            (NAME, https),
            (" MHohlios/wiffletree-storage ", https),
            (https, https),
            (
                "git@github.com:MHohlios/wiffletree-storage.git",
                "git@github.com:MHohlios/wiffletree-storage.git",
            ),
        ] {
            let parsed = parse(input).unwrap();
            assert_eq!(
                parsed,
                ("MHohlios".into(), "wiffletree-storage".into(), url.into())
            );
        }
        for input in [
            "gitlab.com:me/storage",
            "https://gitlab.com/me/storage.git",
            "me",
            "/tmp/storage.git",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
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
            assert_eq!(calls[1][3], "orgs/Sage-Social");
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
            let git_calls = github.git.lock().unwrap().clone();
            let push = git_calls.iter().find(|a| a[0] == "push").unwrap();
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
        assert!(host.model_selection().unwrap().is_enabled(Provider::Codex));
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
        let (_first, first) = set_up_elsewhere(&github);
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
        review
            .verifiers
            .iter_mut()
            .for_each(|v| v.provider = Some(Provider::Codex));
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
        // A machine with no saved setup: no PR keeper model and no Codex.
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        assert!(host.still_needs().unwrap().is_empty());
        host.choose_settings_repository(NAME, Some(JoinWith::Repository), &github.tools())
            .unwrap();
        let needs = host.snapshot().unwrap().still_needs;
        assert!(needs[0].contains("needs a PR keeper model"), "{needs:?}");
        assert!(
            needs[1..]
                .iter()
                .all(|n| n.contains("Codex, which is not enabled")),
            "{needs:?}"
        );
        assert!(needs.len() > 1);
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
        assert_eq!(status(&second).waiting, 1);
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
            assert_eq!((status.error, status.waiting), (None, 0));
        }
        let machine = machine_name();
        assert_eq!(
            github.log("%s")[..2],
            [
                format!("Check-ins · {machine}"),
                format!("Review · {machine}")
            ]
        );
        // The status names the repository and what the last pass moved.
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

    #[test]
    fn a_file_changed_on_both_sides_is_kept_here_and_left_out_of_the_commit() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        let additions = "skills/implementing/additions.md";
        fs::write(first.home.join("shared").join(additions), "From the first.").unwrap();
        fs::write(
            second.home.join("shared").join(additions),
            "From the second.",
        )
        .unwrap();
        second.set_check_ins(40, 400).unwrap();
        first.sync_now(&tools).unwrap();
        for _ in 0..2 {
            second.sync_now(&tools).unwrap();
        }
        let rules = |host: &Host| host.skills().state(Skill::Implementing).rules;
        assert_eq!(rules(&second), "From the second.");
        assert_eq!(github.show(additions).unwrap(), "From the first.");
        assert!(
            github
                .show("settings/check-ins.json")
                .unwrap()
                .contains("400")
        );
        first.sync_now(&tools).unwrap();
        assert_eq!(rules(&first), "From the first.");
        assert_eq!(first.settings().checkin_human_turns, 400);
    }

    #[test]
    fn a_rule_saved_twice_within_five_seconds_is_one_commit_the_other_machine_pulls() {
        // A pass is due five seconds after the last of two saves.
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
        assert_eq!(offline.waiting, 1);
        let error = offline.error.unwrap();
        assert!(
            error.contains("Could not resolve host: github.com"),
            "{error}"
        );
        assert_eq!(github.show("settings/check-ins.json"), None);
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
        assert_eq!((online.error, online.waiting, online.pushed), (None, 0, 1));
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
        host.set_check_ins(40, 400).unwrap();
        let bare = github.bare.clone();
        *github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || commit_on(&bare, "skills/reviewing/additions.md", "Read every test.")),
        ));
        host.sync_now(&github.tools()).unwrap();
        let refused = status(&host);
        assert!(refused.error.unwrap().contains("git push failed"));
        assert_eq!(refused.waiting, 1);
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
        assert_eq!((synced.error, synced.waiting), (None, 0));
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
    fn a_save_made_during_a_pass_is_recorded_as_what_was_pushed() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let (_first_home, mut first) = set_up_elsewhere(&github);
        let (_second_home, mut second) = joined(&github);
        let tools = github.tools();
        first.set_check_ins(30, 300).unwrap();
        // The pass has read 30 and 300 when the human saves 40 and 400.
        let shared = first.home.join("shared");
        *github.before.lock().unwrap() = Some((
            "config",
            Box::new(move || SharedStore::at(shared).set_check_ins(40, 400).unwrap()),
        ));
        first.sync_now(&tools).unwrap();
        assert!(
            github
                .show("settings/check-ins.json")
                .unwrap()
                .contains("400")
        );
        // A later change made only on the other machine is pulled, not taken for a conflict.
        second.sync_now(&tools).unwrap();
        second.set_check_ins(50, 500).unwrap();
        second.sync_now(&tools).unwrap();
        first.sync_now(&tools).unwrap();
        assert_eq!(first.settings().checkin_human_turns, 500);
        assert_eq!(status(&first).pulled, 1);
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
        assert_eq!((synced.pushed, synced.waiting, synced.error), (1, 0, None));
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
        assert!(status(&host).waiting >= 4);
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
        assert_eq!((synced.waiting, synced.error), (0, None));
    }
}
