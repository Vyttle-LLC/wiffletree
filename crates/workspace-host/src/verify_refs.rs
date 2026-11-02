//! Ref verification (`brain-compile`, decision 6): what an entry's `ref` names, and whether its
//! repository's default branch on GitHub holds it. `verify` is deterministic and local: it never
//! fetches and never asks GitHub. Results are cached by each clone's default-branch head, so a
//! repeat costs one ref read. `verify_with` also reads named PRs' states with `gh`, and asks
//! GitHub about the commits of a repository a Space lists and this Mac has not cloned, each once
//! per call; the brain index stores what it finds during the brain's pass, for the browser, and a
//! compile's `for_compile` fetches each clone first, once, then uses it and records the release
//! tag containing a verified commit.
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
    /// `owner/name` on GitHub, which a compile asks about a named PR.
    pub(crate) github: Option<String>,
}

/// A repository a Space lists with no clone on this machine, which only GitHub can answer for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    /// `owner/name`.
    pub(crate) github: String,
    /// Where `gh` runs.
    pub(crate) at: PathBuf,
}

/// This machine's clones of the workspace repositories, by log folder in lower case, then the
/// repositories Spaces list that it has no clone of, by the same key.
#[derive(Clone, Debug, Default)]
pub(crate) struct Clones(
    pub(crate) HashMap<String, Vec<RepoClone>>,
    pub(crate) HashMap<String, Vec<Listed>>,
);
impl Clones {
    /// Adds each `owner/name` of `listed`, a Space's list, whose name has no clone here. A name
    /// under two owners is kept under both, and the first that holds a commit answers.
    pub(crate) fn with_listed(mut self, listed: &[String], at: &Path) -> Self {
        for github in listed {
            let name = github.rsplit('/').next().unwrap_or(github);
            let name = name.to_ascii_lowercase();
            if self.0.contains_key(&name) {
                continue;
            }
            let entry = self.1.entry(name).or_default();
            if !entry
                .iter()
                .any(|known| known.github.eq_ignore_ascii_case(github))
            {
                entry.push(Listed {
                    github: github.clone(),
                    at: at.to_owned(),
                });
            }
        }
        self
    }
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
                    github: (repository.github.as_ref())
                        .map(|github| format!("{}/{}", github.owner, github.name)),
                });
        }
        Self(clones, HashMap::new())
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
            let key = folder.to_ascii_lowercase();
            if clones.1.contains_key(&key) {
                return unverified(NOT_ASKED);
            }
            let Some(here) = clones.0.get(&key) else {
                return unverified(NOT_LISTED);
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
                    reason: Some(ON_A_BRANCH.into()),
                },
                None => unverified("its commit is not in this machine's clone"),
            }
        })
        .collect()
}

/// One `ref` as a compile run checked it, with the release tag that holds its commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) verification: RefVerification,
    pub(crate) tag: Option<String>,
}

/// Why a ref in a repository only GitHub can answer for is unverified, until it answers.
pub(crate) const NOT_ASKED: &str = "GitHub could not be asked";
/// Why a ref whose commit GitHub or a clone holds off the default branch is claimed.
pub(crate) const ON_A_BRANCH: &str = "its commit is not on the default branch";
/// Why a ref in a folder no clone or Space's list matches is unverified.
pub(crate) const NOT_LISTED: &str = "no repository of that name";

/// How a verification's reason ends for a PR closed without merging, which is final: it is not
/// asked about again.
pub(crate) const CLOSED_UNMERGED: &str = "was closed without merging";

/// A named PR's state on GitHub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PullState {
    Merged,
    Open,
    Closed,
}

/// What `gh api` answered: its JSON, or that it said no such thing, or nothing to go on.
enum Answer {
    Found(Value),
    NotFound,
    Unasked,
}

fn ask(gh: &publish::Tool<runtime::GhAnswer>, at: &Path, endpoint: &str) -> Answer {
    let Ok(answer) = gh(at, &["api", "--hostname", "github.com", endpoint]) else {
        return Answer::Unasked;
    };
    if answer.status.success() {
        return (serde_json::from_str(&answer.output)).map_or(Answer::Unasked, Answer::Found);
    }
    // GitHub answers an unknown commit with a 422 where it cannot read the name as a SHA.
    match ["HTTP 404", "HTTP 422"]
        .iter()
        .any(|code| answer.error.contains(code))
    {
        true => Answer::NotFound,
        false => Answer::Unasked,
    }
}

fn pull_state(
    gh: &publish::Tool<runtime::GhAnswer>,
    at: &Path,
    github: &str,
    number: u64,
) -> Option<PullState> {
    let endpoint = format!("repos/{github}/pulls/{number}");
    let Answer::Found(pull) = ask(gh, at, &endpoint) else {
        return None;
    };
    Some(match (pull["merged"].as_bool(), pull["state"].as_str()) {
        (Some(true), _) => PullState::Merged,
        (_, Some("open")) => PullState::Open,
        _ => PullState::Closed,
    })
}

/// The oldest tag holding `sha` in the clone, as evidence of a release.
fn tag_containing(clone: &Path, sha: &str) -> Option<String> {
    let answer =
        runtime::git_answer(clone, &["tag", "--contains", sha, "--sort=creatordate"]).ok()?;
    answer.status.success().then_some(())?;
    answer.output.lines().next().map(str::to_owned)
}

/// What GitHub says of a commit in one repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Holds(Found),
    NoCommit,
    Unasked,
}

/// Asks GitHub about one repository, and each thing once, however many entries name it.
struct Asking<'a> {
    gh: &'a publish::Tool<runtime::GhAnswer>,
    defaults: HashMap<String, Option<String>>,
    commits: HashMap<(String, String), Asked>,
}
impl Asking<'_> {
    /// The repository's default branch; `None` when its login cannot read it, as GitHub answers
    /// a private repository with a 404 then too.
    fn default_branch(&mut self, listed: &Listed) -> Option<String> {
        if let Some(known) = self.defaults.get(&listed.github) {
            return known.clone();
        }
        let branch = match ask(self.gh, &listed.at, &format!("repos/{}", listed.github)) {
            Answer::Found(repository) => repository["default_branch"].as_str().map(str::to_owned),
            _ => None,
        };
        self.defaults.insert(listed.github.clone(), branch.clone());
        branch
    }
    fn commit(&mut self, listed: &Listed, sha: &str) -> Asked {
        let asked = (listed.github.clone(), sha.to_owned());
        if let Some(known) = self.commits.get(&asked) {
            return *known;
        }
        let answer = self.ask_commit(listed, sha);
        self.commits.insert(asked, answer);
        answer
    }
    fn ask_commit(&mut self, listed: &Listed, sha: &str) -> Asked {
        let Some(default) = self.default_branch(listed) else {
            return Asked::Unasked;
        };
        let github = &listed.github;
        let endpoint = format!("repos/{github}/commits/{sha}");
        let full = match ask(self.gh, &listed.at, &endpoint) {
            Answer::Found(commit) => commit["sha"].as_str().map(str::to_owned),
            Answer::NotFound => return Asked::NoCommit,
            Answer::Unasked => None,
        };
        let Some(full) = full else {
            return Asked::Unasked;
        };
        let endpoint = format!("repos/{github}/compare/{default}...{full}");
        match ask(self.gh, &listed.at, &endpoint) {
            Answer::Found(compared) => match compared["status"].as_str() {
                Some("behind" | "identical") => Asked::Holds(Found::OnDefault),
                Some("ahead" | "diverged") => Asked::Holds(Found::Elsewhere),
                _ => Asked::Unasked,
            },
            Answer::NotFound => Asked::NoCommit,
            Answer::Unasked => Asked::Unasked,
        }
    }
    /// The verification of commits in a repository only GitHub can answer for: the first
    /// repository that holds one answers. One that could not be asked keeps a commit unverified
    /// as "could not be asked" rather than "no such commit".
    fn verify(&mut self, listed: &[Listed], commits: &[String]) -> RefVerification {
        let mut unasked = false;
        for repository in listed {
            let mut held = None;
            for sha in commits {
                match self.commit(repository, sha) {
                    Asked::Holds(found) => held = held.max(Some(found)),
                    Asked::NoCommit => {}
                    Asked::Unasked => unasked = true,
                }
            }
            match held {
                Some(Found::OnDefault) => {
                    return RefVerification {
                        state: RefState::Verified,
                        reason: None,
                    };
                }
                Some(Found::Elsewhere) => {
                    return RefVerification {
                        state: RefState::Claimed,
                        reason: Some(ON_A_BRANCH.into()),
                    };
                }
                None => {}
            }
        }
        RefVerification {
            state: RefState::Unverified,
            reason: Some(match unasked {
                true => NOT_ASKED.to_owned(),
                false => "no such commit".to_owned(),
            }),
        }
    }
}

/// `verify`, then with `gh` the commits of each folder only GitHub can answer for, and each
/// named PR's state: one that merged verifies its entry and an open one claims it. The brain
/// index and compile runs both verify this way, so an entry reads the same wherever it is shown;
/// each commit and PR is asked about once per call.
pub(crate) fn verify_with(
    clones: &Clones,
    refs: &[(&str, &str)],
    cache: &mut Cache,
    gh: Option<&publish::Tool<runtime::GhAnswer>>,
) -> Vec<RefVerification> {
    let mut pulls: HashMap<(String, u64), Option<PullState>> = HashMap::new();
    let verified = verify(clones, refs, cache);
    let Some(gh) = gh else {
        return verified;
    };
    let mut asking = Asking {
        gh,
        defaults: HashMap::new(),
        commits: HashMap::new(),
    };
    (refs.iter())
        .zip(verified)
        .map(|((folder, reference), verification)| {
            let key = folder.to_ascii_lowercase();
            let anchored = anchors(reference);
            let listed = clones
                .1
                .get(&key)
                .filter(|_| !anchored.commits.is_empty() || anchored.pull_request.is_some());
            let verification = match listed.filter(|_| !anchored.commits.is_empty()) {
                Some(listed) => asking.verify(listed, &anchored.commits),
                None => verification,
            };
            let Some(number) = anchored.pull_request else {
                return verification;
            };
            if verification.state == RefState::Verified {
                return verification;
            }
            let here = (clones.0.get(&key).into_iter().flatten())
                .filter_map(|clone| Some((clone.path.as_path(), clone.github.as_deref()?)));
            let there = (listed.into_iter().flatten())
                .map(|listed| (listed.at.as_path(), listed.github.as_str()));
            let state = here.chain(there).find_map(|(at, github)| {
                *(pulls.entry((github.to_owned(), number)))
                    .or_insert_with(|| pull_state(gh, at, github, number))
            });
            if listed.is_some() && state.is_none() && anchored.commits.is_empty() {
                return RefVerification {
                    state: RefState::Unverified,
                    reason: Some(NOT_ASKED.to_owned()),
                };
            }
            match state {
                Some(PullState::Merged) => RefVerification {
                    state: RefState::Verified,
                    reason: None,
                },
                Some(PullState::Open) => RefVerification {
                    state: RefState::Claimed,
                    reason: Some(format!("PR #{number} is open")),
                },
                Some(PullState::Closed) => RefVerification {
                    reason: Some(format!("PR #{number} {CLOSED_UNMERGED}")),
                    ..verification
                },
                None => verification,
            }
        })
        .collect()
}

/// A compile run's check of each `(folder, ref)`: each clone the refs name is fetched once, then
/// `verify_with` runs, and a verified commit's release tag is recorded.
pub(crate) fn for_compile(
    clones: &Clones,
    refs: &[(&str, &str)],
    tools: &publish::Tools,
) -> Vec<Checked> {
    let mut fetched: HashSet<&Path> = HashSet::new();
    for (folder, _) in refs {
        for clone in clones
            .0
            .get(&folder.to_ascii_lowercase())
            .into_iter()
            .flatten()
        {
            if fetched.insert(&clone.path) {
                // An offline fetch leaves the clone as it was, which verify reads as it is.
                let _ = (tools.git)(&clone.path, &["fetch", "--quiet", "origin"]);
            }
        }
    }
    let verified = verify_with(clones, refs, &mut Cache::default(), Some(&tools.gh));
    refs.iter()
        .zip(verified)
        .map(|((folder, reference), verification)| {
            let anchors = anchors(reference);
            let here = clones.0.get(&folder.to_ascii_lowercase());
            let tag = match verification.state {
                RefState::Verified => (here.into_iter().flatten()).find_map(|clone| {
                    (anchors.commits.iter()).find_map(|sha| tag_containing(&clone.path, sha))
                }),
                _ => None,
            };
            Checked { verification, tag }
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
    use std::sync::{Arc, Mutex};

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
        let clones = Clones(
            HashMap::from([(
                "relay".to_owned(),
                vec![RepoClone {
                    path: clone.clone(),
                    defaults: vec!["refs/remotes/origin/HEAD".into()],
                    github: Some("Sage-Social/relay".into()),
                }],
            )]),
            HashMap::new(),
        );
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
        assert_eq!(elsewhere.reason.as_deref(), Some(NOT_LISTED));
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

    /// Tools whose `gh` says PR #81 merged and #92 is open, counting each `git fetch`.
    fn compile_tools(fetches: Arc<Mutex<Vec<PathBuf>>>) -> publish::Tools {
        let gh = |_: &Path, args: &[&str]| -> Result<runtime::GhAnswer> {
            let pull = match args.last().copied() {
                Some("repos/Sage-Social/relay/pulls/81") => {
                    json!({"state": "closed", "merged": true})
                }
                Some("repos/Sage-Social/relay/pulls/92") => {
                    json!({"state": "open", "merged": false})
                }
                other => panic!("unexpected gh call {other:?}"),
            };
            Ok(runtime::GhAnswer {
                status: publish::tests::exit(0),
                output: pull.to_string(),
                error: String::new(),
            })
        };
        let git = move |path: &Path, args: &[&str]| -> Result<runtime::GitAnswer> {
            assert_eq!(args, ["fetch", "--quiet", "origin"]);
            fetches.lock().unwrap().push(path.to_owned());
            Ok(runtime::GitAnswer {
                status: publish::tests::exit(0),
                output: String::new(),
                error: String::new(),
            })
        };
        publish::Tools {
            gh: Arc::new(gh),
            git: Arc::new(git),
        }
    }

    #[test]
    fn a_compile_fetches_once_reads_named_prs_and_records_the_release_tag() {
        let relay = relay();
        let clone = relay.clones.0["relay"][0].path.clone();
        git(&clone, &["tag", "v1.8.0", &relay.merged]);
        let fetches = Arc::new(Mutex::new(vec![]));
        let tools = compile_tools(fetches.clone());
        let (merged, merged_pr, open_pr) = (
            format!("main @ {}", relay.merged),
            format!("side @ {} · PR #81", relay.side),
            format!("wiffletree/retry-telnyx-timeout @ {} · PR #92", relay.side),
        );
        let refs = [
            ("relay", merged.as_str()),
            ("relay", merged_pr.as_str()),
            ("relay", open_pr.as_str()),
            ("relay", "—"),
            ("watch", merged.as_str()),
        ];
        let checked = for_compile(&relay.clones, &refs, &tools);
        let states: Vec<RefState> = checked.iter().map(|c| c.verification.state).collect();
        assert_eq!(
            states,
            [
                RefState::Verified,
                RefState::Verified,
                RefState::Claimed,
                RefState::Unverified,
                RefState::Unverified,
            ]
        );
        assert_eq!(checked[0].tag.as_deref(), Some("v1.8.0"));
        assert_eq!(checked[1].tag, None);
        assert_eq!(
            checked[2].verification.reason.as_deref(),
            Some("PR #92 is open")
        );
        assert_eq!(checked[4].verification.reason.as_deref(), Some(NOT_LISTED));
        assert_eq!(*fetches.lock().unwrap(), [clone]);
        // The brain index verifies the same refs alike, PRs included, without fetching.
        let indexed = verify_with(&relay.clones, &refs, &mut Cache::default(), Some(&tools.gh));
        let compiled: Vec<RefVerification> = checked.into_iter().map(|c| c.verification).collect();
        assert_eq!(indexed, compiled);
        assert_eq!(fetches.lock().unwrap().len(), 1);
    }

    /// A `gh` answering `repos/` endpoints from `answers`, 404 for the rest, counting each ask.
    fn github(
        answers: Vec<(&'static str, Result<Value, &'static str>)>,
    ) -> (publish::Tool<runtime::GhAnswer>, Arc<Mutex<Vec<String>>>) {
        let asked = Arc::new(Mutex::new(vec![]));
        let log = asked.clone();
        let gh = move |_: &Path, args: &[&str]| -> Result<runtime::GhAnswer> {
            let endpoint = *args.last().unwrap();
            log.lock().unwrap().push(endpoint.to_owned());
            let answer = (answers.iter()).find(|(asked, _)| *asked == endpoint);
            let (code, output, error) = match answer.map(|(_, answer)| answer) {
                Some(Ok(json)) => (0, json.to_string(), ""),
                Some(Err(error)) => (1, String::new(), *error),
                None => (1, String::new(), "gh: Not Found (HTTP 404)"),
            };
            Ok(runtime::GhAnswer {
                status: publish::tests::exit(code),
                output,
                error: error.into(),
            })
        };
        (Arc::new(gh), asked)
    }

    const SHA: &str = "7d03b1e7d03b1e7d03b1e7d03b1e7d03b1e7d03b";

    fn listed(names: &[&str]) -> Clones {
        let names: Vec<String> = names.iter().map(|name| name.to_string()).collect();
        Clones::default().with_listed(&names, Path::new("/tmp"))
    }

    fn pica(
        extra: Vec<(&'static str, Result<Value, &'static str>)>,
    ) -> Vec<(&'static str, Result<Value, &'static str>)> {
        let mut answers = vec![
            (
                "repos/Vyttle-LLC/pica-api",
                Ok(json!({"default_branch": "main"})),
            ),
            (
                "repos/Vyttle-LLC/pica-api/commits/7d03b1e",
                Ok(json!({"sha": SHA})),
            ),
        ];
        answers.extend(extra);
        answers
    }

    const COMPARE: &str =
        "repos/Vyttle-LLC/pica-api/compare/main...7d03b1e7d03b1e7d03b1e7d03b1e7d03b1e7d03b";

    fn verified(
        clones: &Clones,
        reference: &str,
        gh: &publish::Tool<runtime::GhAnswer>,
    ) -> RefVerification {
        let refs = [("pica-api", reference)];
        verify_with(clones, &refs, &mut Cache::default(), Some(gh)).remove(0)
    }

    #[test]
    fn a_repository_not_cloned_here_is_verified_through_github() {
        let clones = listed(&["Vyttle-LLC/pica-api"]);
        for (status, state) in [
            ("behind", RefState::Verified),
            ("identical", RefState::Verified),
            ("ahead", RefState::Claimed),
            ("diverged", RefState::Claimed),
        ] {
            let (gh, _) = github(pica(vec![(COMPARE, Ok(json!({"status": status})))]));
            let answer = verified(&clones, "main @ 7d03b1e", &gh);
            assert_eq!(answer.state, state, "{status}");
        }
    }

    #[test]
    fn a_commit_github_does_not_hold_is_unverified_with_no_such_commit() {
        let clones = listed(&["Vyttle-LLC/pica-api"]);
        let (gh, _) = github(vec![(
            "repos/Vyttle-LLC/pica-api",
            Ok(json!({"default_branch": "main"})),
        )]);
        let answer = verified(&clones, "main @ 7d03b1e", &gh);
        assert_eq!(answer.state, RefState::Unverified);
        assert_eq!(answer.reason.as_deref(), Some("no such commit"));
    }

    #[test]
    fn a_login_github_refuses_is_unverified_and_counted_as_not_asked() {
        let clones = listed(&["Vyttle-LLC/pica-api"]);
        for answers in [
            vec![("repos/Vyttle-LLC/pica-api", Err("HTTP 403: Forbidden"))],
            vec![],
            pica(vec![(COMPARE, Err("HTTP 502: Bad Gateway"))]),
        ] {
            let (gh, _) = github(answers);
            let answer = verified(&clones, "main @ 7d03b1e", &gh);
            assert_eq!(answer.state, RefState::Unverified);
            assert_eq!(answer.reason.as_deref(), Some(NOT_ASKED));
        }
    }

    #[test]
    fn a_pr_github_answers_settles_a_repository_not_cloned_here() {
        let clones = listed(&["Vyttle-LLC/pica-api"]);
        let pull = "repos/Vyttle-LLC/pica-api/pulls/92";
        let reference = "side @ 7d03b1e · PR #92";
        let merged = json!({"state": "closed", "merged": true});
        let open = json!({"state": "open", "merged": false});
        let on_side = (COMPARE, Ok(json!({"status": "ahead"})));
        let (gh, _) = github(pica(vec![on_side.clone(), (pull, Ok(merged))]));
        assert_eq!(verified(&clones, reference, &gh).state, RefState::Verified);
        let (gh, _) = github(pica(vec![on_side, (pull, Ok(open))]));
        let answer = verified(&clones, reference, &gh);
        assert_eq!(answer.state, RefState::Claimed);
        assert_eq!(answer.reason.as_deref(), Some("PR #92 is open"));
    }

    #[test]
    fn two_owners_of_one_name_are_each_tried_and_each_commit_is_asked_once() {
        let clones = listed(&["Other-Org/pica-api", "Vyttle-LLC/pica-api"]);
        let (gh, asked) = github(pica(vec![
            (
                "repos/Other-Org/pica-api",
                Ok(json!({"default_branch": "main"})),
            ),
            (COMPARE, Ok(json!({"status": "behind"}))),
        ]));
        let refs = [
            ("pica-api", "main @ 7d03b1e"),
            ("pica-api", "other @ 7d03b1e"),
        ];
        let answers = verify_with(&clones, &refs, &mut Cache::default(), Some(&gh));
        assert!(answers.iter().all(|v| v.state == RefState::Verified));
        let asked = asked.lock().unwrap();
        let once = |endpoint: &str| asked.iter().filter(|e| *e == endpoint).count();
        assert_eq!(once(COMPARE), 1);
        assert_eq!(once("repos/Other-Org/pica-api/commits/7d03b1e"), 1);
        assert_eq!(once("repos/Vyttle-LLC/pica-api"), 1);
    }

    #[test]
    fn a_folder_no_space_lists_and_a_ref_with_no_anchor_ask_github_nothing() {
        let clones = listed(&["Vyttle-LLC/pica-api"]);
        let (gh, asked) = github(vec![]);
        let refs = [
            ("assistant", "main @ 7d03b1e"),
            ("planning", "—"),
            ("pica-api", "—"),
            ("pica-api", "feature/x @ —"),
        ];
        let answers = verify_with(&clones, &refs, &mut Cache::default(), Some(&gh));
        assert!(answers.iter().all(|v| v.state == RefState::Unverified));
        assert_eq!(answers[0].reason.as_deref(), Some(NOT_LISTED));
        assert!(asked.lock().unwrap().is_empty());
    }

    #[test]
    fn a_listed_repository_with_a_clone_here_is_read_from_the_clone() {
        let relay = relay();
        let names = ["Sage-Social/relay".to_owned()];
        let clones = relay.clones.clone().with_listed(&names, Path::new("/tmp"));
        assert!(clones.1.is_empty());
    }
}
