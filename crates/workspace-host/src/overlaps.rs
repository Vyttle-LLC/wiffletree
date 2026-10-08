//! Early warnings for open ticket branches: how far the base has moved on, and which files would
//! conflict with it or with other open tickets in the same repository, whatever their project.
//! Read-only apart from fetching the base's remote-tracking ref; branches and worktrees are never
//! touched. Only committed work is compared.
use crate::*;
use std::collections::{HashMap, HashSet};

/// The least time between two checks, and so two fetches, of one repository.
pub(crate) const OVERLAP_REFRESH_MS: i64 = 5 * 60 * 1000;

/// One repository's open ticket branches, gathered on the host thread and checked off it.
pub(crate) struct OverlapCheck {
    pub repository: PathBuf,
    pub base: String,
    pub branches: Vec<OpenBranch>,
    /// When this host last fetched the base.
    pub fetched_at: Option<i64>,
}
pub(crate) struct OpenBranch {
    pub ticket_id: String,
    pub title: String,
    pub project: String,
    pub branch: String,
}

impl Host {
    /// Each repository with an open ticket, keyed by repository ID.
    pub(crate) fn overlap_checks(&self) -> Result<Vec<(String, OverlapCheck)>> {
        let projects = self.projects()?;
        let sessions = self.sessions()?;
        let project_of = |coordinator: &str| {
            let session = sessions.iter().find(|s| s.id == coordinator)?;
            projects.iter().find(|p| p.id == session.project_id)
        };
        let tickets = self.tickets()?;
        let mut checks = vec![];
        for repository in self.repositories()? {
            let branches: Vec<_> = tickets
                .iter()
                .filter(|t| t.is_open() && t.repository_id == repository.id)
                .map(|t| OpenBranch {
                    ticket_id: t.id.clone(),
                    title: t.title.clone(),
                    project: project_of(&t.coordinator_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_default(),
                    branch: t.branch.clone(),
                })
                .collect();
            if !branches.is_empty() {
                checks.push((
                    repository.id.clone(),
                    OverlapCheck {
                        repository: repository.path.into(),
                        base: repository.base,
                        branches,
                        fetched_at: self.base_fetched_at.get(&repository.id).copied(),
                    },
                ));
            }
        }
        Ok(checks)
    }
    /// Replaces a repository's cached warnings; true when they changed.
    pub(crate) fn set_branch_warnings(
        &mut self,
        repository: String,
        fetched_at: Option<i64>,
        warnings: Vec<BranchWarnings>,
    ) -> bool {
        if let Some(at) = fetched_at {
            self.base_fetched_at.insert(repository.clone(), at);
        }
        self.branch_warnings.insert(repository, warnings.clone()) != Some(warnings)
    }
    /// Cached warnings about open tickets, without those about tickets finished since.
    pub(crate) fn open_branch_warnings(&self) -> Result<Vec<BranchWarnings>> {
        let open: HashSet<String> = self
            .tickets()?
            .into_iter()
            .filter(Ticket::is_open)
            .map(|t| t.id)
            .collect();
        Ok(self
            .branch_warnings
            .values()
            .flatten()
            .filter_map(|w| among(w, &open))
            .collect())
    }
}

/// The warnings that concern only open tickets, if any are left.
fn among(warnings: &BranchWarnings, open: &HashSet<String>) -> Option<BranchWarnings> {
    if !open.contains(&warnings.ticket_id) {
        return None;
    }
    let mut warnings = warnings.clone();
    warnings.overlaps.retain(|o| open.contains(&o.ticket_id));
    (warnings.base_ahead > 0 || !warnings.overlaps.is_empty() || !warnings.failures.is_empty())
        .then_some(warnings)
}

/// Updates the base's remote-tracking ref when the base is one, such as `origin/main`, and
/// says whether it did. Only that ref is written, whatever the remote's configured refspecs.
pub(crate) fn fetch_base(repository: &Path, base: &str) -> Result<bool> {
    let Some((remote, branch)) = base.split_once('/') else {
        return Ok(false);
    };
    if !runtime::git_output(repository, &["remote"])?
        .lines()
        .any(|r| r == remote)
    {
        return Ok(false);
    }
    let refspec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    let follow_head = format!("remote.{remote}.followRemoteHEAD=never");
    runtime::git_output(
        repository,
        &[
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
            "-c",
            &follow_head,
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-write-fetch-head",
            "--no-recurse-submodules",
            "--refmap=",
            remote,
            &refspec,
        ],
    )?;
    Ok(true)
}

/// Warnings for every branch that has any. A branch Git cannot compare, for example one not
/// created yet, is left out. A failed fetch is a warning on every branch.
pub(crate) fn check(
    check: &OverlapCheck,
    fetch_error: Option<&anyhow::Error>,
) -> Vec<BranchWarnings> {
    let repository = &check.repository;
    let stale = fetch_error.map(|error| {
        let when = match check.fetched_at {
            Some(at) => format!("as of {}", schedules::rfc3339(at)),
            None => "not fetched since the host started".into(),
        };
        format!("{} {when}; last fetch failed: {error:#}", check.base)
    });
    let changed: Vec<_> = check
        .branches
        .iter()
        .filter_map(|b| Some((b, changed_files(repository, &check.base, &b.branch).ok()?)))
        .collect();
    // Each pair is merged once, whichever branch asks first.
    let mut pairs: HashMap<(usize, usize), std::result::Result<Vec<String>, String>> =
        HashMap::new();
    let mut warnings = vec![];
    for (index, (branch, files)) in changed.iter().enumerate() {
        let Ok(base_ahead) = commits_ahead(repository, &branch.branch, &check.base) else {
            continue;
        };
        let mut failures: Vec<String> = stale.iter().cloned().collect();
        let mut base_conflicts = vec![];
        if base_ahead > 0 {
            match conflicts(repository, &branch.branch, &check.base) {
                Ok(files) => base_conflicts = files,
                Err(error) => failures.push(format!(
                    "conflict check against {} failed: {error:#}",
                    check.base
                )),
            }
        }
        let mut overlaps = vec![];
        for (other_index, (other, theirs)) in changed.iter().enumerate() {
            if other_index == index || other.branch == branch.branch {
                continue;
            }
            let shared: Vec<String> = files
                .iter()
                .filter(|f| theirs.contains(f))
                .cloned()
                .collect();
            if shared.is_empty() {
                continue;
            }
            let pair = (index.min(other_index), index.max(other_index));
            let merged = pairs.entry(pair).or_insert_with(|| {
                conflicts(repository, &branch.branch, &other.branch).map_err(|e| format!("{e:#}"))
            });
            overlaps.push(BranchOverlap {
                ticket_id: other.ticket_id.clone(),
                title: other.title.clone(),
                project: other.project.clone(),
                files: shared,
                conflicts: merged.clone().unwrap_or_default(),
                failure: merged.clone().err(),
            });
        }
        if base_ahead > 0 || !overlaps.is_empty() || !failures.is_empty() {
            warnings.push(BranchWarnings {
                ticket_id: branch.ticket_id.clone(),
                base: check.base.clone(),
                base_ahead,
                base_conflicts,
                overlaps,
                failures,
            });
        }
    }
    warnings
}

/// Files the branch changed since it left the base. A rename lists both its old and new path,
/// so two branches renaming or deleting one file are seen to overlap.
fn changed_files(repository: &Path, base: &str, branch: &str) -> Result<Vec<String>> {
    let range = format!("{base}...{branch}");
    Ok(paths(&runtime::git_output(
        repository,
        &["diff", "--name-only", "--no-renames", "-z", &range],
    )?))
}

/// Commits on the base since the branch's merge-base.
fn commits_ahead(repository: &Path, branch: &str, base: &str) -> Result<u32> {
    let range = format!("{branch}..{base}");
    Ok(
        runtime::git_output(repository, &["rev-list", "--count", &range])?
            .trim()
            .parse()?,
    )
}

/// Files that would conflict when merging the two commits, without touching any checkout.
fn conflicts(repository: &Path, ours: &str, theirs: &str) -> Result<Vec<String>> {
    let answer = runtime::git_answer(
        repository,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            ours,
            theirs,
        ],
    )?;
    match answer.status.code() {
        Some(0) => Ok(vec![]),
        // The first entry is the merged tree; conflicted files follow.
        Some(1) => Ok(paths(&answer.output).into_iter().skip(1).collect()),
        _ => bail!("git merge-tree failed{}", answer.reason()),
    }
}

fn paths(output: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    output
        .split('\0')
        .filter(|p| !p.is_empty() && seen.insert(*p))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(["-c", "user.name=F", "-c", "user.email=f@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
    /// Runs the Git commands on a new branch from `main` and commits the result.
    fn on_branch(repository: &Path, branch: &str, commands: &[&[&str]]) {
        git(repository, &["checkout", "-q", "-b", branch, "main"]);
        for command in commands {
            git(repository, command);
        }
        git(repository, &["commit", "-q", "-m", branch]);
        git(repository, &["checkout", "-q", "main"]);
    }
    /// Commits `files` with the given contents on `branch`, creating it from `main` if needed.
    fn commit(repository: &Path, branch: &str, files: &[(&str, &str)]) {
        let exists = std::process::Command::new("git")
            .current_dir(repository)
            .args(["rev-parse", "--verify", "--quiet", branch])
            .output()
            .unwrap()
            .status
            .success();
        if exists {
            git(repository, &["checkout", "-q", branch]);
        } else {
            git(repository, &["checkout", "-q", "-b", branch, "main"]);
        }
        for (name, contents) in files {
            fs::write(repository.join(name), contents).unwrap();
        }
        git(repository, &["add", "."]);
        git(repository, &["commit", "-q", "-m", branch]);
        git(repository, &["checkout", "-q", "main"]);
    }
    fn fixture() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        git(folder.path(), &["init", "-q", "-b", "main"]);
        fs::write(folder.path().join("shared.txt"), "one\n").unwrap();
        fs::write(folder.path().join("other.txt"), "one\n").unwrap();
        git(folder.path(), &["add", "."]);
        git(folder.path(), &["commit", "-q", "-m", "start"]);
        folder
    }
    fn open(ticket: &str, project: &str) -> OpenBranch {
        OpenBranch {
            ticket_id: ticket.into(),
            title: ticket.into(),
            project: project.into(),
            branch: ticket.into(),
        }
    }
    fn run(repository: &Path, base: &str, branches: Vec<OpenBranch>) -> Vec<BranchWarnings> {
        let check_of = OverlapCheck {
            repository: repository.into(),
            base: base.into(),
            branches,
            fetched_at: None,
        };
        check(&check_of, None)
    }
    fn clone_of(upstream: &Path) -> tempfile::TempDir {
        let clone = tempfile::tempdir().unwrap();
        git(
            clone.path(),
            &["clone", "-q", upstream.to_str().unwrap(), "."],
        );
        clone
    }

    #[test]
    fn base_ahead_with_a_conflict_names_the_file() {
        let repo = fixture();
        commit(repo.path(), "t1", &[("shared.txt", "ticket\n")]);
        commit(repo.path(), "main", &[("shared.txt", "main\n")]);
        commit(repo.path(), "main", &[("new.txt", "main\n")]);
        let warnings = run(repo.path(), "main", vec![open("t1", "P")]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].base_ahead, 2);
        assert_eq!(warnings[0].base_conflicts, ["shared.txt"]);
        assert_eq!(
            warnings[0].lines(),
            ["main is 2 commits ahead; conflicts in shared.txt"]
        );
    }

    #[test]
    fn base_ahead_without_a_conflict_is_fetched_and_counted() {
        let upstream = fixture();
        let clone = clone_of(upstream.path());
        commit(clone.path(), "t1", &[("shared.txt", "ticket\n")]);
        commit(upstream.path(), "main", &[("other.txt", "main\n")]);
        assert!(fetch_base(clone.path(), "origin/main").unwrap());
        let warnings = run(clone.path(), "origin/main", vec![open("t1", "P")]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].base_ahead, 1);
        assert!(warnings[0].base_conflicts.is_empty());
        assert_eq!(warnings[0].lines(), ["origin/main is 1 commit ahead"]);
    }

    #[test]
    fn two_tickets_changing_one_file_overlap_across_projects() {
        let repo = fixture();
        commit(repo.path(), "t1", &[("shared.txt", "first\n")]);
        commit(
            repo.path(),
            "t2",
            &[("shared.txt", "second\n"), ("other.txt", "two\n")],
        );
        let warnings = run(repo.path(), "main", vec![open("t1", "P"), open("t2", "Q")]);
        assert_eq!(warnings.len(), 2);
        let first = warnings.iter().find(|w| w.ticket_id == "t1").unwrap();
        assert_eq!(first.base_ahead, 0);
        assert_eq!(first.overlaps.len(), 1);
        assert_eq!(first.overlaps[0].files, ["shared.txt"]);
        assert_eq!(first.overlaps[0].conflicts, ["shared.txt"]);
        assert_eq!(
            first.lines(),
            ["overlaps ticket t2 (project Q) in shared.txt; conflicts in shared.txt"]
        );
        let second = warnings.iter().find(|w| w.ticket_id == "t2").unwrap();
        assert_eq!(second.overlaps[0].project, "P");
    }

    #[test]
    fn current_branches_on_separate_files_have_no_warnings() {
        let repo = fixture();
        commit(repo.path(), "t1", &[("shared.txt", "first\n")]);
        commit(repo.path(), "t2", &[("other.txt", "second\n")]);
        let mut branches = vec![open("t1", "P"), open("t2", "P")];
        // A ticket whose branch does not exist yet is skipped, not an error.
        branches.push(open("t3", "P"));
        assert!(run(repo.path(), "main", branches).is_empty());
    }

    #[test]
    fn fetching_writes_only_the_bases_remote_tracking_ref_whatever_the_refspecs_say() {
        let upstream = fixture();
        let clone = clone_of(upstream.path());
        git(clone.path(), &["branch", "victim", "main"]);
        git(
            clone.path(),
            &[
                "config",
                "--add",
                "remote.origin.fetch",
                "+refs/heads/main:refs/heads/victim",
            ],
        );
        let local = |clone: &Path| {
            (
                git(clone, &["for-each-ref", "refs/heads"]),
                git(clone, &["symbolic-ref", "HEAD"]),
                git(clone, &["rev-parse", "HEAD"]),
                fs::read(clone.join(".git/index")).unwrap(),
            )
        };
        let before = local(clone.path());
        commit(upstream.path(), "main", &[("other.txt", "main\n")]);
        assert!(fetch_base(clone.path(), "origin/main").unwrap());
        assert!(local(clone.path()) == before);
        assert_eq!(
            git(clone.path(), &["rev-parse", "origin/main"]),
            git(upstream.path(), &["rev-parse", "main"])
        );
        assert!(!clone.path().join(".git/FETCH_HEAD").exists());
    }

    #[test]
    fn a_failed_fetch_is_a_warning_naming_the_last_good_fetch() {
        let upstream = fixture();
        let clone = clone_of(upstream.path());
        commit(clone.path(), "t1", &[("shared.txt", "ticket\n")]);
        git(
            clone.path(),
            &["remote", "set-url", "origin", "/nonexistent/upstream"],
        );
        let error = fetch_base(clone.path(), "origin/main").unwrap_err();
        let stale = OverlapCheck {
            repository: clone.path().into(),
            base: "origin/main".into(),
            branches: vec![open("t1", "P")],
            fetched_at: Some(0),
        };
        let warnings = check(&stale, Some(&error));
        let lines = warnings[0].lines();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with(
                "origin/main as of 1970-01-01T00:00:00Z; last fetch failed: Git query failed: fatal:"
            ),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn renaming_or_deleting_one_file_on_two_branches_is_an_overlap_and_a_conflict() {
        let repo = fixture();
        on_branch(repo.path(), "t1", &[&["mv", "shared.txt", "b.txt"]]);
        on_branch(repo.path(), "t2", &[&["mv", "shared.txt", "c.txt"]]);
        on_branch(repo.path(), "t3", &[&["rm", "-q", "shared.txt"]]);
        for other in ["t2", "t3"] {
            let warnings = run(repo.path(), "main", vec![open("t1", "P"), open(other, "P")]);
            let first = warnings.iter().find(|w| w.ticket_id == "t1").unwrap();
            assert_eq!(first.overlaps[0].files, ["shared.txt"], "{other}");
            assert!(!first.overlaps[0].conflicts.is_empty(), "{other}");
        }
    }

    #[test]
    fn a_conflict_check_git_cannot_run_is_reported_not_taken_as_clean() {
        let repo = fixture();
        git(repo.path(), &["checkout", "-q", "--orphan", "lone"]);
        git(repo.path(), &["commit", "-q", "-m", "lone"]);
        git(repo.path(), &["checkout", "-q", "main"]);
        let reason = format!("{:#}", conflicts(repo.path(), "main", "lone").unwrap_err());
        assert!(
            reason.starts_with("git merge-tree failed: fatal:"),
            "{reason}"
        );
        let warnings = BranchWarnings {
            overlaps: vec![BranchOverlap {
                title: "t2".into(),
                project: "P".into(),
                files: vec!["a.rs".into()],
                failure: Some(reason.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            warnings.lines(),
            [format!(
                "overlaps ticket t2 (project P) in a.rs; conflict check failed: {reason}"
            )]
        );
    }
}
