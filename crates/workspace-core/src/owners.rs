//! Which repositories publish a ticket without asking the human. One rule, shared by the host
//! that enforces it and the Repositories page that shows it.
use crate::{KeeperSettings, Repository, Serialize};

/// The branch `base` names on `origin`, such as `main` for `origin/main`. `None` for a local
/// base or one on another remote, which a pull request from `origin` cannot merge into.
pub fn origin_branch(base: &str) -> Option<&str> {
    base.strip_prefix("origin/").filter(|b| !b.is_empty())
}

/// The entry that allows a repository: the owner GitHub reports, or `owner/name` for a fork,
/// which its owner's entry never covers. `None` until GitHub has answered.
pub fn owner_entry(repository: &Repository) -> Option<String> {
    let facts = repository.github.as_ref()?.facts.as_ref()?;
    let (owner, _) = facts.name_with_owner.split_once('/')?;
    Some(if facts.fork {
        facts.name_with_owner.clone()
    } else {
        owner.to_owned()
    })
}

/// Entries compare without regard to letter case, as GitHub's logins do.
pub fn is_entry_allowed(entry: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|a| a.eq_ignore_ascii_case(entry))
}

/// Whether a ticket in the repository publishes without asking: GitHub has answered for its
/// `origin`, its base is on `origin`, and its entry is allowed.
pub fn is_allowed(repository: &Repository, allowed: &[String]) -> bool {
    origin_branch(&repository.base).is_some()
        && owner_entry(repository).is_some_and(|entry| is_entry_allowed(&entry, allowed))
}

/// A repository row's PR keeper label while a trigger is on: "kept" where it publishes without
/// asking, "not kept" elsewhere.
pub fn kept_label(
    repository: &Repository,
    allowed: &[String],
    keeper: &KeeperSettings,
) -> Option<&'static str> {
    keeper
        .is_on()
        .then(|| match is_allowed(repository, allowed) {
            true => "kept",
            false => "not kept",
        })
}

/// The rule a repository's row states, such as "Vyttle-LLC · without asking".
pub fn rule_label(repository: &Repository, allowed: &[String]) -> String {
    let Some(github) = &repository.github else {
        return repository
            .origin_problem
            .clone()
            .unwrap_or_else(|| "Not on GitHub".into());
    };
    let Some(facts) = &github.facts else {
        return match github.problem {
            Some(_) => format!("{} · not found on GitHub · asks first", github.owner),
            None => format!("{} · asks first until GitHub answers", github.owner),
        };
    };
    let who = if facts.fork {
        "Fork"
    } else {
        facts.name_with_owner.split('/').next().unwrap_or_default()
    };
    if origin_branch(&repository.base).is_none() {
        format!("{who} · asks first · base is not on origin")
    } else if is_allowed(repository, allowed) {
        format!("{who} · without asking")
    } else {
        format!("{who} · asks first")
    }
}

/// The rows of the "Open pull requests without asking" block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OwnerEntries {
    pub owners: Vec<OwnerRow>,
    pub forks: Vec<ForkRow>,
    pub not_on_github: Vec<LocalRow>,
}
/// An organization or user and its workspace repositories, forks excepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnerRow {
    /// Also the entry that allows it.
    pub login: String,
    /// `None` until GitHub has answered for one of its repositories.
    pub organization: Option<bool>,
    /// Workspace repositories, so two clones of one GitHub repository count twice.
    pub repositories: usize,
    pub public: usize,
    /// Repositories GitHub has not answered for; they ask first whatever is allowed.
    pub unanswered: usize,
}
impl OwnerRow {
    pub fn kind(&self) -> Option<&'static str> {
        self.organization.map(|organization| match organization {
            true => "Organization",
            false => "User",
        })
    }
    /// Such as "29 repositories · all private".
    pub fn summary(&self) -> String {
        let count = match self.repositories {
            1 => "1 repository".to_owned(),
            n => format!("{n} repositories"),
        };
        let answered = self.repositories - self.unanswered;
        let private = answered - self.public;
        let visibility = match (private, self.public) {
            (0, 0) => None,
            (0, 1) if answered == 1 => Some("public".to_owned()),
            (1, 0) if answered == 1 => Some("private".to_owned()),
            (0, _) => Some("all public".to_owned()),
            (_, 0) => Some("all private".to_owned()),
            (private, public) => Some(format!("{private} private, {public} public")),
        };
        let unanswered = match self.unanswered {
            0 => None,
            n if n == self.repositories => Some("asking first until GitHub answers".to_owned()),
            n => Some(format!("{n} asking first until GitHub answers")),
        };
        [Some(count), visibility, unanswered]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
    }
}
/// A fork, allowed on its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForkRow {
    /// `owner/name`, also the entry that allows it.
    pub entry: String,
    pub parent: Option<String>,
    pub public: bool,
}
impl ForkRow {
    /// Such as "Fork of longbridge/gpui-component".
    pub fn summary(&self) -> String {
        match &self.parent {
            Some(parent) => format!("Fork of {parent}"),
            None => "Fork".into(),
        }
    }
    pub fn detail(&self) -> String {
        let visibility = if self.public { "public" } else { "private" };
        format!("{visibility} · pull requests stay inside the fork")
    }
}
/// A repository with nowhere on GitHub to push to, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LocalRow {
    pub name: String,
    pub reason: String,
}

/// Groups the workspace's repositories for the owners block, in the order they were added. A
/// repository counts under the owner GitHub reports, or under its URL's owner until GitHub has
/// answered.
pub fn owner_entries(repositories: &[Repository]) -> OwnerEntries {
    let mut entries = OwnerEntries::default();
    for repository in repositories {
        let Some(github) = &repository.github else {
            entries.not_on_github.push(LocalRow {
                name: repository.name.clone(),
                reason: repository
                    .origin_problem
                    .clone()
                    .unwrap_or_else(|| "No GitHub remote".into()),
            });
            continue;
        };
        let facts = github.facts.as_ref();
        if let Some(facts) = facts.filter(|f| f.fork) {
            // Two clones of one fork are one thing to allow.
            if !entries
                .forks
                .iter()
                .any(|f| f.entry.eq_ignore_ascii_case(&facts.name_with_owner))
            {
                entries.forks.push(ForkRow {
                    entry: facts.name_with_owner.clone(),
                    parent: facts.parent.clone(),
                    public: facts.public,
                });
            }
            continue;
        }
        let login = facts
            .and_then(|f| f.name_with_owner.split('/').next())
            .unwrap_or(&github.owner);
        let index = entries
            .owners
            .iter()
            .position(|o| o.login.eq_ignore_ascii_case(login))
            .unwrap_or_else(|| {
                entries.owners.push(OwnerRow {
                    login: login.to_owned(),
                    organization: None,
                    repositories: 0,
                    public: 0,
                    unanswered: 0,
                });
                entries.owners.len() - 1
            });
        let row = &mut entries.owners[index];
        row.repositories += 1;
        match facts {
            Some(facts) => {
                row.organization = Some(facts.organization);
                row.public += usize::from(facts.public);
            }
            None => row.unanswered += 1,
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GitHubFacts, GitHubRepository};

    fn repository(name_with_owner: &str, fork: bool, public: bool) -> Repository {
        let (owner, name) = name_with_owner.split_once('/').unwrap();
        Repository {
            id: crate::new_id(),
            name: name.into(),
            path: format!("/code/{name}"),
            base: "origin/main".into(),
            github: Some(GitHubRepository {
                owner: owner.into(),
                name: name.into(),
                facts: Some(GitHubFacts {
                    name_with_owner: name_with_owner.into(),
                    organization: owner != "MHohlios",
                    fork,
                    parent: fork.then(|| format!("longbridge/{name}")),
                    public,
                    checked_at: 1,
                    branches: None,
                }),
                problem: None,
            }),
            origin_problem: None,
            checks: None,
            protected_branches: vec![],
        }
    }
    fn unanswered(owner: &str, name: &str) -> Repository {
        let mut repository = repository(&format!("{owner}/{name}"), false, false);
        repository.github.as_mut().unwrap().facts = None;
        repository
    }
    fn local(name: &str) -> Repository {
        Repository {
            github: None,
            base: "main".into(),
            ..repository(&format!("nobody/{name}"), false, false)
        }
    }
    fn allowed(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| (*e).to_owned()).collect()
    }

    #[test]
    fn an_allowed_owners_repository_publishes_without_asking() {
        let wiffletree = repository("Vyttle-LLC/wiffletree", false, true);
        assert!(is_allowed(&wiffletree, &allowed(&["Vyttle-LLC"])));
        assert!(!is_allowed(&wiffletree, &allowed(&["Sage-Social"])));
        assert!(!is_allowed(&wiffletree, &[]));
        assert_eq!(
            rule_label(&wiffletree, &allowed(&["Vyttle-LLC"])),
            "Vyttle-LLC · without asking"
        );
        assert_eq!(rule_label(&wiffletree, &[]), "Vyttle-LLC · asks first");
    }

    #[test]
    fn entries_compare_without_regard_to_case() {
        let wiffletree = repository("Vyttle-LLC/wiffletree", false, true);
        assert!(is_allowed(&wiffletree, &allowed(&["vyttle-llc"])));
        let fork = repository("MHohlios/gpui-component", true, true);
        assert!(is_allowed(&fork, &allowed(&["mhohlios/GPUI-component"])));
    }

    #[test]
    fn a_fork_is_allowed_only_by_its_own_entry() {
        let fork = repository("MHohlios/gpui-component", true, true);
        assert_eq!(owner_entry(&fork).unwrap(), "MHohlios/gpui-component");
        assert!(!is_allowed(&fork, &allowed(&["MHohlios"])));
        assert_eq!(
            rule_label(&fork, &allowed(&["MHohlios"])),
            "Fork · asks first"
        );
        assert!(is_allowed(&fork, &allowed(&["MHohlios/gpui-component"])));
        // The fork's entry allows nothing else of its owner.
        let other = repository("MHohlios/dotfiles", false, false);
        assert!(!is_allowed(&other, &allowed(&["MHohlios/gpui-component"])));
    }

    #[test]
    fn a_repository_github_has_not_answered_for_asks_first() {
        let pending = unanswered("Vyttle-LLC", "wiffletree");
        assert_eq!(owner_entry(&pending), None);
        assert!(!is_allowed(&pending, &allowed(&["Vyttle-LLC"])));
        assert_eq!(
            rule_label(&pending, &allowed(&["Vyttle-LLC"])),
            "Vyttle-LLC · asks first until GitHub answers"
        );
        let mut missing = pending;
        missing.github.as_mut().unwrap().problem = Some("Could not resolve".into());
        assert_eq!(
            rule_label(&missing, &allowed(&["Vyttle-LLC"])),
            "Vyttle-LLC · not found on GitHub · asks first"
        );
    }

    #[test]
    fn a_base_on_another_remote_or_a_local_branch_never_publishes_without_asking() {
        for base in ["upstream/main", "main", "origin/"] {
            let mut repository = repository("Vyttle-LLC/wiffletree", false, true);
            repository.base = base.into();
            assert!(
                !is_allowed(&repository, &allowed(&["Vyttle-LLC"])),
                "{base}"
            );
            assert_eq!(
                rule_label(&repository, &allowed(&["Vyttle-LLC"])),
                "Vyttle-LLC · asks first · base is not on origin"
            );
        }
        assert_eq!(origin_branch("origin/release/2"), Some("release/2"));
    }

    #[test]
    fn the_owner_github_reports_is_the_one_a_repository_counts_under() {
        let mut moved = repository("new-org/app", false, false);
        moved.github.as_mut().unwrap().owner = "old-org".into();
        assert!(is_allowed(&moved, &allowed(&["new-org"])));
        assert!(!is_allowed(&moved, &allowed(&["old-org"])));
        assert_eq!(owner_entries(&[moved]).owners[0].login, "new-org");
    }

    #[test]
    fn owners_are_listed_with_their_counts_and_visibility() {
        let mut repositories: Vec<_> = (0..29)
            .map(|i| repository(&format!("Sage-Social/r{i}"), false, false))
            .collect();
        repositories.push(repository("Vyttle-LLC/wiffletree", false, true));
        repositories.push(repository("MHohlios/dotfiles", false, false));
        repositories.push(repository("MHohlios/site", false, true));
        let entries = owner_entries(&repositories);
        let rows: Vec<_> = entries
            .owners
            .iter()
            .map(|o| (o.login.as_str(), o.kind(), o.summary()))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "Sage-Social",
                    Some("Organization"),
                    "29 repositories · all private".to_owned()
                ),
                (
                    "Vyttle-LLC",
                    Some("Organization"),
                    "1 repository · public".to_owned()
                ),
                (
                    "MHohlios",
                    Some("User"),
                    "2 repositories · 1 private, 1 public".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn two_clones_of_one_repository_count_twice() {
        let clones = [
            repository("Sage-Social/app", false, false),
            repository("sage-social/app", false, false),
        ];
        let entries = owner_entries(&clones);
        assert_eq!(entries.owners.len(), 1);
        assert_eq!(entries.owners[0].repositories, 2);
    }

    #[test]
    fn a_fork_has_its_own_row_and_is_not_counted_under_its_owner() {
        let entries = owner_entries(&[
            repository("MHohlios/dotfiles", false, false),
            repository("MHohlios/gpui-component", true, true),
        ]);
        assert_eq!(entries.owners[0].repositories, 1);
        let fork = &entries.forks[0];
        assert_eq!(fork.entry, "MHohlios/gpui-component");
        assert_eq!(fork.summary(), "Fork of longbridge/gpui-component");
        assert_eq!(fork.detail(), "public · pull requests stay inside the fork");
    }

    #[test]
    fn a_repository_that_is_not_on_github_has_no_entry_and_says_why() {
        let scratch = local("scratch-prototypes");
        let mut two_urls = local("mirrored");
        two_urls.origin_problem = Some("origin pushes to more than one place".into());
        assert_eq!(owner_entry(&scratch), None);
        assert!(!is_allowed(&scratch, &allowed(&["nobody"])));
        assert_eq!(rule_label(&scratch, &[]), "Not on GitHub");
        assert_eq!(
            rule_label(&two_urls, &[]),
            "origin pushes to more than one place"
        );
        let entries = owner_entries(&[scratch, two_urls]);
        assert!(entries.owners.is_empty() && entries.forks.is_empty());
        let reasons: Vec<_> = entries
            .not_on_github
            .iter()
            .map(|r| r.reason.as_str())
            .collect();
        assert_eq!(
            reasons,
            ["No GitHub remote", "origin pushes to more than one place"]
        );
    }

    #[test]
    fn owners_are_listed_before_github_answers() {
        let repositories: Vec<_> = (0..29)
            .map(|i| unanswered("Sage-Social", &format!("r{i}")))
            .collect();
        let entries = owner_entries(&repositories);
        assert_eq!(entries.owners[0].kind(), None);
        assert_eq!(
            entries.owners[0].summary(),
            "29 repositories · asking first until GitHub answers"
        );
        let mut mixed = repositories;
        mixed.push(repository("Sage-Social/app", false, false));
        assert_eq!(
            owner_entries(&mixed).owners[0].summary(),
            "30 repositories · private · 29 asking first until GitHub answers"
        );
    }
}
