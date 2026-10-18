//! Stacked tickets: a ticket whose branch starts from another ticket's branch in the same
//! repository, is measured against it, and whose pull request opens into it.
use crate::*;

/// What a ticket's branch is measured against and where its pull request opens.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Base {
    /// The repository's base, such as `origin/main`.
    Repository(String),
    Stacked {
        /// The repository's base.
        repository: String,
        /// The parent ticket's branch.
        branch: String,
        parent: String,
        /// The host has pushed the parent's branch to GitHub.
        pushed: bool,
        /// The parent's pull request has merged.
        merged: bool,
    },
}
impl Base {
    /// `parent` is the ticket's `stacked_on` ticket, which a stacked ticket must have.
    pub(crate) fn of(ticket: &Ticket, repository: &Repository, parent: Option<&Ticket>) -> Self {
        match parent.filter(|p| ticket.stacked_on.as_ref() == Some(&p.id)) {
            Some(parent) => Self::Stacked {
                repository: repository.base.clone(),
                branch: parent.branch.clone(),
                parent: parent.title.clone(),
                pushed: parent.pushed.is_some(),
                merged: parent
                    .pull_request
                    .as_ref()
                    .is_some_and(|pr| pr.state == PrState::Merged),
            },
            None => Self::Repository(repository.base.clone()),
        }
    }
    /// The ref reviews, sizes and merge-bases measure against: the parent's branch for a
    /// stacked ticket, so its reviewers see only its own changes.
    pub(crate) fn measured(&self) -> &str {
        match self {
            Self::Repository(base) => base,
            Self::Stacked { branch, .. } => branch,
        }
    }
    /// Whether two commits carry the same patch against the measured ref, or for a stacked
    /// ticket against the repository's base, as after a rebase of the whole stack.
    pub(crate) fn same_patch(&self, worktree: &Path, one: &str, other: &str) -> Result<bool> {
        let bases = match self {
            Self::Repository(base) => vec![base],
            Self::Stacked {
                repository, branch, ..
            } => vec![branch, repository],
        };
        for base in bases {
            if worktrees::patch_id(worktree, base, one)?
                == worktrees::patch_id(worktree, base, other)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// The branch on origin the pull request opens into: the parent's branch once the host has
    /// pushed it, or the repository's base after the parent's pull request merged.
    pub(crate) fn pull_request_base(&self) -> Result<String> {
        let repository = match self {
            Self::Stacked {
                branch,
                parent,
                pushed,
                merged: false,
                ..
            } => {
                ensure!(
                    *pushed,
                    "This ticket is stacked on \"{parent}\", whose branch {branch} is not on GitHub yet. Publish that ticket first, then publish this one; nothing was pushed."
                );
                return Ok(branch.clone());
            }
            Self::Stacked { repository, .. } | Self::Repository(repository) => repository,
        };
        let base = origin_branch(repository).with_context(|| {
            format!(
                "The repository's base {repository} is not on origin, so the ticket's pull request would cross repositories"
            )
        })?;
        Ok(base.to_owned())
    }
}

impl Host {
    pub(crate) fn ticket_base(&self, ticket: &Ticket) -> Result<Base> {
        let repository = self.ticket_repository(ticket)?;
        let parent = match &ticket.stacked_on {
            Some(id) => Some(self.ticket(id).context("The parent ticket was not found")?),
            None => None,
        };
        Ok(Base::of(ticket, &repository, parent.as_ref()))
    }
    /// The parent a new ticket may stack on: a ticket of the same project and repository that
    /// is not closed.
    pub(crate) fn stack_parent(
        &self,
        coordinator: &Session,
        repository: &Repository,
        id: &str,
    ) -> Result<Ticket> {
        let parent = self
            .ticket(id)
            .with_context(|| format!("No ticket {id} to stack on"))?;
        ensure!(
            self.session(&parent.coordinator_id)?.project_id == coordinator.project_id,
            "Ticket \"{}\" belongs to another project; stack only on this project's tickets",
            parent.title
        );
        ensure!(
            parent.repository_id == repository.id,
            "Ticket \"{}\" is in another repository; a stacked ticket shares its parent's repository",
            parent.title
        );
        ensure!(
            parent.state != TicketState::Closed,
            "Ticket \"{}\" is closed; stack only on a ticket whose work continues",
            parent.title
        );
        Ok(parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::{Fixture, fixture};
    use crate::test_support::git;

    /// A ticket stacked on the fixture's reviewed ticket.
    fn child(f: &mut Fixture) -> Ticket {
        let repository = f.ticket().repository_id;
        f.host
            .create_stacked_ticket(
                &f.coordinator.id.clone(),
                &repository,
                "Child",
                "Do",
                Some(&f.ticket.clone()),
            )
            .unwrap()
    }
    /// Commits `file` on the child's branch and ends a passed review there.
    fn reviewed_child(f: &mut Fixture) -> Ticket {
        let mut child = child(f);
        let worktree = Path::new(&child.worktree);
        fs::write(worktree.join("c.txt"), "three\n").unwrap();
        git(worktree, &["add", "."]);
        git(worktree, &["commit", "-q", "-m", "c"]);
        let head = git(worktree, &["rev-parse", "HEAD"]);
        let mut parent = f.ticket();
        parent.verification.as_mut().unwrap().rounds[0].commit = head;
        child.verification = parent.verification;
        f.host.save_ticket(&child).unwrap();
        child
    }

    #[test]
    fn a_stacked_ticket_starts_at_its_parents_tip_and_shows_its_parent() {
        let mut f = fixture();
        let child = child(&mut f);
        assert_eq!(child.stacked_on.as_ref(), Some(&f.ticket));
        let head = git(Path::new(&child.worktree), &["rev-parse", "HEAD"]);
        assert_eq!(head, f.head());
        let context = f.host.agent_context(&f.coordinator.id).unwrap();
        let shown = context["tickets"].as_array().unwrap();
        let shown = shown.iter().find(|t| t["id"] == child.id.as_str()).unwrap();
        assert_eq!(shown["stacked_on"], f.ticket.as_str());
        // Without a parent, a ticket starts from the repository's base as before.
        let repository = child.repository_id.clone();
        let plain = (f.host)
            .create_ticket(&f.coordinator.id.clone(), &repository, "Plain", "Do")
            .unwrap();
        assert_eq!(plain.stacked_on, None);
        let start = git(Path::new(&plain.worktree), &["rev-parse", "HEAD"]);
        assert_eq!(start, git(&f.repository, &["rev-parse", "origin/main"]));
    }

    #[test]
    fn a_parent_in_another_repository_or_project_or_closed_is_refused() {
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        let other = f.home.path().join("other");
        fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q", "-b", "main"]);
        git(&other, &["commit", "-q", "--allow-empty", "-m", "start"]);
        let other = (f.host)
            .attach_repository(&f.coordinator.project_id, other.to_str().unwrap(), "main")
            .unwrap();
        let elsewhere = f
            .host
            .create_ticket(&coordinator, &other.id, "Elsewhere", "Do");
        let repository = f.ticket().repository_id;
        let refused = |f: &mut Fixture, parent: &str, reason: &str| {
            let error = (f.host)
                .create_stacked_ticket(&coordinator, &repository, "Child", "Do", Some(parent))
                .expect_err("stacking was not refused");
            assert!(format!("{error:#}").contains(reason), "{error:#}");
        };
        refused(&mut f, &elsewhere.unwrap().id, "is in another repository");
        refused(&mut f, "missing", "No ticket missing to stack on");

        let project = f.host.create_project("Other").unwrap();
        let theirs = (f.host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project.id)
            .unwrap();
        let path = f.repository.to_str().unwrap().to_owned();
        let shared = (f.host)
            .attach_repository(&project.id, &path, "origin/main")
            .unwrap();
        let foreign = (f.host)
            .create_ticket(&theirs.id, &shared.id, "Theirs", "Do")
            .unwrap();
        refused(&mut f, &foreign.id, "belongs to another project");

        let mut parent = f.ticket();
        parent.state = TicketState::Closed;
        f.host.save_ticket(&parent).unwrap();
        let closed = f.ticket.clone();
        refused(&mut f, &closed, "is closed");
    }

    #[test]
    fn a_stacked_pr_opens_into_its_parents_branch_once_the_parent_is_pushed() {
        let mut f = fixture();
        f.allow("allowed-org");
        let child = reviewed_child(&mut f);
        let coordinator = f.coordinator.id.clone();
        let publish = |f: &mut Fixture| {
            (f.host).publish_ticket(&coordinator, &child.id, Some("Child"), Some("Why"))
        };
        let error = publish(&mut f).expect_err("published before its parent");
        assert!(format!("{error:#}").contains("Publish that ticket first"));
        assert!(f.host.ticket(&child.id).unwrap().publishing.is_none());

        let mut parent = f.ticket();
        parent.pushed = Some(f.head());
        f.host.save_ticket(&parent).unwrap();
        let (_, job) = publish(&mut f).unwrap();
        let request = job.unwrap().request;
        assert_eq!(request.base, parent.branch);
        // Its size counts only its own commit, not the parent's two.
        assert!(
            request.size.starts_with("1 commit · 1 file"),
            "{}",
            request.size
        );
    }

    #[test]
    fn once_the_parents_pr_merged_a_stacked_pr_opens_into_the_repositorys_base() {
        let mut f = fixture();
        let mut merged = crate::publish::tests::watch_open_pr(&mut f, 1);
        merged.state = PrState::Merged;
        let mut child = f.ticket();
        child.stacked_on = Some("parent".into());
        let mut parent = f.ticket();
        parent.id = "parent".into();
        let repository = f.host.ticket_repository(&child).unwrap();
        let base = |parent: &Ticket| Base::of(&child, &repository, Some(parent));
        assert!(base(&parent).pull_request_base().is_err());
        parent.pull_request = Some(merged);
        assert_eq!(base(&parent).pull_request_base().unwrap(), "main");
        child.stacked_on = None;
        assert_eq!(
            Base::of(&child, &repository, None)
                .pull_request_base()
                .unwrap(),
            "main"
        );
    }

    #[test]
    fn a_stacked_tickets_review_sees_only_its_own_changes() {
        let mut f = fixture();
        let child = reviewed_child(&mut f);
        let head = git(Path::new(&child.worktree), &["rev-parse", "HEAD"]);
        let lines = f.host.diff_lines(&child, &head);
        assert!(
            lines.starts_with(&format!("Base: {} ({})", f.head(), f.ticket().branch)),
            "{lines}"
        );
        assert!(lines.ends_with("1 file changed, 1 insertion(+)"), "{lines}");
        // The parent gains a fix and the child is rebased onto it: the review still holds.
        let parent = f.commit("a.txt", "one, fixed\n");
        git(Path::new(&child.worktree), &["rebase", "-q", &parent]);
        f.host.check_verified_head(&child, &head).unwrap();
    }
}
