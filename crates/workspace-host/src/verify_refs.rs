//! Ref verification (`brain-compile`, decision 2): what an entry's `ref` names, and whether this
//! machine's clone of its repository holds it on the repository's default branch on GitHub.
//! Deterministic and local: it never fetches and never asks GitHub, so a PR's state is not read
//! here. Results are cached by each clone's default-branch head, so a repeat costs one ref read.
use crate::*;
use std::collections::{HashMap, HashSet};
use workspace_core::{RefAnchors, RefState, RefVerification};

/// Above this many unchecked commits in one clone, its default branch is listed once instead of
/// asking Git about each commit.
const ONE_BY_ONE: usize = 20;
/// The cache forgets everything past this many commits, so it stays bounded.
const CACHE_LIMIT: usize = 10_000;

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

/// One clone of a workspace repository and the refs that may name its default branch on
/// GitHub, best first: the default branch GitHub reported, then `origin/HEAD`. The repository's
/// configured base is not one of them, since a feature base is not the default branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RepoClone {
    pub(crate) path: PathBuf,
    pub(crate) defaults: Vec<String>,
}

/// This machine's clones of the workspace repositories, by log folder in lower case.
#[derive(Clone, Debug, Default)]
pub(crate) struct Clones(pub(crate) HashMap<String, Vec<RepoClone>>);
impl Clones {
    pub(crate) fn of(repositories: &[Repository]) -> Self {
        let mut clones: HashMap<String, Vec<RepoClone>> = HashMap::new();
        for repository in repositories {
            let reported = (repository.github.as_ref())
                .and_then(|github| github.facts.as_ref())
                .and_then(|facts| facts.branches.as_ref())
                .and_then(|branches| branches.default_branch.as_ref())
                .map(|branch| format!("refs/remotes/origin/{branch}"));
            let defaults = reported
                .into_iter()
                .chain(["refs/remotes/origin/HEAD".to_owned()])
                .collect();
            clones
                .entry(memory::repository_log_name(repository).to_ascii_lowercase())
                .or_default()
                .push(RepoClone {
                    path: PathBuf::from(&repository.path),
                    defaults,
                });
        }
        Self(clones)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Found {
    Elsewhere,
    OnDefault,
}

/// What was learned about named commits, by clone and its default branch's head: `None` for a
/// commit the clone does not hold.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    found: HashMap<(PathBuf, String), HashMap<String, Option<Found>>>,
    size: usize,
}

/// The commit the clone's default branch is at, by the first of its refs that resolves.
fn default_head(clone: &RepoClone) -> Option<String> {
    clone.defaults.iter().find_map(|reference| {
        let answer = runtime::git_answer(
            &clone.path,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{reference}^{{commit}}"),
            ],
        )
        .ok()?;
        answer
            .status
            .success()
            .then(|| answer.output.trim().to_owned())
    })
}

/// Each `(folder, ref)`'s verification, in order. Per clone, Git reads its default branch's
/// head, then checks only the commits `cache` does not know at that head.
pub(crate) fn verify(
    clones: &Clones,
    refs: &[(&str, &str)],
    cache: &mut Cache,
) -> Vec<RefVerification> {
    let unverified = |reason: &str| RefVerification {
        state: RefState::Unverified,
        reason: Some(reason.to_owned()),
    };
    let anchored: Vec<RefAnchors> = refs.iter().map(|(_, r)| anchors(r)).collect();
    let mut named: HashMap<&Path, (&RepoClone, HashSet<&str>)> = HashMap::new();
    for ((folder, _), anchors) in refs.iter().zip(&anchored) {
        for clone in clones
            .0
            .get(&folder.to_ascii_lowercase())
            .into_iter()
            .flatten()
        {
            let (_, commits) = named.entry(&clone.path).or_insert((clone, HashSet::new()));
            commits.extend(anchors.commits.iter().map(String::as_str));
        }
    }
    // Forgotten only between calls, so this call's answers are all still there when it reads
    // them back below, however many it adds.
    if cache.size > CACHE_LIMIT {
        *cache = Cache::default();
    }
    let mut heads: HashMap<&Path, String> = HashMap::new();
    for (path, (clone, commits)) in named {
        if commits.is_empty() {
            continue;
        }
        let Some(head) = default_head(clone) else {
            continue;
        };
        let known = cache
            .found
            .entry((path.to_owned(), head.clone()))
            .or_default();
        let unknown: Vec<&str> = commits
            .into_iter()
            .filter(|c| !known.contains_key(*c))
            .collect();
        cache.size += unknown.len();
        known.extend(look_up(path, &head, &unknown));
        heads.insert(path, head);
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
            let checked: Vec<&HashMap<String, Option<Found>>> = (here.iter())
                .filter_map(|clone| {
                    let head = heads.get(clone.path.as_path())?;
                    cache.found.get(&(clone.path.clone(), head.clone()))
                })
                .collect();
            if checked.is_empty() {
                return unverified("its repository's default branch is not known on this machine");
            }
            let state = (checked.iter())
                .flat_map(|known| {
                    anchors
                        .commits
                        .iter()
                        .filter_map(|sha| known.get(sha).copied().flatten())
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

/// What the clone holds of the named commits, and whether the default branch at `head`
/// contains each: one question per commit for a few, or one listing of the branch for many.
fn look_up(clone: &Path, head: &str, commits: &[&str]) -> Vec<(String, Option<Found>)> {
    if commits.is_empty() {
        return vec![];
    }
    let input: String = commits
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
    let full: Vec<(&str, Option<&str>)> = (commits.iter().copied())
        .zip(checked.lines())
        .map(|(named, line)| {
            let held = line.len() == 40 && line.bytes().all(|b| b.is_ascii_hexdigit());
            (named, held.then_some(line))
        })
        .collect();
    let held = full.iter().filter(|(_, sha)| sha.is_some()).count();
    let listed: Option<HashSet<String>> = (held > ONE_BY_ONE).then(|| {
        runtime::git_bytes_large(clone, &["rev-list", head, "--"], b"")
            .map(|listing| {
                String::from_utf8_lossy(&listing)
                    .lines()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    });
    full.into_iter()
        .map(|(named, sha)| {
            let found = sha.map(|sha| {
                let on_default = match &listed {
                    Some(listed) => listed.contains(sha),
                    None => runtime::git_answer(clone, &["merge-base", "--is-ancestor", sha, head])
                        .is_ok_and(|answer| answer.status.success()),
                };
                match on_default {
                    true => Found::OnDefault,
                    false => Found::Elsewhere,
                }
            });
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

    /// A clone whose `origin/main` is the default branch, with a commit on it, one on a
    /// feature base that `origin` also has, and one on a local branch only.
    struct Relay {
        _folder: tempfile::TempDir,
        clones: Clones,
        merged: String,
        on_feature: String,
        side: String,
    }

    fn relay() -> Relay {
        let folder = tempfile::tempdir().unwrap();
        let clone = folder.path().join("relay");
        git(
            folder.path(),
            &["init", "-q", "-b", "main", clone.to_str().unwrap()],
        );
        let commit = |message: &str| {
            git(&clone, &["commit", "-q", "--allow-empty", "-m", message]);
            git(&clone, &["rev-parse", "HEAD"])
        };
        let merged = commit("merged");
        git(&clone, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(
            &clone,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        git(&clone, &["checkout", "-q", "-b", "Parental-controls"]);
        let on_feature = commit("on the feature base");
        git(
            &clone,
            &[
                "update-ref",
                "refs/remotes/origin/Parental-controls",
                "HEAD",
            ],
        );
        git(&clone, &["checkout", "-q", "-b", "side"]);
        let side = commit("side");
        git(&clone, &["checkout", "-q", "main"]);
        let clones = Clones(HashMap::from([(
            "relay".to_owned(),
            vec![RepoClone {
                path: clone.clone(),
                defaults: vec!["refs/remotes/origin/HEAD".into()],
            }],
        )]));
        Relay {
            _folder: folder,
            clones,
            merged,
            on_feature,
            side,
        }
    }

    #[test]
    fn a_commit_on_the_default_branch_is_verified_one_elsewhere_claimed() {
        let relay = relay();
        let mut cache = Cache::default();
        let mut verified = |folder, reference: String| {
            verify(&relay.clones, &[(folder, reference.as_str())], &mut cache)[0].clone()
        };
        let short = &relay.merged[..7];
        assert_eq!(
            verified("Relay", format!("main @ {short}")).state,
            RefState::Verified
        );
        assert_eq!(
            verified("relay", format!("side @ {}", relay.side)).state,
            RefState::Claimed
        );
        let feature = verified("relay", format!("Parental-controls @ {}", relay.on_feature));
        assert_eq!(
            feature.state,
            RefState::Claimed,
            "a feature base is not the default branch"
        );
        let missing = verified("relay", "main @ 0000000".into());
        assert_eq!(missing.state, RefState::Unverified);
        let elsewhere = verified("other", format!("main @ {}", relay.merged));
        assert_eq!(elsewhere.reason.as_deref(), Some("not on this machine"));
        assert_eq!(
            verified("relay", "main @ —".into()).state,
            RefState::Unverified
        );
    }

    #[test]
    fn the_default_branch_github_reported_comes_before_origin_head() {
        let relay = relay();
        let mut clones = relay.clones.clone();
        let clone = &mut clones.0.get_mut("relay").unwrap()[0];
        clone
            .defaults
            .insert(0, "refs/remotes/origin/Parental-controls".into());
        let reference = format!("Parental-controls @ {}", relay.on_feature);
        let state = verify(&clones, &[("relay", &reference)], &mut Cache::default())[0].state;
        assert_eq!(state, RefState::Verified);
    }

    #[test]
    fn a_repeat_comes_from_the_cache_until_the_default_branch_moves() {
        let relay = relay();
        let mut cache = Cache::default();
        let reference = format!("side @ {}", relay.side);
        let refs = [("relay", reference.as_str())];
        assert_eq!(
            verify(&relay.clones, &refs, &mut cache)[0].state,
            RefState::Claimed
        );
        let path = &relay.clones.0["relay"][0].path;
        let known = |cache: &Cache| cache.found.keys().filter(|(p, _)| p == path).count();
        assert_eq!(known(&cache), 1);
        verify(&relay.clones, &refs, &mut cache);
        assert_eq!(
            (known(&cache), cache.size),
            (1, 1),
            "nothing new was checked"
        );
        git(
            path,
            &["update-ref", "refs/remotes/origin/main", &relay.side],
        );
        assert_eq!(
            verify(&relay.clones, &refs, &mut cache)[0].state,
            RefState::Verified
        );
        assert_eq!(known(&cache), 2, "the moved head is a new key");
    }

    #[test]
    fn many_commits_are_checked_with_one_listing_of_the_default_branch() {
        let relay = relay();
        let clone = &relay.clones.0["relay"][0].path;
        let mut commits = vec![];
        for n in 0..=ONE_BY_ONE {
            git(
                clone,
                &["commit", "-q", "--allow-empty", "-m", &format!("c{n}")],
            );
            commits.push(format!("main @ {}", git(clone, &["rev-parse", "HEAD"])));
        }
        let refs: Vec<(&str, &str)> = commits.iter().map(|r| ("relay", r.as_str())).collect();
        let states = verify(&relay.clones, &refs, &mut Cache::default());
        assert!(
            states.iter().all(|v| v.state == RefState::Claimed),
            "origin/main has not moved"
        );
    }

    #[test]
    fn a_full_cache_keeps_every_answer_of_the_call_that_fills_it() {
        let (first, second) = (relay(), relay());
        let mut clones = first.clones.clone();
        clones
            .0
            .insert("watch".into(), second.clones.0["relay"].clone());
        let mut cache = Cache {
            size: CACHE_LIMIT,
            ..Cache::default()
        };
        let (one, two) = (
            format!("main @ {}", first.merged),
            format!("main @ {}", second.merged),
        );
        let refs = [("relay", one.as_str()), ("watch", two.as_str())];
        let states: Vec<RefState> = (verify(&clones, &refs, &mut cache).into_iter())
            .map(|v| v.state)
            .collect();
        assert_eq!(states, [RefState::Verified, RefState::Verified]);
        let next = verify(&clones, &refs[..1], &mut cache);
        assert_eq!(
            next[0].state,
            RefState::Verified,
            "forgotten between calls only"
        );
        assert_eq!(cache.size, 1);
    }
}
