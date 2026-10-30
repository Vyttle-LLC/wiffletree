//! Stacked tickets: a ticket whose branch starts from another ticket's branch in the same
//! repository, is measured against it, and whose pull request opens into it.
use crate::*;

/// What a ticket's branch is measured against and where its pull request opens.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Base {
    /// The repository's base, such as `origin/main`.
    Repository(String),
    Stacked {
        /// The parent's base: its own branch on origin, or the repository's base.
        repository: String,
        /// The parent ticket's branch.
        branch: String,
        parent: String,
        /// The commit the host last pushed to the parent's branch on GitHub.
        pushed: Option<String>,
        /// The parent's pull request has merged.
        merged: bool,
    },
}
impl Base {
    /// `parent` is the ticket's `stacked_on` ticket, which a stacked ticket must have.
    pub(crate) fn of(ticket: &Ticket, repository: &Repository, parent: Option<&Ticket>) -> Self {
        match parent.filter(|p| ticket.stacked_on.as_ref() == Some(&p.id)) {
            Some(parent) => Self::Stacked {
                repository: effective_base(parent, repository),
                branch: parent.branch.clone(),
                parent: parent.title.clone(),
                pushed: parent.pushed.clone(),
                merged: parent
                    .pull_request
                    .as_ref()
                    .is_some_and(|pr| pr.state == PrState::Merged),
            },
            None => Self::Repository(effective_base(ticket, repository)),
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
    /// Whether two commits carry the same patch, each measured against the ref it was built on.
    /// A stacked ticket's patch is its own changes alone, so a rebase of the whole stack needs a
    /// new review.
    pub(crate) fn same_patch(&self, worktree: &Path, one: &str, other: &str) -> Result<bool> {
        Ok(
            worktrees::patch_id(worktree, &self.built_on(worktree, one), one)?
                == worktrees::patch_id(worktree, &self.built_on(worktree, other), other)?,
        )
    }
    /// The measured ref, except once the parent's pull request merged: then the newer of the
    /// commit's merge-bases with the parent's branch and with the parent's base. A commit still
    /// on the parent's branch is measured from the parent's head; one rebased onto the base,
    /// which holds the parent's changes however they were merged, from where it left the base,
    /// however far the base has moved since.
    fn built_on(&self, worktree: &Path, commit: &str) -> String {
        let measured = self.measured();
        let Self::Stacked {
            repository,
            merged: true,
            ..
        } = self
        else {
            return measured.to_owned();
        };
        match (
            worktrees::merge_base(worktree, measured, commit),
            worktrees::merge_base(worktree, repository, commit),
        ) {
            (Ok(parent), Ok(base))
                if parent != base && publish::is_ancestor(worktree, &parent, &base) =>
            {
                base
            }
            _ => measured.to_owned(),
        }
    }
    /// The parent's last pushed head, once the parent's pull request merged: a rebase of the
    /// child replays only the commits after it.
    pub(crate) fn merged_parent_head(&self) -> Option<&str> {
        match self {
            Self::Stacked {
                pushed: Some(head),
                merged: true,
                ..
            } => Some(head),
            _ => None,
        }
    }
    /// The branch on origin the pull request opens into: the parent's branch once the host has
    /// pushed it, or the parent's base after the parent's pull request merged.
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
                    pushed.is_some(),
                    "This ticket is stacked on \"{parent}\", whose branch {branch} is not on GitHub yet. Publish that ticket first, then publish this one; nothing was pushed."
                );
                return Ok(branch.clone());
            }
            Self::Stacked { repository, .. } | Self::Repository(repository) => repository,
        };
        let base = origin_branch(repository).with_context(|| {
            format!(
                "The ticket's base {repository} is not on origin, so the ticket's pull request would cross repositories"
            )
        })?;
        Ok(base.to_owned())
    }
}

/// The base a ticket that is not stacked starts from and is measured against: `origin/<base>`
/// for a ticket created on a branch of origin, or else the repository's base.
pub(crate) fn effective_base(ticket: &Ticket, repository: &Repository) -> String {
    match &ticket.base {
        Some(base) => format!("origin/{base}"),
        None => repository.base.clone(),
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
    use crate::publish::tests::{Fixture, Remote, fixture, remote};
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
            (f.host).publish_ticket(&coordinator, &child.id, Some("Child"), Some("Why"), None)
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

    /// A `release` branch one commit past `main`, on GitHub only, and a ticket created on it.
    fn on_release(f: &mut Fixture) -> (Remote, String, Ticket) {
        let r = remote(f);
        git(&f.repository, &["checkout", "-q", "-b", "release"]);
        fs::write(f.repository.join("r.txt"), "release\n").unwrap();
        git(&f.repository, &["add", "."]);
        git(&f.repository, &["commit", "-q", "-m", "r"]);
        let release = git(&f.repository, &["rev-parse", "HEAD"]);
        git(
            &f.repository,
            &["push", "-q", r.bare.to_str().unwrap(), "release"],
        );
        git(&f.repository, &["checkout", "-q", "main"]);
        git(&f.repository, &["branch", "-q", "-D", "release"]);
        let ticket = create_on(f, &r, "On release", None, Some("release")).unwrap();
        (r, release, ticket)
    }
    fn create_on(
        f: &mut Fixture,
        r: &Remote,
        title: &str,
        stack_on: Option<&str>,
        base: Option<&str>,
    ) -> Result<Ticket> {
        let coordinator = f.coordinator.id.clone();
        let repository = f.ticket().repository_id;
        (f.host).create_ticket_with(
            &coordinator,
            &repository,
            title,
            "Do",
            stack_on,
            base,
            &r.tools.git,
        )
    }

    #[test]
    fn a_ticket_on_a_base_starts_there_is_measured_against_it_and_opens_its_pr_into_it() {
        let mut f = fixture();
        f.allow("allowed-org");
        let (r, release, mut ticket) = on_release(&mut f);
        assert_eq!(ticket.base.as_deref(), Some("release"));
        let worktree = Path::new(&ticket.worktree);
        assert_eq!(git(worktree, &["rev-parse", "HEAD"]), release);
        let context = f.host.agent_context(&f.coordinator.id).unwrap();
        let shown = context["tickets"].as_array().unwrap();
        let shown = shown
            .iter()
            .find(|t| t["id"] == ticket.id.as_str())
            .unwrap();
        assert_eq!(shown["base"], "release");

        fs::write(worktree.join("t.txt"), "ticket\n").unwrap();
        git(worktree, &["add", "."]);
        git(worktree, &["commit", "-q", "-m", "t"]);
        let head = git(worktree, &["rev-parse", "HEAD"]);
        let lines = f.host.diff_lines(&ticket, &head);
        assert!(
            lines.starts_with(&format!("Base: {release} (origin/release)")),
            "{lines}"
        );
        assert!(lines.ends_with("1 file changed, 1 insertion(+)"), "{lines}");
        let checks = f.host.overlap_checks().unwrap();
        let on_release = checks.iter().find(|(_, c)| c.base == "origin/release");
        let branches = &on_release.unwrap().1.branches;
        assert_eq!(branches.len(), 1, "compared only with tickets on its base");

        let mut verification = f.ticket().verification;
        verification.as_mut().unwrap().rounds[0].commit = head;
        ticket.verification = verification;
        f.host.save_ticket(&ticket).unwrap();
        let coordinator = f.coordinator.id.clone();
        let (_, job) = (f.host)
            .publish_ticket(&coordinator, &ticket.id, Some("T"), Some("Why"), None)
            .unwrap();
        assert_eq!(job.unwrap().request.base, "release");

        // A ticket stacked on it opens into its branch, then into release once it merged.
        let mut child = create_on(&mut f, &r, "Child", Some(&ticket.id), None).unwrap();
        assert_eq!(child.base.as_deref(), Some("release"));
        let repository = f.host.ticket_repository(&child).unwrap();
        ticket.pushed = Some(release);
        assert_eq!(
            Base::of(&child, &repository, Some(&ticket))
                .pull_request_base()
                .unwrap(),
            ticket.branch
        );
        let mut merged = crate::publish::tests::watch_open_pr(&mut f, 1);
        merged.state = PrState::Merged;
        ticket.pull_request = Some(merged);
        child.stacked_on = Some(ticket.id.clone());
        assert_eq!(
            Base::of(&child, &repository, Some(&ticket))
                .pull_request_base()
                .unwrap(),
            "release"
        );
    }

    #[test]
    fn a_base_not_on_origin_with_a_parent_or_on_another_base_is_refused() {
        let mut f = fixture();
        let (r, _, ticket) = on_release(&mut f);
        let refused = |f: &mut Fixture, stack_on: Option<&str>, base: &str, reason: &str| {
            let error = create_on(f, &r, "Other", stack_on, Some(base))
                .expect_err("creation was not refused");
            assert!(format!("{error:#}").contains(reason), "{error:#}");
        };
        // A branch only this clone has is not on origin.
        git(&f.repository, &["branch", "local-only"]);
        refused(
            &mut f,
            None,
            "local-only",
            "Branch local-only is not on origin",
        );
        refused(&mut f, None, "bad..name", "is not a branch name");
        refused(&mut f, Some(&ticket.id), "release", "not both");
        let title = |f: &mut Fixture, base| create_on(f, &r, "On release", None, base);
        let error = title(&mut f, Some("main")).unwrap_err();
        assert!(format!("{error:#}").contains("Ticket title already exists on another base"));
        let error = title(&mut f, None).unwrap_err();
        assert!(format!("{error:#}").contains("Ticket title already exists on another base"));
        assert_eq!(title(&mut f, Some("release")).unwrap().id, ticket.id);
        let tickets = f.host.tickets().unwrap();
        assert!(!tickets.iter().any(|t| t.title == "Other"));
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
        let worktree = Path::new(&child.worktree);
        git(worktree, &["rebase", "-q", &parent]);
        f.host.check_verified_head(&child, &head, None).unwrap();
        // Undoing the parent's fix matches the reviewed tree against main, but not the child's
        // own reviewed changes, so it needs a new review.
        fs::write(worktree.join("a.txt"), "one\n").unwrap();
        git(worktree, &["commit", "-q", "-am", "revert the fix"]);
        assert!(f.host.check_verified_head(&child, &head, None).is_err());
    }

    #[test]
    fn a_child_rebased_off_its_squash_merged_parent_keeps_its_review() {
        rebased_off_merged_parent(true);
    }

    #[test]
    fn a_child_rebased_off_its_parents_merge_commit_keeps_its_review() {
        rebased_off_merged_parent(false);
    }

    /// The parent merges into main, which gained an unrelated commit first, as a squash or a
    /// merge commit; the keeper rebases the reviewed child onto main, which then moves on.
    fn rebased_off_merged_parent(squash: bool) {
        let mut f = fixture();
        // The keeper's rebase commits as the repository's configured identity, as kept tests set
        // it; a runner without one cannot guess an email.
        for (key, value) in [("user.name", "F"), ("user.email", "f@example.invalid")] {
            git(&f.repository, &["config", key, value]);
        }
        let mut parent = f.ticket();
        parent.pushed = Some(f.head());
        f.host.save_ticket(&parent).unwrap();
        let child = reviewed_child(&mut f);
        let worktree = Path::new(&child.worktree);
        let reviewed = git(worktree, &["rev-parse", "HEAD"]);
        let repository = &f.repository.clone();
        git(
            repository,
            &["checkout", "-q", "-b", "merged", "origin/main"],
        );
        fs::write(repository.join("u.txt"), "unrelated\n").unwrap();
        git(repository, &["add", "."]);
        git(repository, &["commit", "-q", "-m", "Unrelated"]);
        let parent_head = parent.pushed.clone().unwrap();
        if squash {
            fs::write(repository.join("a.txt"), "one\n").unwrap();
            fs::write(repository.join("b.txt"), "two\n").unwrap();
            git(repository, &["add", "."]);
            git(repository, &["commit", "-q", "-m", "Fix contrast (#180)"]);
        } else {
            let message = "Merge pull request #180";
            git(
                repository,
                &["merge", "-q", "--no-ff", "-m", message, &parent_head],
            );
        }
        let main_tip = git(repository, &["rev-parse", "HEAD"]);
        git(
            repository,
            &["update-ref", "refs/remotes/origin/main", &main_tip],
        );
        git(repository, &["checkout", "-q", "main"]);
        let mut merged = crate::publish::tests::watch_open_pr(&mut f, 180);
        merged.state = PrState::Merged;
        let mut parent = f.ticket();
        parent.pull_request = Some(merged);
        f.host.save_ticket(&parent).unwrap();

        let base = f.host.ticket_base(&child).unwrap();
        assert_eq!(base.merged_parent_head(), parent.pushed.as_deref());
        let job = crate::keeper::Job {
            ticket_id: child.id.clone(),
            repository: f.repository.clone(),
            base,
            pr_base: "origin/main".into(),
            base_commit: main_tip.clone(),
            head: reviewed.clone(),
            verified: reviewed.clone(),
            checkout: f.home.path().join("keeper-checkout"),
            checks: None,
            tries: false,
        };
        let outcome = crate::keeper::rebase(&job, |_, _| Ok(true));
        let rebased = match outcome.result {
            crate::keeper::Rebased::Same(rebased) => rebased,
            crate::keeper::Rebased::Changed(_) => panic!("the rebase changed the patch"),
            crate::keeper::Rebased::Conflict(files) => panic!("the rebase conflicts in {files:?}"),
            crate::keeper::Rebased::Failed(why) => panic!("the rebase failed: {why}"),
            _ => panic!("the base moved or was deleted"),
        };
        // Only the child's own commit was replayed onto the merge.
        assert_eq!(
            git(worktree, &["rev-parse", &format!("{rebased}^")]),
            main_tip
        );
        git(worktree, &["reset", "-q", "--hard", &rebased]);
        f.host.check_verified_head(&child, &reviewed, None).unwrap();

        // The base moves on past the rebase with another unrelated commit: the review stands.
        git(repository, &["checkout", "-q", "merged"]);
        fs::write(repository.join("v.txt"), "later\n").unwrap();
        git(repository, &["add", "."]);
        git(repository, &["commit", "-q", "-m", "Later"]);
        git(
            repository,
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        git(repository, &["checkout", "-q", "main"]);
        f.host.check_verified_head(&child, &reviewed, None).unwrap();
    }
}
