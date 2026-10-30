//! One clone and one sync engine per repository, under `<data folder>/repos/`. The engine
//! fetches, records, commits and pushes; rule sets, chosen by path, decide what happens to their
//! files. Each commit holds one rule set's changes, and a file under no rule set is pulled and
//! never committed. The settings repository's rule set is `sync`.
use crate::*;
use github_access::{remote_head, run, run_at};
use publish::GitTool;
use std::collections::BTreeMap;

pub(crate) const FOLDER: &str = "repos";
/// The folder of `repos/` that holds GitHub clones, one folder per owner. It has a dot, which no
/// GitHub owner's name can, so it never meets an owner's folder or a `local-<id>` one.
const GITHUB: &str = "github.com";

/// Where the clone of the GitHub repository `owner/name` lives: `repos/github.com/<owner>/<name>/`.
/// Nesting by owner keeps two repositories apart with no escaping, as `a-b/c` and `a/b-c`, which
/// one folder named `<owner>-<name>` could not.
pub(crate) fn clone_folder(home: &Path, name: &str) -> PathBuf {
    let (owner, name) = name.split_once('/').unwrap_or((name, ""));
    home.join(FOLDER).join(GITHUB).join(owner).join(name)
}

/// Where the local storage repository `local-<id>` lives.
pub(crate) fn local_folder(home: &Path, key: &str) -> PathBuf {
    home.join(FOLDER).join(key)
}

/// How sync's record names a repository: `owner/name` in lower case, as GitHub matches names, or
/// `local-<id>`. The shared folder with no settings repository is the empty key.
pub(crate) fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// Moves each GitHub clone an earlier release left at `repos/<owner>-<name>/` to its
/// `clone_folder`: by the name this Mac recorded for the repository the clone `holds`, as the
/// settings repository's clone may still have a redirected name as `origin`, and else by the
/// repository its `origin` names. Each rename is recorded in `repo_moves` before it is made and
/// forgotten after, so one cut short is finished at the next start, and a second start moves
/// nothing. A recorded repository's clone then has `origin` naming it as GitHub does. A clone
/// whose folder is taken, or whose `origin` names no GitHub repository, stays where it is, with a
/// log line.
pub(crate) fn migrate_clone_folders(db: &Connection, home: &Path) -> Result<()> {
    let root = home.join(FOLDER);
    let moves: Vec<(String, String)> = (db.prepare("SELECT from_path,to_path FROM repo_moves")?)
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (from, to) in moves {
        finish_move(db, &root, &from, &to)?;
    }
    let recorded = recorded_repositories(db, home)?;
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let from = entry.file_name().to_string_lossy().into_owned();
        if from == GITHUB || from.starts_with("local-") || !entry.path().join(".git").exists() {
            continue;
        }
        let canonical = (recorded.iter())
            .find(|repository| holds(&entry.path(), repository))
            .map(|repository| repository.name.clone());
        let named = canonical.or_else(|| {
            let url = origin(&entry.path())?;
            pr_watch::github_repository(&url).map(|(owner, name)| format!("{owner}/{name}"))
        });
        let Some(name) = named else {
            eprintln!(
                "Not moving {}: its origin names no GitHub repository",
                entry.path().display()
            );
            continue;
        };
        let to = format!("{GITHUB}/{name}");
        if root.join(&to).exists() {
            eprintln!(
                "Not moving {}: {} is taken",
                entry.path().display(),
                root.join(&to).display()
            );
            continue;
        }
        db.execute(
            "INSERT OR REPLACE INTO repo_moves(from_path,to_path) VALUES (?1,?2)",
            params![from, to],
        )?;
        finish_move(db, &root, &from, &to)?;
    }
    for repository in &recorded {
        let clone = clone_folder(home, &repository.name);
        if clone.join(".git").exists() && holds(&clone, repository) {
            name_origin(&clone, &repository.name)?;
        }
    }
    Ok(())
}

/// The GitHub repositories this Mac recorded a use for: the settings repository and storage.
fn recorded_repositories(db: &Connection, home: &Path) -> Result<Vec<SettingsRepository>> {
    let stored: Vec<String> = (db.prepare("SELECT data FROM storage_repositories")?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut recorded: Vec<SettingsRepository> = (stored.iter())
        .map(|data| serde_json::from_str(data))
        .collect::<serde_json::Result<_>>()?;
    recorded.extend(shared::chosen(home));
    Ok(recorded)
}

/// Keys sync's record and pauses by repository, for a store from before more than one clone kept
/// a record: every row belonged to the settings repository's clone, so it moves to that
/// repository's key, or to the shared folder's empty key when none is chosen. One transaction,
/// run only while the record has no `repository` column.
pub(crate) fn migrate_records(db: &Connection, home: &Path) -> Result<()> {
    let keyed: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sync_record') WHERE name='repository')",
        [],
        |r| r.get(0),
    )?;
    if keyed {
        return Ok(());
    }
    let repository = shared::chosen(home).map_or_else(String::new, |r| key(&r.name));
    let tx = db.unchecked_transaction()?;
    tx.execute_batch(
        "ALTER TABLE sync_record RENAME TO sync_record_unkeyed;
         ALTER TABLE sync_paused RENAME TO sync_paused_unkeyed;",
    )?;
    tx.execute_batch(include_str!("sync.sql"))?;
    tx.execute(
        "INSERT INTO sync_record SELECT ?1,path,synced,written FROM sync_record_unkeyed",
        [&repository],
    )?;
    tx.execute(
        "INSERT INTO sync_paused SELECT ?1,path,why,theirs FROM sync_paused_unkeyed",
        [&repository],
    )?;
    tx.execute_batch("DROP TABLE sync_record_unkeyed; DROP TABLE sync_paused_unkeyed;")?;
    tx.commit()?;
    Ok(())
}

/// Renames `repos/<from>` to `repos/<to>` unless that happened already, then forgets the move.
fn finish_move(db: &Connection, root: &Path, from: &str, to: &str) -> Result<()> {
    let (source, target) = (root.join(from), root.join(to));
    match (source.exists(), target.exists()) {
        (true, false) => {
            eprintln!("Moving {} to {}", source.display(), target.display());
            fs::create_dir_all(target.parent().context("A clone is in a folder")?)?;
            fs::rename(&source, &target)?;
        }
        (true, true) => eprintln!(
            "Not moving {}: {} is taken",
            source.display(),
            target.display()
        ),
        (false, _) => {}
    }
    db.execute("DELETE FROM repo_moves WHERE from_path=?1", [from])?;
    Ok(())
}

/// Whether the clone at `clone` is of `repository`: its `origin` names GitHub's name for it, or
/// the repository of its recorded push URL, which an older release left as `origin` when the
/// human named a URL GitHub redirects. A clone found under `repos/` is used only when this holds,
/// as a folder may hold another repository's clone: one put there by hand, or one a release that
/// named folders `<owner>-<name>` could not keep apart, as it could not `a-b/c` and `a/b-c`.
/// Reads `.git/config` itself, as the shared folder is resolved often.
pub(crate) fn holds(clone: &Path, repository: &SettingsRepository) -> bool {
    let named = |url: &str| pr_watch::github_repository(url).map(|(o, n)| format!("{o}/{n}"));
    let pushed = named(&repository.push_url);
    origin(clone)
        .and_then(|url| named(&url))
        .is_some_and(|origin| {
            origin.eq_ignore_ascii_case(&repository.name)
                || pushed.is_some_and(|pushed| origin.eq_ignore_ascii_case(&pushed))
        })
}

/// Sets the clone's `origin` to GitHub's URL for `owner/name`, even when the repository was
/// renamed since the URL it was cloned from, so `holds` knows it. Sync names its URL on every
/// command and never fetches or pushes through `origin`.
pub(crate) fn name_origin(clone: &Path, name: &str) -> Result<()> {
    let origin = format!("https://github.com/{name}.git");
    runtime::git_output(clone, &["remote", "set-url", "origin", &origin])?;
    Ok(())
}

/// The URL `origin` fetches, as the clone's `.git/config` holds it.
fn origin(clone: &Path) -> Option<String> {
    let config = fs::read_to_string(clone.join(".git/config")).ok()?;
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
            return Some(url.to_owned());
        }
    }
    None
}

/// Refuses a clone under `repos/` that is in the folder `repository` would use but is of another
/// repository.
pub(crate) fn refuse_another(home: &Path, repository: &SettingsRepository) -> Result<()> {
    let clone = clone_folder(home, &repository.name);
    ensure!(
        !clone.exists() || holds(&clone, repository),
        "{} holds a clone of another repository than {}. Move it aside, then try again.",
        clone.display(),
        repository.name
    );
    Ok(())
}

/// Checks a private GitHub repository named as `owner/name` or by its URL, as `gh` reaches it,
/// and clones its default branch into a `shared-staging-*` folder of the data folder, removed
/// when dropped or at the next start: the repository as this Mac will record it, whether its
/// owner is an organization, and the clone.
pub(crate) fn clone_to_staging(
    home: &Path,
    input: &str,
    tools: &publish::Tools,
) -> Result<(SettingsRepository, bool, tempfile::TempDir)> {
    use github_access::{candidates, first_readable, gh_protocol, private_repository};
    let (owner, name, urls) = candidates(input, &gh_protocol(tools, home))?;
    let private = private_repository(tools, home, &owner, &name)?;
    let (push_url, head) = first_readable(home, urls, &private.default_branch, &tools.git)?;
    let repository = SettingsRepository {
        name: private.name,
        push_url,
        branch: private.default_branch,
        readers: private.readers,
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
    name_origin(staging.path(), &repository.name)?;
    if head.is_none() {
        let branch = format!("refs/heads/{}", repository.branch);
        run(
            &tools.git,
            staging.path(),
            &["symbolic-ref", "HEAD", &branch],
        )?;
    }
    Ok((repository, private.organization, staging))
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
    /// `compare` failed: this pass commits nothing of this rule set, and leaves its files as
    /// they are, while the other rule sets go on.
    fn failed(&mut self, error: anyhow::Error);
    /// The files of its commit to hold back with those a scan `held`, which belong together.
    fn also_held(&self, _held: &[scan::Held]) -> Vec<scan::Held> {
        vec![]
    }
}

/// The only URL and branch a clone is fetched from and pushed to.
pub(crate) struct Remote<'a> {
    pub(crate) url: &'a str,
    pub(crate) branch: &'a str,
}

/// The head a pass pushed on top of GitHub's, if it committed anything, or why committing or
/// pushing failed.
pub(crate) type Pushed = std::result::Result<Option<String>, String>;

/// What a pass pushed, and the files a scan held back from its commits.
pub(crate) struct Outcome {
    pub(crate) pushed: Pushed,
    pub(crate) held: Vec<scan::Held>,
}

/// Fetches the branch, lets each rule set compare its files, takes GitHub's version of the files
/// no rule set claims, moves the clone to GitHub's head, and commits each rule set's changes
/// apart before pushing them together. With `scanner`, the files under the scanned paths are
/// scanned before they are committed, and a file that fails or cannot be scanned is held: left
/// out of the commit and kept here. A rule set whose compare fails is told so and commits
/// nothing; the others go on.
pub(crate) fn pass(
    clone: &Path,
    remote: Remote,
    previous: &str,
    rule_sets: &mut [&mut dyn RuleSet],
    scanner: Option<&scan::Scanner>,
    git: &GitTool,
) -> Result<Outcome> {
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
    let mut compared = vec![];
    for rule_set in rule_sets.iter_mut() {
        let result = rule_set.compare(clone, &head);
        compared.push(result.is_ok());
        if let Err(error) = result {
            rule_set.failed(error);
        }
    }
    let owner = |path: &str| rule_sets.iter().position(|rule_set| rule_set.claims(path));
    take_other_files(clone, previous, &head, &|path| owner(path).is_some())?;
    runtime::git_output(clone, &["reset", "--quiet", &head])?;
    let commits: Vec<_> = (rule_sets.iter_mut().zip(compared).enumerate())
        .filter(|(_, (_, compared))| *compared)
        .filter_map(|(at, (rule_set, _))| Some((at, rule_set.commit()?)))
        .collect();
    let mut held = vec![];
    let mut scanned_commits = vec![];
    for (at, (mut files, message)) in commits {
        if let Some(scanner) = scanner {
            let mut found = scanner.scan(clone, &head, &files);
            found.extend(rule_sets[at].also_held(&found));
            files.retain(|(path, _)| !found.iter().any(|held| &held.path == path));
            held.extend(found);
        }
        scanned_commits.push((files, message));
    }
    let owner = |path: &str| rule_sets.iter().position(|rule_set| rule_set.claims(path));
    let pushed = commit_and_push(clone, &remote, scanned_commits, &owner, git);
    Ok(Outcome {
        pushed: pushed.map_err(|error| format!("{error:#}")),
        held,
    })
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
    for (files, message) in commits.into_iter().filter(|(files, _)| !files.is_empty()) {
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
/// and the commit it last synced, kept per repository by its `key`.
pub(crate) struct Record {
    pub(crate) commit: String,
    pub(crate) synced: BTreeMap<String, Content>,
    pub(crate) written: BTreeMap<String, Content>,
}
impl Record {
    /// `None` until a record was started.
    pub(crate) fn read(db: &Connection, repository: &str) -> Result<Option<Self>> {
        let rows: Vec<(String, Content, Content)> = db
            .prepare("SELECT path,synced,written FROM sync_record WHERE repository=?1")?
            .query_map([repository], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
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
    /// Starts the record of the files `owns` takes afresh at the clone's `head`, with each file
    /// as `synced` there and as `written` in the folder, and none of them paused or held. The
    /// other rule sets' files in the repository keep their record, pauses and holds.
    pub(crate) fn restart(
        db: &Connection,
        repository: &str,
        head: &str,
        synced: &BTreeMap<String, Vec<u8>>,
        written: &BTreeMap<String, Vec<u8>>,
        owns: &dyn Fn(&str) -> bool,
    ) -> Result<()> {
        let paths: std::collections::BTreeSet<&String> =
            synced.keys().chain(written.keys()).collect();
        let tx = db.unchecked_transaction()?;
        for table in ["sync_record", "sync_paused", "sync_held"] {
            let listed: Vec<String> = (tx
                .prepare(&format!("SELECT path FROM {table} WHERE repository=?1"))?)
            .query_map([repository], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
            for path in listed.iter().filter(|path| path.is_empty() || owns(path)) {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE repository=?1 AND path=?2"),
                    params![repository, path],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO sync_record(repository,path,synced) VALUES (?1,'',?2)",
            params![repository, head.as_bytes()],
        )?;
        for path in paths {
            tx.execute(
                "INSERT INTO sync_record(repository,path,synced,written) VALUES (?1,?2,?3,?4)",
                params![repository, path, synced.get(path), written.get(path)],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Notes that the host wrote `content` to `path` of the repository's clone, `None` for a file
    /// removed, so the next pass commits it rather than pausing it as an outside edit.
    pub(crate) fn wrote(
        db: &Connection,
        repository: &str,
        path: &str,
        content: Option<&[u8]>,
    ) -> Result<()> {
        db.execute(
            "INSERT INTO sync_record(repository,path,written) VALUES (?1,?2,?3) ON CONFLICT(repository,path) DO UPDATE SET written=excluded.written",
            params![repository, path, content],
        )?;
        Ok(())
    }
}

const MAX_BACKOFF_MS: i64 = 1_800_000;
pub(crate) const PUSH_DELAY_MS: i64 = 5_000;

/// When a repository's next pass is due: every `every` milliseconds, the shortest cadence of
/// its rule sets, backing off to thirty minutes while GitHub cannot be reached, and a rule set's
/// delay after the last write, so a burst of saves is one commit. One runs at a time.
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
    /// Fetches every `every` milliseconds, as the rule sets the repository holds change: a
    /// shorter cadence brings the next fetch forward at once, a longer one starts with the next
    /// pass. A repository backing off keeps its wait.
    pub(crate) fn set_every(&mut self, every: i64) {
        if every < self.every && self.failures == 0 {
            self.pull_at -= self.every - every;
        }
        self.every = every;
    }
    /// Pushes `delay` milliseconds after a write at `at`. While GitHub cannot be reached, a
    /// save waits for the next try.
    pub(crate) fn wrote(&mut self, at: i64, delay: i64) {
        self.push_at = Some(at + delay);
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
        /// Its compare fails, as a rule set's can on a file it cannot read.
        breaks: bool,
        failed: Option<String>,
    }
    impl Under {
        fn new(folder: &'static str, files: &'static [&'static str]) -> Self {
            Self {
                folder,
                files,
                changed: vec![],
                breaks: false,
                failed: None,
            }
        }
    }
    impl RuleSet for Under {
        fn claims(&self, path: &str) -> bool {
            path.starts_with(self.folder)
        }
        fn compare(&mut self, clone: &Path, remote: &str) -> Result<()> {
            ensure!(!self.breaks, "{} can't be read", self.folder);
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
        fn failed(&mut self, error: anyhow::Error) {
            self.failed = Some(format!("{error:#}"));
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
        let git = &github.tools().git;
        let outcome = pass(&clone, remote, &previous, rule_sets, None, git).unwrap();
        assert_eq!(
            outcome.pushed.unwrap().as_deref(),
            Some(&*github.log("%H")[0])
        );
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
    fn a_rule_set_whose_compare_fails_commits_nothing_and_the_others_still_push() {
        let folder = tempfile::tempdir().unwrap();
        let github = Arc::new(GitHub::new(folder.path(), true));
        let clone = cloned(&github, folder.path());
        let previous = git(&clone, &["rev-parse", "HEAD"]).trim().to_owned();
        for file in ["settings/review.json", "spaces/sage/space.yaml"] {
            fs::create_dir_all(clone.join(file).parent().unwrap()).unwrap();
            fs::write(clone.join(file), "{}\n").unwrap();
        }
        let mut settings = Under::new("settings/", &["settings/review.json"]);
        let mut space = Under::new("spaces/", &["spaces/sage/space.yaml"]);
        space.breaks = true;
        let remote = Remote {
            url: URL,
            branch: "main",
        };
        let rule_sets: &mut [&mut dyn RuleSet] = &mut [&mut settings, &mut space];
        let git = &github.tools().git;
        let outcome = pass(&clone, remote, &previous, rule_sets, None, git).unwrap();
        assert!(outcome.pushed.unwrap().is_some());
        assert_eq!(github.log("%s"), ["settings/", "Initial commit"]);
        assert_eq!(space.failed.as_deref(), Some("spaces/ can't be read"));
        assert_eq!(settings.failed, None);
        let kept = fs::read_to_string(clone.join("spaces/sage/space.yaml")).unwrap();
        assert_eq!(kept, "{}\n", "its files stay as they were");
    }

    #[test]
    fn earlier_clone_folders_move_to_one_folder_per_owner_and_a_cut_short_move_finishes() {
        let home = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("sync.sql")).unwrap();
        let root = home.path().join(FOLDER);
        // `a-b/c` and `a/b-c` both took `a-b-c` before; only one could be there.
        let earlier = |folder: &str, name: &str| {
            let clone = root.join(folder);
            fs::create_dir_all(&clone).unwrap();
            git(&clone, &["init", "-q"]);
            git(
                &clone,
                &[
                    "remote",
                    "add",
                    "origin",
                    &format!("https://github.com/{name}.git"),
                ],
            );
        };
        earlier("a-b-c", "a-b/c");
        earlier(
            "Sage-Social-wiffletree-storage",
            "Sage-Social/wiffletree-storage",
        );
        fs::create_dir_all(root.join("local-7k2q/.git")).unwrap();
        // A move recorded by a start that stopped before renaming.
        db.execute(
            "INSERT INTO repo_moves VALUES ('Sage-Social-wiffletree-storage','github.com/Sage-Social/wiffletree-storage')",
            [],
        )
        .unwrap();
        migrate_clone_folders(&db, home.path()).unwrap();
        let clone = clone_folder(home.path(), "a-b/c");
        assert_eq!(clone, root.join("github.com/a-b/c"));
        assert!(clone.join(".git").exists() && !root.join("a-b-c").exists());
        assert_ne!(clone, clone_folder(home.path(), "a/b-c"));
        let storage = clone_folder(home.path(), "Sage-Social/wiffletree-storage");
        assert!(storage.join(".git").exists());
        assert!(root.join("local-7k2q/.git").exists(), "local storage stays");
        let moves: i64 =
            (db.query_row("SELECT count(*) FROM repo_moves", [], |r| r.get(0))).unwrap();
        assert_eq!(moves, 0);
        let listed = |root: &Path| {
            let mut names: Vec<_> = (fs::read_dir(root).unwrap())
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };
        let before = listed(&root);
        migrate_clone_folders(&db, home.path()).unwrap();
        assert_eq!(listed(&root), before, "a second start moves nothing");
        assert_eq!(before, ["github.com", "local-7k2q"]);
    }

    #[test]
    fn a_settings_clone_whose_origin_is_a_redirect_moves_to_its_recorded_name() {
        let home = tempfile::tempdir().unwrap();
        let former = "https://github.com/MHohlios/former-settings.git";
        let repository = SettingsRepository {
            name: "MHohlios/wiffletree-storage".into(),
            push_url: former.into(),
            branch: "main".into(),
            readers: Readers::Invited,
        };
        let settings = HostSettings {
            settings_repository: Some(repository.clone()),
            ..HostSettings::new("/tmp/workspaces".into())
        };
        let file = home.path().join(settings::FILE);
        fs::write(file, serde_json::to_vec(&settings).unwrap()).unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("sync.sql")).unwrap();
        let earlier = home.path().join(FOLDER).join("MHohlios-former-settings");
        fs::create_dir_all(&earlier).unwrap();
        git(&earlier, &["init", "-q"]);
        git(&earlier, &["remote", "add", "origin", former]);
        migrate_clone_folders(&db, home.path()).unwrap();
        let clone = clone_folder(home.path(), &repository.name);
        assert!(clone.join(".git").exists() && !earlier.exists());
        let origin = git(&clone, &["remote", "get-url", "origin"]);
        assert_eq!(origin, "https://github.com/MHohlios/wiffletree-storage.git");
        assert!(holds(&clone, &repository));
    }

    #[test]
    fn the_record_from_before_repositories_were_keyed_moves_to_the_settings_clone_once() {
        let home = tempfile::tempdir().unwrap();
        let repository = SettingsRepository {
            name: "MHohlios/wiffletree-storage".into(),
            push_url: URL.into(),
            branch: "main".into(),
            readers: Readers::Invited,
        };
        let settings = HostSettings {
            settings_repository: Some(repository),
            ..HostSettings::new("/tmp/workspaces".into())
        };
        fs::write(
            home.path().join(settings::FILE),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE sync_record (path TEXT PRIMARY KEY, synced BLOB, written BLOB);
             CREATE TABLE sync_paused (path TEXT PRIMARY KEY, why TEXT NOT NULL, theirs BLOB);
             INSERT INTO sync_record VALUES ('',X'616263',NULL), ('settings/review.json',X'7b7d',X'7b7d');
             INSERT INTO sync_paused VALUES ('settings/review.json','{\"kind\":\"outside_wiffletree\"}',NULL);",
        )
        .unwrap();
        db.execute_batch(include_str!("sync.sql")).unwrap();
        migrate_records(&db, home.path()).unwrap();
        migrate_records(&db, home.path()).unwrap();
        let key = "mhohlios/wiffletree-storage";
        let record = Record::read(&db, key).unwrap().unwrap();
        assert_eq!(record.commit, "abc");
        assert_eq!(
            record.synced["settings/review.json"].as_deref(),
            Some(&b"{}"[..])
        );
        let paused: Vec<(String, String)> = (db.prepare("SELECT repository,path FROM sync_paused"))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(paused, [(key.into(), "settings/review.json".into())]);
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

    #[test]
    fn a_shorter_cadence_brings_the_next_fetch_forward_at_once() {
        let mut pace = Pace::every(&[300_000]);
        assert!(pace.start(0));
        pace.finish(0, true);
        pace.set_every(60_000);
        assert_eq!(pace.next_due(), Some(60_000));
        pace.set_every(60_000);
        assert_eq!(
            pace.next_due(),
            Some(60_000),
            "the same cadence changes nothing"
        );
        pace.set_every(300_000);
        assert_eq!(
            pace.next_due(),
            Some(60_000),
            "a longer one waits for the next pass"
        );
    }
}
