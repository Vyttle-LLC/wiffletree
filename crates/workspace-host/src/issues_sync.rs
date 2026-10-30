//! The issue rule set of a storage repository's sync engine (OpenSpec change `shared-issues`,
//! tasks 1.4 and 5.2): `wiffletree-issues.yaml` and `issues/`, merged against what this Mac last
//! synced. Changes from anyone are pulled. An `issue.md` both sides changed is merged three-way
//! one front matter field and the description at a time: a part changed on both sides to
//! different values pauses alone, keeping this Mac's value here and pushing the other side's,
//! while the rest of the file syncs. `events.jsonl` takes the union of both sides' lines. Any
//! other file both sides changed, a comment among them, and an `issue.md` whose versions do not
//! all parse pause whole. A file that does not parse is taken as it is; a file here that
//! Wiffletree did not write is paused and never pushed. New issues carry a temporary ID until a
//! pass numbers them on top of GitHub's head, in the content it commits; the host renames them
//! here once they are pushed.
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

/// The part of `issue.md` that is its body; every other part is a front matter field by key.
pub(crate) const DESCRIPTION: &str = "description";

/// A file both sides changed, merged: what this Mac keeps, what it pushes, and each part that
/// paused with the other side's value as written. `None` for a file that is not merged so: one
/// not changed on both sides without a paused part, or one whose versions do not all parse,
/// which pauses whole.
pub(crate) type MergedFile = (Vec<u8>, Vec<u8>, Vec<(String, Option<String>)>);

pub(crate) fn merge_file(
    path: &str,
    base: Option<&[u8]>,
    mine: &[u8],
    theirs: &[u8],
    paused: &BTreeMap<String, Vec<String>>,
) -> Option<MergedFile> {
    let both = base != Some(mine) && base != Some(theirs);
    if path.ends_with("/issue.md") && (both || paused.contains_key(path)) {
        let paused = paused.get(path).map_or(&[][..], Vec::as_slice);
        return merge_issue(base, mine, theirs, paused).ok();
    }
    if path.ends_with("/events.jsonl") && both {
        let union = union_events(mine, theirs);
        return Some((union.clone(), union, vec![]));
    }
    None
}

/// `issue.md`'s parts: each front matter field by key, and the description.
fn parts(content: &[u8]) -> Result<BTreeMap<String, serde_yaml::Value>> {
    let (front, description) = tracker::issue_parts(content)?;
    let mut parts = BTreeMap::new();
    for (key, value) in front {
        let key = key.as_str().context("A front matter key is not text")?;
        ensure!(
            key != DESCRIPTION,
            "The front matter has a description field"
        );
        parts.insert(key.to_owned(), value);
    }
    parts.insert(DESCRIPTION.into(), serde_yaml::Value::String(description));
    Ok(parts)
}

/// A part's value as written: the description's text, or a field's YAML.
fn part_text(part: &str, value: &serde_yaml::Value) -> String {
    match (part, value) {
        (DESCRIPTION, serde_yaml::Value::String(text)) => text.clone(),
        _ => (serde_yaml::to_string(value).unwrap_or_default().trim_end()).to_owned(),
    }
}

/// The value `part_text` wrote.
fn part_value(part: &str, text: &str) -> Result<serde_yaml::Value> {
    Ok(match part {
        DESCRIPTION => serde_yaml::Value::String(text.to_owned()),
        _ => serde_yaml::from_str(text)?,
    })
}

/// The part `part` of `issue.md`, as written; `None` when it has none.
pub(crate) fn issue_part(content: &[u8], part: &str) -> Result<Option<String>> {
    Ok(parts(content)?
        .get(part)
        .map(|value| part_text(part, value)))
}

/// `issue.md` with `part` set to `value`, or removed for `None`.
pub(crate) fn with_part(content: &[u8], part: &str, value: Option<&str>) -> Result<Vec<u8>> {
    let mut parts = parts(content)?;
    match value {
        Some(text) => parts.insert(part.to_owned(), part_value(part, text)?),
        None => parts.remove(part),
    };
    render(&parts)
}

fn render(parts: &BTreeMap<String, serde_yaml::Value>) -> Result<Vec<u8>> {
    let mut front = serde_yaml::Mapping::new();
    for (key, value) in parts.iter().filter(|(key, _)| *key != DESCRIPTION) {
        front.insert(serde_yaml::Value::String(key.clone()), value.clone());
    }
    let description = parts.get(DESCRIPTION).and_then(|d| d.as_str());
    tracker::issue_from_parts(&front, description.unwrap_or_default())
}

/// Merges `issue.md` three-way, one part at a time: a part changed on one side takes that
/// side's value; one changed on both sides to different values, or `paused` before and still
/// different, keeps this Mac's value here and pushes the other side's. A file is written anew
/// only where its parts changed, so an untouched one keeps its bytes.
fn merge_issue(
    base: Option<&[u8]>,
    mine: &[u8],
    theirs: &[u8],
    paused: &[String],
) -> Result<MergedFile> {
    let base = base.map(parts).transpose()?.unwrap_or_default();
    let (m, t) = (parts(mine)?, parts(theirs)?);
    let names: std::collections::BTreeSet<&String> =
        (base.keys()).chain(m.keys()).chain(t.keys()).collect();
    let (mut here, mut pushed, mut held) = (BTreeMap::new(), BTreeMap::new(), vec![]);
    for name in names {
        let (b, mv, tv) = (base.get(name), m.get(name), t.get(name));
        let (kept, push) = if mv == tv {
            (mv, tv)
        } else if paused.contains(name) || (mv != b && tv != b) {
            held.push((name.clone(), tv.map(|value| part_text(name, value))));
            (mv, tv)
        } else if mv == b {
            (tv, tv)
        } else {
            (mv, mv)
        };
        if let Some(kept) = kept {
            here.insert(name.clone(), kept.clone());
        }
        if let Some(push) = push {
            pushed.insert(name.clone(), push.clone());
        }
    }
    let as_written =
        |merged: &BTreeMap<String, serde_yaml::Value>, side, bytes: &[u8]| match merged == side {
            true => Ok(bytes.to_vec()),
            false => render(merged),
        };
    Ok((
        as_written(&here, &m, mine)?,
        as_written(&pushed, &t, theirs)?,
        held,
    ))
}

/// `events.jsonl` as the union of both sides' lines, each once, ordered by their `at`; a line
/// that does not parse keeps its place among those before it.
fn union_events(mine: &[u8], theirs: &[u8]) -> Vec<u8> {
    let (mine, theirs) = (
        String::from_utf8_lossy(mine),
        String::from_utf8_lossy(theirs),
    );
    let mut lines: Vec<&str> = vec![];
    for line in mine.lines().chain(theirs.lines()) {
        if !line.trim().is_empty() && !lines.contains(&line) {
            lines.push(line);
        }
    }
    let mut at = String::new();
    let mut keyed: Vec<(String, &str)> = (lines.into_iter())
        .map(|line| {
            if let Ok(event) = serde_json::from_str::<IssueEvent>(line) {
                at = event.at;
            }
            (at.clone(), line)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (_, line) in keyed {
        out.push_str(line);
        out.push('\n');
    }
    out.into_bytes()
}

/// Who last changed `path` on GitHub's head `remote`: its commit's author.
pub(crate) fn last_author(clone: &Path, remote: &str, path: &str) -> String {
    let literal = format!(":(literal){path}");
    let args = ["log", "-1", "--format=%an", remote, "--", &literal];
    let author = runtime::git_output(clone, &args).unwrap_or_default();
    match author.trim() {
        "" => "a teammate".into(),
        author => author.to_owned(),
    }
}

/// The issue rule set for one pass.
pub(crate) struct IssueRules {
    pub(crate) files: WholeFiles,
    /// The tracker's prefix, `None` before the repository is a tracker here.
    prefix: Option<String>,
    /// The clone and GitHub's head, as `compare` saw them.
    compared: Option<(PathBuf, String)>,
    /// The committed files outside a new issue's folder whose content names its number.
    references: Vec<(String, Vec<IssueId>)>,
}
impl IssueRules {
    pub(crate) fn new(input: &Input) -> Self {
        let roots = vec![TRACKER.into(), ISSUES.into()];
        Self {
            files: WholeFiles::new(input, roots, "Issues".into()),
            prefix: input.prefix.clone(),
            compared: None,
            references: vec![],
        }
    }
    /// Numbers this Mac's new issues on top of GitHub's head: each takes one more than the
    /// highest number its history has had, unless GitHub already holds it, as when this Mac
    /// pushed it and stopped before renaming it here; that one is adopted, renamed here
    /// whether or not the pass pushes. Rewrites each temporary ID in the paths and content
    /// committed.
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
        let (mut numbered, mut adopted) = (vec![], vec![]);
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
            match already {
                Some(there) => adopted.push((id, there)),
                None => {
                    next += 1;
                    let number = IssueId::Numbered {
                        prefix: prefix.clone(),
                        number: next,
                    };
                    numbered.push((id, number));
                }
            }
        }
        for (path, content) in committed.iter_mut() {
            let mut named = vec![];
            for (old, new) in numbered.iter().chain(&adopted) {
                let own = path.starts_with(&format!("{}/", issue_folder(old)));
                let (old, new) = (old.to_string(), new.to_string());
                *path = replace_id(path, &old, &new);
                if let Some(bytes) = content
                    && let Ok(text) = std::str::from_utf8(bytes)
                {
                    let rewritten = replace_id(text, &old, &new);
                    if !own && rewritten != text {
                        named.push(IssueId::parse(&new)?);
                    }
                    *bytes = rewritten.into_bytes();
                }
            }
            if !named.is_empty() {
                self.references.push((path.clone(), named));
            }
        }
        // A new issue GitHub already holds needs no commit of what is the same there.
        let there = repos::files_at(clone, remote, &|path| path.starts_with("issues/"))?;
        committed.retain(|(path, content)| there.get(path) != content.as_ref());
        self.files.found.numbered = numbered;
        self.files.found.adopted = adopted;
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
    /// A new issue whose `issue.md` is held, or that a held file names, stays wholly on this
    /// Mac, unnumbered, and so does every file that would name its number. Holding those may
    /// hold more new issues in turn, so this repeats until nothing more is held.
    fn also_held(&self, held: &[scan::Held]) -> Vec<scan::Held> {
        let found = &self.files.found;
        let names = |path: &str, number: &IssueId| {
            (self.references.iter()).any(|(named, ids)| named == path && ids.contains(number))
        };
        let mut also: Vec<scan::Held> = vec![];
        let mut kept: Vec<&IssueId> = vec![];
        loop {
            let all = || held.iter().chain(&also);
            let next = found.numbered.iter().find_map(|(_, number)| {
                let file = format!("{}/issue.md", issue_folder(number));
                let cause = all().find(|h| h.path == file || names(&h.path, number))?;
                (!kept.contains(&number)).then(|| (number, cause.check.clone()))
            });
            let Some((number, check)) = next else {
                return also;
            };
            kept.push(number);
            let folder = format!("{}/", issue_folder(number));
            for (path, _) in &found.committed {
                let (own, naming) = (path.starts_with(&folder), names(path, number));
                if (own || naming) && !held.iter().chain(&also).any(|h| &h.path == path) {
                    let reason = match own {
                        true => "its new issue is held",
                        false => "it names a new issue that is held",
                    };
                    also.push(scan::Held {
                        path: path.clone(),
                        check: check.clone(),
                        rule: None,
                        reason: Some(reason.into()),
                    });
                }
            }
        }
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
        (host.list_issues(Some(&space.id), None).unwrap().into_iter())
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
        let followed = host.read_issue(&filed.id).unwrap();
        assert_eq!(
            followed.id,
            id("SAGE-2"),
            "an open temporary ID follows its number"
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
        let listing = host.list_issues(Some(&sage.id), None).unwrap().remove(0);
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
        let before = host.list_issues(None, None).unwrap();
        assert_eq!(before.len(), 2);
        host.db
            .execute_batch("DROP TABLE issue_index; DROP TABLE issue_index_format;")
            .unwrap();
        host.db.execute_batch(include_str!("issues.sql")).unwrap();
        host.rebuild_issue_index().unwrap();
        assert_eq!(host.list_issues(None, None).unwrap(), before);
    }

    #[test]
    fn an_issue_pushed_by_a_pass_that_never_finished_is_adopted_not_filed_again() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host, sage) = sage_here(&github);
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        // The push reaches GitHub; the host stops before the pass is finished here.
        let key = repos::key(NAME);
        let pass = host.storage_pass(&key).unwrap().unwrap();
        storage::pass(pass, &github.tools()).unwrap();
        let commits = github.log("%H").len();
        sync(&mut host, &github).unwrap();
        sync(&mut host, &github).unwrap();
        assert_eq!(github.log("%H").len(), commits, "nothing is filed again");
        assert_eq!(listed(&host, &sage), ["SAGE-1"]);
        assert!(!clone_of(&host).join(issue_folder(&filed.id)).exists());
        assert_eq!(host.issue_trackers().unwrap()[1].waiting, 0);
    }

    #[test]
    fn a_file_a_shell_put_in_a_new_issue_stays_paused_after_it_is_numbered() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host, sage) = sage_here(&github);
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        let comments = clone_of(&host)
            .join(issue_folder(&filed.id))
            .join("comments");
        fs::create_dir_all(&comments).unwrap();
        fs::write(
            comments.join("x.md"),
            "---\nauthor: agent\ncreated: now\n---\nHi\n",
        )
        .unwrap();
        sync(&mut host, &github).unwrap();
        sync(&mut host, &github).unwrap();
        assert!(github.show("issues/SAGE-1/issue.md").is_some());
        assert!(github.show("issues/SAGE-1/comments/x.md").is_none());
        let paused = host.read_issue(&id("SAGE-1")).unwrap().paused;
        assert_eq!(paused.len(), 1, "{paused:?}");
        assert_eq!(paused[0].path, "issues/SAGE-1/comments/x.md");
    }

    #[test]
    fn a_shell_edit_naming_a_new_issue_stays_paused_when_the_id_is_rewritten() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        let edited = clone_of(&host).join("issues/SAGE-1/issue.md");
        let text = fs::read_to_string(&edited).unwrap();
        let shell = format!("{text}Outside shell edit, see {}.\n", filed.id);
        fs::write(&edited, shell).unwrap();
        sync(&mut host, &github).unwrap();
        sync(&mut host, &github).unwrap();
        assert!(github.show("issues/SAGE-2/issue.md").is_some());
        let there = github.show("issues/SAGE-1/issue.md").unwrap();
        assert!(!there.contains("Outside shell edit"), "{there}");
        let paused = host.read_issue(&id("SAGE-1")).unwrap().paused;
        assert_eq!(paused.len(), 1, "{paused:?}");
    }

    #[test]
    fn an_issue_naming_a_held_new_issue_waits_with_it() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        let phone = "Call the parent at (415) 555-0134 tonight.";
        let filed = file(&mut host, &sage, "Watch app drops replies", phone);
        let mut first = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        first.related.push(filed.id.to_string());
        host.edit_issue(first).unwrap();
        sync(&mut host, &github).unwrap();
        let there = github.show("issues/SAGE-1/issue.md").unwrap();
        assert!(!there.contains("SAGE-2"), "{there}");
        let held: Vec<String> = (host.held_notes().unwrap().into_iter())
            .map(|note| match note {
                WorkspaceNote::Held(held) => held.path,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(
            held.contains(&"issues/SAGE-1/issue.md".to_owned()),
            "{held:?}"
        );
        let here = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert_eq!(here.related, ["CEL-148".to_owned(), filed.id.to_string()]);
    }

    #[test]
    fn holds_reach_every_new_issue_a_held_file_names_and_every_file_naming_those() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        let phone = "Call the parent at (415) 555-0134 tonight.";
        let b = file(&mut host, &sage, "Watch app drops replies", phone);
        let mut a = file(&mut host, &sage, "Replies need a retry", "");
        a.related.push(b.id.to_string());
        host.edit_issue(a.clone()).unwrap();
        host.comment_on_issue(&a.id, "Seen on two watches.")
            .unwrap();
        let mut first = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        first.related.push(a.id.to_string());
        host.edit_issue(first).unwrap();
        let commits = github.log("%H").len();
        sync(&mut host, &github).unwrap();
        assert_eq!(github.log("%H").len(), commits, "nothing reaches GitHub");
        let there = github.show("issues/SAGE-1/issue.md").unwrap();
        assert!(
            !there.contains("SAGE-2") && !there.contains("SAGE-3"),
            "{there}"
        );
        let mut held: Vec<String> = (host.held_notes().unwrap().into_iter())
            .map(|note| match note {
                WorkspaceNote::Held(held) => held.path,
                other => panic!("{other:?}"),
            })
            .collect();
        held.sort();
        let (a, b) = (issue_folder(&a.id), issue_folder(&b.id));
        let mut expected = vec![
            "issues/SAGE-1/issue.md".to_owned(),
            format!("{a}/issue.md"),
            format!("{b}/issue.md"),
        ];
        let comments = clone_of(&host).join(&a).join("comments");
        let comment = fs::read_dir(comments).unwrap().next().unwrap().unwrap();
        expected.push(format!(
            "{a}/comments/{}",
            comment.file_name().to_str().unwrap()
        ));
        expected.sort();
        assert_eq!(held, expected, "each held under the ID this Mac shows");
    }

    #[test]
    fn an_issue_added_or_edited_by_hand_in_a_local_tracker_is_listed_and_counted() {
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let personal = host.spaces().unwrap().remove(0).space;
        let prefix = host.tracker(&personal).unwrap().prefix;
        let filed = file(&mut host, &personal, "Renew the passport", "");
        let folder = host.storage_folder(&personal.storage.repository);
        let by_hand = format!("{prefix}-9");
        fs::create_dir_all(folder.join("issues").join(&by_hand)).unwrap();
        let text = String::from_utf8(issue_md(&blurry(&by_hand)).unwrap()).unwrap();
        fs::write(folder.join(format!("issues/{by_hand}/issue.md")), &text).unwrap();
        let mine = folder.join(issue_folder(&filed.id)).join("issue.md");
        let edited = fs::read_to_string(&mine)
            .unwrap()
            .replace("Renew the passport", "Renew both passports");
        fs::write(&mine, edited).unwrap();
        let listings = host.list_issues(Some(&personal.id), None).unwrap();
        let titles: Vec<(&str, bool)> = (listings.iter())
            .map(|listing| (listing.title.as_str(), listing.unread))
            .collect();
        assert_eq!(
            titles,
            [
                ("Renew both passports", true),
                ("Theme previews look blurry on Retina displays", true)
            ]
        );
        let tracker = &host.issue_trackers().unwrap()[0];
        assert_eq!((tracker.issues, tracker.unread), (2, 2));
        fs::remove_dir_all(folder.join("issues").join(&by_hand)).unwrap();
        assert_eq!(listed(&host, &personal), [filed.id.to_string()]);
    }

    #[test]
    fn a_teammates_change_is_unread_until_opened_and_the_humans_own_never_is() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (_home, mut host, sage) = sage_here(&github);
        let unread = |host: &Host| -> Vec<(String, bool)> {
            (host.list_issues(Some(&sage.id), None).unwrap().into_iter())
                .map(|listing| (listing.id.to_string(), listing.unread))
                .collect()
        };
        assert_eq!(unread(&host), [("SAGE-1".into(), false)], "joined as read");
        github.commit(
            "issues/SAGE-1/comments/2026-10-30T09-00-00Z-pshah.md",
            "---\nauthor: pshah\ncreated: 2026-10-30T09:00:00Z\n---\nThe gallery export is 1x.\n",
        );
        github.commit("issues/SAGE-2/issue.md", theirs("SAGE-2"));
        sync(&mut host, &github).unwrap();
        assert_eq!(
            unread(&host),
            [("SAGE-1".into(), true), ("SAGE-2".into(), true)]
        );
        assert_eq!(host.issue_trackers().unwrap()[1].unread, 2);
        host.mark_issue_read(&id("SAGE-1")).unwrap();
        host.comment_on_issue(&id("SAGE-2"), "Mine.").unwrap();
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        let mut expected = vec![("SAGE-1".into(), false), ("SAGE-2".into(), false)];
        expected.push((filed.id.to_string(), false));
        assert_eq!(unread(&host), expected);
        sync(&mut host, &github).unwrap();
        assert_eq!(
            unread(&host)[2],
            ("SAGE-3".into(), false),
            "numbering keeps it read"
        );
        assert_eq!(host.issue_trackers().unwrap()[1].unread, 0);
    }

    #[test]
    fn search_finds_words_in_titles_descriptions_and_comments_in_any_case() {
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let personal = host.spaces().unwrap().remove(0).space;
        let passport = file(&mut host, &personal, "Renew the passport", "Before March.");
        let dentist = file(&mut host, &personal, "Book the dentist", "");
        host.comment_on_issue(&dentist.id, "Ask about the Retainer.")
            .unwrap();
        let found = |search: &str| -> Vec<String> {
            (host.list_issues(None, Some(search)).unwrap().into_iter())
                .map(|listing| listing.id.to_string())
                .collect()
        };
        let (passport, dentist) = (passport.id.to_string(), dentist.id.to_string());
        assert_eq!(found("PASSPORT"), [passport.as_str()]);
        assert_eq!(found("march"), [passport.as_str()]);
        assert_eq!(found("retainer dentist"), [dentist.as_str()]);
        assert_eq!(found(&dentist), [dentist.as_str()]);
        assert!(found("retainer march").is_empty(), "every word must match");
        assert_eq!(found("  ").len(), 2);
    }

    #[test]
    fn an_issue_a_teammate_deleted_is_no_longer_listed_or_counted() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        github.commit("issues/SAGE-2/issue.md", theirs("SAGE-2"));
        github.commit(
            "issues/SAGE-2/comments/a-pshah.md",
            "---\nauthor: pshah\ncreated: now\n---\nHi\n",
        );
        let (_home, mut host, sage) = sage_here(&github);
        assert_eq!(listed(&host, &sage), ["SAGE-1", "SAGE-2"]);
        delete_on(&github.bare, "issues/SAGE-2");
        sync(&mut host, &github).unwrap();
        assert_eq!(listed(&host, &sage), ["SAGE-1"]);
        assert!(!clone_of(&host).join("issues/SAGE-2").exists());
        assert_eq!(host.issue_trackers().unwrap()[1].issues, 1);
        host.rebuild_issue_index().unwrap();
        assert_eq!(listed(&host, &sage), ["SAGE-1"]);
    }

    #[test]
    fn the_space_of_the_last_issue_filed_is_kept_across_launches() {
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        assert_eq!(host.last_filed_space().unwrap(), None);
        let personal = host.spaces().unwrap().remove(0).space;
        file(&mut host, &personal, "Renew the passport", "");
        drop(host);
        let again = Host::open_set_up(home.path()).unwrap();
        assert_eq!(again.last_filed_space().unwrap(), Some(personal.id));
    }

    /// A teammate's edit of `path` on GitHub, replacing `from` with `to`.
    fn teammate_edits(github: &Arc<GitHub>, path: &str, from: &str, to: &str) {
        let text = github.show(path).unwrap();
        assert!(text.contains(from), "{text}");
        github.commit(path, text.replace(from, to));
    }

    /// Sage on this Mac with SAGE-1 pulled, as the teammate pushed it.
    fn sage_with_an_issue() -> (TempDir, Arc<GitHub>, TempDir, Host) {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit("issues/SAGE-1/issue.md", theirs("SAGE-1"));
        let (home, host, _) = sage_here(&github);
        (folder, github, home, host)
    }

    fn issue_notes(host: &Host) -> Vec<IssueNote> {
        (host.workspace_notes(&[]).unwrap().into_iter())
            .filter_map(|note| match note {
                WorkspaceNote::Issue(note) => Some(note),
                _ => None,
            })
            .collect()
    }

    const ISSUE: &str = "issues/SAGE-1/issue.md";

    #[test]
    fn two_people_changing_different_fields_both_reach_both_macs_and_nothing_pauses() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.assignee = Some("jlin".into());
        host.edit_issue(mine).unwrap();
        teammate_edits(&github, ISSUE, "priority: high", "priority: urgent");
        sync(&mut host, &github).unwrap();
        let there = github.show(ISSUE).unwrap();
        assert!(
            there.contains("priority: urgent\nassignee: jlin\n"),
            "{there}"
        );
        let here = host.read_issue(&id("SAGE-1")).unwrap();
        let issue = here.issue.unwrap();
        assert_eq!(
            (issue.priority, issue.assignee.as_deref()),
            (Priority::Urgent, Some("jlin"))
        );
        assert!(here.paused.is_empty() && issue_notes(&host).is_empty());
        assert_eq!(host.issue_trackers().unwrap()[1].waiting, 0);
    }

    #[test]
    fn the_same_field_changed_on_both_sides_pauses_alone_and_keep_mine_pushes_it() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.priority = Priority::Low;
        host.edit_issue(mine).unwrap();
        teammate_edits(&github, ISSUE, "priority: high", "priority: urgent");
        teammate_edits(&github, ISSUE, "title: Theme", "title: Gallery");
        sync(&mut host, &github).unwrap();
        let there = github.show(ISSUE).unwrap();
        assert!(there.contains("priority: urgent"), "theirs stays: {there}");
        let issue = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert_eq!(issue.priority, Priority::Low, "this Mac keeps its own");
        assert!(issue.title.starts_with("Gallery"), "the rest is taken");
        let notes = issue_notes(&host);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].text, "Paused: you and F both changed the priority");
        let paused = notes[0].paused.clone().unwrap();
        assert_eq!(paused.path, "issues/SAGE-1/issue.md#priority");
        let lines = host.compare_paused(Some(NAME), &paused.path).unwrap();
        assert_eq!(
            lines,
            [
                skills::Line::Removed("low".into()),
                skills::Line::Added("urgent".into())
            ]
        );
        // Still paused, a later pass pushes nothing of it.
        sync(&mut host, &github).unwrap();
        assert!(github.show(ISSUE).unwrap().contains("priority: urgent"));
        host.settle_paused(Some(NAME), &paused.path, Settle::KeepThisMachines)
            .unwrap();
        sync(&mut host, &github).unwrap();
        let there = github.show(ISSUE).unwrap();
        assert!(
            there.contains("title: Gallery") && there.contains("priority: low"),
            "{there}"
        );
        assert!(issue_notes(&host).is_empty());
        assert_eq!(host.issue_trackers().unwrap()[1].paused, 0);
    }

    #[test]
    fn both_changing_the_description_keeps_this_macs_until_use_theirs_takes_the_other() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.description = "Mine: previews are 1x.".into();
        host.edit_issue(mine).unwrap();
        teammate_edits(
            &github,
            ISSUE,
            "Screenshots are scaled at 1x.",
            "Theirs: gallery at 1x.",
        );
        teammate_edits(&github, ISSUE, "- bug", "- bug\n- design");
        sync(&mut host, &github).unwrap();
        let issue = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert_eq!(issue.description, "Mine: previews are 1x.");
        assert_eq!(issue.labels, ["bug", "design"]);
        let there = github.show(ISSUE).unwrap();
        assert!(!there.contains("Mine:"), "never pushed: {there}");
        let path = "issues/SAGE-1/issue.md#description";
        host.settle_paused(Some(NAME), path, Settle::UseTheOthers)
            .unwrap();
        let issue = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert!(issue.description.starts_with("Theirs: gallery at 1x."));
        let commits = github.log("%H").len();
        sync(&mut host, &github).unwrap();
        assert_eq!(github.log("%H").len(), commits, "nothing to push");
        assert!(issue_notes(&host).is_empty());
    }

    #[test]
    fn a_pause_closes_when_both_versions_become_equal_and_a_newer_one_replaces_theirs() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.priority = Priority::Low;
        host.edit_issue(mine).unwrap();
        teammate_edits(&github, ISSUE, "priority: high", "priority: urgent");
        sync(&mut host, &github).unwrap();
        teammate_edits(&github, ISSUE, "priority: urgent", "priority: medium");
        sync(&mut host, &github).unwrap();
        let path = "issues/SAGE-1/issue.md#priority";
        let lines = host.compare_paused(Some(NAME), path).unwrap();
        assert!(
            lines.contains(&skills::Line::Added("medium".into())),
            "{lines:?}"
        );
        teammate_edits(&github, ISSUE, "priority: medium", "priority: low");
        sync(&mut host, &github).unwrap();
        assert!(issue_notes(&host).is_empty(), "equal now");
    }

    #[test]
    fn comments_and_events_from_both_sides_at_once_all_arrive_in_time_order() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        sync(&mut host, &github).unwrap();
        host.comment_on_issue(&id("SAGE-1"), "Seen on two watches.")
            .unwrap();
        host.move_issue(&id("SAGE-1"), IssueState::Todo).unwrap();
        github.commit(
            "issues/SAGE-1/comments/2026-10-30T09-00-00Z-pshah.md",
            "---\nauthor: pshah\ncreated: 2026-10-30T09:00:00Z\n---\nRepro'd too.\n",
        );
        let early = r#"{"at":"2026-01-01T00:00:00Z","by":"pshah","kind":"priority","from":"high","to":"high"}"#;
        github.commit("issues/SAGE-1/events.jsonl", format!("{early}\n"));
        sync(&mut host, &github).unwrap();
        let record = host.read_issue(&id("SAGE-1")).unwrap();
        assert_eq!(record.comments.len(), 2);
        assert!(record.paused.is_empty());
        let notes: Vec<(IssueNoteKind, String)> = (issue_notes(&host).into_iter())
            .map(|note| (note.kind, note.text))
            .collect();
        let followed = "pshah commented on an issue you follow".to_owned();
        assert_eq!(
            notes,
            [(IssueNoteKind::Commented, followed)],
            "nothing paused"
        );
        let events = github.show("issues/SAGE-1/events.jsonl").unwrap();
        let lines: Vec<&str> = events.lines().collect();
        assert_eq!(lines.len(), 2, "{events}");
        assert_eq!(lines[0], early, "ordered by time");
        assert_eq!(record.events.len(), 2);
    }

    #[test]
    fn a_comment_edited_on_both_sides_pauses_whole_and_names_a_teammate() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let comment = host
            .comment_on_issue(&id("SAGE-1"), "Seen on two watches.")
            .unwrap();
        sync(&mut host, &github).unwrap();
        let path = format!("issues/SAGE-1/comments/{}", comment.file);
        let sage = space_named(&host, "Sage");
        let mine = github.show(&path).unwrap().replace("two", "three");
        let files = vec![(path.clone(), mine.clone().into_bytes())];
        let (id, did) = (id("SAGE-1"), "edited".into());
        host.write_issue_files(&sage, tracker::Write { id, files, did })
            .unwrap();
        teammate_edits(&github, &path, "two", "four");
        sync(&mut host, &github).unwrap();
        assert!(github.show(&path).unwrap().contains("four"), "theirs stays");
        assert_eq!(
            fs::read_to_string(clone_of(&host).join(&path)).unwrap(),
            mine
        );
        let notes = issue_notes(&host);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(
            notes[0].text,
            "Paused: you and a teammate both changed a comment"
        );
        let paused = notes[0].paused.as_ref().unwrap();
        assert_eq!(paused.path, path, "the whole file, not a part");
        assert!(matches!(paused.paused, Paused::TwoMachines { .. }));
    }

    #[test]
    fn a_merge_that_meets_an_outside_edit_pauses_the_file_whole_and_pushes_nothing() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        teammate_edits(&github, ISSUE, "priority: high", "priority: urgent");
        let file = clone_of(&host).join(ISSUE);
        let shell = fs::read_to_string(&file)
            .unwrap()
            .replace("assignee: pshah", "assignee: agent");
        fs::write(&file, &shell).unwrap();
        let commits = github.log("%H").len();
        sync(&mut host, &github).unwrap();
        assert_eq!(github.log("%H").len(), commits);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            shell,
            "kept, not merged"
        );
        let notes = issue_notes(&host);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(
            notes[0].text,
            "The issue was changed outside Wiffletree on this Mac"
        );
        assert_eq!(
            notes[0].paused.as_ref().unwrap().paused,
            Paused::OutsideWiffletree
        );
    }

    #[test]
    fn a_merged_issue_that_fails_a_scan_is_held_and_its_number_never_pushed() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.description = "Call the parent at (415) 555-0134 tonight.".into();
        host.edit_issue(mine).unwrap();
        teammate_edits(&github, ISSUE, "priority: high", "priority: urgent");
        sync(&mut host, &github).unwrap();
        let there = github.show(ISSUE).unwrap();
        assert!(!there.contains("555-0134"), "{there}");
        let issue = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert_eq!(issue.priority, Priority::Urgent, "theirs taken here");
        assert!(issue.description.contains("555-0134"), "mine kept here");
        let held = host.held_notes().unwrap();
        assert_eq!(held.len(), 1, "{held:?}");
        sync(&mut host, &github).unwrap();
        assert!(!github.show(ISSUE).unwrap().contains("555-0134"));
    }

    #[test]
    fn a_merge_pushed_while_the_human_edits_the_issue_is_merged_again_and_reverts_nothing() {
        let (_folder, github, _home, mut host) = sage_with_an_issue();
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.priority = Priority::Low;
        host.edit_issue(mine).unwrap();
        teammate_edits(&github, ISSUE, "title: Theme", "title: Gallery");
        // The pass merges and pushes off the host thread; meanwhile the human assigns it.
        let key = repos::key(NAME);
        let pass = host.storage_pass(&key).unwrap().unwrap();
        let passed = storage::pass(pass, &github.tools());
        let mut mine = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        mine.assignee = Some("jlin".into());
        host.edit_issue(mine).unwrap();
        host.finish_storage_pass(&key, passed).unwrap();
        assert!(github.show(ISSUE).unwrap().contains("title: Gallery"));
        sync(&mut host, &github).unwrap();
        let there = github.show(ISSUE).unwrap();
        assert!(there.contains("title: Gallery"), "not reverted: {there}");
        assert!(there.contains("priority: low\nassignee: jlin\n"), "{there}");
        let issue = host.read_issue(&id("SAGE-1")).unwrap().issue.unwrap();
        assert!(issue.title.starts_with("Gallery"));
        assert!(issue_notes(&host).is_empty());
    }

    #[test]
    fn a_choice_not_to_follow_a_new_issue_holds_once_it_is_numbered() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host, sage) = sage_here(&github);
        let filed = file(&mut host, &sage, "Watch app drops replies", "");
        assert!(
            host.read_issue(&filed.id).unwrap().following,
            "filed by the human"
        );
        host.follow_issue(&filed.id, false).unwrap();
        sync(&mut host, &github).unwrap();
        let numbered = host.read_issue(&id("SAGE-1")).unwrap();
        assert!(!numbered.following);
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
