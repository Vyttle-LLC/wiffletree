//! Ref verification (`brain-compile`, decision 2): what an entry's `ref` names, and whether this
//! machine's clone of its repository holds it. Deterministic and local: it never fetches and
//! never asks GitHub, so a PR's state is not read here.
use crate::*;
use std::collections::{HashMap, HashSet};
use workspace_core::{RefAnchors, RefState, RefVerification};

/// The anchors of a `ref` in the entry contract's forms: `—`, `<branch> @ —`, or
/// `<branch> @ <sha>[…<sha>][ · PR #NN]`.
pub(crate) fn anchors(reference: &str) -> RefAnchors {
    let reference = reference.trim();
    let Some((branch, rest)) = reference.split_once(" @ ") else {
        return RefAnchors::default();
    };
    let (commits, pull_request) = match rest.split_once(" · PR #") {
        Some((commits, number)) => (commits, number.trim().parse().ok()),
        None => (rest, None),
    };
    let commits = (commits.split('…'))
        .map(str::trim)
        .filter(|sha| (7..=40).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .collect();
    RefAnchors {
        branch: Some(branch.trim().to_owned()),
        commits,
        pull_request,
    }
}

/// This machine's clones of the workspace repositories, by log folder: each one's GitHub name,
/// or its workspace name, in lower case, with its clone and its default branch.
#[derive(Clone, Debug, Default)]
pub(crate) struct Clones(pub(crate) HashMap<String, Vec<(PathBuf, String)>>);
impl Clones {
    pub(crate) fn of(repositories: &[Repository]) -> Self {
        let mut clones: HashMap<String, Vec<(PathBuf, String)>> = HashMap::new();
        for repository in repositories {
            let folder = (repository.github.as_ref())
                .map(|github| github.name.clone())
                .unwrap_or_else(|| repository.name.clone());
            clones
                .entry(folder.to_ascii_lowercase())
                .or_default()
                .push((PathBuf::from(&repository.path), repository.base.clone()));
        }
        Self(clones)
    }
}

/// Each `(folder, ref)`'s verification, in order. Git runs twice per clone at most, however many
/// refs there are: one listing of the default branch and one lookup of every named commit.
pub(crate) fn verify(clones: &Clones, refs: &[(&str, &str)]) -> Vec<RefVerification> {
    let unverified = |reason: &str| RefVerification {
        state: RefState::Unverified,
        reason: Some(reason.to_owned()),
    };
    let anchored: Vec<RefAnchors> = refs.iter().map(|(_, r)| anchors(r)).collect();
    let mut found: HashMap<(String, String), Found> = HashMap::new();
    let mut by_clone: HashMap<&Path, (&str, HashSet<&str>)> = HashMap::new();
    for ((folder, _), anchors) in refs.iter().zip(&anchored) {
        for (clone, base) in clones
            .0
            .get(&folder.to_ascii_lowercase())
            .into_iter()
            .flatten()
        {
            let (_, commits) = by_clone.entry(clone).or_insert((base, HashSet::new()));
            commits.extend(anchors.commits.iter().map(String::as_str));
        }
    }
    for (clone, (base, commits)) in by_clone {
        for (sha, state) in look_up(clone, base, commits) {
            let key = (clone.to_string_lossy().into_owned(), sha);
            let best = found.entry(key).or_insert(state);
            *best = (*best).max(state);
        }
    }
    refs.iter()
        .zip(anchored)
        .map(|((folder, _), anchors)| {
            if anchors.commits.is_empty() {
                return unverified("its ref names no commit");
            }
            let Some(here) = clones.0.get(&folder.to_ascii_lowercase()) else {
                return unverified("not on this machine");
            };
            let state = (here.iter())
                .flat_map(|(clone, _)| {
                    let clone = clone.to_string_lossy().into_owned();
                    (anchors.commits.iter())
                        .filter_map(|sha| found.get(&(clone.clone(), sha.clone())).copied())
                        .collect::<Vec<_>>()
                })
                .max();
            match state {
                Some(Found::OnDefault) => RefVerification {
                    state: RefState::Verified,
                    reason: None,
                },
                Some(Found::Elsewhere) => RefVerification {
                    state: RefState::Claimed,
                    reason: Some("its commit is not on the default branch".into()),
                },
                None => unverified("its commit is not in this machine's clone"),
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Found {
    Elsewhere,
    OnDefault,
}

/// The named commits the clone holds, keyed as named, and whether `base` contains each.
fn look_up(clone: &Path, base: &str, commits: HashSet<&str>) -> Vec<(String, Found)> {
    if commits.is_empty() {
        return vec![];
    }
    let named: Vec<&str> = commits.into_iter().collect();
    let input: String = named
        .iter()
        .map(|sha| format!("{sha}^{{commit}}\n"))
        .collect();
    let Ok(checked) = runtime::git_bytes_large(
        clone,
        &["cat-file", "--batch-check=%(objectname)"],
        input.as_bytes(),
    ) else {
        return vec![];
    };
    let checked = String::from_utf8_lossy(&checked);
    let full: Vec<(&str, &str)> = (named.iter().copied())
        .zip(checked.lines())
        .filter(|(_, line)| line.len() == 40 && line.bytes().all(|b| b.is_ascii_hexdigit()))
        .collect();
    if full.is_empty() {
        return vec![];
    }
    let on_default: HashSet<String> =
        runtime::git_bytes_large(clone, &["rev-list", base, "--"], b"")
            .map(|listing| {
                String::from_utf8_lossy(&listing)
                    .lines()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
    full.into_iter()
        .map(|(named, sha)| {
            let found = match on_default.contains(sha) {
                true => Found::OnDefault,
                false => Found::Elsewhere,
            };
            (named.to_owned(), found)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;

    #[test]
    fn a_ref_names_its_branch_commits_and_pull_request() {
        assert_eq!(anchors("—"), RefAnchors::default());
        let bare = anchors("feature/x @ —");
        assert_eq!(bare.branch.as_deref(), Some("feature/x"));
        assert!(bare.commits.is_empty());
        let full = anchors("main @ a41f2c0…7d03b1e · PR #81");
        assert_eq!(full.commits, ["a41f2c0", "7d03b1e"]);
        assert_eq!(full.pull_request, Some(81));
    }

    #[test]
    fn a_commit_on_the_default_branch_is_verified_one_elsewhere_claimed() {
        let folder = tempfile::tempdir().unwrap();
        let clone = folder.path().join("relay");
        git(
            folder.path(),
            &["init", "-q", "-b", "main", clone.to_str().unwrap()],
        );
        let commit = |message: &str| {
            git(&clone, &["commit", "-q", "--allow-empty", "-m", message]);
            runtime::git_output(&clone, &["rev-parse", "HEAD"])
                .unwrap()
                .trim()
                .to_owned()
        };
        let merged = commit("merged");
        git(&clone, &["checkout", "-q", "-b", "side"]);
        let side = commit("side");
        git(&clone, &["checkout", "-q", "main"]);
        let clones = Clones(HashMap::from([(
            "relay".to_owned(),
            vec![(clone.clone(), "main".to_owned())],
        )]));
        let verified =
            |folder, reference: String| verify(&clones, &[(folder, reference.as_str())])[0].clone();
        let short = &merged[..7];
        assert_eq!(
            verified("Relay", format!("main @ {short}")).state,
            RefState::Verified
        );
        assert_eq!(
            verified("relay", format!("side @ {side}")).state,
            RefState::Claimed
        );
        let missing = verified("relay", "main @ 0000000".into());
        assert_eq!(missing.state, RefState::Unverified);
        let elsewhere = verified("other", format!("main @ {merged}"));
        assert_eq!(elsewhere.reason.as_deref(), Some("not on this machine"));
        assert_eq!(
            verified("relay", "main @ —".into()).state,
            RefState::Unverified
        );
    }
}
