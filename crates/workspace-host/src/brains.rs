//! Brains (OpenSpec change `brain-registry-sync`): the registry of the brains this Mac's Spaces
//! name, each brain's `wiffletree-brain.json`, the host's own checkout, and the batches that reach
//! `main` only through a pull request set to auto-merge, or, for Set up, merged by the code owner
//! after their review. A pass runs off the host thread. Its only writes to GitHub are pushes of
//! `wiffletree/brain/` branches, never forced, and `gh pr create`, `gh pr merge --auto --squash`
//! and `gh pr close`; everything it watches it reads with GETs.
//! Nothing here stores or logs a brain file's content or a check's output text.
use crate::*;
use github_access::{gh_json, remote_head, run, run_at};
use publish::{GitTool, Tools};
use std::collections::BTreeMap;

const SCHEMA: &str = include_str!("brains.sql");
/// The file in the checkout's `.git` that tells the legacy safety-net to keep away.
const MARKER: &str = "wiffletree-managed";
/// Where each batch's worktree of its brain's checkout lives, under the data folder.
const BATCHES: &str = "brain-batches";
/// Every branch the host pushes to a brain is under this.
const BRANCH_PREFIX: &str = "wiffletree/brain/";
const MAIN: &str = "main";
/// The legacy pointer, under the human's home folder: the path of one brain per machine.
pub(crate) const LEGACY_POINTER: &str = ".config/claude-second-brain";
const ALLOWLIST: &str = "brain-gate-allowlist.toml";
const CALLER: &str = ".github/workflows/brain-gate.yml";
const CODEOWNERS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
/// The paths `CODEOWNERS` keeps from changing without the code owner's review.
const OWNED: [&str; 5] = [
    "/.github/",
    "/wiffletree-brain.json",
    "/brain-gate-allowlist.toml",
    "/.gitleaks.toml",
    "/.gitleaksignore",
];
/// The gate's jobs, each its own required check on `main`.
const GATE_CHECKS: [&str; 7] = [
    "secret-scan",
    "sensitive-info",
    "markdown-lint",
    "wikilinks",
    "log-schema",
    "path-ownership",
    "gate-tests",
];
/// Batches merged within this long stay on the Brains page.
const SHOWN_AFTER_MERGE_MS: i64 = 86_400_000;

/// A gate release that Set up pins the caller to: its tag and commit.
#[derive(Clone, Copy)]
struct GateRelease {
    tag: &'static str,
    commit: &'static str,
}
/// The release Set up pins: the tag `brain-gate-v1.0.0` points at this commit, the merge of #175
/// that moved the gate into Wiffletree (`brain-gate` task 5.1).
const GATE_RELEASE: GateRelease = GateRelease {
    tag: "brain-gate-v1.0.0",
    commit: "083ab25267aa260569ab61106ff8fdff5bb2ba0e",
};

/// Creates the brain tables once and, in the same change, turns each Space's local brain folder
/// the `spaces` upgrade left whose `origin` is on GitHub into that repository as its brain, which
/// waits for the human's grant. A folder without one leaves its Space with no brain and a note
/// saying why.
pub(crate) fn migrate(host: &mut Host) -> Result<()> {
    let present: bool = host.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='brains')",
        [],
        |r| r.get(0),
    )?;
    if present {
        return Ok(());
    }
    host.atomically(|host| {
        host.db.execute_batch(SCHEMA)?;
        host.upgrade_local_brains()
    })
}

/// The GitHub repository, `owner/name`, that the folder's `origin` names.
fn origin_repository(folder: &Path) -> Option<String> {
    let answer = runtime::git_answer(folder, &["remote", "get-url", "origin"]).ok()?;
    let url = answer.status.success().then_some(answer.output.trim())?;
    pr_watch::github_repository(url).map(|(owner, name)| format!("{owner}/{name}"))
}

/// A folder inside the repository, such as `second-brain`, or a file there.
fn inside_repository(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// Refuses a path in `worktree` that passes through a symlink, which the brain may have
/// committed to point outside it, so the host never writes or reads through one.
fn refuse_symlinks(worktree: &Path, path: &str) -> Result<()> {
    let mut at = worktree.to_path_buf();
    for component in Path::new(path).components() {
        at.push(component);
        ensure!(
            !fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()),
            "{path} goes through a symlink in the brain, which Wiffletree does not write"
        );
    }
    Ok(())
}

/// A lowercase ULID: 48 bits of `at` in milliseconds, then 80 random bits, in Crockford's base
/// 32. Batch ids and log entry ids both use it.
pub(crate) fn new_ulid(at: i64) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let time = (at as u128) & ((1 << 48) - 1);
    let random = u128::from_be_bytes(*uuid::Uuid::new_v4().as_bytes()) & ((1 << 80) - 1);
    let mut value = (time << 80) | random;
    let mut id = [0u8; 26];
    for slot in id.iter_mut().rev() {
        *slot = ALPHABET[(value & 31) as usize];
        value >>= 5;
    }
    String::from_utf8_lossy(&id).into_owned()
}

/// Whether Git or `gh` failed because GitHub could not be reached, so the work waits and retries.
fn offline(error: &str) -> bool {
    [
        "Could not resolve host",
        "could not resolve host",
        "error connecting to",
        "Connection refused",
        "Network is unreachable",
        "timed out",
        "no such host",
    ]
    .iter()
    .any(|marker| error.contains(marker))
}

/// Pushes `commit` to the batch branch `branch` at `url`, with no tags or submodules and never
/// forced: only a branch under `wiffletree/brain/`, and only a commit that contains the branch's
/// head on GitHub. The only way the host pushes to a brain.
fn push_branch(git: &GitTool, dir: &Path, url: &str, commit: &str, branch: &str) -> Result<()> {
    ensure!(
        branch.starts_with(BRANCH_PREFIX) && !branch.contains("..") && !branch.contains(':'),
        "The host pushes only its own batch branches to a brain"
    );
    if let Some(on) = remote_head(dir, url, branch, git)? {
        if on == commit {
            return Ok(());
        }
        ensure!(
            publish::is_ancestor(dir, &on, commit),
            "{branch} moved on GitHub since this machine pushed it"
        );
    }
    let refspec = format!("{commit}:refs/heads/{branch}");
    let args = ["push", "--no-follow-tags", "--recurse-submodules=no"];
    run_at(git, dir, url, &[&args[..], &[url, &refspec]].concat())?;
    Ok(())
}

/// Deletes a batch branch the host pushed, as discarding its batch does.
fn delete_branch(git: &GitTool, dir: &Path, url: &str, branch: &str) -> Result<()> {
    ensure!(
        branch.starts_with(BRANCH_PREFIX),
        "The host deletes only its own batch branches"
    );
    let reference = format!("refs/heads/{branch}");
    run_at(
        git,
        dir,
        url,
        &["push", "--no-follow-tags", url, "--delete", &reference],
    )?;
    Ok(())
}

/// What the host read of `wiffletree-brain.json` at `commit`, which is always `main`'s head.
fn read_config(checkout: &Path, commit: &str) -> Result<BrainConfigReading> {
    let listed = runtime::git_output(
        checkout,
        &["ls-tree", "--name-only", commit, "--", BrainConfig::FILE],
    )?;
    if listed.trim().is_empty() {
        return Ok(BrainConfigReading::Missing);
    }
    let object = format!("{commit}:{}", BrainConfig::FILE);
    let bytes = runtime::git_bytes(checkout, &["cat-file", "blob", &object], b"")?;
    Ok(parse_config(&bytes))
}

/// Validates the file's JSON, version and brain folder. Its reasons name a place in the file,
/// never a value from it.
fn parse_config(bytes: &[u8]) -> BrainConfigReading {
    let invalid = |reason: String| BrainConfigReading::Invalid { reason };
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(e) => {
            return invalid(format!(
                "{} is not valid JSON (line {}, column {})",
                BrainConfig::FILE,
                e.line(),
                e.column()
            ));
        }
    };
    let Some(version) = value["version"].as_u64().filter(|v| *v > 0) else {
        return invalid(format!("{} names no version", BrainConfig::FILE));
    };
    if version > BrainConfig::VERSION {
        return BrainConfigReading::Newer { version };
    }
    let config: BrainConfig = match serde_json::from_value(value) {
        Ok(config) => config,
        Err(_) => {
            return invalid(format!(
                "{} has a key of the wrong type; see the brain gate's README",
                BrainConfig::FILE
            ));
        }
    };
    if !inside_repository(&config.brain_dir) {
        return invalid(format!(
            "{} names a brain folder outside the repository",
            BrainConfig::FILE
        ));
    }
    BrainConfigReading::Read {
        config: Box::new(config),
    }
}

/// The files in the checkout that differ from its `main`, which only something other than the
/// host can have changed, as the host writes only in batch worktrees.
fn changed_files(checkout: &Path) -> Result<Vec<String>> {
    let status = runtime::git_output(
        checkout,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut entries = status.split('\0').filter(|e| !e.is_empty());
    let mut files = vec![];
    while let Some(entry) = entries.next() {
        let (code, path) = entry.split_at(entry.len().min(3));
        files.push(path.to_owned());
        // A rename or copy names its source next.
        if code.contains('R') || code.contains('C') {
            entries.next();
        }
    }
    Ok(files)
}

/// One brain's pass, gathered on the host thread and run off it.
pub(crate) struct Pass {
    home: PathBuf,
    repository: String,
    checkout: PathBuf,
    push_url: Option<String>,
    granted: bool,
    login: Option<String>,
    work: Vec<Work>,
}

/// A batch the pass may push, open, watch or discard.
struct Work {
    batch: BrainBatch,
    worktree: PathBuf,
    /// The commit GitHub has on the batch branch from this host.
    pushed: Option<String>,
    /// The commit the host last made in the worktree; any other head is not the host's.
    written: Option<String>,
    /// The human chose to discard it; cleared once it is.
    discard: bool,
    /// The discard was chosen before the pass started, so the pass owns clearing it.
    discard_requested: bool,
}

/// What a pass found and did.
pub(crate) struct Passed {
    repository: String,
    /// The pass read GitHub's `main`.
    pub(crate) reached: bool,
    push_url: Option<String>,
    login: Option<String>,
    head: Option<String>,
    /// `None` when the pass did not get as far as checking.
    held: Option<Option<BrainHold>>,
    config: Option<BrainConfigReading>,
    /// The gate checks `main` does not require, when the pass could write and read them.
    missing_checks: Option<Vec<String>>,
    /// The checks `main` requires, read alongside them.
    required_checks: Option<Vec<String>>,
    problem: Option<String>,
    work: Vec<Work>,
}

/// Runs a brain's pass: clones the checkout if needed, fetches `main` and fast-forwards to it
/// unless something else changed the checkout, reads the config on `main`, then, when the human
/// granted the brain and writing is not held, pushes, opens and sets to auto-merge each batch
/// and discards the ones the human discarded, and reads every open batch PR's state and checks.
pub(crate) fn pass(mut pass: Pass, tools: &Tools) -> Passed {
    let mut passed = Passed {
        repository: pass.repository.clone(),
        reached: false,
        push_url: pass.push_url.clone(),
        login: pass.login.clone(),
        head: None,
        held: None,
        config: None,
        missing_checks: None,
        required_checks: None,
        problem: None,
        work: vec![],
    };
    let result = run_pass(&mut pass, &mut passed, tools);
    passed.work = pass.work;
    match result {
        Ok(()) => passed.reached = true,
        Err(error) => passed.problem = Some(format!("{error:#}")),
    }
    passed
}

fn run_pass(pass: &mut Pass, passed: &mut Passed, tools: &Tools) -> Result<()> {
    let git = &tools.git;
    let url = match &pass.push_url {
        Some(url) => url.clone(),
        None => {
            let protocol = github_access::gh_protocol(tools, &pass.home);
            let (_, _, urls) = github_access::candidates(&pass.repository, &protocol)?;
            github_access::first_readable(&pass.home, urls, MAIN, git)?.0
        }
    };
    passed.push_url = Some(url.clone());
    let checkout = &pass.checkout;
    if !checkout.join(".git").exists() {
        clone(&pass.home, checkout, &url, &pass.repository, git)?;
    }
    ensure!(
        origin_repository(checkout).is_some_and(|o| o.eq_ignore_ascii_case(&pass.repository)),
        "{} holds a clone of another repository than {}. Move it aside, then Sync now.",
        checkout.display(),
        pass.repository
    );
    fs::write(
        checkout.join(".git").join(MARKER),
        "Wiffletree's own checkout of a brain: it only fast-forwards main here and writes on batch branches.\n",
    )?;
    let changed = changed_files(checkout)?;
    let branch = format!("refs/heads/{MAIN}");
    let fetch = ["fetch", "--quiet", "--no-tags", "--recurse-submodules=no"];
    run_at(
        git,
        checkout,
        &url,
        &[&fetch[..], &[&url, &branch]].concat(),
    )?;
    let fetched = runtime::git_output(checkout, &["rev-parse", "FETCH_HEAD"])?
        .trim()
        .to_owned();
    let held = if !changed.is_empty() {
        Some(BrainHold::ChangedFiles { files: changed })
    } else if !publish::is_ancestor(checkout, "HEAD", &fetched) {
        Some(BrainHold::NotFastForward)
    } else {
        runtime::git_output(checkout, &["merge", "--ff-only", "--quiet", &fetched])?;
        None
    };
    let writing = pass.granted && held.is_none();
    // Set up names the human as the code owner, and batch branches carry their login.
    if passed.login.is_none() {
        passed.login = spaces::gh_login(tools, &pass.home);
    }
    passed.held = Some(held);
    passed.head = Some(fetched.clone());
    passed.config = Some(read_config(checkout, &fetched)?);
    let required = required_checks(tools, &pass.home, &pass.repository)?;
    let missing: Vec<String> = (GATE_CHECKS.iter())
        .filter(|check| !required.iter().any(|r| r == *check))
        .map(|check| check.to_string())
        .collect();
    passed.missing_checks = Some(missing.clone());
    passed.required_checks = Some(required.clone());
    let gate = Gate {
        required: &required,
        missing: &missing,
    };
    let place: Place = (&pass.home, &pass.checkout, &pass.repository, &url);
    for work in &mut pass.work {
        match work_on(place, work, tools, (&mut passed.login, gate), writing) {
            Ok(()) => {}
            // Out of reach: the whole pass waits and backs off.
            Err(error) if offline(&format!("{error:#}")) => return Err(error),
            // Only this batch fails; the brain's other batches go on.
            Err(error) => {
                work.batch.problem = Some(BrainProblem::Error {
                    reason: capped(&format!("{error:#}")),
                })
            }
        }
    }
    Ok(())
}

/// What the brain's `main` requires of a PR, and the gate checks among them it lacks.
#[derive(Clone, Copy)]
struct Gate<'a> {
    required: &'a [String],
    missing: &'a [String],
}

/// The home folder, checkout, repository and URL a batch's work runs with.
type Place<'a> = (&'a Path, &'a Path, &'a str, &'a str);

/// Watches one batch, and when the host may write, discards or publishes it.
fn work_on(
    (home, checkout, repository, url): Place,
    work: &mut Work,
    tools: &Tools,
    (login, gate): (&mut Option<String>, Gate),
    writing: bool,
) -> Result<()> {
    if work.batch.pull_request.is_some() && !work.batch.is_finished() {
        watch(tools, home, repository, &mut work.batch, gate)?;
    }
    if !writing {
        return Ok(());
    }
    // A discard cut short after closing the PR still deletes the branch.
    if work.discard {
        return discard(tools, (home, checkout), url, repository, work);
    }
    if work.batch.is_finished() {
        return Ok(());
    }
    if publish_batch((home, repository, url), work, tools, (login, gate.missing))? {
        watch(tools, home, repository, &mut work.batch, gate)?;
    }
    Ok(())
}

/// Git's or `gh`'s reason, short enough for an Inbox item.
fn capped(reason: &str) -> String {
    const MAX: usize = 300;
    match reason.char_indices().nth(MAX) {
        Some((at, _)) => format!("{}…", &reason[..at]),
        None => reason.to_owned(),
    }
}

/// Clones the brain's `main` into the host's checkout and names GitHub's URL as `origin`.
fn clone(home: &Path, checkout: &Path, url: &str, repository: &str, git: &GitTool) -> Result<()> {
    fs::create_dir_all(checkout.parent().context("A checkout is in a folder")?)?;
    let into = checkout
        .to_str()
        .context("The data folder's path is not UTF-8")?;
    let args = ["clone", "--quiet", "--no-tags", "--single-branch"];
    run_at(
        git,
        home,
        url,
        &[&args[..], &["--branch", MAIN, url, into]].concat(),
    )?;
    repos::name_origin(checkout, repository)
}

/// Pushes the batch's commits, opens its PR and turns on auto-merge, each when not done yet.
/// `false` when the batch's worktree changed outside Wiffletree or GitHub refused the push, so
/// the batch failed and has nothing new to watch. Auto-merge waits while `missing` names a gate
/// check `main` does not require, and is never turned on for Set up, which waits for the code
/// owner's review.
fn publish_batch(
    (home, repository, url): (&Path, &str, &str),
    work: &mut Work,
    tools: &Tools,
    (login, missing): (&mut Option<String>, &[String]),
) -> Result<bool> {
    let login = match login {
        Some(login) => login.clone(),
        None => login
            .insert(
                spaces::gh_login(tools, home)
                    .context("gh is not signed in to GitHub. Run gh auth login, then Sync now.")?,
            )
            .clone(),
    };
    let batch = &mut work.batch;
    let id = batch.id.clone();
    let branch = (batch.branch)
        .get_or_insert_with(|| format!("{BRANCH_PREFIX}{login}/{id}"))
        .clone();
    let head = runtime::git_output(&work.worktree, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    if work.written.as_deref() != Some(&head) || !changed_files(&work.worktree)?.is_empty() {
        batch.stage = BrainStage::Failed;
        batch.problem = Some(BrainProblem::ChangedOutside);
        return Ok(false);
    }
    if work.pushed.as_deref() != Some(&head) {
        match push_branch(&tools.git, &work.worktree, url, &head, &branch) {
            Ok(()) => work.pushed = Some(head),
            Err(error) if offline(&format!("{error:#}")) => return Err(error),
            Err(error) => {
                batch.stage = BrainStage::Failed;
                batch.problem = Some(BrainProblem::PushRefused {
                    reason: capped(&format!("{error:#}")),
                });
                return Ok(false);
            }
        }
    }
    let number = match &batch.pull_request {
        Some(pr) => pr.number,
        None => {
            let pr = open_pull_request(tools, home, repository, batch, &branch)?;
            let number = pr.number;
            batch.pull_request = Some(pr);
            batch.stage = BrainStage::PrOpen;
            number
        }
    };
    let waiting = batch.pull_request.as_ref().is_some_and(|pr| !pr.auto_merge);
    if batch.kind == BrainBatchKind::SetUp {
        batch.problem = Some(BrainProblem::AwaitingReview);
    } else if waiting && !missing.is_empty() {
        batch.problem = Some(BrainProblem::GateNotRequired {
            missing: missing.to_vec(),
        });
    } else if waiting {
        let number = number.to_string();
        let args = [
            "pr", "merge", &number, "--repo", repository, "--auto", "--squash",
        ];
        let answer = (tools.gh)(home, &args)?;
        match answer.status.success() {
            true => {
                if let Some(pr) = &mut batch.pull_request {
                    pr.auto_merge = true;
                }
            }
            false if offline(&answer.error) => {
                bail!("GitHub could not be reached: {}", answer.error)
            }
            false => batch.problem = Some(BrainProblem::AutoMergeRefused),
        }
    }
    Ok(true)
}

/// The batch branch's PR: the one already open from an earlier pass cut short, or a new one into
/// `main`. Its body lists the files and never their content.
fn open_pull_request(
    tools: &Tools,
    home: &Path,
    repository: &str,
    batch: &BrainBatch,
    branch: &str,
) -> Result<BrainPullRequest> {
    let gh = |args: &[&str]| -> Result<String> {
        let answer = (tools.gh)(home, args)?;
        ensure!(answer.status.success(), "gh failed: {}", answer.error);
        Ok(answer.output)
    };
    let listed = gh(&[
        "pr",
        "list",
        "--repo",
        repository,
        "--head",
        branch,
        "--state",
        "all",
        "--json",
        "number,url",
        "--limit",
        "1",
    ])?;
    let listed: Value = serde_json::from_str(&listed)?;
    if let Some(pr) = listed.as_array().and_then(|prs| prs.first()) {
        return Ok(BrainPullRequest {
            number: pr["number"]
                .as_u64()
                .context("gh listed a PR with no number")?,
            url: pr["url"].as_str().unwrap_or_default().to_owned(),
            auto_merge: false,
        });
    }
    let files: String = (batch.files.iter())
        .map(|file| format!("- `{file}`\n"))
        .collect();
    let merges = match batch.kind {
        BrainBatchKind::SetUp => "It waits for the code owner's review; it does not auto-merge.",
        _ => "It merges by itself once the brain gate's required checks pass.",
    };
    let body = format!(
        "Opened by Wiffletree for a {} batch ({}).\n\nFiles:\n{files}\n{merges}\n",
        kind_name(batch.kind),
        batch.id
    );
    let created = gh(&[
        "pr",
        "create",
        "--repo",
        repository,
        "--base",
        MAIN,
        "--head",
        branch,
        "--title",
        &batch.title,
        "--body",
        &body,
    ])?;
    let url = created.lines().last().unwrap_or_default().trim().to_owned();
    let number = (url.rsplit('/').next())
        .and_then(|n| n.parse().ok())
        .context("gh named no pull request")?;
    Ok(BrainPullRequest {
        number,
        url,
        auto_merge: false,
    })
}

fn kind_name(kind: BrainBatchKind) -> &'static str {
    match kind {
        BrainBatchKind::Log => "log",
        BrainBatchKind::Compile => "compile",
        BrainBatchKind::Config => "config",
        BrainBatchKind::SetUp => "set-up",
    }
}

/// Reads the batch PR's state and its head's checks with read-only `gh api` GETs, keeping each
/// check's name and conclusion and its annotations' files and lines, and from them the batch's
/// stage and problem, judged on the checks `main` requires.
fn watch(
    tools: &Tools,
    home: &Path,
    repository: &str,
    batch: &mut BrainBatch,
    gate: Gate,
) -> Result<()> {
    let Some(record) = &mut batch.pull_request else {
        return Ok(());
    };
    let pr = gh_json(
        tools,
        home,
        &format!("repos/{repository}/pulls/{}", record.number),
    )?;
    if pr["merged"] == true {
        batch.stage = BrainStage::Merged;
        batch.problem = None;
        return Ok(());
    }
    if pr["state"] == "closed" {
        batch.stage = BrainStage::Discarded;
        batch.problem = None;
        return Ok(());
    }
    record.auto_merge |= !pr["auto_merge"].is_null();
    let auto_merge = record.auto_merge;
    let head = pr["head"]["sha"].as_str().context("GitHub named no head")?;
    let runs = gh_json(
        tools,
        home,
        &format!("repos/{repository}/commits/{head}/check-runs?per_page=100"),
    )?;
    // A check run again keeps its name; the newest run counts.
    let mut latest: BTreeMap<String, &Value> = BTreeMap::new();
    for run in runs["check_runs"].as_array().into_iter().flatten() {
        let Some(name) = run["name"].as_str() else {
            continue;
        };
        let newer = latest
            .get(name)
            .is_none_or(|kept| run["id"].as_u64() > kept["id"].as_u64());
        if newer {
            latest.insert(name.to_owned(), run);
        }
    }
    let mut checks = vec![];
    for (name, run) in latest {
        let conclusion = match (run["status"].as_str(), run["conclusion"].as_str()) {
            (Some("completed"), Some("success")) => BrainCheckConclusion::Success,
            (Some("completed"), Some("neutral")) => BrainCheckConclusion::Neutral,
            (Some("completed"), Some("skipped")) => BrainCheckConclusion::Skipped,
            (Some("completed"), _) => BrainCheckConclusion::Failure,
            _ => BrainCheckConclusion::Pending,
        };
        let annotated = run["output"]["annotations_count"].as_u64().unwrap_or(0) > 0;
        let annotations = match (conclusion, run["id"].as_u64()) {
            (BrainCheckConclusion::Failure, Some(id)) if annotated => {
                let endpoint =
                    format!("repos/{repository}/check-runs/{id}/annotations?per_page=50");
                (gh_json(tools, home, &endpoint)?
                    .as_array()
                    .into_iter()
                    .flatten())
                .filter_map(|a| {
                    Some(BrainAnnotation {
                        path: a["path"].as_str()?.to_owned(),
                        line: a["start_line"].as_u64()?,
                    })
                })
                .collect()
            }
            _ => vec![],
        };
        checks.push(BrainCheck {
            name,
            conclusion,
            annotations,
        });
    }
    // Only the checks main requires decide; another workflow failing does not hold the batch.
    let required = |c: &&BrainCheck| gate.required.contains(&c.name);
    let failed: Vec<String> = (checks.iter().filter(required))
        .filter(|c| c.conclusion == BrainCheckConclusion::Failure)
        .map(|c| c.name.clone())
        .collect();
    let passed = |name: &String| {
        checks.iter().any(|c| {
            c.name == *name
                && matches!(
                    c.conclusion,
                    BrainCheckConclusion::Success
                        | BrainCheckConclusion::Neutral
                        | BrainCheckConclusion::Skipped
                )
        })
    };
    let settled = !gate.required.is_empty() && gate.required.iter().all(passed);
    let refused = (batch.problem.take()).filter(|p| *p == BrainProblem::AutoMergeRefused);
    let waiting = match auto_merge {
        true => BrainStage::Checks,
        false => BrainStage::PrOpen,
    };
    (batch.stage, batch.problem) = if !failed.is_empty() {
        (
            BrainStage::Failed,
            Some(BrainProblem::ChecksFailed { checks: failed }),
        )
    } else if pr["mergeable_state"] == "dirty" {
        (BrainStage::Failed, Some(BrainProblem::Conflict))
    } else if batch.kind == BrainBatchKind::SetUp {
        (BrainStage::PrOpen, Some(BrainProblem::AwaitingReview))
    } else if !gate.missing.is_empty() {
        // Even with auto-merge on, a gate check main does not require keeps the item open.
        let missing = gate.missing.to_vec();
        (waiting, Some(BrainProblem::GateNotRequired { missing }))
    } else if !auto_merge {
        (BrainStage::PrOpen, refused)
    } else if settled {
        (BrainStage::AutoMerging, None)
    } else {
        (BrainStage::Checks, None)
    };
    batch.checks = checks;
    Ok(())
}

/// Closes the batch's PR unless it is closed already, and deletes the branch the host pushed,
/// unless GitHub merged it first. Git runs in the checkout, as the batch's worktree may be gone.
fn discard(
    tools: &Tools,
    (home, checkout): (&Path, &Path),
    url: &str,
    repository: &str,
    work: &mut Work,
) -> Result<()> {
    if work.batch.stage == BrainStage::Merged {
        work.discard = false;
        return Ok(());
    }
    if let Some(pr) =
        (work.batch.pull_request.as_ref()).filter(|_| work.batch.stage != BrainStage::Discarded)
    {
        let number = pr.number.to_string();
        let answer = (tools.gh)(home, &["pr", "close", &number, "--repo", repository])?;
        ensure!(
            answer.status.success() || answer.error.contains("already closed"),
            "gh could not close PR #{number}: {}",
            answer.error
        );
    }
    if let (Some(branch), Some(_)) = (&work.batch.branch, &work.pushed)
        && remote_head(checkout, url, branch, &tools.git)?.is_some()
    {
        delete_branch(&tools.git, checkout, url, branch)?;
    }
    work.batch.stage = BrainStage::Discarded;
    work.batch.problem = None;
    work.discard = false;
    Ok(())
}

/// The Inbox prompt for a batch's problem. It names the brain, the PR, the check or reason and
/// each reported file and line, and never a matched value.
fn item_prompt(batch: &BrainBatch, problem: &BrainProblem) -> String {
    let pr = match &batch.pull_request {
        Some(pr) => format!("PR #{}", pr.number),
        None => "its batch".into(),
    };
    let about = format!("The brain {}'s {pr} ({})", batch.repository, batch.title);
    match problem {
        BrainProblem::ChecksFailed { checks } => {
            let places: Vec<String> = (batch.checks.iter())
                .filter(|c| checks.contains(&c.name))
                .flat_map(|c| c.annotations.iter())
                .take(20)
                .map(|a| format!("{} line {}", a.path, a.line))
                .collect();
            let at = match places.is_empty() {
                true => String::new(),
                false => format!(" at {}", places.join(", ")),
            };
            format!(
                "{about} failed {}{at}. Auto-merge waits until every required check passes. On the Brains page, redact the line, mark it not sensitive with a reason, or discard the batch.",
                checks.join(", ")
            )
        }
        BrainProblem::PushRefused { reason } => format!(
            "GitHub refused the push of {about}: {reason}. Auto-merge waits. Discard the batch, or settle the branch on GitHub and Sync now."
        ),
        BrainProblem::Conflict => format!(
            "{about} conflicts with main. Auto-merge waits. Discard the batch so its entries can go in a new one."
        ),
        BrainProblem::AutoMergeRefused => format!(
            "GitHub refused auto-merge for {about}, so it stays open. Turn on Allow auto-merge for {} and protect main with the brain gate's required checks ({}), as the Repository settings in the gate's README say, then Sync now.",
            batch.repository,
            GATE_CHECKS.join(", ")
        ),
        BrainProblem::GateNotRequired { missing } => format!(
            "{about} stays open without auto-merge: main of {} does not require the brain gate's {} check{}. Protect main with all seven, as the Repository settings in the gate's README say, then Sync now.",
            batch.repository,
            missing.join(", "),
            if missing.len() == 1 { "" } else { "s" }
        ),
        // Unreached: Set up batches come from the Brains page, which shows the wait itself.
        BrainProblem::AwaitingReview => format!(
            "{about} waits for your review: CODEOWNERS covers the brain gate's files, so it does not auto-merge. Review and merge it on GitHub; the host sees the merge at its next pass."
        ),
        BrainProblem::Error { reason } => format!(
            "{about} could not move on: {reason}. Auto-merge waits; the host tries again at the next pass. Discard the batch if it keeps failing."
        ),
        BrainProblem::ChangedOutside => format!(
            "Something other than Wiffletree changed {about}'s worktree, so the host does not push it. Discard the batch so its entries can go in a new one."
        ),
    }
}

/// The checks `main` requires, from its branch protection and rulesets, read with GETs. With no
/// protection there are none.
fn required_checks(tools: &Tools, home: &Path, repository: &str) -> Result<Vec<String>> {
    let branch = gh_json(tools, home, &format!("repos/{repository}/branches/{MAIN}"))?;
    let protection = &branch["protection"]["required_status_checks"];
    let mut required: Vec<String> = (protection["contexts"].as_array().into_iter().flatten())
        .chain(
            (protection["checks"].as_array().into_iter().flatten()).map(|check| &check["context"]),
        )
        .filter_map(|context| context.as_str().map(str::to_owned))
        .collect();
    // Rulesets may require them instead; an older GitHub without them has none.
    if let Ok(rules) = gh_json(
        tools,
        home,
        &format!("repos/{repository}/rules/branches/{MAIN}"),
    ) {
        for rule in rules.as_array().into_iter().flatten() {
            let checks = &rule["parameters"]["required_status_checks"];
            required.extend(
                (checks.as_array().into_iter().flatten())
                    .filter_map(|check| check["context"].as_str().map(str::to_owned)),
            );
        }
    }
    // Protection and rulesets may require the same check; it counts once.
    required.sort_unstable();
    required.dedup();
    Ok(required)
}

/// What a batch's Inbox item says, so a change in it replaces the item.
fn item_key(batch: &BrainBatch) -> Option<String> {
    let problem = batch.problem.as_ref()?;
    serde_json::to_string(&(problem, &batch.checks)).ok()
}

/// Keeps a field's name and replaces its value; a line with no field keeps its indent.
fn redact_line(line: &str, rule: &str) -> String {
    let removed = format!("[removed: {rule}]");
    match line.find(": ") {
        Some(at) => format!("{}: {removed}", &line[..at]),
        None => {
            let indent = line.len() - line.trim_start().len();
            format!("{}{removed}", &line[..indent])
        }
    }
}

/// The `CODEOWNERS` file to write with the lines Set up adds for `login`: the existing file on
/// `main` with only the missing lines appended, or a new `.github/CODEOWNERS`. `None` when every
/// line is there.
fn codeowners(checkout: &Path, head: &str, login: &str) -> Result<Option<(String, Vec<u8>)>> {
    let existing = CODEOWNERS.iter().find_map(|path| {
        let object = format!("{head}:{path}");
        let bytes = runtime::git_bytes(checkout, &["cat-file", "blob", &object], b"").ok()?;
        Some((path.to_string(), String::from_utf8(bytes).ok()?))
    });
    let (path, mut content) = existing.unwrap_or_else(|| (CODEOWNERS[0].to_owned(), String::new()));
    let listed = |pattern: &str| {
        (content.lines()).any(|line| line.split_whitespace().next() == Some(pattern))
    };
    let missing: Vec<&str> = OWNED.into_iter().filter(|p| !listed(p)).collect();
    if missing.is_empty() {
        return Ok(None);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if content.is_empty() {
        content.push_str("# The brain gate's files change only with the code owner's review.\n");
    }
    for pattern in missing {
        content.push_str(&format!("{pattern:<28} @{login}\n"));
    }
    Ok(Some((path, content.into_bytes())))
}

/// The gate's caller workflow, pinned to `release`, as the gate's README gives it.
fn caller(release: GateRelease) -> String {
    let GateRelease { tag, commit } = release;
    let mut jobs = String::new();
    for check in GATE_CHECKS {
        jobs.push_str(&format!(
            "\n  {check}:\n    runs-on: ubuntu-latest\n    steps:\n"
        ));
        if check != "gate-tests" {
            jobs.push_str("      - uses: actions/checkout@v5\n");
            if ["secret-scan", "sensitive-info", "path-ownership"].contains(&check) {
                jobs.push_str("        with:\n          fetch-depth: 0\n");
            }
        }
        jobs.push_str(&format!(
            "      - uses: Vyttle-LLC/wiffletree/gate@{commit} # {tag}\n        with:\n          check: {check}\n"
        ));
    }
    format!(
        "# The brain gate (Vyttle-LLC/wiffletree/gate). Each job is its own required check.\nname: Brain gate\n\non:\n  pull_request:\n    branches: [main]\n    types: [opened, synchronize, reopened]\n\npermissions:\n  contents: read\n\nconcurrency:\n  group: brain-gate-${{{{ github.event.pull_request.number }}}}\n  cancel-in-progress: true\n\njobs:{jobs}"
    )
}

const ALLOWLIST_TEXT: &str = "# The brain gate's allow-list (see the gate's README, Suppressing a false positive).\n# [[value]] exempts one matched text for one rule; [[path]] exempts files by glob.\n# Every entry needs a reason.\n";

fn pretty(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

impl Host {
    fn upgrade_local_brains(&mut self) -> Result<()> {
        for SpaceRecord {
            mut space,
            mut local,
        } in self.spaces()?
        {
            let Some(SpaceBrain {
                repository: BrainRepository::Local { extra },
                ..
            }) = &space.brain
            else {
                continue;
            };
            let extra = extra.clone();
            let Some(path) = local.brain_path.take() else {
                continue;
            };
            let named = origin_repository(Path::new(&path))
                .filter(|named| self.refuse_second_use(named, storage::Use::Brain).is_ok());
            match named.as_deref().and_then(|named| named.split_once('/')) {
                Some((owner, name)) => {
                    if let Some(brain) = &mut space.brain {
                        brain.repository = BrainRepository::GitHub {
                            owner: owner.into(),
                            name: name.into(),
                            extra,
                        };
                    }
                }
                None => {
                    space.brain = None;
                    local.notes.push(format!(
                        "Its brain folder {path} has no GitHub origin, so this Space has no brain now. Name a brain repository in the Space's settings."
                    ));
                    local.brain_without_origin = Some(path);
                }
            }
            self.db.execute(
                "UPDATE spaces SET data=?2,local=?3 WHERE id=?1",
                params![space.id.as_str(), encode(&space)?, encode(&local)?],
            )?;
            self.write_space_file(&space)?;
        }
        Ok(())
    }

    /// Each GitHub brain the Spaces name, as `owner/name` the first Space names it, with the
    /// Spaces that name it.
    fn named_brains(&self) -> Result<Vec<(String, Vec<SpaceId>)>> {
        let mut named: Vec<(String, Vec<SpaceId>)> = vec![];
        for record in self.spaces()? {
            let Some(SpaceBrain {
                repository: BrainRepository::GitHub { owner, name, .. },
                ..
            }) = &record.space.brain
            else {
                continue;
            };
            let identity = format!("{owner}/{name}");
            match (named.iter_mut()).find(|(n, _)| n.eq_ignore_ascii_case(&identity)) {
                Some((_, spaces)) => spaces.push(record.space.id),
                None => named.push((identity, vec![record.space.id])),
            }
        }
        Ok(named)
    }

    /// Registers each brain a Space names that is not registered yet, waiting for the grant. A
    /// brain no Space names stays until the human confirms forgetting it.
    pub(crate) fn refresh_brain_registry(&self) -> Result<()> {
        for (repository, _) in self.named_brains()? {
            let registered: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM brains WHERE repository=?1)",
                [&repository],
                |r| r.get(0),
            )?;
            if registered {
                continue;
            }
            let brain = Brain {
                repository: repository.clone(),
                spaces: vec![],
                granted: false,
                checkout: String::new(),
                push_url: None,
                head: None,
                synced_at: None,
                problem: None,
                held: None,
                config: BrainConfigReading::Unread,
                missing_checks: vec![],
                required_checks: vec![],
                legacy_pointer: false,
                last_pull_request: None,
            };
            self.db.execute(
                "INSERT INTO brains(repository,data) VALUES (?1,?2)",
                params![repository, encode(&brain)?],
            )?;
        }
        Ok(())
    }

    /// Every registered brain, with the Spaces that name it, its checkout and its last PR.
    pub fn brains(&self) -> Result<Vec<Brain>> {
        self.refresh_brain_registry()?;
        let stored: Vec<Brain> = self.list_data("SELECT data FROM brains ORDER BY rowid", [])?;
        let named = self.named_brains()?;
        stored
            .into_iter()
            .map(|mut brain| {
                brain.spaces = (named.iter())
                    .find(|(n, _)| n.eq_ignore_ascii_case(&brain.repository))
                    .map(|(_, spaces)| spaces.clone())
                    .unwrap_or_default();
                let checkout = repos::clone_folder(&self.home, &brain.repository);
                brain.checkout = checkout.to_string_lossy().into_owned();
                brain.last_pull_request = self.last_pull_request(&brain.repository)?;
                Ok(brain)
            })
            .collect()
    }

    /// The brain's most recently opened batch PR, merged or not, however old.
    fn last_pull_request(&self, repository: &str) -> Result<Option<BrainLastPullRequest>> {
        let last: Option<BrainBatch> = self
            .db
            .query_row(
                "SELECT data FROM brain_batches WHERE repository=?1 AND json_extract(data,'$.pull_request') IS NOT NULL ORDER BY json_extract(data,'$.pull_request.number') DESC LIMIT 1",
                [repository],
                |r| decode(r, 0),
            )
            .optional()?;
        Ok(last.and_then(|batch| {
            Some(BrainLastPullRequest {
                number: batch.pull_request?.number,
                kind: batch.kind,
                stage: batch.stage,
            })
        }))
    }

    pub fn brain(&self, repository: &str) -> Result<Brain> {
        (self.brains()?.into_iter())
            .find(|b| b.repository.eq_ignore_ascii_case(repository))
            .with_context(|| format!("No Space names the brain {repository}"))
    }

    fn save_brain(&self, brain: &Brain) -> Result<()> {
        self.db.execute(
            "UPDATE brains SET data=?2 WHERE repository=?1",
            params![brain.repository, encode(brain)?],
        )?;
        Ok(())
    }

    fn legacy_pointer_target(&self) -> Option<PathBuf> {
        let named = fs::read_to_string(&self.legacy_pointer).ok()?;
        let named = named.trim();
        let path = match named.strip_prefix("~/") {
            Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
            None => PathBuf::from(named),
        };
        Some(fs::canonicalize(&path).unwrap_or(path))
    }

    /// The brains page: every registered brain, with the legacy pointer's warning when it names
    /// the human's clone of one, the batches not yet merged or merged in the last day, and this
    /// host as a compiler.
    pub fn brains_page(&self) -> Result<BrainsPage> {
        let shown_since = now() - SHOWN_AFTER_MERGE_MS;
        let batches = (self.brain_batches(None)?.into_iter())
            .filter(|b| !b.is_finished() || b.updated_at >= shown_since)
            .collect();
        let pointer = self.legacy_pointer_target();
        let named = pointer.as_deref().and_then(origin_repository);
        let mut brains = self.brains()?;
        for brain in &mut brains {
            brain.legacy_pointer = pointer.as_deref() != Some(Path::new(&brain.checkout))
                && named
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&brain.repository));
        }
        let this_host = BrainCompiler {
            host_id: self.host_id()?,
            name: sync::machine_name(),
            extra: Extra::new(),
        };
        Ok(BrainsPage {
            brains,
            batches,
            this_host: Some(this_host),
        })
    }

    /// The human's grant: the host may push batch branches to the brain, open PRs from them into
    /// `main` and turn on auto-merge for them.
    pub fn confirm_brain(&mut self, repository: &str) -> Result<Brain> {
        let mut brain = self.brain(repository)?;
        ensure!(
            !brain.spaces.is_empty(),
            "No Space names {repository}; name it in a Space's settings first"
        );
        self.refuse_second_use(&brain.repository, storage::Use::Brain)?;
        brain.granted = true;
        self.save_brain(&brain)?;
        self.brains_now = true;
        Ok(brain)
    }

    /// Forgets a brain no Space names any longer: its batches here, their worktrees and the
    /// host's checkout. Open PRs stay on GitHub for the human to close.
    pub fn forget_brain(&mut self, repository: &str) -> Result<()> {
        let brain = self.brain(repository)?;
        ensure!(
            brain.spaces.is_empty(),
            "A Space still names {repository}; remove it from its Spaces first"
        );
        for batch in self.brain_batches(Some(&brain.repository))? {
            let _ = fs::remove_dir_all(self.batch_folder(&batch.id));
        }
        self.db.execute(
            "DELETE FROM brain_batches WHERE repository=?1",
            [&brain.repository],
        )?;
        self.db.execute(
            "DELETE FROM brains WHERE repository=?1",
            [&brain.repository],
        )?;
        let checkout = PathBuf::from(&brain.checkout);
        // Only the host's own, marked checkout is ever deleted.
        if checkout.join(".git").join(MARKER).exists() {
            fs::remove_dir_all(&checkout)?;
        }
        Ok(())
    }

    /// The brain of a project, a thread or a job run: its Space's, and in no other way.
    pub fn owner_brain(&self, owner: &SessionOwner) -> Result<Option<SpaceBrain>> {
        Ok(match owner {
            SessionOwner::Project(project) => {
                self.space(&self.project(project)?.space)?.space.brain
            }
            SessionOwner::Space(space) => self.space(space)?.space.brain,
            SessionOwner::Brain(identity) => (self.spaces()?.into_iter())
                .filter_map(|record| record.space.brain)
                .find(|brain| match &brain.repository {
                    BrainRepository::GitHub { owner, name, .. } => {
                        format!("{owner}/{name}").eq_ignore_ascii_case(identity)
                    }
                    BrainRepository::Local { .. } => false,
                }),
        })
    }

    /// The brain to pre-fill for a Space with these code repositories: the one another Space
    /// names for their GitHub owner, preferring a brain the owner holds, or none for the human's
    /// own or another user's repositories. It only suggests.
    pub fn suggest_space_brain(&self, repositories: &[String]) -> Result<Option<String>> {
        let mut owners: Vec<(String, usize)> = vec![];
        for identity in repositories {
            validate_repository_identity(identity)?;
            let owner = repository_owner(identity);
            match (owners.iter_mut()).find(|(o, _)| o.eq_ignore_ascii_case(owner)) {
                Some((_, count)) => *count += 1,
                None => owners.push((owner.to_owned(), 1)),
            }
        }
        let highest = owners.iter().map(|(_, count)| *count).max();
        let Some((owner, _)) = owners.into_iter().find(|(_, c)| Some(*c) == highest) else {
            return Ok(None);
        };
        if self.personal_owner(&owner)? {
            return Ok(None);
        }
        let spaces = self.spaces()?;
        let brain_of = |space: &Space| match &space.brain {
            Some(SpaceBrain {
                repository: BrainRepository::GitHub { owner, name, .. },
                ..
            }) => Some((owner.clone(), format!("{owner}/{name}"))),
            _ => None,
        };
        let held_by_owner = (spaces.iter())
            .filter_map(|r| brain_of(&r.space))
            .find(|(held, _)| held.eq_ignore_ascii_case(&owner));
        let by_repositories = (spaces.iter())
            .filter(|r| {
                (r.space.repositories.iter())
                    .any(|i| repository_owner(i).eq_ignore_ascii_case(&owner))
            })
            .find_map(|r| brain_of(&r.space));
        Ok(held_by_owner.or(by_repositories).map(|(_, brain)| brain))
    }

    /// The human's own login, or a user's repositories on this Mac: personal, with no brain.
    fn personal_owner(&self, owner: &str) -> Result<bool> {
        if (self.gh_login.as_deref()).is_some_and(|login| login.eq_ignore_ascii_case(owner)) {
            return Ok(true);
        }
        Ok(self.repositories()?.iter().any(|r| {
            r.github.as_ref().is_some_and(|github| {
                github.owner.eq_ignore_ascii_case(owner)
                    && github.facts.as_ref().is_some_and(|f| !f.organization)
            })
        }))
    }

    fn batch_folder(&self, id: &str) -> PathBuf {
        self.home.join(BATCHES).join(id)
    }

    /// The batches of `repository`, or of every brain, oldest first, with their checks.
    pub fn brain_batches(&self, repository: Option<&str>) -> Result<Vec<BrainBatch>> {
        let mut batches: Vec<BrainBatch> = self.list_data(
            "SELECT data FROM brain_batches WHERE ?1 IS NULL OR repository=?1 ORDER BY rowid",
            [repository],
        )?;
        for batch in &mut batches {
            batch.checks = self.batch_checks(&batch.id)?;
        }
        Ok(batches)
    }

    pub fn brain_batch(&self, id: &str) -> Result<BrainBatch> {
        let mut batch: BrainBatch = self
            .db
            .query_row("SELECT data FROM brain_batches WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?
            .context("Brain batch not found")?;
        batch.checks = self.batch_checks(id)?;
        Ok(batch)
    }

    fn batch_checks(&self, id: &str) -> Result<Vec<BrainCheck>> {
        Ok(self
            .db
            .prepare(
                "SELECT name,conclusion,annotations FROM brain_batch_checks WHERE batch_id=?1 ORDER BY name",
            )?
            .query_map([id], |r| {
                Ok(BrainCheck {
                    name: r.get(0)?,
                    conclusion: serde_json::from_value(Value::String(r.get(1)?))
                        .unwrap_or(BrainCheckConclusion::Pending),
                    annotations: decode(r, 2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Stores a batch, its checks apart, with what the host pushed of it.
    fn save_batch(&self, batch: &BrainBatch, pushed: Option<&str>, discard: bool) -> Result<()> {
        let mut stored = batch.clone();
        stored.checks = vec![];
        stored.updated_at = now();
        self.db.execute(
            "UPDATE brain_batches SET data=?2,pushed=?3,discard=?4 WHERE id=?1",
            params![batch.id, encode(&stored)?, pushed, discard],
        )?;
        self.db.execute(
            "DELETE FROM brain_batch_checks WHERE batch_id=?1",
            [&batch.id],
        )?;
        for check in &batch.checks {
            self.db.execute(
                "INSERT INTO brain_batch_checks VALUES (?1,?2,?3,?4)",
                params![
                    batch.id,
                    check.name,
                    tag(&check.conclusion)?,
                    encode(&check.annotations)?
                ],
            )?;
        }
        Ok(())
    }

    /// Commits `files` on a new batch worktree from the brain's `main` as the host last fetched
    /// it, with the human's Git identity, and queues the batch for the next pass. The content
    /// goes only to Git, never to the store.
    pub(crate) fn queue_brain_batch(
        &mut self,
        repository: &str,
        kind: BrainBatchKind,
        source: BrainSource,
        title: &str,
        files: &[(String, Vec<u8>)],
        git: &GitTool,
    ) -> Result<BrainBatch> {
        text(title, 256)?;
        ensure!(!files.is_empty(), "A batch changes at least one file");
        let brain = self.brain(repository)?;
        ensure!(
            !brain.spaces.is_empty(),
            "No Space names {repository} any longer"
        );
        ensure!(
            brain.held.is_none(),
            "Writing to {repository} is held until its checkout is settled; see the Brains page"
        );
        let head = (brain.head.clone()).with_context(|| {
            format!("{repository} has not been fetched yet; Sync now, then try again")
        })?;
        for (path, _) in files {
            ensure!(
                inside_repository(path),
                "A batch writes only inside its repository"
            );
        }
        let checkout = PathBuf::from(&brain.checkout);
        let id = new_ulid(now());
        let worktree = self.batch_folder(&id);
        fs::create_dir_all(self.home.join(BATCHES))?;
        let into = worktree
            .to_str()
            .context("The data folder's path is not UTF-8")?;
        run(
            git,
            &checkout,
            &["worktree", "add", "--quiet", "--detach", into, &head],
        )?;
        let paths: Vec<String> = files.iter().map(|(path, _)| path.clone()).collect();
        let committed = (|| {
            for (path, content) in files {
                refuse_symlinks(&worktree, path)?;
                let file = worktree.join(path);
                fs::create_dir_all(file.parent().context("A file is in a folder")?)?;
                fs::write(file, content)?;
            }
            let added: Vec<&str> = paths.iter().map(String::as_str).collect();
            run(git, &worktree, &[&["add", "--"][..], &added].concat())?;
            ensure!(
                repos::commit_staged(&worktree, title, git)?,
                "The batch changes nothing on main"
            );
            Ok(())
        })();
        if let Err(error) = committed {
            self.remove_batch_worktree(&checkout, &id);
            return Err(error);
        }
        let at = now();
        let batch = BrainBatch {
            id,
            repository: brain.repository,
            kind,
            source,
            title: title.into(),
            files: paths,
            branch: None,
            pull_request: None,
            stage: BrainStage::Queued,
            problem: None,
            checks: vec![],
            created_at: at,
            updated_at: at,
        };
        let written = runtime::git_output(&worktree, &["rev-parse", "HEAD"])?;
        self.db.execute(
            "INSERT INTO brain_batches(id,repository,written,data) VALUES (?1,?2,?3,?4)",
            params![batch.id, batch.repository, written.trim(), encode(&batch)?],
        )?;
        self.brains_now = true;
        Ok(batch)
    }

    pub(crate) fn remove_batch_worktree(&self, checkout: &Path, id: &str) {
        let _ = fs::remove_dir_all(self.batch_folder(id));
        let _ = runtime::git_output(checkout, &["worktree", "prune"]);
    }

    /// Set up: one batch adding `wiffletree-brain.json` with this host as compiler, the gate's
    /// pinned caller, its config and allow-list, and the `CODEOWNERS` lines that make the PR wait
    /// for the code owner's review. Files already on `main` are kept.
    pub fn set_up_brain(&mut self, repository: &str, git: &GitTool) -> Result<BrainBatch> {
        let release = GATE_RELEASE;
        let brain = self.brain(repository)?;
        match &brain.config {
            BrainConfigReading::Missing => {}
            BrainConfigReading::Unread => {
                bail!("{repository} has not been fetched yet; Sync now, then try again")
            }
            _ => bail!("{repository} already has {}", BrainConfig::FILE),
        }
        let setting_up = (self.brain_batches(Some(&brain.repository))?.iter())
            .any(|b| b.kind == BrainBatchKind::SetUp && !b.is_finished());
        ensure!(!setting_up, "{repository}'s Set up PR is already open");
        let login = (self.gh_login.clone()).context(
            "Wiffletree has not read your GitHub login yet. Sign gh in, Sync now, then try again.",
        )?;
        let head = brain.head.clone().context("The brain has no main yet")?;
        let checkout = PathBuf::from(&brain.checkout);
        let folder = (self.spaces()?.into_iter())
            .filter(|r| brain.spaces.contains(&r.space.id))
            .find_map(|r| r.space.brain.map(|b| b.folder))
            .unwrap_or_else(|| SpaceBrain::DEFAULT_FOLDER.into());
        let config = json!({
            "version": BrainConfig::VERSION,
            "brain_dir": folder,
            "compiler": {"host_id": self.host_id()?, "name": sync::machine_name()},
            "gate": {"version": release.tag, "allowlist": ALLOWLIST},
        });
        let on_main = |path: &str| {
            runtime::git_answer(&checkout, &["cat-file", "-e", &format!("{head}:{path}")])
                .is_ok_and(|answer| answer.status.success())
        };
        let mut files = vec![(BrainConfig::FILE.to_owned(), pretty(&config)?)];
        if !on_main(CALLER) {
            files.push((CALLER.into(), caller(release).into_bytes()));
        }
        if !on_main(ALLOWLIST) {
            files.push((ALLOWLIST.into(), ALLOWLIST_TEXT.as_bytes().to_vec()));
        }
        files.extend(codeowners(&checkout, &head, &login)?);
        self.queue_brain_batch(
            &brain.repository,
            BrainBatchKind::SetUp,
            BrainSource::BrainsPage,
            "Set up the brain gate and wiffletree-brain.json",
            &files,
            git,
        )
    }

    /// A batch that changes only the compiler in `wiffletree-brain.json` on `main`, keeping every
    /// key this host does not know. The compiler stays the merged one until it merges.
    pub fn set_brain_compiler(
        &mut self,
        repository: &str,
        compiler: &BrainCompiler,
        git: &GitTool,
    ) -> Result<BrainBatch> {
        text(&compiler.host_id, 128)?;
        text(&compiler.name, 128)?;
        let brain = self.brain(repository)?;
        match &brain.config {
            BrainConfigReading::Read { .. } => {}
            BrainConfigReading::Missing => {
                bail!("{repository} has no {}; Set up first", BrainConfig::FILE)
            }
            BrainConfigReading::Newer { .. } => {
                bail!(
                    "A newer Wiffletree wrote {}; update to change it",
                    BrainConfig::FILE
                )
            }
            BrainConfigReading::Invalid { reason } => bail!("{reason}"),
            BrainConfigReading::Unread => {
                bail!("{repository} has not been fetched yet; Sync now, then try again")
            }
        }
        let head = brain.head.clone().context("The brain has no main yet")?;
        let object = format!("{head}:{}", BrainConfig::FILE);
        let bytes = runtime::git_bytes(
            Path::new(&brain.checkout),
            &["cat-file", "blob", &object],
            b"",
        )?;
        let mut config: Value = serde_json::from_slice(&bytes)?;
        config["compiler"] = serde_json::to_value(compiler)?;
        self.queue_brain_batch(
            &brain.repository,
            BrainBatchKind::Config,
            BrainSource::BrainsPage,
            &format!("Config: compile on {}", compiler.name),
            &[(BrainConfig::FILE.into(), pretty(&config)?)],
            git,
        )
    }

    /// The human's amendment of an unmerged batch: one commit on its branch that the next pass
    /// pushes without force, or its discard. Nothing is offered once it merged.
    pub fn amend_brain_batch(
        &mut self,
        id: &str,
        amendment: &BrainAmendment,
        git: &GitTool,
    ) -> Result<BrainBatch> {
        let mut batch = self.brain_batch(id)?;
        ensure!(
            !batch.is_finished(),
            "This batch has {}; its files no longer change",
            match batch.stage {
                BrainStage::Merged => "merged",
                _ => "been discarded",
            }
        );
        let (pushed, written): (Option<String>, Option<String>) = self.db.query_row(
            "SELECT pushed,written FROM brain_batches WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let checkout = repos::clone_folder(&self.home, &batch.repository);
        let (path, line, rule, marked) = match amendment {
            BrainAmendment::Discard => {
                if batch.pull_request.is_none() && pushed.is_none() {
                    batch.stage = BrainStage::Discarded;
                    batch.problem = None;
                    self.save_batch(&batch, None, false)?;
                    self.remove_batch_worktree(&checkout, id);
                    self.sync_item(&batch)?;
                } else {
                    self.db
                        .execute("UPDATE brain_batches SET discard=1 WHERE id=?1", [id])?;
                    self.brains_now = true;
                }
                return Ok(batch);
            }
            BrainAmendment::Redact { path, line, rule } => (path, *line, rule, None),
            BrainAmendment::NotSensitive {
                path,
                line,
                rule,
                reason,
            } => (path, *line, rule, Some(reason)),
        };
        ensure!(
            batch.is_amendable(),
            "Only a batch whose pull request is open can be amended"
        );
        ensure!(
            GATE_RULES.contains(&rule.as_str()),
            "{rule} is not one of the gate's sensitive-info rules"
        );
        ensure!(
            batch.files.contains(path),
            "{path} is not one of this batch's files"
        );
        if let Some(reason) = marked {
            text(reason, 200)?;
            ensure!(
                !reason.contains('\n') && !reason.contains("-->"),
                "A reason is one line without -->"
            );
        }
        let worktree = self.batch_folder(id);
        let head = runtime::git_output(&worktree, &["rev-parse", "HEAD"])?;
        ensure!(
            written.as_deref() == Some(head.trim()) && changed_files(&worktree)?.is_empty(),
            "Something other than Wiffletree changed this batch's worktree; discard the batch"
        );
        refuse_symlinks(&worktree, path)?;
        let file = worktree.join(path);
        let content = fs::read_to_string(&file)?;
        let mut lines: Vec<String> = content.split('\n').map(str::to_owned).collect();
        let at = usize::try_from(line)?
            .checked_sub(1)
            .filter(|at| *at < lines.len())
            .with_context(|| format!("{path} has no line {line}"))?;
        let message = match marked {
            None => {
                lines[at] = redact_line(&lines[at], rule);
                format!("Redact {path} line {line} ({rule})")
            }
            Some(reason) => {
                lines[at] = format!("{} <!-- gate:allow {rule} {} -->", lines[at], reason.trim());
                format!("Mark {path} line {line} not sensitive ({rule})")
            }
        };
        fs::write(&file, lines.join("\n"))?;
        run(git, &worktree, &["add", "--", path])?;
        ensure!(
            repos::commit_staged(&worktree, &message, git)?,
            "The amendment changes nothing"
        );
        let written = runtime::git_output(&worktree, &["rev-parse", "HEAD"])?;
        self.db.execute(
            "UPDATE brain_batches SET written=?2 WHERE id=?1",
            params![id, written.trim()],
        )?;
        self.brains_now = true;
        Ok(batch)
    }

    /// Each brain to pass with how often: every minute while it has a batch not yet merged or
    /// discarded, and every five minutes otherwise. A brain no Space names is not passed. Runs
    /// after every event, so it reads only the Spaces and the batches' stages.
    pub(crate) fn brain_cadences(&self) -> Result<Vec<(String, i64)>> {
        self.refresh_brain_registry()?;
        let active: Vec<String> = (self.db.prepare(
            "SELECT DISTINCT repository FROM brain_batches WHERE json_extract(data,'$.stage') NOT IN ('merged','discarded')",
        )?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
        Ok((self.named_brains()?.into_iter())
            .map(|(repository, _)| {
                let every = match active.iter().any(|a| a.eq_ignore_ascii_case(&repository)) {
                    true => pr_watch::ACTIVE_MS,
                    false => pr_watch::IDLE_MS,
                };
                (repository, every)
            })
            .collect())
    }

    /// What a pass of the brain needs, or `None` for one no Space names, or whose checkout the
    /// legacy pointer names, which the host refuses.
    pub(crate) fn brain_pass(&mut self, repository: &str) -> Result<Option<Pass>> {
        let mut brain = self.brain(repository)?;
        if brain.spaces.is_empty() {
            return Ok(None);
        }
        let checkout = PathBuf::from(&brain.checkout);
        let pointed = self.legacy_pointer_target();
        let canonical = fs::canonicalize(&checkout).unwrap_or_else(|_| checkout.clone());
        if pointed.as_deref() == Some(canonical.as_path()) {
            brain.problem = Some(format!(
                "The legacy pointer ~/{LEGACY_POINTER} names the host's checkout of {repository}, which the host never shares. Point it at your own clone."
            ));
            self.save_brain(&brain)?;
            return Ok(None);
        }
        type Row = (String, Option<String>, Option<String>, bool);
        let rows: Vec<Row> = self
            .db
            .prepare("SELECT id,pushed,written,discard FROM brain_batches WHERE repository=?1")?
            .query_map([&brain.repository], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut work = vec![];
        for (id, pushed, written, discard) in rows {
            let batch = self.brain_batch(&id)?;
            // A discard cut short keeps its batch in the pass until its branch is deleted.
            if !batch.is_finished() || discard {
                work.push(Work {
                    worktree: self.batch_folder(&id),
                    batch,
                    pushed,
                    written,
                    discard,
                    discard_requested: discard,
                });
            }
        }
        Ok(Some(Pass {
            home: self.home.clone(),
            repository: brain.repository,
            checkout,
            push_url: brain.push_url,
            granted: brain.granted,
            login: self.gh_login.clone(),
            work,
        }))
    }

    /// Records a pass: the brain's state, each batch's stage and checks, and its Inbox item.
    /// `true` when a batch merged, so `main` is fetched again at once.
    pub(crate) fn finish_brain_pass(&mut self, passed: Passed) -> Result<bool> {
        let Some(mut brain) = (self.brains()?.into_iter())
            .find(|b| b.repository.eq_ignore_ascii_case(&passed.repository))
        else {
            return Ok(false);
        };
        self.record_gh_login(passed.login);
        brain.push_url = passed.push_url.or(brain.push_url);
        brain.problem = passed.problem;
        if let Some(head) = passed.head {
            brain.head = Some(head);
            brain.synced_at = passed.reached.then(now).or(brain.synced_at);
        }
        if let Some(held) = passed.held {
            brain.held = held;
        }
        if let Some(config) = passed.config {
            brain.config = config;
        }
        if let Some(missing) = passed.missing_checks {
            brain.missing_checks = missing;
        }
        if let Some(required) = passed.required_checks {
            brain.required_checks = required;
        }
        self.save_brain(&brain)?;
        let checkout = PathBuf::from(&brain.checkout);
        let mut merged = false;
        for mut work in passed.work {
            let Ok(before) = self.brain_batch(&work.batch.id) else {
                continue;
            };
            // The human amended it while the pass ran, so the new commit is the host's own and
            // the next pass pushes it.
            let written: Option<String> = self.db.query_row(
                "SELECT written FROM brain_batches WHERE id=?1",
                [&work.batch.id],
                |r| r.get(0),
            )?;
            if work.batch.problem == Some(BrainProblem::ChangedOutside) && written != work.written {
                work.batch.stage = before.stage;
                work.batch.problem = before.problem.clone();
            }
            let published = work.batch.pull_request.is_some() || work.pushed.is_some();
            if before.is_finished() && !work.discard_requested {
                // The human discarded it here while the pass ran. Had the pass published it
                // meanwhile, the next pass closes its PR and deletes its branch.
                if published && !work.batch.is_finished() {
                    self.save_batch(&work.batch, work.pushed.as_deref(), true)?;
                }
                continue;
            }
            merged |= work.batch.stage == BrainStage::Merged && before.stage != BrainStage::Merged;
            // A discard chosen while the pass ran waits for the next one.
            let discard = match work.discard_requested {
                true => work.discard,
                false => self.db.query_row(
                    "SELECT discard FROM brain_batches WHERE id=?1",
                    [&work.batch.id],
                    |r| r.get(0),
                )?,
            };
            self.save_batch(&work.batch, work.pushed.as_deref(), discard)?;
            if work.batch.is_finished() && !discard {
                self.remove_batch_worktree(&checkout, &work.batch.id);
            }
            // An item that cannot be raised stops only this batch's; the others are recorded.
            if let Err(error) = self.sync_item(&work.batch) {
                eprintln!("Brain batch {}: {error:#}", work.batch.id);
            }
        }
        Ok(merged)
    }

    /// Keeps a batch's one Inbox item in step with its problem: raised on the project that wrote
    /// it, replaced when the problem changes and cleared when there is none. A batch from no
    /// project shows its problem on the Brains page only.
    fn sync_item(&mut self, batch: &BrainBatch) -> Result<()> {
        let (was, attention, items): (Option<String>, Option<String>, i64) = self.db.query_row(
            "SELECT item_key,attention_id,items FROM brain_batches WHERE id=?1",
            [&batch.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let key = item_key(batch);
        if key == was {
            return Ok(());
        }
        if let Some(id) = attention {
            let open: Option<String> = self
                .db
                .query_row(
                    "SELECT id FROM attention WHERE id=?1 AND json_extract(data,'$.answer') IS NULL",
                    [&id],
                    |r| r.get(0),
                )
                .optional()?;
            if open.is_some() {
                let answer = match key {
                    Some(_) => "Replaced: the batch changed",
                    None => "Cleared: the batch moved on",
                };
                self.resolve_attention(&id, answer)?;
            }
        }
        let mut raised = None;
        if let (Some(problem), BrainSource::Project(project)) = (&batch.problem, &batch.source)
            && let Some(coordinator) = self.coordinator_of(project)?
        {
            let operation = format!("{BRAIN_NOTICE_PREFIX}{}:{}", batch.id, items + 1);
            let prompt = item_prompt(batch, problem);
            raised = Some(
                self.request_attention(&coordinator, "local", &operation, &prompt, &[])?
                    .id,
            );
        }
        self.db.execute(
            "UPDATE brain_batches SET item_key=?2,attention_id=?3,items=items+1 WHERE id=?1",
            params![batch.id, key, raised],
        )?;
        Ok(())
    }

    pub(crate) fn coordinator_of(&self, project: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM sessions WHERE project_id=?1 AND parent_id IS NULL AND role=?2",
                params![project, tag(&Role::ProjectOrchestrator)?],
                |r| r.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::publish::tests::exit;
    use crate::test_support::git;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub(crate) const REPO: &str = "Sage-Social/sagehaven-docs";
    const URL: &str = "https://github.com/Sage-Social/sagehaven-docs.git";
    /// A value the sensitive scan would match, planted in check output that must never be kept.
    const PLANTED: &str = "555-867-5309";
    const RELEASE: GateRelease = GATE_RELEASE;

    struct FakePr {
        number: u64,
        branch: String,
        closed: bool,
        merged: bool,
        auto_merge: bool,
    }

    /// The brain on GitHub as a local bare repository, a `gh` that answers from what it holds, and
    /// every command each tool was sent. No test reaches GitHub.
    pub(crate) struct GitHub {
        folder: PathBuf,
        bare: PathBuf,
        gh: Mutex<Vec<Vec<String>>>,
        git: Mutex<Vec<Vec<String>>>,
        offline: AtomicBool,
        refuses_auto_merge: AtomicBool,
        prs: Mutex<Vec<FakePr>>,
        check_runs: Mutex<Value>,
        annotations: Mutex<Value>,
        /// The checks `main`'s branch protection requires.
        required: Mutex<Vec<&'static str>>,
    }
    impl GitHub {
        pub(crate) fn new(folder: &Path) -> Arc<Self> {
            let bare = folder.join("github.git");
            git(
                folder,
                &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
            );
            let work = folder.join("created-on-github");
            git(
                folder,
                &["init", "-q", "-b", "main", work.to_str().unwrap()],
            );
            fs::create_dir_all(work.join("second-brain/logs")).unwrap();
            fs::write(work.join("README.md"), "# Sage brain\n").unwrap();
            fs::write(work.join("second-brain/logs/.gitkeep"), "").unwrap();
            git(&work, &["add", "."]);
            git(&work, &["commit", "-q", "-m", "Initial commit"]);
            git(&work, &["push", "-q", bare.to_str().unwrap(), "main"]);
            Arc::new(Self {
                folder: folder.into(),
                bare,
                gh: Mutex::default(),
                git: Mutex::default(),
                offline: AtomicBool::new(false),
                refuses_auto_merge: AtomicBool::new(false),
                prs: Mutex::default(),
                check_runs: Mutex::new(json!({"check_runs": []})),
                annotations: Mutex::new(json!([])),
                required: Mutex::new(GATE_CHECKS.to_vec()),
            })
        }
        pub(crate) fn tools(self: &Arc<Self>) -> Tools {
            let github = self.clone();
            let gh = move |_: &Path, args: &[&str]| -> Result<runtime::GhAnswer> {
                Ok(github.answer_gh(args))
            };
            let github = self.clone();
            let git = move |path: &Path, args: &[&str]| -> Result<runtime::GitAnswer> {
                github
                    .git
                    .lock()
                    .unwrap()
                    .push(args.iter().map(|a| a.to_string()).collect());
                let bare = github.bare.to_str().unwrap();
                let is_url = |a: &str| a.starts_with("https://github.com/");
                if github.offline.load(Ordering::Relaxed) && args.iter().any(|a| is_url(a)) {
                    return Ok(runtime::GitAnswer {
                        status: exit(128),
                        output: String::new(),
                        error: "fatal: unable to access: Could not resolve host: github.com".into(),
                    });
                }
                let mut local = vec!["-c", "user.name=F", "-c", "user.email=f@example.invalid"];
                local.extend(args.iter().map(|&a| if is_url(a) { bare } else { a }));
                runtime::git_answer(path, &local)
            };
            Tools {
                gh: Arc::new(gh),
                git: Arc::new(git),
            }
        }
        fn answer_gh(&self, args: &[&str]) -> runtime::GhAnswer {
            self.gh
                .lock()
                .unwrap()
                .push(args.iter().map(|a| a.to_string()).collect());
            let answer = |code: i32, output: String, error: &str| runtime::GhAnswer {
                status: exit(code),
                output,
                error: error.into(),
            };
            if self.offline.load(Ordering::Relaxed) {
                return answer(1, String::new(), "error connecting to api.github.com");
            }
            let value_of = |flag: &str| {
                let at = args.iter().position(|a| *a == flag).unwrap();
                args[at + 1].to_owned()
            };
            let mut prs = self.prs.lock().unwrap();
            match args {
                ["config", ..] => answer(0, "https\n".into(), ""),
                ["api", "--hostname", "github.com", endpoint] => {
                    let found = self.api(endpoint, &prs);
                    match found {
                        Some(value) => answer(0, value.to_string(), ""),
                        None => answer(1, String::new(), "HTTP 404: Not Found"),
                    }
                }
                ["pr", "list", ..] => {
                    let head = value_of("--head");
                    let listed: Vec<Value> = (prs.iter())
                        .filter(|pr| pr.branch == head)
                        .map(|pr| json!({"number": pr.number, "url": url_of(pr.number)}))
                        .collect();
                    answer(0, Value::from(listed).to_string(), "")
                }
                ["pr", "create", ..] => {
                    assert_eq!(value_of("--base"), "main");
                    assert!(!value_of("--body").contains(PLANTED));
                    let number = 214 + prs.len() as u64;
                    prs.push(FakePr {
                        number,
                        branch: value_of("--head"),
                        closed: false,
                        merged: false,
                        auto_merge: false,
                    });
                    answer(0, format!("{}\n", url_of(number)), "")
                }
                ["pr", "merge", number, ..] => {
                    assert!(args.contains(&"--auto") && args.contains(&"--squash"));
                    if self.refuses_auto_merge.load(Ordering::Relaxed) {
                        return answer(
                            1,
                            String::new(),
                            "GraphQL: Auto merge is not allowed for this repository (enablePullRequestAutoMerge)",
                        );
                    }
                    let pr = prs.iter_mut().find(|pr| pr.number.to_string() == *number);
                    pr.unwrap().auto_merge = true;
                    answer(0, String::new(), "")
                }
                ["pr", "close", number, ..] => {
                    let pr = prs.iter_mut().find(|pr| pr.number.to_string() == *number);
                    pr.unwrap().closed = true;
                    answer(0, String::new(), "")
                }
                other => panic!("unexpected gh call {other:?}"),
            }
        }
        fn api(&self, endpoint: &str, prs: &[FakePr]) -> Option<Value> {
            if endpoint == "user" {
                return Some(json!({"login": "mhohlios"}));
            }
            let rest = endpoint.strip_prefix(&format!("repos/{REPO}/"))?;
            if let Some(number) = rest.strip_prefix("pulls/") {
                let pr = prs.iter().find(|pr| pr.number.to_string() == number)?;
                let head = self.head(&pr.branch).unwrap_or_default();
                let auto_merge = match pr.auto_merge {
                    true => json!({"merge_method": "squash"}),
                    false => Value::Null,
                };
                return Some(json!({
                    "state": if pr.closed || pr.merged { "closed" } else { "open" },
                    "merged": pr.merged,
                    "mergeable_state": "blocked",
                    "auto_merge": auto_merge,
                    "head": {"sha": head},
                }));
            }
            // Main requires each check three ways at once, as protection and a ruleset may.
            let required = self.required.lock().unwrap().clone();
            let contexts: Vec<Value> = (required.iter())
                .map(|name| json!({"context": name}))
                .collect();
            if rest == "branches/main" {
                return Some(json!({"name": "main", "protected": true,
                    "protection": {"required_status_checks": {"contexts": required, "checks": contexts}}}));
            }
            if rest == "rules/branches/main" {
                return Some(json!([{"type": "required_status_checks",
                    "parameters": {"required_status_checks": contexts}}]));
            }
            if rest.starts_with("commits/") {
                return Some(self.check_runs.lock().unwrap().clone());
            }
            if rest.starts_with("check-runs/") {
                return Some(self.annotations.lock().unwrap().clone());
            }
            None
        }
        /// The branch's head on GitHub.
        fn head(&self, branch: &str) -> Option<String> {
            let answer =
                runtime::git_answer(&self.bare, &["rev-parse", &format!("refs/heads/{branch}")])
                    .unwrap();
            answer
                .status
                .success()
                .then(|| answer.output.trim().to_owned())
        }
        /// GitHub squash-merges the PR, as auto-merge does once its checks pass.
        fn merge(&self, number: u64) {
            let mut prs = self.prs.lock().unwrap();
            let pr = prs.iter_mut().find(|pr| pr.number == number).unwrap();
            let work = self.folder.join(format!("merge-{number}"));
            let bare = self.bare.to_str().unwrap();
            git(&self.folder, &["clone", "-q", bare, work.to_str().unwrap()]);
            git(&work, &["fetch", "-q", "origin", &pr.branch]);
            git(&work, &["merge", "-q", "--squash", "FETCH_HEAD"]);
            git(&work, &["commit", "-q", "-m", &format!("PR #{number}")]);
            git(&work, &["push", "-q", "origin", "main"]);
            pr.merged = true;
        }
        /// A teammate's commit pushed straight to `main`.
        fn commit_on_main(&self, file: &str) {
            let work = self.folder.join(format!("teammate-{}", new_id()));
            let bare = self.bare.to_str().unwrap();
            git(&self.folder, &["clone", "-q", bare, work.to_str().unwrap()]);
            let path = work.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "From a teammate.\n").unwrap();
            git(&work, &["add", "."]);
            git(&work, &["commit", "-q", "-m", "A teammate's change"]);
            git(&work, &["push", "-q", "origin", "main"]);
        }
        /// A teammate's commit on `main` of `file` as a symlink to `target`.
        #[cfg(unix)]
        fn commit_symlink_on_main(&self, file: &str, target: &Path) {
            let work = self.folder.join(format!("teammate-{}", new_id()));
            let bare = self.bare.to_str().unwrap();
            git(&self.folder, &["clone", "-q", bare, work.to_str().unwrap()]);
            let path = work.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, path).unwrap();
            git(&work, &["add", "."]);
            git(&work, &["commit", "-q", "-m", "A symlink"]);
            git(&work, &["push", "-q", "origin", "main"]);
        }
        fn main_log(&self) -> Vec<String> {
            let log = git(&self.bare, &["log", "--format=%s", "main"]);
            log.lines().map(str::to_owned).collect()
        }
        fn gh_calls(&self, first: &str) -> Vec<Vec<String>> {
            (self.gh.lock().unwrap().iter())
                .filter(|call| call.first().map(String::as_str) == Some(first))
                .cloned()
                .collect()
        }
        /// Every push the host sent: never forced, never `main`, never a tag.
        fn assert_pushes_are_batch_branches_only(&self) -> usize {
            let calls = self.git.lock().unwrap();
            let pushes: Vec<&Vec<String>> = (calls.iter())
                .filter(|call| {
                    publish::subcommand(&call.iter().map(String::as_str).collect::<Vec<_>>())
                        == "push"
                })
                .collect();
            for push in &pushes {
                for arg in push.iter() {
                    assert!(
                        !matches!(
                            arg.as_str(),
                            "--force"
                                | "-f"
                                | "--force-with-lease"
                                | "--tags"
                                | "--mirror"
                                | "--all"
                        ),
                        "{push:?}"
                    );
                    assert!(!arg.starts_with('+'), "a forced refspec: {push:?}");
                    if let Some((_, to)) = arg.split_once(':').filter(|_| !arg.contains("://")) {
                        assert!(to.starts_with("refs/heads/wiffletree/brain/"), "{push:?}");
                    }
                    assert!(
                        !arg.ends_with("refs/heads/main") && arg != "main",
                        "{push:?}"
                    );
                }
            }
            pushes.len()
        }
    }
    fn url_of(number: u64) -> String {
        format!("https://github.com/{REPO}/pull/{number}")
    }

    /// A host whose Sage Space names the brain, with no legacy pointer.
    fn host_with_brain(home: &Path) -> (Host, SpaceId) {
        let mut host = Host::open_set_up(home).unwrap();
        host.legacy_pointer = home.join("no-pointer");
        let sage = host.create_space("Sage", "", "", None).unwrap().space;
        let sage = name_brain(&mut host, &sage.id, REPO);
        (host, sage)
    }
    pub(crate) fn name_brain(host: &mut Host, space: &SpaceId, repository: &str) -> SpaceId {
        let mut space = host.space(space).unwrap().space;
        let (owner, name) = repository.split_once('/').unwrap();
        space.brain = Some(SpaceBrain {
            repository: BrainRepository::GitHub {
                owner: owner.into(),
                name: name.into(),
                extra: Extra::new(),
            },
            folder: "second-brain".into(),
            extra: Extra::new(),
        });
        host.update_space(space).unwrap().space.id
    }
    /// Runs the brain's pass as the service does, off the host thread's state.
    pub(crate) fn run(host: &mut Host, tools: &Tools) -> bool {
        let pass = host.brain_pass(REPO).unwrap().unwrap();
        let passed = super::pass(pass, tools);
        let reached = passed.reached;
        host.finish_brain_pass(passed).unwrap();
        reached
    }
    fn entry(name: &str, body: &str) -> (String, Vec<u8>) {
        (
            format!("second-brain/logs/calls/2026/10/2026-10-30-{name}.md"),
            body.as_bytes().to_vec(),
        )
    }
    fn queue_log(host: &mut Host, tools: &Tools, source: BrainSource) -> BrainBatch {
        let files = [entry(
            "01jbatchentry",
            "## 2026-10-30 — Retry\n- type: decision\n- changed: retry on timeout\n",
        )];
        let title = "Log batch: Call relay retry (1 entry)";
        (host.queue_brain_batch(REPO, BrainBatchKind::Log, source, title, &files, &tools.git))
            .unwrap()
    }
    fn failing_sensitive_scan(github: &GitHub, file: &str) {
        *github.check_runs.lock().unwrap() = json!({"check_runs": [
            {"id": 7, "name": "sensitive-info", "status": "completed", "conclusion": "failure",
             "output": {"title": format!("possible phone {PLANTED}"), "summary": PLANTED, "text": PLANTED, "annotations_count": 1}},
            {"id": 8, "name": "secret-scan", "status": "completed", "conclusion": "success",
             "output": {"title": "ok", "summary": "", "annotations_count": 0}},
        ]});
        *github.annotations.lock().unwrap() = json!([
            {"path": file, "start_line": 3, "end_line": 3, "annotation_level": "failure",
             "message": format!("possible phone number {PLANTED} [rule: phone]"), "title": PLANTED, "raw_details": PLANTED},
        ]);
    }
    fn passing_checks(github: &GitHub) {
        *github.check_runs.lock().unwrap() = json!({"check_runs": GATE_CHECKS.iter().enumerate().map(|(i, name)| json!(
            {"id": 100 + i, "name": name, "status": "completed", "conclusion": "success", "output": {"annotations_count": 0}}
        )).collect::<Vec<_>>()});
    }
    /// Everything the store holds, as text.
    fn store_dump(host: &Host) -> String {
        let mut dump = String::new();
        for table in [
            "brains",
            "brain_batches",
            "brain_batch_checks",
            "attention",
            "activity",
        ] {
            let rows: Vec<String> = (host.db.prepare(&format!("SELECT * FROM {table}")).unwrap())
                .query_map([], |r| {
                    let mut row = String::new();
                    for at in 0..r.as_ref().column_count() {
                        let value: rusqlite::types::Value = r.get(at)?;
                        row.push_str(&format!("{value:?}|"));
                    }
                    Ok(row)
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            dump.push_str(&rows.join("\n"));
        }
        dump
    }

    #[test]
    fn the_upgrade_turns_a_folder_with_a_github_origin_into_its_repository_and_pushes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let folders = tempfile::tempdir().unwrap();
        let vyttle = folders.path().join("vyttle-docs");
        let notes = folders.path().join("notes");
        for folder in [&vyttle, &notes] {
            fs::create_dir_all(folder).unwrap();
            git(folder, &["init", "-q"]);
        }
        git(
            &vyttle,
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:Vyttle-LLC/vyttle-docs.git",
            ],
        );
        let (vyttle_space, notes_space) = {
            let mut host = Host::open_set_up(home.path()).unwrap();
            let one = host.create_space("Vyttle", "", "", None).unwrap().space.id;
            let two = host.create_space("Notes", "", "", None).unwrap().space.id;
            host.set_space_brain(&one, vyttle.to_str().unwrap())
                .unwrap();
            host.set_space_brain(&two, notes.to_str().unwrap()).unwrap();
            (one, two)
        };
        // Back to before this release: the `spaces` upgrade left both as local brains.
        let db = Connection::open(home.path().join(STORE_FILE)).unwrap();
        db.execute_batch(
            "DROP TABLE brain_batch_checks; DROP TABLE brain_batches; DROP TABLE brains;",
        )
        .unwrap();
        drop(db);

        let mut host = Host::open_set_up(home.path()).unwrap();
        host.legacy_pointer = home.path().join("no-pointer");
        let upgraded = host.space(&vyttle_space).unwrap();
        assert_eq!(
            upgraded.space.brain.unwrap().repository,
            BrainRepository::GitHub {
                owner: "Vyttle-LLC".into(),
                name: "vyttle-docs".into(),
                extra: Extra::new()
            }
        );
        assert_eq!(
            upgraded.local.brain_path, None,
            "the human's folder is never the checkout"
        );
        let without = host.space(&notes_space).unwrap();
        assert_eq!(without.space.brain, None);
        assert!(without.local.notes[0].contains("has no GitHub origin"));
        let left = without.local.brain_without_origin.unwrap();
        assert!(
            left.ends_with("notes"),
            "the Brains page names the folder: {left}"
        );
        let brains = host.brains().unwrap();
        assert_eq!(brains.len(), 1);
        assert_eq!(brains[0].repository, "Vyttle-LLC/vyttle-docs");
        assert!(!brains[0].granted, "the grant waits for the human");
        // A second start changes nothing.
        drop(host);
        let host = Host::open_set_up(home.path()).unwrap();
        assert_eq!(host.space(&notes_space).unwrap().local.notes.len(), 1);
    }

    #[test]
    fn two_spaces_naming_one_brain_register_it_once_and_it_leaves_only_when_forgotten() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let other = host
            .create_space("Sage ops", "", "", None)
            .unwrap()
            .space
            .id;
        name_brain(&mut host, &other, "sage-social/SAGEHAVEN-DOCS");
        let brains = host.brains().unwrap();
        assert_eq!(brains.len(), 1);
        assert_eq!(brains[0].spaces, [sage.clone(), other.clone()]);
        assert_eq!(
            PathBuf::from(&brains[0].checkout),
            host.home
                .join("repos/github.com/Sage-Social/sagehaven-docs"),
            "under the one clone root, as main names clones"
        );
        let error = host.forget_brain(REPO).unwrap_err();
        assert!(format!("{error:#}").contains("still names"));
        for space in [&sage, &other] {
            let mut space = host.space(space).unwrap().space;
            space.brain = None;
            host.update_space(space).unwrap();
        }
        assert_eq!(host.brains().unwrap()[0].spaces, [], "kept until confirmed");
        assert!(
            host.brain_cadences().unwrap().is_empty(),
            "and never passed"
        );
        host.forget_brain(REPO).unwrap();
        assert!(host.brains().unwrap().is_empty());
    }

    #[test]
    fn a_brain_cannot_also_be_storage_or_settings_and_no_space_naming_it_cannot_be_granted() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        let error = host.confirm_brain("octo/unnamed").unwrap_err();
        assert!(format!("{error:#}").contains("No Space names"));
        let error = (host.refuse_second_use(REPO, storage::Use::Storage)).unwrap_err();
        assert!(format!("{error:#}").contains("pull requests"));
    }

    #[test]
    fn every_brain_command_is_the_humans_and_no_agent_tool_reaches_one() {
        let commands = [
            Command::ConfirmBrain {
                repository: REPO.into(),
            },
            Command::ForgetBrain {
                repository: REPO.into(),
            },
            Command::SetUpBrain {
                repository: REPO.into(),
            },
            Command::SetBrainCompiler {
                repository: REPO.into(),
                compiler: BrainCompiler {
                    host_id: "h".into(),
                    name: "Mac".into(),
                    extra: Extra::new(),
                },
            },
            Command::AmendBrainBatch {
                batch_id: "b".into(),
                amendment: BrainAmendment::Discard,
            },
            Command::UpdateSpace {
                space: Box::new(Space {
                    id: SpaceId("s".into()),
                    name: "S".into(),
                    description: String::new(),
                    instructions: String::new(),
                    brain: None,
                    storage: SpaceStorage {
                        repository: StorageRepository::Local { id: "l".into() },
                        folder: "s".into(),
                    },
                    repositories: vec![],
                    models: SpaceModels::default(),
                    sources: vec![],
                    extra: Extra::new(),
                }),
            },
        ];
        for command in &commands {
            assert!(command.is_human_only(), "{command:?}");
            assert!(!command.is_read(), "{command:?}");
        }
        let tools = mcp::tools();
        for tool in tools.as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert!(!name.contains("brain"), "{name}");
        }
        assert!(
            ensure_unreserved("brain:01j:1").is_err(),
            "agents cannot raise a batch item"
        );
    }

    #[test]
    fn a_projects_threads_and_created_projects_brain_is_their_spaces() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let calls = host.create_project_in("Calls", &sage).unwrap();
        let personal = host.default_space().unwrap();
        let personal = (host.spaces().unwrap().into_iter())
            .map(|r| r.space.id)
            .find(|id| *id != sage)
            .unwrap_or(personal);
        let notes = host.create_project_in("Notes", &personal).unwrap();
        let brain = |host: &Host, owner: SessionOwner| match host.owner_brain(&owner).unwrap() {
            Some(SpaceBrain {
                repository: BrainRepository::GitHub { owner, name, .. },
                ..
            }) => Some(format!("{owner}/{name}")),
            _ => None,
        };
        assert_eq!(
            brain(&host, SessionOwner::Project(calls.id.clone())).as_deref(),
            Some(REPO)
        );
        assert_eq!(
            brain(&host, SessionOwner::Space(sage.clone())).as_deref(),
            Some(REPO)
        );
        assert_eq!(
            brain(&host, SessionOwner::Brain(REPO.into())).as_deref(),
            Some(REPO)
        );
        assert_eq!(
            brain(&host, SessionOwner::Project(notes.id)),
            None,
            "no brain"
        );
        let coordinator = host.coordinator_of(&calls.id).unwrap().unwrap();
        let created = host
            .create_project_from(&coordinator, "Billing", "Build billing.", None)
            .unwrap();
        let project = created.project.id;
        assert_eq!(
            brain(&host, SessionOwner::Project(project)).as_deref(),
            Some(REPO)
        );
    }

    #[test]
    fn the_owner_suggests_a_brain_only_prefilled_and_personal_repositories_suggest_none() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let vyttle = host.create_space("Vyttle", "", "", None).unwrap().space.id;
        name_brain(&mut host, &vyttle, "Vyttle-LLC/vyttle-docs");
        let before = host.brains().unwrap();
        let suggested = host
            .suggest_space_brain(&["Vyttle-LLC/wiffletree".into(), "Vyttle-LLC/site".into()])
            .unwrap();
        assert_eq!(suggested.as_deref(), Some("Vyttle-LLC/vyttle-docs"));
        assert_eq!(
            host.brains().unwrap(),
            before,
            "suggesting registers nothing"
        );
        host.record_gh_login(Some("MHohlios".into()));
        let personal = host
            .suggest_space_brain(&["MHohlios/dotfiles".into()])
            .unwrap();
        assert_eq!(personal, None);
        let unknown = host.suggest_space_brain(&["octo/app".into()]).unwrap();
        assert_eq!(unknown, None);
    }

    #[test]
    fn the_config_is_validated_and_a_newer_version_is_only_noted() {
        let read = |text: &str| parse_config(text.as_bytes());
        assert!(matches!(
            read("{not json"),
            BrainConfigReading::Invalid { .. }
        ));
        assert!(matches!(
            read(r#"{"brain_dir":"x"}"#),
            BrainConfigReading::Invalid { .. }
        ));
        assert_eq!(
            read(r#"{"version":2}"#),
            BrainConfigReading::Newer { version: 2 }
        );
        for outside in ["../elsewhere", "/etc", ""] {
            let text = json!({"version": 1, "brain_dir": outside}).to_string();
            assert!(
                matches!(read(&text), BrainConfigReading::Invalid { .. }),
                "{outside}"
            );
        }
        let secret = json!({"version": 1, "compiler": PLANTED}).to_string();
        let BrainConfigReading::Invalid { reason } = read(&secret) else {
            panic!("a string compiler is refused");
        };
        assert!(!reason.contains(PLANTED), "reasons name no value");
        let text = r#"{"version":1,"compiler":{"host_id":"h1","name":"Michael's Mac","seat":2},"future":true}"#;
        let BrainConfigReading::Read { config } = read(text) else {
            panic!("version 1 reads");
        };
        assert_eq!(config.brain_dir, "second-brain");
        assert_eq!(config.compiler.as_ref().unwrap().name, "Michael's Mac");
        let written = serde_json::to_value(&config).unwrap();
        assert_eq!(written["future"], true, "unknown keys are kept");
        assert_eq!(written["compiler"]["seat"], 2);
    }

    #[test]
    fn a_batch_is_pushed_opened_and_set_to_auto_merge_and_main_is_never_pushed() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        assert!(run(&mut host, &tools));
        let brain = host.brain(REPO).unwrap();
        let checkout = PathBuf::from(&brain.checkout);
        assert!(checkout.join(".git").join(MARKER).exists());
        assert_eq!(brain.config, BrainConfigReading::Missing);
        assert_eq!(github.assert_pushes_are_batch_branches_only(), 0);

        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        assert_eq!(batch.stage, BrainStage::Queued);
        run(&mut host, &tools);
        let queued = host.brain_batch(&batch.id).unwrap();
        assert_eq!(
            queued.stage,
            BrainStage::Queued,
            "nothing is pushed before the grant"
        );
        assert_eq!(github.assert_pushes_are_batch_branches_only(), 0);
        assert!(github.gh_calls("pr").is_empty());

        host.confirm_brain(REPO).unwrap();
        assert!(run(&mut host, &tools));
        let opened = host.brain_batch(&batch.id).unwrap();
        let branch = format!("wiffletree/brain/mhohlios/{}", batch.id);
        assert_eq!(opened.branch.as_deref(), Some(branch.as_str()));
        let pr = opened.pull_request.clone().unwrap();
        assert_eq!((pr.number, pr.auto_merge), (214, true));
        assert_eq!(opened.stage, BrainStage::Checks);
        assert!(github.head(&branch).is_some());
        assert_eq!(
            github.main_log(),
            ["Initial commit"],
            "main is never pushed"
        );
        let merges = github.gh_calls("pr");
        assert!(
            merges
                .iter()
                .any(|c| c[..2] == ["pr", "merge"] && c.contains(&"--auto".into()))
        );
        assert_eq!(batch.id.len(), 26);
        assert!(
            batch
                .id
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        );

        passing_checks(&github);
        run(&mut host, &tools);
        assert_eq!(
            host.brain_batch(&batch.id).unwrap().stage,
            BrainStage::AutoMerging
        );
        github.merge(214);
        run(&mut host, &tools);
        let merged = host.brain_batch(&batch.id).unwrap();
        assert_eq!(merged.stage, BrainStage::Merged);
        assert!(
            !home.path().join(BATCHES).join(&batch.id).exists(),
            "its worktree is gone"
        );
        // The next pass fast-forwards main to the merge, where the next batch starts.
        run(&mut host, &tools);
        let main = git(&checkout, &["log", "--format=%s", "-1"]);
        assert_eq!(main, "PR #214");
        assert!(checkout.join(&batch.files[0]).exists());
        let pushes = github.assert_pushes_are_batch_branches_only();
        assert_eq!(pushes, 1);
        // Two days on, the batch leaves the page but stays the brain's last PR.
        host.db
            .execute(
                "UPDATE brain_batches SET data=json_set(data,'$.updated_at',?2) WHERE id=?1",
                params![batch.id, now() - 2 * SHOWN_AFTER_MERGE_MS],
            )
            .unwrap();
        let page = host.brains_page().unwrap();
        assert!(page.batches.is_empty());
        assert_eq!(
            page.brains[0].last_pull_request,
            Some(BrainLastPullRequest {
                number: 214,
                kind: BrainBatchKind::Log,
                stage: BrainStage::Merged,
            })
        );
        let error = host
            .amend_brain_batch(&batch.id, &BrainAmendment::Discard, &tools.git)
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("merged"),
            "nothing is offered after a merge"
        );
    }

    #[test]
    fn stages_survive_a_restart_and_nothing_is_pushed_or_opened_twice() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let id = {
            let (mut host, _) = host_with_brain(home.path());
            run(&mut host, &tools);
            host.confirm_brain(REPO).unwrap();
            let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
            run(&mut host, &tools);
            batch.id
        };
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.legacy_pointer = home.path().join("no-pointer");
        let batch = host.brain_batch(&id).unwrap();
        assert_eq!(batch.stage, BrainStage::Checks);
        assert!(host.brain(REPO).unwrap().granted);
        run(&mut host, &tools);
        assert_eq!(
            github
                .gh_calls("pr")
                .iter()
                .filter(|c| c[1] == "create")
                .count(),
            1
        );
        assert_eq!(github.assert_pushes_are_batch_branches_only(), 1);
    }

    #[test]
    fn offline_keeps_the_batch_queued_and_backs_off_until_sync_now() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        github.offline.store(true, Ordering::Relaxed);
        assert!(!run(&mut host, &tools));
        assert_eq!(
            host.brain_batch(&batch.id).unwrap().stage,
            BrainStage::Queued
        );
        assert!(
            host.brain(REPO)
                .unwrap()
                .problem
                .unwrap()
                .contains("Could not resolve host")
        );
        let mut pace = repos::Pace::every(&[pr_watch::ACTIVE_MS]);
        let mut at = 0;
        for _ in 0..6 {
            assert!(pace.start(at));
            pace.finish(at, false);
            let due = pace.next_due().unwrap();
            assert!(due - at <= 1_800_000, "thirty minutes at most");
            at = due;
        }
        pace.sync_now();
        assert!(pace.start(at - 1), "Sync now tries at once");
        github.offline.store(false, Ordering::Relaxed);
        assert!(run(&mut host, &tools));
        assert_eq!(
            host.brain_batch(&batch.id).unwrap().stage,
            BrainStage::Checks
        );
    }

    #[test]
    fn a_refused_auto_merge_keeps_the_pr_open_with_one_item_naming_the_settings() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let project = host.create_project_in("Calls", &sage).unwrap();
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        github.refuses_auto_merge.store(true, Ordering::Relaxed);
        let batch = queue_log(&mut host, &tools, BrainSource::Project(project.id.clone()));
        run(&mut host, &tools);
        run(&mut host, &tools);
        let open = host.brain_batch(&batch.id).unwrap();
        assert_eq!(open.stage, BrainStage::PrOpen);
        assert_eq!(open.problem, Some(BrainProblem::AutoMergeRefused));
        let items = host.open_attention(&project.id).unwrap();
        assert_eq!(items.len(), 1, "one item, not one per pass");
        assert!(
            items[0].prompt.contains("Allow auto-merge")
                && items[0].prompt.contains("sensitive-info")
        );
        assert!(!github.prs.lock().unwrap()[0].closed, "the PR stays open");
    }

    #[test]
    fn a_failed_check_keeps_names_files_and_lines_and_no_text_and_a_redaction_clears_its_item() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let project = host.create_project_in("Calls", &sage).unwrap();
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::Project(project.id.clone()));
        let file = batch.files[0].clone();
        run(&mut host, &tools);
        failing_sensitive_scan(&github, &file);
        run(&mut host, &tools);
        let failed = host.brain_batch(&batch.id).unwrap();
        assert_eq!(failed.stage, BrainStage::Failed);
        assert_eq!(
            failed.problem,
            Some(BrainProblem::ChecksFailed {
                checks: vec!["sensitive-info".into()]
            })
        );
        let check = failed
            .checks
            .iter()
            .find(|c| c.name == "sensitive-info")
            .unwrap();
        assert_eq!(
            check.annotations,
            [BrainAnnotation {
                path: file.clone(),
                line: 3
            }]
        );
        let items = host.open_attention(&project.id).unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            items[0].prompt.contains("PR #214")
                && items[0].prompt.contains(&format!("{file} line 3"))
        );
        assert!(
            !store_dump(&host).contains(PLANTED),
            "no check text reaches the store"
        );
        assert!(
            !serde_json::to_string(&host.brains_page().unwrap())
                .unwrap()
                .contains(PLANTED)
        );

        let branch = failed.branch.clone().unwrap();
        let before = github.head(&branch).unwrap();
        host.amend_brain_batch(
            &batch.id,
            &BrainAmendment::Redact {
                path: file.clone(),
                line: 3,
                rule: "phone".into(),
            },
            &tools.git,
        )
        .unwrap();
        *github.check_runs.lock().unwrap() = json!({"check_runs": []});
        run(&mut host, &tools);
        let after = github.head(&branch).unwrap();
        assert_ne!(after, before);
        assert_eq!(
            git(&github.bare, &["rev-parse", &format!("{after}~")]),
            before,
            "one commit on top"
        );
        let line = git(&github.bare, &["show", &format!("{after}:{file}")]);
        assert!(line.contains("- changed: [removed: phone]"), "{line}");
        assert!(
            host.open_attention(&project.id).unwrap().is_empty(),
            "the item is cleared"
        );
        passing_checks(&github);
        run(&mut host, &tools);
        assert_eq!(
            host.brain_batch(&batch.id).unwrap().stage,
            BrainStage::AutoMerging
        );
        github.assert_pushes_are_batch_branches_only();
    }

    #[test]
    fn not_sensitive_appends_the_gates_marker_and_discard_closes_the_pr_and_deletes_the_branch() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        let file = batch.files[0].clone();
        run(&mut host, &tools);
        let marked = BrainAmendment::NotSensitive {
            path: file.clone(),
            line: 3,
            rule: "phone".into(),
            reason: "fixture call ID".into(),
        };
        host.amend_brain_batch(&batch.id, &marked, &tools.git)
            .unwrap();
        run(&mut host, &tools);
        let branch = host.brain_batch(&batch.id).unwrap().branch.unwrap();
        let head = github.head(&branch).unwrap();
        let shown = git(&github.bare, &["show", &format!("{head}:{file}")]);
        assert!(
            shown.contains("- changed: retry on timeout <!-- gate:allow phone fixture call ID -->")
        );
        for refused in [
            BrainAmendment::NotSensitive {
                path: file.clone(),
                line: 3,
                rule: "phone".into(),
                reason: "x --> y".into(),
            },
            BrainAmendment::Redact {
                path: file.clone(),
                line: 3,
                rule: "made-up".into(),
            },
            BrainAmendment::Redact {
                path: "README.md".into(),
                line: 1,
                rule: "phone".into(),
            },
            BrainAmendment::Redact {
                path: file.clone(),
                line: 99,
                rule: "phone".into(),
            },
        ] {
            assert!(
                host.amend_brain_batch(&batch.id, &refused, &tools.git)
                    .is_err(),
                "{refused:?}"
            );
        }

        host.amend_brain_batch(&batch.id, &BrainAmendment::Discard, &tools.git)
            .unwrap();
        run(&mut host, &tools);
        let discarded = host.brain_batch(&batch.id).unwrap();
        assert_eq!(discarded.stage, BrainStage::Discarded);
        assert!(github.prs.lock().unwrap()[0].closed);
        assert_eq!(
            github.head(&branch),
            None,
            "the branch the host pushed is deleted"
        );
        assert_eq!(github.main_log(), ["Initial commit"]);
        github.assert_pushes_are_batch_branches_only();
    }

    #[test]
    fn an_edit_in_the_checkout_or_a_local_commit_holds_writing_without_reset_or_overwrite() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let checkout = PathBuf::from(host.brain(REPO).unwrap().checkout);
        fs::write(checkout.join("README.md"), "Edited in Obsidian.\n").unwrap();
        github.commit_on_main("second-brain/notes/teammate.md");
        run(&mut host, &tools);
        let brain = host.brain(REPO).unwrap();
        assert_eq!(
            brain.held,
            Some(BrainHold::ChangedFiles {
                files: vec!["README.md".into()]
            })
        );
        let kept = fs::read_to_string(checkout.join("README.md")).unwrap();
        assert_eq!(kept, "Edited in Obsidian.\n");
        assert!(
            !checkout.join("second-brain/notes/teammate.md").exists(),
            "no merge while held"
        );
        let error = host
            .queue_brain_batch(
                REPO,
                BrainBatchKind::Log,
                BrainSource::BrainsPage,
                "t",
                &[entry("x", "y")],
                &tools.git,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("held"));

        // The human settles it; a teammate's commit on main is then taken by fast-forward.
        git(&checkout, &["checkout", "--", "README.md"]);
        run(&mut host, &tools);
        assert_eq!(host.brain(REPO).unwrap().held, None);
        assert!(checkout.join("second-brain/notes/teammate.md").exists());

        // A commit made in the checkout cannot be fast-forwarded past, and is kept.
        fs::write(checkout.join("local.md"), "Local.\n").unwrap();
        git(&checkout, &["add", "local.md"]);
        git(&checkout, &["commit", "-q", "-m", "Local"]);
        github.commit_on_main("second-brain/notes/other.md");
        run(&mut host, &tools);
        assert_eq!(
            host.brain(REPO).unwrap().held,
            Some(BrainHold::NotFastForward)
        );
        assert_eq!(git(&checkout, &["log", "--format=%s", "-1"]), "Local");
        assert_eq!(github.assert_pushes_are_batch_branches_only(), 0);
    }

    #[test]
    fn set_up_queues_the_config_the_pinned_caller_and_the_codeowners_lines_in_one_batch() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        let error = host.set_up_brain(REPO, &tools.git).unwrap_err();
        assert!(format!("{error:#}").contains("Sync now"), "not fetched yet");
        run(&mut host, &tools);
        let batch = host.set_up_brain(REPO, &tools.git).unwrap();
        assert_eq!(RELEASE.commit, "083ab25267aa260569ab61106ff8fdff5bb2ba0e");
        assert_eq!(batch.kind, BrainBatchKind::SetUp);
        assert_eq!(
            batch.files,
            [BrainConfig::FILE, CALLER, ALLOWLIST, ".github/CODEOWNERS"]
        );
        let worktree = home.path().join(BATCHES).join(&batch.id);
        let config = fs::read(worktree.join(BrainConfig::FILE)).unwrap();
        let BrainConfigReading::Read { config } = parse_config(&config) else {
            panic!("Set up writes a valid config");
        };
        assert_eq!(config.compiler.unwrap().host_id, host.host_id().unwrap());
        assert_eq!(config.gate.unwrap()["version"], RELEASE.tag);
        let caller = fs::read_to_string(worktree.join(CALLER)).unwrap();
        assert_eq!(
            caller
                .matches(&format!("gate@{} # {}", RELEASE.commit, RELEASE.tag))
                .count(),
            7
        );
        let owners = fs::read_to_string(worktree.join(".github/CODEOWNERS")).unwrap();
        assert!(owners.contains("/wiffletree-brain.json") && owners.contains("@mhohlios"));
        let error = host.set_up_brain(REPO, &tools.git).unwrap_err();
        assert!(
            format!("{error:#}").contains("already open"),
            "one Set up PR"
        );
    }

    #[cfg(unix)]
    #[test]
    fn set_up_never_writes_through_a_codeowners_symlink_the_brain_committed() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "Not the brain's.\n").unwrap();
        github.commit_symlink_on_main(".github/CODEOWNERS", &victim);
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        let error = host.set_up_brain(REPO, &tools.git).unwrap_err();
        assert!(format!("{error:#}").contains("symlink"), "{error:#}");
        assert_eq!(fs::read_to_string(&victim).unwrap(), "Not the brain's.\n");
        assert!(host.brain_batches(None).unwrap().is_empty());
    }

    #[test]
    fn set_up_opens_its_pr_without_auto_merge_and_waits_for_the_code_owners_review() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = host.set_up_brain(REPO, &tools.git).unwrap();
        run(&mut host, &tools);
        passing_checks(&github);
        run(&mut host, &tools);
        let open = host.brain_batch(&batch.id).unwrap();
        assert_eq!(open.stage, BrainStage::PrOpen);
        assert_eq!(open.problem, Some(BrainProblem::AwaitingReview));
        assert!(!open.pull_request.as_ref().unwrap().auto_merge);
        assert!(
            github.gh_calls("pr").iter().all(|c| c[1] != "merge"),
            "no auto-merge"
        );
        let raised: Option<String> = (host.db)
            .query_row(
                "SELECT attention_id FROM brain_batches WHERE id=?1",
                [&batch.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raised, None, "the Brains page shows it; no Inbox item");

        github.merge(214);
        run(&mut host, &tools);
        let merged = host.brain_batch(&batch.id).unwrap();
        assert_eq!((merged.stage, merged.problem), (BrainStage::Merged, None));
    }

    #[test]
    fn an_amendment_made_while_a_pass_runs_is_pushed_and_not_taken_for_an_outside_change() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        run(&mut host, &tools);
        let branch = host.brain_batch(&batch.id).unwrap().branch.unwrap();
        let before = github.head(&branch).unwrap();

        let pass = host.brain_pass(REPO).unwrap().unwrap();
        let redact = BrainAmendment::Redact {
            path: batch.files[0].clone(),
            line: 3,
            rule: "phone".into(),
        };
        host.amend_brain_batch(&batch.id, &redact, &tools.git)
            .unwrap();
        host.finish_brain_pass(super::pass(pass, &tools)).unwrap();
        let raced = host.brain_batch(&batch.id).unwrap();
        assert_ne!(raced.problem, Some(BrainProblem::ChangedOutside));
        assert_ne!(raced.stage, BrainStage::Failed);

        run(&mut host, &tools);
        let after = github.head(&branch).unwrap();
        assert_eq!(
            git(&github.bare, &["rev-parse", &format!("{after}~")]),
            before,
            "the amendment is pushed as one commit on top"
        );
        assert_eq!(host.brain_batch(&batch.id).unwrap().problem, None);
    }

    #[test]
    fn set_up_keeps_an_existing_codeowners_file_and_appends_only_missing_lines() {
        let folder = tempfile::tempdir().unwrap();
        let checkout = folder.path();
        git(checkout, &["init", "-q", "-b", "main"]);
        fs::write(
            checkout.join("CODEOWNERS"),
            "* @sage-social/docs\n/.github/ @mhohlios",
        )
        .unwrap();
        git(checkout, &["add", "."]);
        git(checkout, &["commit", "-q", "-m", "Owners"]);
        let (path, content) = codeowners(checkout, "HEAD", "mhohlios").unwrap().unwrap();
        assert_eq!(path, "CODEOWNERS");
        let content = String::from_utf8(content).unwrap();
        assert!(content.starts_with("* @sage-social/docs\n/.github/ @mhohlios\n"));
        assert_eq!(content.matches("/.github/").count(), 1);
        assert_eq!(content.lines().count(), 6);
    }

    #[test]
    fn a_compiler_change_is_a_config_only_batch_that_keeps_unknown_keys() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let config = json!({"version": 1, "compiler": {"host_id": "old", "name": "Old Mac"}, "future": {"kept": true}});
        let work = folder.path().join("config");
        git(
            folder.path(),
            &[
                "clone",
                "-q",
                github.bare.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        fs::write(work.join(BrainConfig::FILE), config.to_string()).unwrap();
        // On another branch only, the file would not count: main is all that is read.
        git(&work, &["checkout", "-q", "-b", "elsewhere"]);
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "Config on a branch"]);
        git(&work, &["push", "-q", "origin", "elsewhere"]);
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        assert_eq!(
            host.brain(REPO).unwrap().config,
            BrainConfigReading::Missing
        );
        git(&work, &["push", "-q", "origin", "elsewhere:main"]);
        run(&mut host, &tools);
        assert_eq!(
            host.brain(REPO)
                .unwrap()
                .config
                .config()
                .unwrap()
                .compiler
                .as_ref()
                .unwrap()
                .name,
            "Old Mac"
        );
        let compiler = BrainCompiler {
            host_id: "new".into(),
            name: "Michael's Mac".into(),
            extra: Extra::new(),
        };
        let batch = host
            .set_brain_compiler(REPO, &compiler, &tools.git)
            .unwrap();
        assert_eq!(
            (batch.kind, batch.files.clone()),
            (BrainBatchKind::Config, vec![BrainConfig::FILE.to_owned()])
        );
        let written: Value = serde_json::from_slice(
            &fs::read(
                home.path()
                    .join(BATCHES)
                    .join(&batch.id)
                    .join(BrainConfig::FILE),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(written["compiler"]["host_id"], "new");
        assert_eq!(written["future"]["kept"], true);
        assert_eq!(
            host.brain(REPO)
                .unwrap()
                .config
                .config()
                .unwrap()
                .compiler
                .as_ref()
                .unwrap()
                .name,
            "Old Mac",
            "the merged compiler stands until the PR merges"
        );
    }

    #[test]
    fn the_legacy_pointer_never_names_the_checkout_and_its_clone_of_a_brain_is_warned_about() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        let pointer = folder.path().join("claude-second-brain");
        host.legacy_pointer = pointer.clone();
        let checkout = host.brain(REPO).unwrap().checkout;
        fs::write(&pointer, format!("{checkout}\n")).unwrap();
        assert!(host.brain_pass(REPO).unwrap().is_none(), "refused");
        assert!(
            host.brain(REPO)
                .unwrap()
                .problem
                .unwrap()
                .contains("legacy pointer")
        );
        assert!(!host.brains_page().unwrap().brains[0].legacy_pointer);

        let own = folder.path().join("sagehaven-docs");
        fs::create_dir_all(&own).unwrap();
        git(&own, &["init", "-q"]);
        git(&own, &["remote", "add", "origin", URL]);
        fs::write(&pointer, own.to_str().unwrap()).unwrap();
        assert!(host.brain_pass(REPO).unwrap().is_some());
        assert!(host.brains_page().unwrap().brains[0].legacy_pointer);
    }

    #[test]
    fn a_discard_made_while_a_pass_runs_is_kept_and_a_batch_published_meanwhile_is_closed() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        // Not granted: the pass returns the batch as it was.
        let quiet = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        let pass = host.brain_pass(REPO).unwrap().unwrap();
        host.amend_brain_batch(&quiet.id, &BrainAmendment::Discard, &tools.git)
            .unwrap();
        host.finish_brain_pass(super::pass(pass, &tools)).unwrap();
        assert_eq!(
            host.brain_batch(&quiet.id).unwrap().stage,
            BrainStage::Discarded
        );

        // Granted: the pass publishes it while the human discards it here.
        host.confirm_brain(REPO).unwrap();
        let raced = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        let pass = host.brain_pass(REPO).unwrap().unwrap();
        let passed = super::pass(pass, &tools);
        host.amend_brain_batch(&raced.id, &BrainAmendment::Discard, &tools.git)
            .unwrap();
        host.finish_brain_pass(passed).unwrap();
        let branch = format!("wiffletree/brain/mhohlios/{}", raced.id);
        assert!(github.head(&branch).is_some(), "published meanwhile");
        run(&mut host, &tools);
        assert_eq!(
            host.brain_batch(&raced.id).unwrap().stage,
            BrainStage::Discarded
        );
        assert!(github.prs.lock().unwrap()[0].closed);
        assert_eq!(github.head(&branch), None);
    }

    #[test]
    fn a_discard_cut_short_after_closing_the_pr_still_deletes_the_branch() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        run(&mut host, &tools);
        host.amend_brain_batch(&batch.id, &BrainAmendment::Discard, &tools.git)
            .unwrap();
        // An earlier try closed the PR, then lost GitHub before deleting the branch.
        github.prs.lock().unwrap()[0].closed = true;
        let branch = format!("wiffletree/brain/mhohlios/{}", batch.id);
        run(&mut host, &tools);
        assert_eq!(
            host.brain_batch(&batch.id).unwrap().stage,
            BrainStage::Discarded
        );
        assert_eq!(
            github.head(&branch),
            None,
            "the branch is deleted on the retry"
        );
        assert!(!home.path().join(BATCHES).join(&batch.id).exists());
    }

    #[test]
    fn auto_merge_waits_while_main_does_not_require_every_gate_check() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let project = host.create_project_in("Calls", &sage).unwrap();
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        github
            .required
            .lock()
            .unwrap()
            .retain(|c| *c != "sensitive-info");
        let batch = queue_log(&mut host, &tools, BrainSource::Project(project.id.clone()));
        run(&mut host, &tools);
        run(&mut host, &tools);
        let waiting = host.brain_batch(&batch.id).unwrap();
        assert_eq!(waiting.stage, BrainStage::PrOpen);
        let missing = vec!["sensitive-info".to_owned()];
        assert_eq!(
            waiting.problem,
            Some(BrainProblem::GateNotRequired {
                missing: missing.clone()
            })
        );
        let brain = host.brain(REPO).unwrap();
        assert_eq!(brain.missing_checks, missing);
        assert!(
            !brain.required_checks.contains(&missing[0]) && brain.required_checks.len() == 6,
            "the page lists only what main requires"
        );
        assert!(
            github.gh_calls("pr").iter().all(|c| c[1] != "merge"),
            "no auto-merge"
        );
        let items = host.open_attention(&project.id).unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            items[0]
                .prompt
                .contains("does not require the brain gate's sensitive-info")
        );

        *github.required.lock().unwrap() = GATE_CHECKS.to_vec();
        run(&mut host, &tools);
        let merging = host.brain_batch(&batch.id).unwrap();
        assert!(merging.pull_request.unwrap().auto_merge);
        assert_eq!(merging.problem, None);
        assert!(host.open_attention(&project.id).unwrap().is_empty());
    }

    #[test]
    fn a_commit_or_edit_made_outside_wiffletree_in_a_batch_worktree_is_never_pushed() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        let worktree = home.path().join(BATCHES).join(&batch.id);
        fs::write(worktree.join("outside.md"), "Not Wiffletree's.\n").unwrap();
        git(&worktree, &["add", "outside.md"]);
        git(&worktree, &["commit", "-q", "-m", "Outside"]);
        run(&mut host, &tools);
        let refused = host.brain_batch(&batch.id).unwrap();
        assert_eq!(refused.problem, Some(BrainProblem::ChangedOutside));
        assert_eq!(github.assert_pushes_are_batch_branches_only(), 0);
        assert!(github.gh_calls("pr").is_empty());

        // An uncommitted edit is refused too, and never swept into an amendment.
        let second = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        run(&mut host, &tools);
        let file = second.files[0].clone();
        let worktree = home.path().join(BATCHES).join(&second.id);
        fs::write(
            worktree.join(&file),
            "Edited outside.\n- changed: x\n- why: y\n",
        )
        .unwrap();
        let amend = BrainAmendment::Redact {
            path: file,
            line: 3,
            rule: "phone".into(),
        };
        let error = host
            .amend_brain_batch(&second.id, &amend, &tools.git)
            .unwrap_err();
        assert!(format!("{error:#}").contains("Something other than Wiffletree"));
        run(&mut host, &tools);
        let second = host.brain_batch(&second.id).unwrap();
        assert_eq!(second.problem, Some(BrainProblem::ChangedOutside));
        assert_eq!(
            github.assert_pushes_are_batch_branches_only(),
            1,
            "only its first push"
        );
    }

    #[test]
    fn a_gate_check_dropped_from_main_after_auto_merge_is_on_raises_the_item_again() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let project = host.create_project_in("Calls", &sage).unwrap();
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::Project(project.id.clone()));
        run(&mut host, &tools);
        passing_checks(&github);
        run(&mut host, &tools);
        let merging = host.brain_batch(&batch.id).unwrap();
        assert_eq!(merging.stage, BrainStage::AutoMerging);
        assert!(host.open_attention(&project.id).unwrap().is_empty());

        github
            .required
            .lock()
            .unwrap()
            .retain(|c| *c != "sensitive-info");
        run(&mut host, &tools);
        run(&mut host, &tools);
        let exposed = host.brain_batch(&batch.id).unwrap();
        assert_eq!(
            exposed.problem,
            Some(BrainProblem::GateNotRequired {
                missing: vec!["sensitive-info".into()]
            })
        );
        assert_ne!(exposed.stage, BrainStage::AutoMerging);
        let items = host.open_attention(&project.id).unwrap();
        assert_eq!(items.len(), 1, "one item, kept across passes");
        assert!(items[0].prompt.contains("sensitive-info"));
    }

    #[test]
    fn a_failing_workflow_main_does_not_require_does_not_fail_the_batch() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = host_with_brain(home.path());
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let batch = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        run(&mut host, &tools);
        passing_checks(&github);
        let mut runs = github.check_runs.lock().unwrap().clone();
        runs["check_runs"].as_array_mut().unwrap().push(json!(
            {"id": 900, "name": "summary-comment", "status": "completed", "conclusion": "failure", "output": {"annotations_count": 0}}
        ));
        *github.check_runs.lock().unwrap() = runs;
        run(&mut host, &tools);
        let batch = host.brain_batch(&batch.id).unwrap();
        assert_eq!(batch.stage, BrainStage::AutoMerging);
        assert_eq!(batch.problem, None);
        assert!(
            batch.checks.iter().any(|c| c.name == "summary-comment"),
            "still shown"
        );
    }

    #[test]
    fn an_error_on_one_batch_is_its_problem_and_the_brains_other_batches_go_on() {
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = host_with_brain(home.path());
        let project = host.create_project_in("Calls", &sage).unwrap();
        run(&mut host, &tools);
        host.confirm_brain(REPO).unwrap();
        let broken = queue_log(&mut host, &tools, BrainSource::Project(project.id.clone()));
        let healthy = queue_log(&mut host, &tools, BrainSource::BrainsPage);
        run(&mut host, &tools);
        // GitHub no longer answers for the first batch's PR.
        github.prs.lock().unwrap().retain(|pr| pr.number != 214);
        passing_checks(&github);
        assert!(run(&mut host, &tools), "the pass itself goes on");
        let broken = host.brain_batch(&broken.id).unwrap();
        assert!(
            matches!(&broken.problem, Some(BrainProblem::Error { reason }) if reason.contains("404")),
            "{:?}",
            broken.problem
        );
        assert_eq!(host.open_attention(&project.id).unwrap().len(), 1);
        assert_eq!(
            host.brain_batch(&healthy.id).unwrap().stage,
            BrainStage::AutoMerging
        );
    }

    #[test]
    fn a_long_git_reason_is_capped_so_its_item_fits_the_inbox() {
        let reason = capped(&"refused ".repeat(2_000));
        assert!(reason.chars().count() <= 301 && reason.ends_with('…'));
        let mut batch = BrainBatch {
            id: "01j".into(),
            repository: REPO.into(),
            kind: BrainBatchKind::Log,
            source: BrainSource::BrainsPage,
            title: "t".repeat(256),
            files: vec![],
            branch: None,
            pull_request: None,
            stage: BrainStage::Failed,
            problem: None,
            checks: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let problem = BrainProblem::PushRefused { reason };
        batch.problem = Some(problem.clone());
        assert!(item_prompt(&batch, &problem).len() < 4096);
    }

    #[test]
    fn a_field_line_keeps_its_name_when_redacted() {
        assert_eq!(
            redact_line("- changed: call 555", "phone"),
            "- changed: [removed: phone]"
        );
        assert_eq!(redact_line("  more 555", "phone"), "  [removed: phone]");
    }
}
