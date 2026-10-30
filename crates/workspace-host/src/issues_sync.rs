//! The issue rule set of a storage repository's sync engine (OpenSpec change `shared-issues`,
//! task 1.4): `wiffletree-issues.yaml` and `issues/`, merged as whole files against what this
//! Mac last synced until the per-field merge exists. Changes from anyone are pulled, and a file
//! that does not parse is taken as it is; a file here that Wiffletree did not write is paused
//! and never pushed. New issues carry a temporary ID until a pass numbers them on top of
//! GitHub's head, in the content it commits; the host renames them here once they are pushed.
use crate::*;
use repos::{Content, RuleSet};
use std::collections::BTreeMap;
use storage::{Found, Input, TRACKER, WholeFiles};
use tracker::{ISSUES, issue_folder, issue_of};

/// How long after the human's last change to an issue its push waits.
pub(crate) const PUSH_DELAY_MS: i64 = 2_000;
/// How many times one pass pushes again after GitHub refused because the branch moved.
pub(crate) const PUSH_TRIES: usize = 5;

/// `text` with each whole `old`, an issue ID, written `new`; `CEL-new-abcd` in
/// `XCEL-new-abcde` is left.
pub(crate) fn replace_id(text: &str, old: &str, new: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(old) {
        let before = rest[..at].chars().next_back();
        let after = &rest[at + old.len()..];
        out.push_str(&rest[..at]);
        match before.is_some_and(word) || after.chars().next().is_some_and(word) {
            true => out.push_str(old),
            false => out.push_str(new),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The issue rule set for one pass.
pub(crate) struct IssueRules {
    pub(crate) files: WholeFiles,
    /// The tracker's prefix, `None` before the repository is a tracker here.
    prefix: Option<String>,
    /// The clone and GitHub's head, as `compare` saw them.
    compared: Option<(PathBuf, String)>,
}
impl IssueRules {
    pub(crate) fn new(input: &Input) -> Self {
        let roots = vec![TRACKER.into(), ISSUES.into()];
        Self {
            files: WholeFiles::new(input, roots, "Issues".into()),
            prefix: input.prefix.clone(),
            compared: None,
        }
    }
    /// Numbers this Mac's new issues on top of GitHub's head: each takes one more than the
    /// highest number its history has had, unless GitHub already holds it, as when this Mac
    /// pushed it and stopped before renaming it here. Rewrites each temporary ID in the paths
    /// and content committed.
    fn number(&mut self) -> Result<()> {
        let (Some(prefix), Some((clone, remote))) = (&self.prefix, &self.compared) else {
            return Ok(());
        };
        let committed = &mut self.files.found.committed;
        let mut temporary: Vec<IssueId> = (committed.iter())
            .filter_map(|(path, _)| issue_of(path))
            .filter(|id| id.is_temporary() && id.prefix() == prefix)
            .collect();
        temporary.sort();
        temporary.dedup();
        if temporary.is_empty() {
            return Ok(());
        }
        let synced = &self.files.synced;
        let pushed: BTreeMap<String, Vec<u8>> = repos::files_at(clone, remote, &|path| {
            path.ends_with("/issue.md") && !synced.contains_key(path)
        })?;
        let mut next = tracker::highest_number(clone, remote, prefix)?;
        let mut numbered = vec![];
        for id in temporary {
            let mine = (committed.iter())
                .find(|(path, _)| *path == format!("{}/issue.md", issue_folder(&id)))
                .and_then(|(_, content)| content.clone());
            let already = (pushed.iter()).find_map(|(path, theirs)| {
                let there = issue_of(path)?;
                let renamed = replace_id(
                    &String::from_utf8_lossy(mine.as_deref()?),
                    &id.to_string(),
                    &there.to_string(),
                );
                (renamed.as_bytes() == theirs.as_slice()).then_some(there)
            });
            let number = already.unwrap_or_else(|| {
                next += 1;
                IssueId::Numbered {
                    prefix: prefix.clone(),
                    number: next,
                }
            });
            numbered.push((id, number));
        }
        for (path, content) in committed.iter_mut() {
            for (old, new) in &numbered {
                let (old, new) = (old.to_string(), new.to_string());
                *path = replace_id(path, &old, &new);
                if let Some(bytes) = content
                    && let Ok(text) = std::str::from_utf8(bytes)
                {
                    *bytes = replace_id(text, &old, &new).into_bytes();
                }
            }
        }
        // A new issue GitHub already holds needs no commit of what is the same there.
        let there = repos::files_at(clone, remote, &|path| path.starts_with("issues/"))?;
        committed.retain(|(path, content)| there.get(path) != content.as_ref());
        self.files.found.numbered = numbered;
        Ok(())
    }
    /// The commit's message: each issue and what changed in it, by ID only.
    fn message(&self) -> String {
        let found = &self.files.found;
        let mut changes: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for (path, content) in &found.committed {
            let (subject, change) = match issue_of(path) {
                None => ("the tracker".to_owned(), "changed"),
                Some(id) => {
                    let new = found.numbered.iter().any(|(_, number)| number == &id);
                    let change = match (path.rsplit('/').next(), content) {
                        (_, None) => "removed",
                        (Some("issue.md"), _) if new => "filed",
                        (Some("issue.md"), _) => "edited",
                        (Some("events.jsonl"), _) => "updated",
                        _ if path.contains("/comments/") => "commented",
                        _ => "changed",
                    };
                    (id.to_string(), change)
                }
            };
            let listed = changes.entry(subject).or_default();
            if !listed.contains(&change) {
                listed.push(change);
            }
        }
        let parts: Vec<String> = (changes.into_iter())
            .map(|(subject, changes)| match subject.as_str() {
                "the tracker" => "changed the tracker".to_owned(),
                _ => format!("{subject} {}", changes.join(", ")),
            })
            .collect();
        format!("Issues: {} · {}", parts.join("; "), sync::machine_name())
    }
}
impl RuleSet for IssueRules {
    fn claims(&self, path: &str) -> bool {
        self.files.claims(path)
    }
    fn compare(&mut self, clone: &Path, remote: &str) -> Result<()> {
        self.files.compare(clone, remote)?;
        self.compared = Some((clone.to_owned(), remote.to_owned()));
        Ok(())
    }
    fn commit(&mut self) -> Option<(Vec<(String, Content)>, String)> {
        if let Err(error) = self.number() {
            // Nothing of a pass that cannot number its new issues is pushed.
            self.files.found.committed.clear();
            self.files.failed(error);
            return None;
        }
        let committed = self.files.found.committed.clone();
        (!committed.is_empty()).then(|| (committed, self.message()))
    }
    fn failed(&mut self, error: anyhow::Error) {
        self.files.failed(error);
    }
    /// A new issue whose `issue.md` a scan holds stays wholly on this Mac, unnumbered.
    fn also_held(&self, held: &[scan::Held]) -> Vec<scan::Held> {
        let found = &self.files.found;
        let mut also = vec![];
        for (_, number) in &found.numbered {
            let file = format!("{}/issue.md", issue_folder(number));
            let Some(cause) = held.iter().find(|held| held.path == file) else {
                continue;
            };
            let folder = format!("{}/", issue_folder(number));
            for (path, _) in &found.committed {
                if path.starts_with(&folder) && !held.iter().any(|h| &h.path == path) {
                    also.push(scan::Held {
                        path: path.clone(),
                        check: cause.check.clone(),
                        rule: None,
                        reason: Some("its issue’s issue.md is held".into()),
                    });
                }
            }
        }
        also
    }
}

/// Puts the paths of `held` that a new issue's number names back under its temporary ID, as
/// the issue stays here unnumbered, and forgets those numbers.
pub(crate) fn keep_held_unnumbered(found: &mut Found, held: &mut [scan::Held]) {
    let mut kept = vec![];
    for (temporary, number) in &found.numbered {
        let file = format!("{}/issue.md", issue_folder(number));
        if held.iter().any(|held| held.path == file) {
            kept.push((temporary.clone(), number.clone()));
        }
    }
    for (temporary, number) in &kept {
        let (from, to) = (
            format!("{}/", issue_folder(number)),
            format!("{}/", issue_folder(temporary)),
        );
        for held in held.iter_mut() {
            if let Some(rest) = held.path.strip_prefix(&from) {
                held.path = format!("{to}{rest}");
            }
        }
        found.committed.retain(|(path, _)| !path.starts_with(&from));
    }
    found.numbered.retain(|numbered| !kept.contains(numbered));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::tests::{host, join_sage, shared_storage, space_named};
    use crate::sync::tests::{GitHub, NAME};
    use crate::test_support::git;
    use crate::tracker::tests::{blurry, id};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;
    use tracker::issue_md;

    /// This Mac, signed in to `gh` as `jlin`, with Sage joined from `github`.
    fn sage_here(github: &Arc<GitHub>) -> (TempDir, Host, Space) {
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, github).unwrap();
        let sage = space_named(&host, "Sage");
        (home, host, sage)
    }

    fn file(host: &mut Host, space: &Space, title: &str, description: &str) -> Issue {
        let new = NewIssue {
            title: title.into(),
            description: description.into(),
            ..NewIssue::default()
        };
        host.create_issue(&space.id, new).unwrap()
    }

    fn sync(host: &mut Host, github: &Arc<GitHub>) -> Result<()> {
        host.sync_issues(None, &github.tools())
    }

    fn listed(host: &Host, space: &Space) -> Vec<String> {
        (host.list_issues(Some(&space.id)).unwrap().into_iter())
            .map(|listing| listing.id.to_string())
            .collect()
    }

    fn clone_of(host: &Host) -> PathBuf {
        repos::clone_folder(&host.home, NAME)
    }

    /// A teammate's issue as their Mac pushed it.
    fn theirs(text: &str) -> Vec<u8> {
        issue_md(&blurry(text)).unwrap()
    }

    fn delete_on(bare: &Path, folder: &str) {
        let work = tempfile::tempdir().unwrap();
        git(work.path(), &["clone", "-q", bare.to_str().unwrap(), "."]);
        git(work.path(), &["rm", "-rq", folder]);
        git(work.path(), &["commit", "-qm", "Remove an issue"]);
        git(work.path(), &["push", "-q", "origin", "main"]);
    }

    #[test]
    fn when_another_mac_pushes_first_the_same_pass_fetches_and_takes_the_next_number() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host, sage) = sage_here(&github);
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        assert!(filed.id.is_temporary());
        assert_eq!(listed(&host, &sage), [filed.id.to_string()]);
        let other = github.clone();
        *github.before.lock().unwrap() = Some((
            "push",
            Box::new(move || other.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"))),
        ));
        sync(&mut host, &github).unwrap();
        let pushed = github.show("issues/SAGE-2/issue.md").unwrap();
        assert!(
            pushed.contains("id: SAGE-2\ntitle: Watch app drops replies\n"),
            "{pushed}"
        );
        let subject = &github.log("%s")[0];
        assert_eq!(
            subject,
            &format!("Issues: SAGE-2 filed · {}", sync::machine_name())
        );
        assert_eq!(listed(&host, &sage), ["SAGE-1", "SAGE-2"]);
        let clone = clone_of(&host);
        assert!(!clone.join(issue_folder(&filed.id)).exists());
        assert_eq!(
            fs::read_to_string(clone.join("issues/SAGE-2/issue.md")).unwrap(),
            pushed
        );
        let tracker = &host.issue_trackers().unwrap()[1];
        assert_eq!((tracker.issues, tracker.waiting), (2, 0));
    }

    #[test]
    fn an_issue_filed_offline_is_numbered_when_pushed_everywhere_this_mac_wrote_its_id() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        // Filed by a teammate through GitHub's web editor.
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        assert_eq!(
            listed(&host, &sage),
            ["SAGE-1"],
            "a hand-added issue is listed"
        );
        github.offline.store(true, Ordering::Relaxed);
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        let mut first = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        first.related.push(filed.id.to_string());
        host.edit_issue(first).unwrap();
        assert!(sync(&mut host, &github).is_err());
        let waiting = host.issue_trackers().unwrap()[1].waiting;
        assert_eq!(waiting, 2, "the new issue and the edit wait");
        github.offline.store(false, Ordering::Relaxed);
        sync(&mut host, &github).unwrap();
        let pushed = github.show("issues/SAGE-2/issue.md").unwrap();
        assert!(pushed.contains("id: SAGE-2\n"), "{pushed}");
        let related = github.show("issues/SAGE-1/issue.md").unwrap();
        assert!(
            related.contains("related:\n- CEL-148\n- SAGE-2\n"),
            "{related}"
        );
        assert!(!related.contains("-new-"));
        let here = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert_eq!(here.related, ["CEL-148", "SAGE-2"]);
        assert_eq!(
            host.read_issue(&id("SAGE-2")).unwrap().issue.unwrap().id,
            id("SAGE-2")
        );
        assert_eq!(host.issue_trackers().unwrap()[1].waiting, 0);
        sync(&mut host, &github).unwrap();
        assert_eq!(github.log("%s").len(), 5, "nothing is pushed twice");
    }

    #[test]
    fn a_number_an_ordinary_commit_deleted_is_never_handed_out_again() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        github.commit("issues/SAGE-2/issue.md", theirs("SAGE-2"));
        delete_on(&github.bare, "issues/SAGE-2");
        let (_home, mut host, sage) = sage_here(&github);
        file(&mut host, &sage, "Watch app drops replies", "");
        sync(&mut host, &github).unwrap();
        assert_eq!(listed(&host, &sage), ["SAGE-1", "SAGE-3"]);
        assert!(github.show("issues/SAGE-2/issue.md").is_none());
    }

    #[test]
    fn a_teammates_issue_that_does_not_parse_is_pulled_listed_with_its_problem_and_not_paused() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host, sage) = sage_here(&github);
        let raw = "---\ntitle: [unclosed\n---\nWritten by hand.\n";
        github.commit("issues/SAGE-7/issue.md", raw);
        sync(&mut host, &github).unwrap();
        let listing = host.list_issues(Some(&sage.id)).unwrap().remove(0);
        assert_eq!(listing.id, id("SAGE-7"));
        assert!(listing.problem.is_some());
        let record = host.read_issue(&id("SAGE-7")).unwrap();
        assert_eq!((record.raw.as_deref(), record.paused), (Some(raw), vec![]));
        assert_eq!(host.issue_trackers().unwrap()[1].paused, 0);
    }

    #[test]
    fn an_edit_in_the_clone_that_wiffletree_did_not_make_is_paused_and_never_pushed() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, _) = sage_here(&github);
        // As an agent's shell might.
        let file = clone_of(&host).join("issues/SAGE-1/issue.md");
        let edited = fs::read_to_string(&file)
            .unwrap()
            .replace("priority: high", "priority: urgent");
        fs::write(&file, &edited).unwrap();
        sync(&mut host, &github).unwrap();
        assert!(
            github
                .show("issues/SAGE-1/issue.md")
                .unwrap()
                .contains("priority: high")
        );
        let paused = host.read_issue(&id("SAGE-1")).unwrap().paused;
        assert_eq!(
            paused,
            [PausedIssueFile {
                path: "issues/SAGE-1/issue.md".into(),
                why: Paused::OutsideWiffletree,
            }]
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            edited,
            "this Mac keeps it"
        );
    }

    #[test]
    fn a_new_issue_whose_text_a_scan_holds_stays_here_whole_and_unnumbered() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host, sage) = sage_here(&github);
        let phone = "Call the parent at (415) 555-0134 tonight.";
        let filed = file(&mut host, &sage, "Watch app drops replies", phone);
        let comment = host
            .comment_on_issue(&filed.id, "Seen on two watches.")
            .unwrap();
        assert!(comment.file.ends_with("-jlin.md") && comment.author == "jlin");
        let commits = github.log("%H").len();
        sync(&mut host, &github).unwrap();
        assert_eq!(
            github.log("%H").len(),
            commits,
            "nothing of the issue is pushed"
        );
        let held: Vec<String> = (host.held_notes().unwrap().into_iter())
            .map(|note| match note {
                WorkspaceNote::Held(held) => held.path,
                other => panic!("{other:?}"),
            })
            .collect();
        let folder = issue_folder(&filed.id);
        assert!(held.contains(&format!("{folder}/issue.md")), "{held:?}");
        assert!(
            held.iter().all(|path| path.starts_with(&folder)),
            "{held:?}"
        );
        assert_eq!(held.len(), 2, "{held:?}");
        assert_eq!(listed(&host, &sage), [filed.id.to_string()]);
        let mut clean = host.read_issue(&filed.id).unwrap().issue.unwrap();
        clean.description = "Replies vanish after a minute.".into();
        host.edit_issue(clean).unwrap();
        sync(&mut host, &github).unwrap();
        assert!(github.show("issues/SAGE-1/issue.md").is_some());
        let comment = format!("issues/SAGE-1/comments/{}", comment.file);
        assert!(github.show(&comment).is_some());
        assert!(host.held_notes().unwrap().is_empty());
    }

    #[test]
    fn a_local_tracker_numbers_each_issue_when_filed_and_commits_it_here() {
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let personal = host.spaces().unwrap().remove(0).space;
        let prefix = host.tracker(&personal).unwrap().prefix;
        let first = file(&mut host, &personal, "Renew the passport", "");
        let second = file(&mut host, &personal, "Book the dentist", "");
        assert_eq!(first.id.to_string(), format!("{prefix}-1"));
        assert_eq!(second.id.to_string(), format!("{prefix}-2"));
        host.move_issue(&second.id, IssueState::Todo).unwrap();
        let folder = host.storage_folder(&personal.storage.repository);
        let subjects = git(&folder, &["log", "--format=%s", "-3"]);
        assert_eq!(
            subjects,
            format!("{prefix}-2: moved to Todo\n{prefix}-2: filed\n{prefix}-1: filed")
        );
        assert!(git(&folder, &["remote"]).is_empty(), "never pushed");
        let events = host.read_issue(&second.id).unwrap().events;
        assert_eq!(events.len(), 1);
        assert_eq!((&*events[0].kind, &*events[0].by), ("state", "jlin"));
        assert_eq!(events[0].fields["to"], "todo");
    }

    #[test]
    fn a_space_edit_and_an_issue_edit_in_one_pass_make_two_commits() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        let comment = host
            .comment_on_issue(&id("SAGE-1"), "Seen on two watches.")
            .unwrap();
        host.update_space(Space {
            description: "Secure family messaging, watch included".into(),
            ..sage
        })
        .unwrap();
        sync(&mut host, &github).unwrap();
        let machine = sync::machine_name();
        let subjects = github.log("%s");
        assert!(subjects[..2].contains(&format!("Issues: SAGE-1 commented · {machine}")));
        assert!(subjects[..2].contains(&format!("Sage: changed the Space · {machine}")));
        let comment = format!("issues/SAGE-1/comments/{}", comment.file);
        for commit in ["main", "main~"] {
            let changed = git(&github.bare, &["show", "--format=", "--name-only", commit]);
            assert!(
                changed == comment || changed == "spaces/sage/space.yaml",
                "{changed}"
            );
        }
    }

    #[test]
    fn without_a_gh_login_issues_are_read_and_nothing_is_written() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host) = host();
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let new = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        let error = host.create_issue(&sage.id, new).unwrap_err();
        assert!(format!("{error:#}").contains("Sign gh in"), "{error:#}");
        assert!(host.comment_on_issue(&id("SAGE-1"), "Seen.").is_err());
        assert_eq!(listed(&host, &sage), ["SAGE-1"]);
        let issues = fs::read_dir(clone_of(&host).join("issues"))
            .unwrap()
            .count();
        assert_eq!(issues, 1);
        assert!(!clone_of(&host).join("issues/SAGE-1/comments").exists());
    }

    #[test]
    fn a_missing_index_is_rebuilt_from_the_files() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        file(&mut host, &sage, "Watch app drops replies", "");
        let before = host.list_issues(None).unwrap();
        assert_eq!(before.len(), 2);
        host.db
            .execute_batch("DROP TABLE issue_index; DROP TABLE issue_index_format;")
            .unwrap();
        host.db.execute_batch(include_str!("issues.sql")).unwrap();
        host.rebuild_issue_index().unwrap();
        assert_eq!(host.list_issues(None).unwrap(), before);
    }

    #[test]
    fn a_temporary_id_is_rewritten_only_where_it_is_a_whole_word() {
        let text = "SAGE-new-abcd, XSAGE-new-abcd and SAGE-new-abcde";
        assert_eq!(
            replace_id(text, "SAGE-new-abcd", "SAGE-9"),
            "SAGE-9, XSAGE-new-abcd and SAGE-new-abcde"
        );
    }
}
