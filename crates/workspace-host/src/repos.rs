//! One clone and one sync engine per repository, under `<data folder>/repos/`. The engine
//! fetches, records, commits and pushes; rule sets, chosen by path, decide what happens to their
//! files. Each commit holds one rule set's changes, and a file under no rule set is pulled and
//! never committed. The settings repository's rule set is `sync`.
use crate::*;
use github_access::{remote_head, run, run_at};
use publish::GitTool;
use std::collections::BTreeMap;

pub(crate) const FOLDER: &str = "repos";

/// Where the clone of the GitHub repository `owner/name` lives.
pub(crate) fn clone_folder(home: &Path, name: &str) -> PathBuf {
    home.join(FOLDER).join(name.replace('/', "-"))
}

/// Whether the clone at `clone` fetches the GitHub repository `owner/name` as `origin`. Two
/// repositories can share a folder name, as `a-b/c` and `a/b-c` do, so a clone found under
/// `repos/` is used only when this holds. Reads `.git/config` itself, as the shared folder is
/// resolved often.
pub(crate) fn holds(clone: &Path, name: &str) -> bool {
    let Ok(config) = fs::read_to_string(clone.join(".git/config")) else {
        return false;
    };
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line == "[remote \"origin\"]";
            continue;
        }
        let url = (line.strip_prefix("url"))
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .map(str::trim);
        if let Some(url) = url.filter(|_| in_origin) {
            return pr_watch::github_repository(url)
                .is_some_and(|(owner, repo)| format!("{owner}/{repo}").eq_ignore_ascii_case(name));
        }
    }
    false
}

/// Refuses a clone under `repos/` that is in the folder `owner/name` would use but is of another
/// repository.
pub(crate) fn refuse_another(home: &Path, name: &str) -> Result<()> {
    let clone = clone_folder(home, name);
    ensure!(
        !clone.exists() || holds(&clone, name),
        "{} holds a clone of another repository, whose name maps to the same folder as {name}. Move it aside, then try again.",
        clone.display()
    );
    Ok(())
}

/// A file's content, `None` for no file.
pub(crate) type Content = Option<Vec<u8>>;

/// The paths a rule set owns in a repository and what a pass does with its files.
pub(crate) trait RuleSet {
    /// Whether the file at `path`, relative to the clone, is this rule set's.
    fn claims(&self, path: &str) -> bool;
    /// Compares this machine's files with GitHub's head `remote`, before the clone moves to it.
    fn compare(&mut self, clone: &Path, remote: &str) -> Result<()>;
    /// The files to commit on GitHub's head, as the pass read them, and the commit's message.
    fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)>;
}

/// The only URL and branch a clone is fetched from and pushed to.
pub(crate) struct Remote<'a> {
    pub(crate) url: &'a str,
    pub(crate) branch: &'a str,
}

/// The head a pass pushed on top of GitHub's, if it committed anything, or why committing or
/// pushing failed.
pub(crate) type Pushed = std::result::Result<Option<String>, String>;

/// Fetches the branch, lets each rule set compare its files, takes GitHub's version of the files
/// no rule set claims, moves the clone to GitHub's head, and commits each rule set's changes
/// apart before pushing them together.
pub(crate) fn pass(
    clone: &Path,
    remote: Remote,
    previous: &str,
    rule_sets: &mut [&mut dyn RuleSet],
    git: &GitTool,
) -> Result<Pushed> {
    let branch = format!("refs/heads/{}", remote.branch);
    let fetch = ["fetch", "--quiet", "--no-tags", "--recurse-submodules=no"];
    run_at(
        git,
        clone,
        remote.url,
        &[&fetch[..], &[remote.url, &branch]].concat(),
    )?;
    let head = runtime::git_output(clone, &["rev-parse", "FETCH_HEAD"])?
        .trim()
        .to_owned();
    for rule_set in rule_sets.iter_mut() {
        rule_set.compare(clone, &head)?;
    }
    let owner = |path: &str| rule_sets.iter().position(|rule_set| rule_set.claims(path));
    take_other_files(clone, previous, &head, &|path| owner(path).is_some())?;
    runtime::git_output(clone, &["reset", "--quiet", &head])?;
    let commits: Vec<_> = rule_sets.iter_mut().filter_map(|r| r.commit()).collect();
    let owner = |path: &str| rule_sets.iter().position(|rule_set| rule_set.claims(path));
    let pushed = commit_and_push(clone, &remote, commits, &owner, git);
    Ok(pushed.map_err(|error| format!("{error:#}")))
}

/// One commit per rule set's changes, then one push of them all; the head pushed, if any.
fn commit_and_push(
    clone: &Path,
    remote: &Remote,
    commits: Vec<(Vec<(String, Content)>, String)>,
    owner: &dyn Fn(&str) -> Option<usize>,
    git: &GitTool,
) -> Result<Option<String>> {
    let mut made = false;
    for (files, message) in commits {
        stage(clone, &files)?;
        made |= commit_staged(clone, &message, git)?;
    }
    if !made {
        return Ok(None);
    }
    push(clone, remote, owner, git)?;
    let head = runtime::git_output(clone, &["rev-parse", "HEAD"])?;
    Ok(Some(head.trim().to_owned()))
}

/// Brings the files no rule set claims to GitHub's `remote`, as they were at `previous`.
pub(crate) fn take_other_files(
    clone: &Path,
    previous: &str,
    remote: &str,
    claimed: &dyn Fn(&str) -> bool,
) -> Result<()> {
    let diff = ["diff", "--name-status", "-z", "--no-renames"];
    let changed = runtime::git_output(clone, &[&diff[..], &[previous, remote]].concat())?;
    let mut fields = changed.split('\0');
    let mut taken = vec![];
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        match status {
            _ if claimed(path) => {}
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

/// The files at `commit` that `keep` takes, read whole.
pub(crate) fn files_at(
    clone: &Path,
    commit: &str,
    keep: &dyn Fn(&str) -> bool,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let listing = runtime::git_output(clone, &["ls-tree", "-r", "-z", commit])?;
    let mut blobs = vec![];
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (kind, path) = entry
            .split_once('\t')
            .context("Git listed an entry with no path")?;
        if keep(path) {
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

/// Commits what is staged, with the human's Git identity; `false` when nothing is.
pub(crate) fn commit_staged(clone: &Path, message: &str, git: &GitTool) -> Result<bool> {
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

/// Pushes the clone's HEAD to the remote's branch at its URL, with no tags or submodules and
/// never forced: only commits on top of the branch's head on GitHub, each changing the files of
/// one rule set, which `owner` names by its place among the repository's rule sets.
pub(crate) fn push(
    clone: &Path,
    remote: &Remote,
    owner: &dyn Fn(&str) -> Option<usize>,
    git: &GitTool,
) -> Result<()> {
    let head = runtime::git_output(clone, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    let on = remote_head(clone, remote.url, remote.branch, git)
        .context("This machine cannot read the repository")?;
    let range = match on {
        Some(on) => {
            ensure!(
                publish::is_ancestor(clone, &on, &head),
                "{} moved on GitHub since this machine read it",
                remote.branch
            );
            format!("{on}..{head}")
        }
        None => head.clone(),
    };
    let commits = runtime::git_output(clone, &["rev-list", &range])?;
    for commit in commits.lines() {
        let args = ["diff-tree", "--root", "-r", "-m", "--no-commit-id"];
        let args = [&args[..], &["--name-only", "--no-renames", "-z", commit]].concat();
        let changed = runtime::git_output(clone, &args)?;
        let mut rule_set = None;
        for path in changed.split('\0').filter(|path| !path.is_empty()) {
            let Some(claimed) = owner(path) else {
                bail!(
                    "Sync changes only the files its rule sets cover, and a commit changes {path}"
                );
            };
            ensure!(
                *rule_set.get_or_insert(claimed) == claimed,
                "A commit changes {path} beside files of another rule set, which sync commits apart"
            );
        }
    }
    let refspec = format!("{head}:refs/heads/{}", remote.branch);
    let args = ["push", "--no-follow-tags", "--recurse-submodules=no"];
    run_at(
        git,
        clone,
        remote.url,
        &[&args[..], &[remote.url, &refspec]].concat(),
    )?;
    Ok(())
}

/// What this machine last synced and last wrote of each file of a clone, `None` for no file,
/// and the commit it last synced. Only the settings repository's clone has a record so far;
/// a second clone's record needs the repository in its key.
pub(crate) struct Record {
    pub(crate) commit: String,
    pub(crate) synced: BTreeMap<String, Content>,
    pub(crate) written: BTreeMap<String, Content>,
}
impl Record {
    /// `None` until a record was started.
    pub(crate) fn read(db: &Connection) -> Result<Option<Self>> {
        let rows: Vec<(String, Content, Content)> = db
            .prepare("SELECT path,synced,written FROM sync_record")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut synced: BTreeMap<String, Content> = BTreeMap::new();
        let mut written = BTreeMap::new();
        for (path, content, wrote) in rows {
            written.insert(path.clone(), wrote);
            synced.insert(path, content);
        }
        let Some(commit) = synced.remove("") else {
            return Ok(None);
        };
        written.remove("");
        let commit = commit.map(String::from_utf8).transpose()?;
        Ok(Some(Self {
            commit: commit.context("Sync's record has no commit")?,
            synced,
            written,
        }))
    }
    /// Starts the record afresh at the clone's `head`, with each file as `synced` there and as
    /// `written` in the folder, and no file paused.
    pub(crate) fn restart(
        db: &mut Connection,
        head: &str,
        synced: &BTreeMap<String, Vec<u8>>,
        written: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let paths: std::collections::BTreeSet<&String> =
            synced.keys().chain(written.keys()).collect();
        let tx = db.transaction()?;
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
}

const MAX_BACKOFF_MS: i64 = 1_800_000;
pub(crate) const PUSH_DELAY_MS: i64 = 5_000;

/// When a repository's next pass is due: every `every` milliseconds, the shortest cadence of
/// its rule sets, backing off to thirty minutes while GitHub cannot be reached, and five seconds
/// after the last write, so a burst of saves is one commit. One runs at a time.
pub(crate) struct Pace {
    every: i64,
    pull_at: i64,
    push_at: Option<i64>,
    running: bool,
    /// Sync now was chosen while a pass ran.
    again: bool,
    failures: u32,
}
impl Pace {
    /// A pace for a repository whose rule sets want fetching at `cadences`.
    pub(crate) fn every(cadences: &[i64]) -> Self {
        Self {
            every: cadences.iter().copied().min().unwrap_or(MAX_BACKOFF_MS),
            pull_at: 0,
            push_at: None,
            running: false,
            again: false,
            failures: 0,
        }
    }
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
        let wait = self.every << self.failures.min(3);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::git;
    use crate::sync::tests::GitHub;
    use crate::test_support::without_background_maintenance;
    use std::sync::Arc;

    const URL: &str = "https://github.com/MHohlios/wiffletree-storage.git";

    /// A rule set over the files under `folder`, committing each of `files` this machine changed.
    struct Under {
        folder: &'static str,
        files: &'static [&'static str],
        changed: Vec<(String, Content)>,
    }
    impl Under {
        fn new(folder: &'static str, files: &'static [&'static str]) -> Self {
            let changed = vec![];
            Self {
                folder,
                files,
                changed,
            }
        }
    }
    impl RuleSet for Under {
        fn claims(&self, path: &str) -> bool {
            path.starts_with(self.folder)
        }
        fn compare(&mut self, clone: &Path, remote: &str) -> Result<()> {
            let folder = self.folder;
            let there = files_at(clone, remote, &|path| path.starts_with(folder))?;
            for file in self.files {
                let here = fs::read(clone.join(file)).ok();
                if here.as_ref() != there.get(*file) {
                    self.changed.push((file.to_string(), here));
                }
            }
            Ok(())
        }
        fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)> {
            let changed = std::mem::take(&mut self.changed);
            (!changed.is_empty()).then(|| (changed, self.folder.to_owned()))
        }
    }

    fn cloned(github: &GitHub, folder: &Path) -> PathBuf {
        let clone = folder.join("clone");
        let (bare, into) = (github.bare.to_str().unwrap(), clone.to_str().unwrap());
        git(folder, &["clone", "-q", bare, into]);
        without_background_maintenance(&clone);
        clone
    }

    fn changed_in(github: &GitHub, commit: &str) -> String {
        git(&github.bare, &["show", "--format=", "--name-only", commit])
            .trim()
            .to_owned()
    }

    #[test]
    fn two_rule_sets_in_one_repository_make_two_commits_and_a_readme_is_only_pulled() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let clone = cloned(&github, folder.path());
        let previous = git(&clone, &["rev-parse", "HEAD"]).trim().to_owned();
        github.commit("README.md", "Changed on GitHub.\n");
        for file in ["settings/review.json", "spaces/sage/space.yaml"] {
            fs::create_dir_all(clone.join(file).parent().unwrap()).unwrap();
            fs::write(clone.join(file), "{}\n").unwrap();
        }
        let mut settings = Under::new("settings/", &["settings/review.json"]);
        let mut space = Under::new("spaces/", &["spaces/sage/space.yaml"]);
        let remote = Remote {
            url: URL,
            branch: "main",
        };
        let rule_sets: &mut [&mut dyn RuleSet] = &mut [&mut settings, &mut space];
        let pushed = pass(&clone, remote, &previous, rule_sets, &github.tools().git).unwrap();
        assert_eq!(pushed.unwrap().as_deref(), Some(&*github.log("%H")[0]));
        assert_eq!(
            github.log("%s"),
            ["spaces/", "settings/", "README.md", "Initial commit"]
        );
        assert_eq!(changed_in(&github, "main"), "spaces/sage/space.yaml");
        assert_eq!(changed_in(&github, "main~"), "settings/review.json");
        let readme = fs::read_to_string(clone.join("README.md")).unwrap();
        assert_eq!(readme, "Changed on GitHub.\n");
    }

    #[test]
    fn a_commit_holding_two_rule_sets_files_is_never_pushed() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let clone = cloned(&github, folder.path());
        for file in ["settings/review.json", "spaces/sage/space.yaml"] {
            fs::create_dir_all(clone.join(file).parent().unwrap()).unwrap();
            fs::write(clone.join(file), "{}\n").unwrap();
        }
        git(&clone, &["add", "--all"]);
        git(&clone, &["commit", "-q", "-m", "Both"]);
        let owner = |path: &str| match path.split('/').next() {
            Some("settings") => Some(0),
            Some("spaces") => Some(1),
            _ => None,
        };
        let remote = Remote {
            url: URL,
            branch: "main",
        };
        let error = push(&clone, &remote, &owner, &github.tools().git).unwrap_err();
        assert!(
            format!("{error:#}").contains("another rule set"),
            "{error:#}"
        );
        assert_eq!(github.log("%s"), ["Initial commit"]);
    }

    #[test]
    fn a_repository_is_fetched_at_the_shortest_cadence_of_its_rule_sets() {
        let mut pace = Pace::every(&[300_000, 60_000]);
        assert!(pace.start(0));
        pace.finish(0, true);
        assert_eq!(pace.next_due(), Some(60_000));
        assert!(pace.start(60_000));
        pace.finish(60_000, false);
        assert_eq!(
            pace.next_due(),
            Some(180_000),
            "backing off while out of reach"
        );
    }
}
