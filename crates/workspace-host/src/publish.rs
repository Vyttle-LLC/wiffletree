//! Publishing a ticket: the host itself pushes the ticket's branch and opens its pull request,
//! after checking the review, the branch and the repository's owner, and asks the human in the
//! Inbox wherever the rules require. Local checks run on the host thread; the network steps run
//! off it. Nothing here approves, merges, closes or comments on a pull request.
use crate::*;

/// GitHub's limit for a pull request's title.
const TITLE_CHARS: usize = 256;

/// One step of a request, with everything it needs off the host thread.
#[derive(Debug)]
pub(crate) struct Job {
    pub ticket_id: String,
    pub request: PublishRequest,
    pub worktree: PathBuf,
    /// The repository as stored, to confirm with GitHub before anything is sent.
    pub repository: Repository,
    /// The commit the host last pushed to the ticket's branch.
    pub pushed: Option<String>,
}

/// What a publish of the ticket would send now, read from its worktree and its repository.
struct Checked {
    repository: Repository,
    /// GitHub's `owner/name`, or the URL's until GitHub has answered.
    name: String,
    push_url: String,
    branch: String,
    base: String,
    commit: String,
}

/// The commit's size against what GitHub already has from the host, or else against the base:
/// such as "2 commits · 3 files changed, 41 insertions(+), 7 deletions(-)".
fn commit_size(worktree: &Path, base: &str, pushed: Option<&str>, commit: &str) -> Result<String> {
    let is_ancestor = |older: &str| {
        runtime::git_answer(worktree, &["merge-base", "--is-ancestor", older, commit])
            .is_ok_and(|answer| answer.status.success())
    };
    let from = match pushed.filter(|pushed| is_ancestor(pushed)) {
        Some(pushed) => pushed.to_owned(),
        None => worktrees::merge_base(worktree, base, commit)?,
    };
    let count = runtime::git_output(
        worktree,
        &["rev-list", "--count", &format!("{from}..{commit}")],
    )?;
    let commits = match count.trim() {
        "1" => "1 commit".to_owned(),
        count => format!("{count} commits"),
    };
    Ok(format!(
        "{commits} · {}",
        worktrees::shortstat(worktree, &from, commit)?
    ))
}

/// Git rewrites a URL given on the command line through `url.*.insteadOf` and
/// `url.*.pushInsteadOf`, as it already did for the URL `origin` reports. A URL some rule still
/// matches would be rewritten once more, to a destination nobody checked.
fn ensure_not_rewritten(repository: &Path, push_url: &str) -> Result<()> {
    let rules = runtime::git_answer(
        repository,
        &[
            "config",
            "-z",
            "--get-regexp",
            r"^url\..*\.(push)?insteadof$",
        ],
    )?;
    // Each entry is its key, a newline and its value.
    let rewritten = rules
        .output
        .split('\0')
        .filter_map(|entry| entry.split_once('\n'))
        .any(|(_, prefix)| push_url.starts_with(prefix));
    ensure!(
        !rewritten,
        "Git's url.*.insteadOf settings would rewrite origin's push URL {push_url} again; nothing was pushed"
    );
    Ok(())
}

impl Host {
    /// Publishes a ticket for its coordinator or implementer: checks it, freezes what will be
    /// sent, and either returns the first step to run or asks the human. Returns the tool's
    /// result and the step to run now, if any.
    pub(crate) fn publish_ticket(
        &mut self,
        caller: &str,
        ticket_id: &str,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<(Value, Option<Job>)> {
        let mut ticket = self.ticket(ticket_id)?;
        let session = self.session(caller)?;
        let implements = session.role == Role::Implementer
            && self.session_runtime(caller)?.ticket_id.as_deref() == Some(ticket_id);
        ensure!(
            implements
                || (session.role == Role::ProjectOrchestrator && ticket.coordinator_id == caller),
            "Only the ticket's coordinator or its implementer publishes it"
        );
        ensure!(
            !ticket
                .publishing
                .as_ref()
                .is_some_and(|r| r.waiting.is_none()),
            "A publish step for this ticket is running; its result will arrive as a message"
        );
        match (
            ticket
                .pull_request
                .as_ref()
                .filter(|pr| pr.state == PrState::Open),
            title,
            body,
        ) {
            (None, Some(title), Some(body)) => {
                ensure!(
                    !title.trim().is_empty()
                        && title.chars().count() <= TITLE_CHARS
                        && !title.contains('\n'),
                    "Give the pull request a one-line title of at most {TITLE_CHARS} characters"
                );
                text(body, MAX_TEXT_BYTES)?;
            }
            (None, ..) => bail!(
                "This ticket has no open pull request, so publishing it opens one: give its title and body"
            ),
            (Some(_), None, None) => {}
            (Some(open), ..) => bail!(
                "PR #{} is open: publish_ticket only pushes to it and cannot edit its title or description. Call it without title and body",
                open.number
            ),
        }
        let checked = self.check(&ticket)?;
        let worktree = Path::new(&ticket.worktree);
        let size = commit_size(
            worktree,
            &checked.repository.base,
            ticket.pushed.as_deref(),
            &checked.commit,
        )?;
        ticket.publishing = Some(PublishRequest {
            id: new_id(),
            caller: caller.into(),
            repository: checked.name,
            push_url: checked.push_url,
            branch: checked.branch,
            base: checked.base,
            commit: checked.commit,
            size,
            title: title.map(Into::into),
            body: body.map(Into::into),
            step: PublishStep::Push,
            waiting: None,
            blocked_review_approved: false,
        });
        self.save_ticket(&ticket)?;
        let job = self.start_step(&mut ticket, &checked.repository)?;
        Ok((self.publish_status(&ticket, &checked.repository), job))
    }

    /// The checks no answer from the human overrides: the ticket is open and reviewed with no
    /// cycle running, the worktree is on the ticket's branch at the reviewed commit or a
    /// patch-equivalent one, and `origin` is one checked place on GitHub that the base tracks.
    fn check(&self, ticket: &Ticket) -> Result<Checked> {
        ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
        if let Some(cycle) = ticket.running_cycle() {
            bail!(
                "Verification cycle {} is still running. Publish the ticket once it ends.",
                cycle.cycle
            );
        }
        let verified = ticket
            .verification
            .as_ref()
            .and_then(|v| v.verified_commit())
            .context("This ticket has not been reviewed. Verify it before publishing it.")?;
        self.check_verified_head(ticket, verified)?;
        let worktree = Path::new(&ticket.worktree);
        let on = runtime::git_answer(worktree, &["symbolic-ref", "--short", "HEAD"])?;
        ensure!(
            on.status.success() && on.output.trim() == ticket.branch,
            "The worktree is on {}, not the ticket's branch {}. Nothing was pushed.",
            match on.output.trim() {
                "" => "a detached HEAD",
                branch => branch,
            },
            ticket.branch
        );
        let repository = self.ticket_repository(ticket)?;
        let (owner, name, push_url) = match repositories::read_origin(Path::new(&repository.path)) {
            repositories::Origin::GitHub {
                owner,
                name,
                push_url,
            } => (owner, name, push_url),
            repositories::Origin::NotOnGitHub => bail!(
                "{} has no GitHub origin, so there is nowhere to push",
                repository.name
            ),
            repositories::Origin::Unusable(reason) => {
                bail!("{}: {reason}. Nothing was pushed.", repository.name)
            }
        };
        let recorded = repository
            .github
            .as_ref()
            .filter(|github| github.owner == owner && github.name == name)
            .with_context(|| {
                format!(
                    "origin now names {owner}/{name}, which Wiffletree has not checked with GitHub yet. Call publish_ticket again in a moment."
                )
            })?;
        ensure_not_rewritten(Path::new(&repository.path), &push_url)?;
        let base = origin_branch(&repository.base).with_context(|| {
            format!(
                "The repository's base {} is not on origin, so the ticket's pull request would cross repositories",
                repository.base
            )
        })?;
        Ok(Checked {
            name: match &recorded.facts {
                Some(facts) => facts.name_with_owner.clone(),
                None => format!("{owner}/{name}"),
            },
            push_url,
            branch: ticket.branch.clone(),
            base: base.to_owned(),
            commit: verification::head(ticket)?,
            repository,
        })
    }

    /// Returns the request's next step to run, or raises its Inbox item and returns nothing when
    /// the human must approve it first.
    fn start_step(&mut self, ticket: &mut Ticket, repository: &Repository) -> Result<Option<Job>> {
        let request = ticket.publishing.clone().context("No publish request")?;
        let allowed = self.settings().allowed_owners;
        let Some(ask) = publish_ask(ticket, &request, repository, &allowed) else {
            return Ok(Some(Job {
                ticket_id: ticket.id.clone(),
                request,
                worktree: ticket.worktree.clone().into(),
                repository: repository.clone(),
                pushed: ticket.pushed.clone(),
            }));
        };
        let step = match request.step {
            PublishStep::Push => "push",
            PublishStep::OpenPullRequest => "pull_request",
        };
        let item = self.request_attention(
            &request.caller,
            "local",
            &format!("{PUBLISH_PREFIX}{}:{step}", request.id),
            &approval_prompt(ticket, &request, repository, ask),
            &[],
        )?;
        ticket.publishing = Some(PublishRequest {
            waiting: Some(item.id),
            ..request
        });
        self.save_ticket(ticket)?;
        Ok(None)
    }

    /// The tool's result for the ticket's request: the steps the host is running, or the step
    /// that waits for the human and why.
    fn publish_status(&self, ticket: &Ticket, repository: &Repository) -> Value {
        let Some(request) = &ticket.publishing else {
            return json!({"status": "ended"});
        };
        if request.waiting.is_none() {
            let steps: &[&str] = match request.opens_pull_request() {
                true => &["push", "pull request"],
                false => &["push"],
            };
            return json!({"status": "publishing", "steps": steps,
                "repository": request.repository, "branch": request.branch,
                "commit": request.commit,
                "next": "Finish your turn. Wiffletree messages you the result."});
        }
        let allowed = self.settings().allowed_owners;
        let reason = match publish_ask(ticket, request, repository, &allowed) {
            Some(PublishAsk::BlockedReview) => "the review ended blocked".to_owned(),
            Some(PublishAsk::EndedPullRequest) => ended_pull_request_notice(ticket),
            _ => not_allowed_reason(repository),
        };
        json!({"status": "waiting_for_human", "step": request.step.label(),
            "repository": request.repository, "reason": reason,
            "next": "Finish your turn. Wiffletree runs each step the human approves and messages you the result."})
    }
}

/// Such as "PR #45 was closed without merging".
fn ended_pull_request_notice(ticket: &Ticket) -> String {
    match ticket.ended_pull_request() {
        Some(pr) if pr.state == PrState::Merged => format!("PR #{} was merged", pr.number),
        Some(pr) => format!("PR #{} was closed without merging", pr.number),
        None => String::new(),
    }
}

/// The Inbox item's text, written by the host from the frozen request. The agent's only words
/// in it are the pull request's title and description; the card shows both in full.
fn approval_prompt(
    ticket: &Ticket,
    request: &PublishRequest,
    repository: &Repository,
    ask: PublishAsk,
) -> String {
    let commit = short_sha(&request.commit);
    let facts = repository.github.as_ref().and_then(|g| g.facts.as_ref());
    let visibility = match facts {
        Some(facts) if facts.public => "public",
        Some(_) => "private",
        None => "visibility unknown",
    };
    let rule = match ask {
        PublishAsk::NotAllowed => not_allowed_reason(repository),
        _ => "its owner is allowed".to_owned(),
    };
    let would_send = match (request.step, &request.title) {
        (PublishStep::Push, Some(title)) if ask == PublishAsk::BlockedReview => format!(
            "Push {} at {commit} and open its pull request, \"{title}\"",
            request.branch
        ),
        _ => format!("Push {} at {commit}", request.branch),
    };
    let mut lines = vec![match (ask, request.step) {
        (PublishAsk::BlockedReview, _) => {
            "Publish this ticket although its review ended blocked?".to_owned()
        }
        (_, PublishStep::Push) => format!("Push this branch to {}?", request.repository),
        (_, PublishStep::OpenPullRequest) => {
            format!("Open this pull request in {}?", request.repository)
        }
    }];
    lines.push(format!(
        "Repository: {} · {visibility} · {rule}",
        request.repository
    ));
    match request.step {
        PublishStep::Push => {
            lines.push(format!("Branch: {}", request.branch));
            lines.push(format!("Commit: {commit} · {}", request.size));
        }
        PublishStep::OpenPullRequest => {
            lines.push(format!("Branches: {} → {}", request.branch, request.base));
            lines.push(format!(
                "Title: {}",
                request.title.as_deref().unwrap_or_default()
            ));
        }
    }
    if let Some(verification) = &ticket.verification {
        lines.push(format!(
            "Review: {}",
            verification.review_line(&request.commit)
        ));
        if verification.outcome == VerificationOutcome::Blocked {
            let unchecked = verification.unchecked();
            if !unchecked.is_empty() {
                lines.push(format!("Did not check: {}", unchecked.join(", ")));
            }
            lines.push("Open findings:".into());
            let open = ticket.open_findings();
            if open.is_empty() {
                lines.push("None".into());
            }
            for entry in open {
                lines.push(format!("{} · {}", entry.heading(), entry.finding.summary));
            }
        }
    }
    if ticket.ended_pull_request().is_some() {
        lines.push(format!(
            "Earlier pull request: {}",
            ended_pull_request_notice(ticket)
        ));
    }
    if ask == PublishAsk::BlockedReview {
        lines.push(format!("Would send: {would_send}"));
    }
    // The Inbox bounds an item's text; the card draws the full request.
    let mut prompt = lines.join("\n");
    if prompt.len() > 4000 {
        let end = (0..=4000).rev().find(|&i| prompt.is_char_boundary(i));
        prompt.truncate(end.unwrap_or_default());
        prompt.push('…');
    }
    prompt
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const ORIGIN: &str = "git@github.com:allowed-org/app.git";

    pub(crate) fn git(path: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(["-c", "user.name=F", "-c", "user.email=f@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    /// A reviewed ticket in a repository whose `origin` names `allowed-org/app` on GitHub, with
    /// its coordinator and implementer. Nothing is allowed yet.
    pub(crate) struct Fixture {
        pub home: tempfile::TempDir,
        pub host: Host,
        pub ticket: String,
        pub coordinator: Session,
        pub implementer: Session,
        pub repository: PathBuf,
        pub worktree: PathBuf,
    }
    impl Fixture {
        pub fn ticket(&self) -> Ticket {
            self.host.ticket(&self.ticket).unwrap()
        }
        pub fn head(&self) -> String {
            git(&self.worktree, &["rev-parse", "HEAD"])
        }
        pub fn commit(&self, file: &str, content: &str) -> String {
            fs::write(self.worktree.join(file), content).unwrap();
            git(&self.worktree, &["add", "."]);
            git(&self.worktree, &["commit", "-q", "-m", file]);
            self.head()
        }
        /// Ends a new verification cycle at the worktree's HEAD.
        pub fn review(&mut self, outcome: VerificationOutcome) {
            let mut ticket = self.ticket();
            let result = match outcome {
                VerificationOutcome::Passed => VerifierResult::Passed,
                VerificationOutcome::Blocked => VerifierResult::Failed,
                VerificationOutcome::Running => VerifierResult::Pending,
            };
            let cycle = ticket.verification.as_ref().map_or(1, |v| v.cycle + 1);
            ticket.verification = Some(Verification {
                cycle,
                max_rounds: 3,
                outcome,
                rounds: vec![VerificationRound {
                    round: 1,
                    commit: self.head(),
                    verifiers: vec![VerifierRun {
                        session_id: "verifier".into(),
                        role: Role::Reviewer,
                        focus: "Correctness".into(),
                        message_id: "m".into(),
                        result,
                        report_id: None,
                        reason: None,
                    }],
                }],
            });
            self.host.save_ticket(&ticket).unwrap();
        }
        /// Records GitHub's answer for the repository.
        pub fn github(&mut self, name_with_owner: &str, fork: bool) {
            let repository = self.host.ticket_repository(&self.ticket()).unwrap();
            let read = repositories::GitHubRead {
                id: repository.id,
                github: Some(GitHubRepository {
                    owner: "allowed-org".into(),
                    name: "app".into(),
                    facts: Some(GitHubFacts {
                        name_with_owner: name_with_owner.into(),
                        organization: true,
                        fork,
                        parent: fork.then(|| "upstream/app".into()),
                        public: false,
                        checked_at: now(),
                    }),
                    problem: None,
                }),
                origin_problem: None,
            };
            self.host
                .record_github(repositories::GitHubReading {
                    repositories: vec![read],
                    error: None,
                })
                .unwrap();
        }
        pub fn allow(&mut self, entry: &str) {
            self.host.set_owner_allowed(entry, true).unwrap();
        }
        pub fn publish(&mut self, caller: &str) -> Result<(Value, Option<Job>)> {
            self.host
                .publish_ticket(caller, &self.ticket.clone(), Some("Fix it"), Some("Why"))
        }
        pub fn open_items(&self) -> Vec<Attention> {
            self.host
                .open_attention(&self.coordinator.project_id)
                .unwrap()
        }
    }

    pub(crate) fn fixture() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let repository = home.path().join("app");
        fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-q", "-b", "main"]);
        git(
            &repository,
            &["commit", "-q", "--allow-empty", "-m", "start"],
        );
        git(&repository, &["remote", "add", "origin", ORIGIN]);
        git(
            &repository,
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        let mut host = Host::open_set_up(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Site").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let attached = host
            .attach_repository(&project.id, repository.to_str().unwrap(), "origin/main")
            .unwrap();
        let ticket = host
            .create_ticket(&coordinator.id, &attached.id, "Fix contrast", "Do")
            .unwrap();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
            .unwrap();
        let mut fixture = Fixture {
            worktree: ticket.worktree.clone().into(),
            repository: fs::canonicalize(&repository).unwrap(),
            ticket: ticket.id,
            host,
            coordinator,
            implementer,
            home,
        };
        fixture.github("allowed-org/app", false);
        fixture.commit("a.txt", "one\n");
        fixture.commit("b.txt", "two\n");
        fixture.review(VerificationOutcome::Passed);
        fixture
    }

    /// The call is refused for `reason`, and nothing is left behind that could be sent.
    fn refused(fixture: &mut Fixture, result: Result<(Value, Option<Job>)>, reason: &str) {
        let error = format!("{:#}", result.err().expect("publish was not refused"));
        assert!(error.contains(reason), "{error}");
        assert!(fixture.ticket().publishing.is_none());
        assert!(fixture.open_items().is_empty());
    }

    #[test]
    fn only_the_tickets_coordinator_or_implementer_may_publish() {
        let mut f = fixture();
        f.allow("allowed-org");
        let ticket = f.ticket.clone();
        let reviewer = f
            .host
            .assign_ticket(&ticket, Role::Reviewer, Provider::Claude, "Review", None)
            .unwrap();
        let tester = f
            .host
            .assign_ticket(&ticket, Role::Tester, Provider::Claude, "Test", None)
            .unwrap();
        let repository = f.ticket().repository_id;
        let other = f
            .host
            .create_ticket(&f.coordinator.id.clone(), &repository, "Other", "Do")
            .unwrap();
        let outsider = f
            .host
            .assign_ticket(&other.id, Role::Implementer, Provider::Claude, "Do", None)
            .unwrap();
        for verifier in [reviewer, tester, outsider] {
            let result = f.publish(&verifier.id);
            refused(
                &mut f,
                result,
                "Only the ticket's coordinator or its implementer",
            );
        }
        // The implementer's call is accepted, and the request names it for the result.
        let (status, job) = f.publish(&f.implementer.id.clone()).unwrap();
        assert_eq!(status["status"], "publishing");
        assert_eq!(job.unwrap().request.caller, f.implementer.id);
    }

    #[test]
    fn a_first_publish_needs_a_title_and_description_and_takes_nothing_else() {
        let mut f = fixture();
        f.allow("allowed-org");
        let (ticket, caller) = (f.ticket.clone(), f.coordinator.id.clone());
        for (title, body) in [(None, None), (None, Some("Why")), (Some("Fix"), None)] {
            let result = f.host.publish_ticket(&caller, &ticket, title, body);
            refused(&mut f, result, "give its title and body");
        }
        let result = f
            .host
            .publish_ticket(&caller, &ticket, Some("Two\nlines"), Some("Why"));
        refused(&mut f, result, "one-line title");
    }

    #[test]
    fn an_unreviewed_ticket_or_a_running_review_is_refused() {
        let mut f = fixture();
        f.allow("allowed-org");
        let caller = f.coordinator.id.clone();
        let mut ticket = f.ticket();
        ticket.verification = None;
        f.host.save_ticket(&ticket).unwrap();
        let result = f.publish(&caller);
        refused(&mut f, result, "This ticket has not been reviewed");
        f.review(VerificationOutcome::Running);
        let result = f.publish(&caller);
        refused(
            &mut f,
            result,
            "Verification cycle 1 is still running. Publish the ticket once it ends.",
        );
    }

    #[test]
    fn head_past_the_reviewed_commit_is_refused_and_a_squash_of_it_goes_ahead() {
        let mut f = fixture();
        f.allow("allowed-org");
        let caller = f.coordinator.id.clone();
        let reviewed = f.head();
        // A squash of the reviewed commits has the same changes.
        git(&f.worktree, &["reset", "-q", "--soft", "origin/main"]);
        git(&f.worktree, &["commit", "-q", "-m", "squashed"]);
        let squashed = f.head();
        assert_ne!(squashed, reviewed);
        let (_, job) = f.publish(&caller).unwrap();
        assert_eq!(job.unwrap().request.commit, squashed);
        // A change the review never saw is not.
        let mut ticket = f.ticket();
        ticket.publishing = None;
        f.host.save_ticket(&ticket).unwrap();
        let moved = f.commit("c.txt", "three\n");
        let result = f.publish(&caller);
        refused(
            &mut f,
            result,
            &format!("verified {reviewed}, current {moved}"),
        );
    }

    #[test]
    fn a_worktree_on_another_branch_is_refused() {
        let mut f = fixture();
        f.allow("allowed-org");
        git(&f.worktree, &["checkout", "-q", "-b", "elsewhere"]);
        let result = f.publish(&f.coordinator.id.clone());
        refused(
            &mut f,
            result,
            "The worktree is on elsewhere, not the ticket's branch wiffletree/fix-contrast. Nothing was pushed.",
        );
    }

    #[test]
    fn an_origin_that_is_not_one_checked_place_on_github_is_refused() {
        let publish = |change: &dyn Fn(&mut Fixture)| {
            let mut f = fixture();
            f.allow("allowed-org");
            change(&mut f);
            let caller = f.coordinator.id.clone();
            let result = f.publish(&caller);
            (f, result)
        };
        let set_url = |f: &mut Fixture, args: &[&str]| {
            git(&f.repository, &[&["remote", "set-url"], args].concat());
        };
        let (mut f, result) = publish(&|f| set_url(f, &["origin", "git@gitlab.com:o/app.git"]));
        refused(
            &mut f,
            result,
            "has no GitHub origin, so there is nowhere to push",
        );
        // A second push URL is a destination nobody checked, even behind an allowed first one.
        let (mut f, result) = publish(&|f| {
            for url in [ORIGIN, "git@github.com:other-org/app.git"] {
                set_url(f, &["--add", "--push", "origin", url]);
            }
        });
        refused(&mut f, result, "origin pushes to more than one place");
        // An `origin` repointed since GitHub was asked names a repository nobody checked.
        let (mut f, result) =
            publish(&|f| set_url(f, &["origin", "git@github.com:other-org/app.git"]));
        refused(&mut f, result, "origin now names other-org/app");
        // A base that is not on origin would make the pull request cross repositories.
        let (mut f, result) = publish(&|f| {
            let path = f.repository.to_str().unwrap().to_owned();
            f.host.add_repository(&path, "main").unwrap();
        });
        refused(&mut f, result, "base main is not on origin");
        // A rule that rewrites only pushes makes `origin` push somewhere it does not fetch from.
        let (mut f, result) = publish(&|f| {
            let rule = "url.git@github.com:other-org/.pushInsteadOf";
            git(
                &f.repository,
                &["config", rule, "git@github.com:allowed-org/"],
            );
        });
        refused(
            &mut f,
            result,
            "origin fetches from a different repository than it pushes to",
        );
    }

    #[test]
    fn a_push_url_that_git_would_rewrite_again_is_refused() {
        let f = fixture();
        assert!(ensure_not_rewritten(&f.repository, ORIGIN).is_ok());
        git(
            &f.repository,
            &[
                "config",
                "url.git@github.com:other-org/.pushInsteadOf",
                "git@github.com:allowed-org/",
            ],
        );
        let error = ensure_not_rewritten(&f.repository, ORIGIN).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("would rewrite origin's push URL"),
            "{error}"
        );
        git(
            &f.repository,
            &[
                "config",
                "--unset",
                "url.git@github.com:other-org/.pushInsteadOf",
            ],
        );
        git(
            &f.repository,
            &[
                "config",
                "url.https://example.invalid/.insteadOf",
                "git@github.com:allowed-org/",
            ],
        );
        assert!(ensure_not_rewritten(&f.repository, ORIGIN).is_err());
    }

    #[test]
    fn a_passed_check_freezes_everything_that_will_be_sent() {
        let mut f = fixture();
        f.allow("allowed-org");
        let (status, job) = f.publish(&f.implementer.id.clone()).unwrap();
        let request = f.ticket().publishing.unwrap();
        assert_eq!(job.unwrap().request, request);
        assert_eq!(
            (request.repository.as_str(), request.push_url.as_str()),
            ("allowed-org/app", ORIGIN)
        );
        assert_eq!(
            (request.branch.as_str(), request.base.as_str()),
            ("wiffletree/fix-contrast", "main")
        );
        assert_eq!(request.commit, f.head());
        assert_eq!(request.size, "2 commits · 2 files changed, 2 insertions(+)");
        assert_eq!(
            (request.title.as_deref(), request.body.as_deref()),
            (Some("Fix it"), Some("Why"))
        );
        assert_eq!(
            (request.step, request.waiting.clone()),
            (PublishStep::Push, None)
        );
        assert_eq!(status["steps"], json!(["push", "pull request"]));
        assert_eq!(status["commit"], json!(request.commit));
        // While its step runs, a second call is refused rather than started beside it.
        let again = f.publish(&f.coordinator.id.clone()).unwrap_err();
        assert!(again.to_string().contains("is running"), "{again}");
    }

    #[test]
    fn an_allowed_owner_and_a_passed_review_run_without_asking() {
        let mut f = fixture();
        f.allow("ALLOWED-ORG");
        let (status, job) = f.publish(&f.coordinator.id.clone()).unwrap();
        assert_eq!(status["status"], "publishing");
        assert!(job.is_some());
        assert!(f.open_items().is_empty());
    }

    /// The call waits for the human for `reason`: one item, written by the host, and no step.
    fn asks(f: &mut Fixture, reason: &str) -> Attention {
        let (status, job) = f.publish(&f.implementer.id.clone()).unwrap();
        assert!(job.is_none());
        assert_eq!(status["status"], "waiting_for_human");
        assert_eq!(status["step"], "push");
        assert_eq!(status["reason"], reason);
        assert!(
            status["next"]
                .as_str()
                .unwrap()
                .starts_with("Finish your turn.")
        );
        let items = f.open_items();
        assert_eq!(items.len(), 1);
        let request = f.ticket().publishing.unwrap();
        assert_eq!(request.waiting.as_deref(), Some(items[0].id.as_str()));
        assert_eq!(
            items[0].operation_id,
            format!("publish:{}:push", request.id)
        );
        assert_eq!(items[0].session_id, f.implementer.id);
        assert!(items[0].is_permission());
        items[0].clone()
    }

    #[test]
    fn an_owner_that_is_not_allowed_asks_and_the_item_names_what_would_be_sent() {
        let mut f = fixture();
        let item = asks(&mut f, "allowed-org is not an allowed owner");
        let short = short_sha(&f.head()).to_owned();
        assert_eq!(
            item.prompt,
            format!(
                "Push this branch to allowed-org/app?\n\
                 Repository: allowed-org/app · private · allowed-org is not an allowed owner\n\
                 Branch: wiffletree/fix-contrast\n\
                 Commit: {short} · 2 commits · 2 files changed, 2 insertions(+)\n\
                 Review: Cycle 1 passed at this commit · Correctness"
            )
        );
    }

    #[test]
    fn an_allowed_owners_fork_an_unanswered_repository_and_a_blocked_review_all_ask() {
        let mut fork = fixture();
        fork.allow("allowed-org");
        fork.github("allowed-org/app", true);
        asks(&mut fork, "allowed-org/app is a fork that is not allowed");

        let mut unanswered = fixture();
        unanswered.allow("allowed-org");
        let mut repository = unanswered
            .host
            .ticket_repository(&unanswered.ticket())
            .unwrap();
        repository.github.as_mut().unwrap().facts = None;
        unanswered
            .host
            .db
            .execute(
                "UPDATE repositories SET data=?2 WHERE id=?1",
                params![repository.id, encode(&repository).unwrap()],
            )
            .unwrap();
        let item = asks(
            &mut unanswered,
            "GitHub has not answered for this repository",
        );
        assert!(item.prompt.contains("allowed-org/app · visibility unknown"));

        let mut blocked = fixture();
        blocked.allow("allowed-org");
        blocked.review(VerificationOutcome::Blocked);
        asks(&mut blocked, "the review ended blocked");
    }
}
