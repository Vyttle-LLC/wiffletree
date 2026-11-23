//! `create_branch`: the host creates a branch on origin for the project coordinator, such as a
//! feature branch to pass as `create_ticket`'s `base`. The checks that read the store run on the
//! host thread; Git runs off it, and pushes exactly one new ref to the push URL it checked.
use crate::*;

/// A branch to create, checked against the store.
pub(crate) struct BranchCreation {
    repository_id: String,
    clone: PathBuf,
    branch: String,
    start: String,
}

impl Host {
    pub(crate) fn prepare_branch(&self, caller: &str, args: &Value) -> Result<BranchCreation> {
        let string = |key: &str| args[key].as_str().with_context(|| format!("Missing {key}"));
        let coordinator = self.session(caller)?;
        ensure!(
            coordinator.role == Role::ProjectOrchestrator,
            "Only the project coordinator creates a branch on origin"
        );
        let repository = self.repository(string("repository_id")?)?;
        self.ensure_uses(&self.project(&coordinator.project_id)?, &repository)?;
        ensure!(
            is_allowed(&repository, &self.settings().allowed_owners),
            "The human has not allowed {}'s owner to publish without asking, so Wiffletree creates no branch there; ask the human",
            repository.name
        );
        let branch = string("branch")?;
        text(branch, 255)?;
        if let Some(why) = ProtectedBranches::of(&repository).why(branch) {
            bail!("{branch} is protected: {why}. Nothing was pushed.");
        }
        let start = match args["start"].as_str() {
            Some(start) => start,
            None => origin_branch(&repository.base).with_context(|| {
                format!(
                    "{}'s base does not track origin; give start",
                    repository.name
                )
            })?,
        };
        text(start, 255)?;
        Ok(BranchCreation {
            repository_id: repository.id,
            clone: repository.path.into(),
            branch: branch.to_owned(),
            start: start.to_owned(),
        })
    }
}

impl BranchCreation {
    pub(crate) fn run(&self, git: &publish::GitTool) -> Result<Value> {
        let (branch, start) = (&self.branch, &self.start);
        let valid = runtime::git_answer(&self.clone, &["check-ref-format", "--branch", branch])?;
        ensure!(
            valid.status.success() && !branch.starts_with("refs/"),
            "\"{branch}\" is not a valid branch name. Nothing was pushed."
        );
        let push_url = match repositories::read_origin(&self.clone) {
            repositories::Origin::GitHub { push_url, .. } => push_url,
            repositories::Origin::NotOnGitHub => bail!("The repository has no GitHub origin"),
            repositories::Origin::Unusable(reason) => bail!("{reason}. Nothing was pushed."),
        };
        publish::ensure_not_rewritten(&self.clone, &push_url)?;
        let (new, from) = (
            format!("refs/heads/{branch}"),
            format!("refs/heads/{start}"),
        );
        let listed = git(&self.clone, &["ls-remote", &push_url, &new, &from])?;
        ensure!(
            listed.status.success(),
            "Origin's branches could not be read{}",
            listed.reason()
        );
        let head = |reference: &str| {
            (listed.output.lines())
                .filter_map(|line| line.split_once('\t'))
                .find(|(_, name)| *name == reference)
                .map(|(commit, _)| commit.to_owned())
        };
        ensure!(
            head(&new).is_none(),
            "{branch} already exists on origin; pass it to create_ticket as base. Nothing was pushed."
        );
        let commit = head(&from)
            .with_context(|| format!("{start} is not on origin. Nothing was pushed."))?;
        checkout::fetch_base(git, &self.clone, start)?;
        let lease = format!("--force-with-lease={new}:");
        let refspec = format!("{commit}:{new}");
        let pushed = git(
            &self.clone,
            &[
                "push",
                "--no-follow-tags",
                "--recurse-submodules=no",
                &lease,
                &push_url,
                &refspec,
            ],
        )?;
        ensure!(
            pushed.status.success(),
            "git push failed{}",
            pushed.reason()
        );
        Ok(json!({
            "repository_id": self.repository_id,
            "branch": branch,
            "start": start,
            "commit": commit,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::*;

    fn create(f: &Fixture, remote: &Remote, args: Value) -> Result<Value> {
        let mut args = args;
        args["repository_id"] = json!(f.ticket().repository_id);
        let creation = f.host.prepare_branch(&f.coordinator.id, &args)?;
        creation.run(&remote.tools.git)
    }
    fn refused(result: Result<Value>, reason: &str) {
        let error = format!("{:#}", result.expect_err("not refused"));
        assert!(error.contains(reason), "{error}");
    }

    #[test]
    fn a_coordinator_creates_a_branch_from_the_base_and_a_ticket_on_it() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        let main = remote.branch("main").unwrap();
        let created = create(&f, &remote, json!({"branch": "feature/plan"})).unwrap();
        assert_eq!(created["branch"], "feature/plan");
        assert_eq!(created["start"], "main");
        assert_eq!(created["commit"], main);
        assert_eq!(remote.branch("feature/plan"), Some(main.clone()));
        let push = remote
            .git_calls()
            .into_iter()
            .find(|c| c[0] == "push")
            .unwrap();
        assert!(push.contains(&"--force-with-lease=refs/heads/feature/plan:".to_owned()));

        // It is then a base a ticket can start from.
        let repository = f.ticket().repository_id;
        let prepared = checkout::Creation::prepare(
            &f.host,
            &f.coordinator.id,
            &repository,
            "On the plan",
            "Do",
            None,
            Some("feature/plan"),
        )
        .unwrap();
        let checkout::Prepared::Create(plan) = prepared else {
            panic!("a new ticket");
        };
        let branches = plan.inspect(&remote.tools.git).unwrap();
        let reserved = plan.reserve(&f.host, &branches).unwrap();
        reserved.run(&remote.tools.git).unwrap();
        let ticket = reserved.apply(&mut f.host).unwrap();
        assert_eq!(ticket.base.as_deref(), Some("feature/plan"));
        let worktree = Path::new(&ticket.worktree);
        assert_eq!(git(worktree, &["rev-parse", "HEAD"]), main);
    }

    #[test]
    fn a_branch_starts_where_another_branch_on_origin_is() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        let bare = remote.bare.to_str().unwrap();
        git(
            &f.worktree,
            &["push", "-q", bare, "HEAD:refs/heads/release/1"],
        );
        let release = f.head();
        let created = create(
            &f,
            &remote,
            json!({"branch": "feature/r", "start": "release/1"}),
        );
        assert_eq!(created.unwrap()["commit"], release);
        assert_eq!(remote.branch("feature/r"), Some(release));
    }

    #[test]
    fn refusals_push_nothing() {
        let mut f = fixture();
        let remote = remote(&f);
        refused(
            create(&f, &remote, json!({"branch": "feature/x"})),
            "has not allowed",
        );
        f.allow("allowed-org");
        refused(create(&f, &remote, json!({"branch": "main"})), "protected");
        refused(
            create(&f, &remote, json!({"branch": "bad..name"})),
            "not a valid",
        );
        refused(
            create(&f, &remote, json!({"branch": "refs/heads/x"})),
            "not a valid",
        );
        refused(
            create(
                &f,
                &remote,
                json!({"branch": "feature/x", "start": "nowhere"}),
            ),
            "nowhere is not on origin",
        );
        let bare = remote.bare.to_str().unwrap();
        git(&f.worktree, &["push", "-q", bare, "HEAD:refs/heads/taken"]);
        refused(
            create(&f, &remote, json!({"branch": "taken"})),
            "already exists",
        );
        assert!(remote.git_calls().iter().all(|c| c[0] != "push"));
        assert_eq!(remote.branch("feature/x"), None);
    }

    #[test]
    fn only_the_coordinator_creates_a_branch() {
        let mut f = fixture();
        f.allow("allowed-org");
        let args = json!({"repository_id": f.ticket().repository_id, "branch": "feature/x"});
        let refused = f
            .host
            .prepare_branch(&f.implementer.id, &args)
            .err()
            .unwrap();
        assert!(format!("{refused:#}").contains("Only the project coordinator"));
    }
}
