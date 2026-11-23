//! A ticket's base, the one place that decides what a ticket is measured against and where its
//! pull request opens, and stacked tickets: a ticket whose branch starts from another ticket's
//! branch in the same repository, is measured against it, and whose pull request opens into it.
use crate::*;

/// What a ticket's branch is measured against and where its pull request opens.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Base {
    /// The repository's base, such as `origin/main`, or the ticket's own branch of origin.
    Repository(String),
    Stacked {
        /// Where the parent's pull request landed, once it merged; until then the parent's base.
        repository: String,
        /// The parent ticket's branch.
        branch: String,
        parent: String,
        /// The commit the host last pushed to the parent's branch on GitHub.
        pushed: Option<String>,
        /// The parent's pull request has merged.
        merged: bool,
        /// The merged branches of other tickets the parent's changes passed through on their
        /// way to `repository`, such as the grandparent's branch once it merged too.
        #[serde(default)]
        merged_through: Vec<String>,
    },
}
impl Base {
    /// `parent` is the ticket's `stacked_on` ticket, which a stacked ticket must have. A merged
    /// parent's children follow where its pull request landed: the branch it merged into, and
    /// while that is another ticket's branch whose pull request merged too, where that one
    /// merged, up the stack. `tickets` are the tickets that branch may belong to.
    pub(crate) fn of(
        ticket: &Ticket,
        repository: &Repository,
        parent: Option<&Ticket>,
        tickets: &[Ticket],
    ) -> Self {
        match parent.filter(|p| ticket.stacked_on.as_ref() == Some(&p.id)) {
            Some(parent) => {
                let landed = landed(parent, tickets);
                let (repository, merged_through) = match landed {
                    Some((branch, through)) => (format!("origin/{branch}"), through),
                    None => (effective_base(parent, repository), vec![]),
                };
                Self::Stacked {
                    merged: parent_merged(parent),
                    repository,
                    branch: parent.branch.clone(),
                    parent: parent.title.clone(),
                    pushed: parent.pushed.clone(),
                    merged_through,
                }
            }
            None => Self::Repository(effective_base(ticket, repository)),
        }
    }
    /// Whether an open pull request into `branch` is where this base puts it: the branch it
    /// would open into, or a merged branch on the way there, such as a stacked ticket's
    /// parent's, which GitHub may keep after the parent merged until the next publish moves it.
    fn holds(&self, branch: &str) -> bool {
        let on_the_way = match self {
            Self::Stacked {
                branch: parent,
                merged_through,
                ..
            } => parent == branch || merged_through.iter().any(|b| b == branch),
            Self::Repository(_) => false,
        };
        on_the_way || self.pull_request_base().is_ok_and(|base| base == branch)
    }
    /// The ref a ticket's open branch is compared with for overlap warnings: where its pull
    /// request lands, a stacked ticket's parent's base until the parent merged.
    pub(crate) fn lands_on(&self) -> &str {
        match self {
            Self::Repository(base)
            | Self::Stacked {
                repository: base, ..
            } => base,
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
    pub(crate) fn built_on(&self, worktree: &Path, commit: &str) -> String {
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

fn parent_merged(parent: &Ticket) -> bool {
    (parent.pull_request.as_ref()).is_some_and(|pr| pr.state == PrState::Merged)
}

/// Where a merged parent's pull request landed, followed up the stack through each branch of
/// another ticket in its repository whose pull request merged too, and the branches it passed.
fn landed(parent: &Ticket, tickets: &[Ticket]) -> Option<(String, Vec<String>)> {
    fn merged(t: &Ticket) -> Option<&PullRequest> {
        t.pull_request.as_ref().filter(|_| parent_merged(t))
    }
    let mut branch = merged(parent)?.base.clone();
    let mut through: Vec<String> = vec![];
    while let Some(pr) = (tickets.iter())
        .filter(|t| t.repository_id == parent.repository_id && t.branch == branch)
        .find_map(merged)
    {
        // A cycle of merged branches cannot happen on GitHub, but must not loop here.
        if through.contains(&branch) {
            break;
        }
        through.push(std::mem::replace(&mut branch, pr.base.clone()));
    }
    Some((branch, through))
}

/// Puts the ticket on `branch` of origin and unstacks it. The repository's own base branch is
/// recorded as no base of the ticket's own.
pub(crate) fn move_base(ticket: &mut Ticket, repository: &Repository, branch: &str) {
    ticket.stacked_on = None;
    ticket.base = (origin_branch(&repository.base) != Some(branch)).then(|| branch.to_owned());
}

/// Follows a retarget of the ticket's open pull request on GitHub: when it opens into a branch
/// its base does not put it in, that branch becomes the ticket's base. A stacked ticket whose
/// pull request left its parent's branch is unstacked, because it no longer builds on the
/// parent. Returns whether the ticket changed.
pub(crate) fn follow_pull_request(
    ticket: &mut Ticket,
    base: &Base,
    repository: &Repository,
) -> bool {
    let Some(pr) = ticket.open_pull_request() else {
        return false;
    };
    // A repository whose base is not on origin publishes nothing, so has no base to follow.
    if base.pull_request_base().is_err() || base.holds(&pr.base) {
        return false;
    }
    let branch = pr.base.clone();
    move_base(ticket, repository, &branch);
    true
}

impl Host {
    pub(crate) fn ticket_base(&self, ticket: &Ticket) -> Result<Base> {
        let repository = self.ticket_repository(ticket)?;
        let parent = match &ticket.stacked_on {
            Some(id) => Some(self.ticket(id).context("The parent ticket was not found")?),
            None => None,
        };
        // Only a merged parent's landing reads the other tickets.
        let tickets = match parent.as_ref().is_some_and(parent_merged) {
            true => self.tickets()?,
            false => vec![],
        };
        Ok(Base::of(ticket, &repository, parent.as_ref(), &tickets))
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
        let base = |parent: &Ticket| Base::of(&child, &repository, Some(parent), &[]);
        assert!(base(&parent).pull_request_base().is_err());
        parent.pull_request = Some(merged);
        assert_eq!(base(&parent).pull_request_base().unwrap(), "main");
        child.stacked_on = None;
        assert_eq!(
            Base::of(&child, &repository, None, &[])
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
            Base::of(&child, &repository, Some(&ticket), &[])
                .pull_request_base()
                .unwrap(),
            ticket.branch
        );
        let mut merged = crate::publish::tests::watch_open_pr(&mut f, 1);
        merged.state = PrState::Merged;
        merged.base = "release".into();
        ticket.pull_request = Some(merged);
        child.stacked_on = Some(ticket.id.clone());
        assert_eq!(
            Base::of(&child, &repository, Some(&ticket), &[])
                .pull_request_base()
                .unwrap(),
            "release"
        );
    }

    /// Records `pr` for the ticket as a PR watcher pass would.
    fn watched(f: &mut Fixture, ticket: &Ticket, pr: PullRequest) {
        let pass = crate::pr_watch::tests::found(ticket, pr);
        (f.host)
            .record_pull_requests(&ticket.repository_id, &pass, 0, 0)
            .unwrap();
    }
    /// The PR watcher's messages to the coordinator about tickets' bases.
    fn base_notices(f: &Fixture) -> Vec<String> {
        (f.host.db)
            .prepare("SELECT body FROM messages WHERE id LIKE 'pr:%:base:%' ORDER BY sequence")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_retargeted_pr_moves_the_tickets_base_for_reviews_overlaps_and_publishes() {
        let mut f = fixture();
        f.allow("allowed-org");
        let (_, _, mut ticket) = on_release(&mut f);
        fs::write(Path::new(&ticket.worktree).join("t.txt"), "ticket\n").unwrap();
        git(Path::new(&ticket.worktree), &["add", "."]);
        git(Path::new(&ticket.worktree), &["commit", "-q", "-m", "t"]);
        let head = git(Path::new(&ticket.worktree), &["rev-parse", "HEAD"]);
        // The watcher stored the PR's reading into main before bases followed it, as release
        // was merged into main and the human retargeted the PR.
        let mut pr = crate::publish::tests::watch_open_pr(&mut f, 688);
        pr.base = "main".into();
        ticket.pull_request = Some(pr.clone());
        ticket.verification = f.ticket().verification;
        ticket.verification.as_mut().unwrap().rounds[0].commit = head.clone();
        f.host.save_ticket(&ticket).unwrap();
        assert_eq!(
            f.host.ticket_base(&ticket).unwrap().measured(),
            "origin/release"
        );

        watched(&mut f, &ticket, pr.clone());
        let ticket = f.host.ticket(&ticket.id).unwrap();
        assert_eq!(
            ticket.base, None,
            "the repository's own base is no base of its own"
        );
        assert_eq!(
            f.host.ticket_base(&ticket).unwrap().measured(),
            "origin/main"
        );
        let lines = f.host.diff_lines(&ticket, &head);
        assert!(lines.contains("(origin/main)"), "{lines}");
        let checks = f.host.overlap_checks().unwrap();
        assert!(checks.iter().all(|(_, c)| c.base == "origin/main"));
        let coordinator = f.coordinator.id.clone();
        let (_, job) = (f.host)
            .publish_ticket(&coordinator, &ticket.id, None, None, None)
            .unwrap();
        let request = job.unwrap().request;
        assert_eq!((request.base.as_str(), request.retargets), ("main", false));
        let notices = base_notices(&f);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("PR #688") && notices[0].contains("now opens into main"));
        // The next pass changes nothing and tells nobody again.
        watched(&mut f, &ticket, pr);
        assert_eq!(base_notices(&f).len(), 1);
    }

    #[test]
    fn a_stacked_pr_moved_off_its_parents_branch_unstacks_the_ticket() {
        let mut f = fixture();
        let mut parent = f.ticket();
        parent.pushed = Some(f.head());
        f.host.save_ticket(&parent).unwrap();
        let child = child(&mut f);
        let mut pr = crate::publish::tests::watch_open_pr(&mut f, 2);
        pr.base = parent.branch.clone();
        // Into its parent's branch, it stays stacked.
        watched(&mut f, &child, pr.clone());
        let stacked = f.host.ticket(&child.id).unwrap();
        assert_eq!(stacked.stacked_on.as_ref(), Some(&parent.id));
        assert!(base_notices(&f).is_empty());

        pr.base = "main".into();
        watched(&mut f, &stacked, pr);
        let moved = f.host.ticket(&child.id).unwrap();
        assert_eq!((&moved.stacked_on, &moved.base), (&None, &None));
        assert_eq!(
            f.host.ticket_base(&moved).unwrap().measured(),
            "origin/main"
        );
        let notices = base_notices(&f);
        assert!(notices[0].contains("no longer stacked"), "{notices:?}");
    }

    #[test]
    fn down_a_stack_each_child_follows_the_branch_its_parent_merged_into() {
        let mut f = fixture();
        let r = remote(&f);
        f.allow("allowed-org");
        // Grandparent, the fixture's ticket on main; parent stacked on it; child on the parent.
        let mut grandparent = f.ticket();
        grandparent.pushed = Some(f.head());
        f.host.save_ticket(&grandparent).unwrap();
        let mut parent = child(&mut f);
        parent.pushed = Some(f.head());
        let mut merged = crate::publish::tests::watch_open_pr(&mut f, 727);
        merged.state = PrState::Merged;
        merged.base = grandparent.branch.clone();
        parent.pull_request = Some(merged);
        f.host.save_ticket(&parent).unwrap();
        let repository = parent.repository_id.clone();
        let coordinator = f.coordinator.id.clone();
        let mut child = (f.host)
            .create_stacked_ticket(
                &coordinator,
                &repository,
                "Grandchild",
                "Do",
                Some(&parent.id),
            )
            .unwrap();
        let worktree = Path::new(&child.worktree);
        fs::write(worktree.join("c.txt"), "three\n").unwrap();
        git(worktree, &["add", "."]);
        git(worktree, &["commit", "-q", "-m", "c"]);
        child.verification = f.ticket().verification;
        child.verification.as_mut().unwrap().rounds[0].commit =
            git(worktree, &["rev-parse", "HEAD"]);
        // Its PR still opens into the merged parent's branch, which GitHub kept.
        let mut open = crate::publish::tests::watch_open_pr(&mut f, 731);
        open.base = parent.branch.clone();
        child.pull_request = Some(open.clone());
        f.host.save_ticket(&child).unwrap();

        let base = f.host.ticket_base(&child).unwrap();
        assert_eq!(base.pull_request_base().unwrap(), grandparent.branch);
        assert_eq!(
            base.measured(),
            parent.branch,
            "reviews still see only its own changes"
        );
        // The watcher leaves it stacked, and its next publish moves the PR.
        watched(&mut f, &child, open);
        assert_eq!(
            f.host.ticket(&child.id).unwrap().stacked_on.as_ref(),
            Some(&parent.id)
        );
        let (_, job) = (f.host)
            .publish_ticket(&coordinator, &child.id, None, None, None)
            .unwrap();
        crate::publish::tests::drive(&mut f, &r, job);
        let edited = r.github.lock().unwrap().edited.clone();
        assert_eq!(edited.len(), 1);
        assert_eq!(edited[0][..3], ["pr", "edit", "731"]);
        assert_eq!(edited[0][5..], ["--base", grandparent.branch.as_str()]);
        let child = f.host.ticket(&child.id).unwrap();
        assert_eq!(
            child.pull_request.as_ref().unwrap().base,
            grandparent.branch
        );
        // Overlap warnings compare it where it lands, not on the parent's creation base.
        let compared_on = |f: &Fixture| {
            let checks = f.host.overlap_checks().unwrap();
            (checks.into_iter())
                .find(|(_, c)| c.branches.iter().any(|b| b.ticket_id == child.id))
                .unwrap()
                .1
                .base
        };
        assert_eq!(compared_on(&f), format!("origin/{}", grandparent.branch));

        // The grandparent merges into main too: the child lands on main, the PR still into the
        // grandparent's merged branch keeps it stacked, and its next publish moves the PR on.
        let mut merged = crate::publish::tests::watch_open_pr(&mut f, 726);
        merged.state = PrState::Merged;
        grandparent.pull_request = Some(merged);
        f.host.save_ticket(&grandparent).unwrap();
        let base = f.host.ticket_base(&child).unwrap();
        assert_eq!(base.pull_request_base().unwrap(), "main");
        assert_eq!(compared_on(&f), "origin/main");
        watched(&mut f, &child, child.pull_request.clone().unwrap());
        assert_eq!(
            f.host.ticket(&child.id).unwrap().stacked_on,
            Some(parent.id)
        );
        let (_, job) = (f.host)
            .publish_ticket(&coordinator, &child.id, None, None, None)
            .unwrap();
        crate::publish::tests::drive(&mut f, &r, job);
        let edited = r.github.lock().unwrap().edited.clone();
        assert_eq!(edited[1][5..], ["--base", "main"]);
    }

    #[test]
    fn a_finished_tickets_open_pr_moved_on_github_moves_its_base_and_tells_nobody() {
        let mut f = fixture();
        let (_, _, mut ticket) = on_release(&mut f);
        let mut pr = crate::publish::tests::watch_open_pr(&mut f, 688);
        pr.base = "release".into();
        ticket.pull_request = Some(pr.clone());
        ticket.state = TicketState::Accepted;
        f.host.save_ticket(&ticket).unwrap();
        pr.base = "main".into();
        watched(&mut f, &ticket, pr);
        let ticket = f.host.ticket(&ticket.id).unwrap();
        assert_eq!(ticket.base, None);
        assert!(base_notices(&f).is_empty());
        // A ticket stacked on it later starts from where its PR now lands.
        let coordinator = f.coordinator.id.clone();
        let repository = ticket.repository_id.clone();
        let child = (f.host)
            .create_stacked_ticket(&coordinator, &repository, "Later", "Do", Some(&ticket.id))
            .unwrap();
        assert_eq!(child.base, None);
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
        // Its review is measured from main, where it now builds, no longer from the parent.
        let lines = f.host.diff_lines(&child, &rebased);
        assert!(
            lines.starts_with(&format!("Base: {main_tip} (origin/main)")),
            "{lines}"
        );
        assert!(lines.ends_with("1 file changed, 1 insertion(+)"), "{lines}");

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
