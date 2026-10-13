//! Publishing a ticket: the host itself pushes the ticket's branch and opens its pull request,
//! after checking the review, the branch and the repository's owner, and asks the human in the
//! Inbox wherever the rules require. Local checks run on the host thread; the network steps run
//! off it. Nothing here approves, merges, closes or comments on a pull request.
use crate::*;
use std::sync::Arc;

/// A command run in a folder with an argument list, never through a shell.
type Tool<Answer> = Arc<dyn Fn(&Path, &[&str]) -> Result<Answer> + Send + Sync>;
/// `gh`, and Git's two commands that reach the network, as a step runs them.
#[derive(Clone)]
pub(crate) struct Tools {
    pub gh: Tool<runtime::GhAnswer>,
    /// `git ls-remote` and `git push`.
    pub git: Tool<runtime::GitAnswer>,
}
impl Default for Tools {
    fn default() -> Self {
        Self {
            gh: Arc::new(runtime::gh_answer),
            git: Arc::new(|path, args| match args.first() {
                Some(&"push") => runtime::git_push(path, args),
                _ => runtime::git_answer(path, args),
            }),
        }
    }
}

/// How a step ended, reported back to the host thread.
pub(crate) struct Outcome {
    pub ticket_id: String,
    pub request_id: String,
    /// GitHub's answer for the repository, read before the step, when it gave one.
    pub github: Option<repositories::GitHubRead>,
    pub result: StepResult,
}
pub(crate) enum StepResult {
    /// GitHub's branch is at the request's commit; `already` when it was there before.
    Pushed { already: bool },
    /// The pull request's URL.
    Opened(String),
    /// GitHub reports the repository under another owner or name, or as a fork when it was
    /// not: nothing was sent.
    Moved(String),
    /// Why the step sent nothing.
    Failed(String),
}

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

/// Whether `commit` contains `older`. A commit this repository does not have is not contained.
fn is_ancestor(worktree: &Path, older: &str, commit: &str) -> bool {
    runtime::git_answer(worktree, &["merge-base", "--is-ancestor", older, commit])
        .is_ok_and(|answer| answer.status.success())
}

/// The commit's size against what GitHub already has from the host, or else against the base:
/// such as "2 commits · 3 files changed, 41 insertions(+), 7 deletions(-)".
fn commit_size(worktree: &Path, base: &str, pushed: Option<&str>, commit: &str) -> Result<String> {
    let from = match pushed.filter(|pushed| is_ancestor(worktree, pushed, commit)) {
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

/// Runs one step off the host thread. First GitHub's current answer for the repository: a
/// transferred repository keeps answering, and accepting pushes, at its old URL, and an
/// unconfirmed destination is never sent to.
pub(crate) fn run(job: &Job, tools: &Tools) -> Outcome {
    let input = repositories::GitHubInput {
        id: job.repository.id.clone(),
        path: job.repository.path.clone().into(),
        github: None,
    };
    let mut reading =
        repositories::read_github(&[input], now(), |path, args| (tools.gh)(path, args));
    let read = reading.repositories.pop();
    let github = read.as_ref().and_then(|read| read.github.as_ref());
    let result = match github.and_then(|github| github.facts.as_ref()) {
        None => StepResult::Failed(format!(
            "GitHub could not confirm the repository: {}",
            reading
                .error
                .or_else(|| github.and_then(|github| github.problem.clone()))
                .unwrap_or_else(|| "origin no longer names one GitHub repository".into())
        )),
        Some(facts) => match moved(job, facts) {
            Some(moved) => StepResult::Moved(moved),
            None => match job.request.step {
                PublishStep::Push => match push(job, tools) {
                    Ok(already) => StepResult::Pushed { already },
                    Err(error) => StepResult::Failed(format!("{error:#}")),
                },
                PublishStep::OpenPullRequest => open_pull_request(job, tools),
            },
        },
    };
    Outcome {
        ticket_id: job.ticket_id.clone(),
        request_id: job.request.id.clone(),
        // An answer without facts would erase the ones stored.
        github: read.filter(|read| read.github.as_ref().is_some_and(|g| g.facts.is_some())),
        result,
    }
}

/// How GitHub's answer differs from what the request was decided on, if it does.
fn moved(job: &Job, facts: &GitHubFacts) -> Option<String> {
    let request = &job.request;
    if !facts
        .name_with_owner
        .eq_ignore_ascii_case(&request.repository)
    {
        return Some(format!(
            "GitHub now reports {} as {}",
            request.repository, facts.name_with_owner
        ));
    }
    let decided = job.repository.github.as_ref()?.facts.as_ref()?;
    (decided.fork != facts.fork).then(|| {
        let now = if facts.fork { "a fork" } else { "not a fork" };
        format!("GitHub now reports {} as {now}", request.repository)
    })
}

/// Pushes the request's commit to its branch at the request's push URL, and returns whether
/// GitHub already had it. One explicit refspec to one URL, so no remote's settings, tags or
/// submodules ride along. GitHub's branch is replaced only when its head is the commit the host
/// last pushed, or was once this branch's tip here.
fn push(job: &Job, tools: &Tools) -> Result<bool> {
    let request = &job.request;
    let reference = format!("refs/heads/{}", request.branch);
    let listed = (tools.git)(&job.worktree, &["ls-remote", &request.push_url, &reference])?;
    ensure!(
        listed.status.success(),
        "GitHub's {} could not be read{}",
        request.branch,
        listed.reason()
    );
    let head = listed
        .output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(_, name)| *name == reference)
        .map(|(head, _)| head);
    let lease = match head {
        Some(head) if head == request.commit => return Ok(true),
        Some(head) if is_ancestor(&job.worktree, head, &request.commit) => None,
        Some(head)
            if job.pushed.as_deref() == Some(head)
                || was_branch_tip(&job.worktree, &reference, head) =>
        {
            Some(format!("--force-with-lease={reference}:{head}"))
        }
        Some(_) => bail!(
            "GitHub's {} has commits Wiffletree did not push; fetch them and verify again",
            request.branch
        ),
        None => None,
    };
    let refspec = format!("{}:{reference}", request.commit);
    let mut args = vec!["push", "--no-follow-tags", "--recurse-submodules=no"];
    args.extend(lease.as_deref());
    args.extend([request.push_url.as_str(), refspec.as_str()]);
    let pushed = (tools.git)(&job.worktree, &args)?;
    ensure!(
        pushed.status.success(),
        "git push failed{}",
        pushed.reason()
    );
    Ok(false)
}

/// Whether `commit` was once the branch's tip in this repository, as its reflog records. A
/// branch an agent's shell pushed before the host published tickets is adopted this way.
fn was_branch_tip(worktree: &Path, reference: &str, commit: &str) -> bool {
    runtime::git_output(worktree, &["log", "-g", "--format=%H", reference])
        .is_ok_and(|tips| tips.lines().any(|tip| tip == commit))
}

/// Opens the pull request with every value from the frozen request, as an argument list.
fn open_pull_request(job: &Job, tools: &Tools) -> StepResult {
    let request = &job.request;
    let answer = (tools.gh)(
        &job.worktree,
        &[
            "pr",
            "create",
            "--repo",
            &request.repository,
            "--head",
            &request.branch,
            "--base",
            &request.base,
            "--title",
            request.title.as_deref().unwrap_or_default(),
            "--body",
            request.body.as_deref().unwrap_or_default(),
        ],
    );
    match answer {
        Ok(answer) if answer.status.success() => {
            let url = answer
                .output
                .lines()
                .map(str::trim)
                .rfind(|line| line.starts_with("https://"));
            match url {
                Some(url) => StepResult::Opened(url.to_owned()),
                None => StepResult::Failed("gh pr create printed no pull request URL".into()),
            }
        }
        Ok(answer) => StepResult::Failed(format!("gh pr create failed: {}", answer.error)),
        Err(error) => StepResult::Failed(format!("{error:#}")),
    }
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

impl Host {
    /// Records a finished step and returns the next one to run, if the request goes on without
    /// asking. A request that ends sends its caller the one result message.
    pub(crate) fn record_published(&mut self, outcome: Outcome) -> Result<Option<Job>> {
        if let Some(read) = outcome.github {
            self.save_github(read)?;
        }
        let mut ticket = self.ticket(&outcome.ticket_id)?;
        let running = ticket
            .publishing
            .clone()
            .filter(|request| request.id == outcome.request_id && request.waiting.is_none());
        let Some(mut request) = running else {
            return Ok(None);
        };
        let project = self.session(&ticket.coordinator_id)?.project_id;
        let sent = format!(
            "{} at {} to {}",
            request.branch,
            short_sha(&request.commit),
            request.repository
        );
        let result = match outcome.result {
            StepResult::Pushed { already } => {
                ticket.pushed = Some(request.commit.clone());
                if !already {
                    let detail = format!("{}: {sent}", ticket.id);
                    Self::event(
                        &self.db,
                        &project,
                        Some(&request.caller),
                        "ticket_pushed",
                        &detail,
                    )?;
                }
                if request.opens_pull_request() {
                    request.step = PublishStep::OpenPullRequest;
                    ticket.publishing = Some(request);
                    self.save_ticket(&ticket)?;
                    return self.continue_request(&mut ticket);
                }
                let pushed = match already {
                    true => format!("GitHub already had {sent}; nothing needed pushing."),
                    false => format!("Pushed {sent}."),
                };
                match ticket
                    .pull_request
                    .as_ref()
                    .filter(|pr| pr.state == PrState::Open)
                {
                    Some(pr) => format!("{pushed} PR #{} ({}) has this commit.", pr.number, pr.url),
                    None => pushed,
                }
            }
            StepResult::Opened(url) => {
                let detail = format!("{}: {url}", ticket.id);
                Self::event(
                    &self.db,
                    &project,
                    Some(&request.caller),
                    "pull_request_opened",
                    &detail,
                )?;
                let number = url.rsplit('/').next().unwrap_or_default();
                format!("Pushed {sent} and opened PR #{number}: {url}")
            }
            StepResult::Moved(moved) => format!(
                "{moved}. {} Call publish_ticket again: it will be decided on where the repository is now.",
                nothing_sent(&request)
            ),
            StepResult::Failed(reason) => format!(
                "Publishing {} to {} failed: {reason}. {}",
                request.branch,
                request.repository,
                nothing_sent(&request)
            ),
        };
        self.end_request(&mut ticket, &result)?;
        Ok(None)
    }

    /// Before a later step of a request runs: the checks again, and that nothing differs from
    /// what the request froze. Then the step, or its Inbox item.
    fn continue_request(&mut self, ticket: &mut Ticket) -> Result<Option<Job>> {
        let request = ticket.publishing.clone().context("No publish request")?;
        match self.recheck(ticket, &request) {
            Ok(repository) => self.start_step(ticket, &repository),
            Err(changed) => {
                self.end_request(ticket, &changed_notice(&request, &changed))?;
                Ok(None)
            }
        }
    }

    /// The repository, when the ticket still passes its checks and would send what the request
    /// froze; otherwise what changed.
    fn recheck(&self, ticket: &Ticket, request: &PublishRequest) -> Result<Repository> {
        let now = self.check(ticket)?;
        for (what, frozen, current) in [
            ("repository", &request.repository, &now.name),
            ("push URL of origin", &request.push_url, &now.push_url),
            ("ticket's branch", &request.branch, &now.branch),
            ("repository's base branch", &request.base, &now.base),
        ] {
            ensure!(
                frozen == current,
                "The {what} changed from {frozen} to {current}"
            );
        }
        if now.commit != request.commit {
            self.check_verified_head(ticket, &request.commit)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "The worktree moved from {} to {}, which has different changes",
                        short_sha(&request.commit),
                        short_sha(&now.commit)
                    )
                })?;
        }
        Ok(now.repository)
    }

    /// Ends the ticket's request: sends its caller the request's one message and withdraws its
    /// Inbox item if one still waits.
    fn end_request(&mut self, ticket: &mut Ticket, result: &str) -> Result<()> {
        let Some(request) = ticket.publishing.take() else {
            return Ok(());
        };
        // Sent before the request is cleared: after a crash in between, the next start ends the
        // request again and the repeated id sends nothing more.
        let id = format!("{PUBLISH_PREFIX}{}", request.id);
        if let Err(error) = self.send(&id, None, &request.caller, result) {
            eprintln!("Workspace publish: {id}: {error:#}");
        }
        if let Some(item) = &request.waiting
            && self.is_unanswered(item)?
        {
            self.resolve_attention(item, "Withdrawn: this publish request ended")?;
        }
        self.save_ticket(ticket)
    }

    fn is_unanswered(&self, attention: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT json_extract(data,'$.answer') IS NULL FROM attention WHERE id=?1",
                [attention],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    /// Ends each request whose step was running when the host last stopped, or whose answered
    /// item was never acted on, and tells its caller. Calling `publish_ticket` again is safe:
    /// pushing the same commit sends nothing, and GitHub refuses a second pull request for the
    /// branch.
    pub(crate) fn end_interrupted_publishes(&mut self) -> Result<()> {
        for mut ticket in self.tickets()? {
            let Some(request) = &ticket.publishing else {
                continue;
            };
            if let Some(item) = &request.waiting
                && self.is_unanswered(item)?
            {
                continue;
            }
            let sent = match request.step {
                PublishStep::Push => "The push may or may not have reached GitHub.".to_owned(),
                PublishStep::OpenPullRequest => format!(
                    "The branch was pushed at {}; the pull request may or may not have been opened.",
                    short_sha(&request.commit)
                ),
            };
            let notice = format!(
                "Publishing {} to {} was interrupted when Wiffletree stopped. {sent} Call publish_ticket again; repeating it is safe.",
                request.branch, request.repository
            );
            self.end_request(&mut ticket, &notice)?;
        }
        Ok(())
    }
}

/// What a request that sends nothing more has sent so far.
fn nothing_sent(request: &PublishRequest) -> String {
    match request.step {
        PublishStep::Push => "Nothing was pushed.".into(),
        PublishStep::OpenPullRequest => format!(
            "The branch was pushed at {}; no pull request was opened.",
            short_sha(&request.commit)
        ),
    }
}

/// The message for a request that ended because what it froze is no longer current.
fn changed_notice(request: &PublishRequest, changed: &anyhow::Error) -> String {
    format!(
        "{changed:#}. {} Call publish_ticket again for what is current.",
        nothing_sent(request)
    )
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

    /// Stands in for GitHub: what it answers, and what was asked of it.
    #[derive(Default)]
    pub(crate) struct FakeGitHub {
        /// `nameWithOwner` and `isFork` of the repository; `None` when GitHub cannot be reached.
        pub answer: Option<(String, bool)>,
        /// Why `gh pr create` fails, when it does.
        pub create_error: Option<String>,
        /// The arguments of every `gh pr create` call.
        pub created: Vec<Vec<String>>,
        /// The arguments of every `git ls-remote` and `git push`.
        pub git: Vec<Vec<String>>,
        /// The local bare repository behind each GitHub URL.
        pub urls: Vec<(String, String)>,
    }
    pub(crate) struct Remote {
        /// The bare repository behind `ORIGIN`.
        pub bare: PathBuf,
        pub github: Arc<std::sync::Mutex<FakeGitHub>>,
        pub tools: Tools,
    }
    impl Remote {
        pub fn branch(&self, branch: &str) -> Option<String> {
            refs(&self.bare)
                .into_iter()
                .find_map(|(name, head)| (name == format!("refs/heads/{branch}")).then_some(head))
        }
        pub fn git_calls(&self) -> Vec<Vec<String>> {
            self.github.lock().unwrap().git.clone()
        }
    }
    pub(crate) fn refs(bare: &Path) -> Vec<(String, String)> {
        git(bare, &["for-each-ref", "--format=%(refname) %(objectname)"])
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(name, head)| (name.to_owned(), head.to_owned()))
            .collect()
    }
    fn exit(code: i32) -> std::process::ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    /// A local bare repository holding `main`, a fake `gh` that answers for `allowed-org/app`,
    /// and a Git that reaches the bare repository wherever the host names `ORIGIN`. No test
    /// reaches GitHub.
    pub(crate) fn remote(f: &Fixture) -> Remote {
        let bare = f.home.path().join("github.git");
        git(
            f.home.path(),
            &["init", "-q", "--bare", bare.to_str().unwrap()],
        );
        git(
            &f.repository,
            &["push", "-q", bare.to_str().unwrap(), "main"],
        );
        let github = Arc::new(std::sync::Mutex::new(FakeGitHub {
            answer: Some(("allowed-org/app".into(), false)),
            urls: vec![(ORIGIN.into(), bare.to_str().unwrap().into())],
            ..Default::default()
        }));
        let state = github.clone();
        let gh = move |_: &Path, args: &[&str]| -> Result<runtime::GhAnswer> {
            let mut state = state.lock().unwrap();
            let answer = |code, output: String, error: &str| {
                Ok(runtime::GhAnswer {
                    status: exit(code),
                    output,
                    error: error.into(),
                })
            };
            if args[..2] == ["api", "graphql"] {
                return match &state.answer {
                    Some((name, fork)) => answer(
                        0,
                        json!({"data": {"r0": {"nameWithOwner": name, "isFork": fork,
                            "visibility": "PRIVATE", "parent": null,
                            "owner": {"__typename": "Organization"}}}})
                        .to_string(),
                        "",
                    ),
                    None => answer(1, String::new(), "error connecting to api.github.com"),
                };
            }
            assert_eq!(
                args[..2],
                ["pr", "create"],
                "the host runs no other gh command"
            );
            state
                .created
                .push(args.iter().map(|a| (*a).to_owned()).collect());
            match &state.create_error {
                Some(error) => answer(1, String::new(), error),
                None => answer(
                    0,
                    format!(
                        "Creating pull request\nhttps://github.com/{}/pull/{}\n",
                        args[3],
                        44 + state.created.len()
                    ),
                    "",
                ),
            }
        };
        let state = github.clone();
        let git_tool = move |path: &Path, args: &[&str]| -> Result<runtime::GitAnswer> {
            let mut state = state.lock().unwrap();
            state
                .git
                .push(args.iter().map(|a| (*a).to_owned()).collect());
            let local: Vec<String> = args
                .iter()
                .map(|arg| {
                    let known = state.urls.iter().find(|(url, _)| url == arg);
                    known.map_or((*arg).to_owned(), |(_, bare)| bare.clone())
                })
                .collect();
            assert!(
                local
                    .iter()
                    .all(|a| !a.contains("github.com") && a != "origin"),
                "a test must never reach GitHub: {local:?}"
            );
            let local: Vec<&str> = local.iter().map(String::as_str).collect();
            match local[0] {
                "push" => runtime::git_push(path, &local),
                _ => runtime::git_answer(path, &local),
            }
        };
        Remote {
            bare,
            github,
            tools: Tools {
                gh: Arc::new(gh),
                git: Arc::new(git_tool),
            },
        }
    }

    /// Runs steps as the service does, until the request waits for the human or ends.
    pub(crate) fn drive(f: &mut Fixture, remote: &Remote, mut step: Option<Job>) {
        while let Some(job) = step {
            step = f.host.record_published(run(&job, &remote.tools)).unwrap();
        }
    }
    /// Every publish result sent so far, as its recipient and body.
    pub(crate) fn results(host: &Host) -> Vec<(String, String)> {
        host.db
            .prepare("SELECT recipient, body FROM messages WHERE id LIKE 'publish:%' AND sender IS NULL ORDER BY sequence")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
    /// Marks the ticket's PR open, as the watcher would after reading it.
    fn watch_open_pr(f: &mut Fixture, number: u64) -> PullRequest {
        let pr = PullRequest {
            number,
            url: format!("https://github.com/allowed-org/app/pull/{number}"),
            state: PrState::Open,
            draft: false,
            base: "main".into(),
            head: f.head(),
            merge_state: MergeState::Clean,
            checks: CheckState::None,
            unresolved_threads: 0,
            comments: CommentCursors::default(),
            merge_commit: None,
        };
        let mut ticket = f.ticket();
        ticket.pull_request = Some(pr.clone());
        f.host.save_ticket(&ticket).unwrap();
        pr
    }
    /// Stops the host and starts it again on the same store.
    fn restart(f: Fixture) -> Fixture {
        let Fixture {
            home,
            host,
            ticket,
            coordinator,
            implementer,
            repository,
            worktree,
        } = f;
        let store = host.home.clone();
        drop(host);
        Fixture {
            host: Host::open(store).unwrap(),
            home,
            ticket,
            coordinator,
            implementer,
            repository,
            worktree,
        }
    }
    const BRANCH: &str = "wiffletree/fix-contrast";

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
        let error = format!("{:#}", result.expect_err("publish was not refused"));
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

    #[test]
    fn a_first_publish_creates_the_branch_opens_the_pull_request_and_tells_the_caller() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        let head = f.head();
        let title = "Fix $(rm -rf ~) \"quoted\" && `x` | ; 'y'";
        let body = "Why:\n- one\n- $HOME stays as written";
        let (caller, ticket) = (f.implementer.id.clone(), f.ticket.clone());
        let (_, step) = f
            .host
            .publish_ticket(&caller, &ticket, Some(title), Some(body))
            .unwrap();
        drive(&mut f, &remote, step);
        assert_eq!(remote.branch(BRANCH), Some(head.clone()));
        // One ref, to the request's URL, with nothing riding along.
        let refspec = format!("{head}:refs/heads/{BRANCH}");
        assert_eq!(
            remote.git_calls(),
            [
                vec!["ls-remote", ORIGIN, "refs/heads/wiffletree/fix-contrast"],
                vec![
                    "push",
                    "--no-follow-tags",
                    "--recurse-submodules=no",
                    ORIGIN,
                    &refspec
                ],
            ]
        );
        // Every value is its own argument, so shell characters arrive unchanged.
        assert_eq!(
            remote.github.lock().unwrap().created,
            [[
                "pr",
                "create",
                "--repo",
                "allowed-org/app",
                "--head",
                BRANCH,
                "--base",
                "main",
                "--title",
                title,
                "--body",
                body
            ]]
        );
        let ticket = f.ticket();
        assert_eq!(
            (ticket.pushed, ticket.publishing),
            (Some(head.clone()), None)
        );
        assert_eq!(
            results(&f.host),
            [(
                caller,
                format!(
                    "Pushed {BRANCH} at {} to allowed-org/app and opened PR #45: https://github.com/allowed-org/app/pull/45",
                    short_sha(&head)
                )
            )]
        );
        assert!(f.open_items().is_empty());
        let kinds: Vec<String> = f
            .host
            .activity(&f.coordinator.project_id, None, 50)
            .unwrap()
            .into_iter()
            .map(|a| a.kind)
            .collect();
        assert!(
            kinds.contains(&"ticket_pushed".into())
                && kinds.contains(&"pull_request_opened".into())
        );
    }

    /// A ticket whose first publish went through and whose PR #45 the watcher has seen.
    fn published() -> (Fixture, Remote) {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        let (_, step) = f.publish(&f.coordinator.id.clone()).unwrap();
        drive(&mut f, &remote, step);
        watch_open_pr(&mut f, 45);
        (f, remote)
    }
    /// Publishes again to the open PR, and returns the result message.
    fn update(f: &mut Fixture, remote: &Remote) -> String {
        let (caller, ticket) = (f.coordinator.id.clone(), f.ticket.clone());
        let (_, step) = f.host.publish_ticket(&caller, &ticket, None, None).unwrap();
        drive(f, remote, step);
        results(&f.host).pop().unwrap().1
    }

    #[test]
    fn an_update_pushes_the_new_commit_to_the_open_pull_request_and_opens_nothing() {
        let (mut f, remote) = published();
        let (caller, ticket) = (f.coordinator.id.clone(), f.ticket.clone());
        // An open pull request's title and description are not edited.
        let edit = f
            .host
            .publish_ticket(&caller, &ticket, Some("New title"), Some("Why"));
        assert!(edit.unwrap_err().to_string().contains("PR #45 is open"));
        let head = f.commit("c.txt", "three\n");
        f.review(VerificationOutcome::Passed);
        let (status, step) = f.host.publish_ticket(&caller, &ticket, None, None).unwrap();
        assert_eq!(status["steps"], json!(["push"]));
        drive(&mut f, &remote, step);
        assert_eq!(remote.branch(BRANCH), Some(head.clone()));
        assert_eq!(remote.github.lock().unwrap().created.len(), 1);
        // A fast-forward needs no lease.
        assert!(
            remote
                .git_calls()
                .iter()
                .flatten()
                .all(|a| !a.starts_with("--force"))
        );
        assert_eq!(
            results(&f.host).pop().unwrap().1,
            format!(
                "Pushed {BRANCH} at {} to allowed-org/app. PR #45 (https://github.com/allowed-org/app/pull/45) has this commit.",
                short_sha(&head)
            )
        );
        assert!(f.open_items().is_empty());
        // The same commit again sends nothing.
        let pushes = |remote: &Remote| remote.git_calls().iter().filter(|c| c[0] == "push").count();
        let before = pushes(&remote);
        let again = update(&mut f, &remote);
        assert!(
            again.contains("GitHub already had") && again.contains("nothing needed pushing"),
            "{again}"
        );
        assert_eq!(pushes(&remote), before);
    }

    #[test]
    fn a_rebased_branch_replaces_what_the_host_pushed() {
        let (mut f, remote) = published();
        let pushed = f.head();
        git(&f.worktree, &["commit", "-q", "--amend", "-m", "reworded"]);
        let rebased = f.head();
        let result = update(&mut f, &remote);
        assert!(result.starts_with("Pushed "), "{result}");
        assert_eq!(remote.branch(BRANCH), Some(rebased.clone()));
        let lease = format!("--force-with-lease=refs/heads/{BRANCH}:{pushed}");
        assert!(remote.git_calls().last().unwrap().contains(&lease));
        assert_eq!(f.ticket().pushed, Some(rebased));
    }

    /// Pushes a commit to the ticket's branch on the remote from another clone.
    fn push_from_elsewhere(f: &Fixture, remote: &Remote) -> String {
        let clone = f.home.path().join("elsewhere");
        let bare = remote.bare.to_str().unwrap();
        git(
            f.home.path(),
            &[
                "clone",
                "-q",
                "--branch",
                BRANCH,
                bare,
                clone.to_str().unwrap(),
            ],
        );
        git(
            &clone,
            &["commit", "-q", "--allow-empty", "-m", "someone else"],
        );
        git(&clone, &["push", "-q", "origin", BRANCH]);
        git(&clone, &["rev-parse", "HEAD"])
    }

    #[test]
    fn commits_the_host_did_not_push_are_never_overwritten() {
        let (mut f, remote) = published();
        let theirs = push_from_elsewhere(&f, &remote);
        git(&f.worktree, &["commit", "-q", "--amend", "-m", "reworded"]);
        for pushed in [f.ticket().pushed, None] {
            // Neither the host's record nor the branch's own history knows their commit.
            let mut ticket = f.ticket();
            ticket.pushed = pushed;
            f.host.save_ticket(&ticket).unwrap();
            let result = update(&mut f, &remote);
            assert_eq!(
                result,
                format!(
                    "Publishing {BRANCH} to allowed-org/app failed: GitHub's {BRANCH} has commits Wiffletree did not push; fetch them and verify again. Nothing was pushed."
                )
            );
            assert_eq!(remote.branch(BRANCH), Some(theirs.clone()));
        }
        assert!(remote.git_calls().iter().filter(|c| c[0] == "push").count() == 1);
    }

    #[test]
    fn a_branch_an_agents_shell_pushed_is_adopted_after_a_rebase() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        // Before the host published tickets, the agent pushed the branch from its shell.
        let shell_pushed = f.head();
        let bare = remote.bare.to_str().unwrap().to_owned();
        git(
            &f.worktree,
            &["push", "-q", &bare, &format!("HEAD:refs/heads/{BRANCH}")],
        );
        watch_open_pr(&mut f, 45);
        git(&f.worktree, &["commit", "-q", "--amend", "-m", "reworded"]);
        let rebased = f.head();
        assert_eq!(f.ticket().pushed, None);
        let result = update(&mut f, &remote);
        assert!(result.starts_with("Pushed "), "{result}");
        assert_eq!(remote.branch(BRANCH), Some(rebased));
        // The lease names the exact head that was once this branch's tip here.
        let lease = format!("--force-with-lease=refs/heads/{BRANCH}:{shell_pushed}");
        assert!(remote.git_calls().last().unwrap().contains(&lease));
    }

    #[test]
    fn the_push_goes_to_the_url_the_request_froze_when_origin_is_repointed() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        let (_, step) = f.publish(&f.coordinator.id.clone()).unwrap();
        let job = step.unwrap();
        // After the check, `origin` is pointed at another repository.
        let other_url = "git@github.com:other-org/app.git";
        let other = f.home.path().join("other.git");
        git(
            f.home.path(),
            &["init", "-q", "--bare", other.to_str().unwrap()],
        );
        remote
            .github
            .lock()
            .unwrap()
            .urls
            .push((other_url.into(), other.to_str().unwrap().into()));
        git(&f.repository, &["remote", "set-url", "origin", other_url]);
        // The push itself names the frozen URL, so it cannot follow `origin`.
        assert!(!push(&job, &remote.tools).unwrap());
        assert_eq!(remote.branch(BRANCH), Some(f.head()));
        assert!(refs(&other).is_empty());
        // And a whole step notices that `origin` is no longer the repository it confirmed.
        git(
            &remote.bare,
            &["update-ref", "-d", &format!("refs/heads/{BRANCH}")],
        );
        remote.github.lock().unwrap().answer = Some(("other-org/app".into(), false));
        let outcome = run(&job, &remote.tools);
        assert!(matches!(&outcome.result, StepResult::Moved(moved)
            if moved == "GitHub now reports allowed-org/app as other-org/app"));
        assert!(remote.branch(BRANCH).is_none() && refs(&other).is_empty());
    }

    #[test]
    fn only_the_tickets_branch_is_pushed_whatever_git_is_configured_to_do() {
        let mut f = fixture();
        let mut remote = remote(&f);
        f.allow("allowed-org");
        let home = f.home.path().to_owned();
        // A submodule with a commit its own remote does not have.
        let library = home.join("library.git");
        let seed = home.join("seed");
        git(
            &home,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                library.to_str().unwrap(),
            ],
        );
        git(
            &home,
            &[
                "clone",
                "-q",
                library.to_str().unwrap(),
                seed.to_str().unwrap(),
            ],
        );
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "library"]);
        git(&seed, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        let library_head = git(&library, &["rev-parse", "refs/heads/main"]);
        git(
            &f.worktree,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "--quiet",
                "add",
                library.to_str().unwrap(),
                "library",
            ],
        );
        git(
            &f.worktree.join("library"),
            &["commit", "-q", "--allow-empty", "-m", "unpushed"],
        );
        f.commit("library.txt", "uses the library\n");
        // An annotated tag the remote lacks, and another local branch.
        git(&f.worktree, &["tag", "-a", "v1", "-m", "v1"]);
        git(&f.worktree, &["branch", "side"]);
        f.review(VerificationOutcome::Passed);
        // Every setting that makes a push send more, in the repository's and the user's config.
        let global = home.join("gitconfig");
        let hostile = [
            ("push.followTags", "true"),
            ("push.recurseSubmodules", "on-demand"),
            ("push.default", "matching"),
            ("remote.origin.push", "refs/heads/*:refs/heads/*"),
            ("remote.origin.mirror", "true"),
        ];
        for (key, value) in hostile {
            git(&f.repository, &["config", key, value]);
            git(
                &home,
                &["config", "--file", global.to_str().unwrap(), key, value],
            );
        }
        let urls = remote.github.lock().unwrap().urls.clone();
        remote.tools.git = Arc::new(move |path, args| {
            // The same commands, run under the hostile user configuration.
            let local: Vec<&str> = args
                .iter()
                .map(|arg| {
                    urls.iter()
                        .find(|(url, _)| url == arg)
                        .map_or(*arg, |(_, bare)| bare.as_str())
                })
                .collect();
            assert!(
                local
                    .iter()
                    .all(|a| !a.contains("github.com") && *a != "origin")
            );
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(path)
                .args(&local)
                .env("GIT_CONFIG_GLOBAL", &global)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()?;
            Ok(runtime::GitAnswer {
                status: output.status,
                output: String::from_utf8(output.stdout)?,
                error: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        });
        let before = refs(&remote.bare);
        let (_, step) = f.publish(&f.coordinator.id.clone()).unwrap();
        drive(&mut f, &remote, step);
        let result = results(&f.host).pop().unwrap().1;
        assert!(result.starts_with("Pushed "), "{result}");
        let mut expected = before;
        expected.push((format!("refs/heads/{BRANCH}"), f.head()));
        expected.sort();
        assert_eq!(
            refs(&remote.bare),
            expected,
            "the destination gained exactly one ref"
        );
        assert_eq!(
            git(&library, &["rev-parse", "refs/heads/main"]),
            library_head,
            "the submodule's commit stayed here"
        );
    }

    #[test]
    fn a_transferred_repository_is_not_pushed_to_and_the_next_call_is_decided_on_its_new_owner() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        // Recorded yesterday as allowed-org/app; GitHub has since moved it, and the old URL
        // still answers.
        remote.github.lock().unwrap().answer = Some(("other-org/app".into(), false));
        let (status, step) = f.publish(&f.implementer.id.clone()).unwrap();
        assert_eq!(status["status"], "publishing");
        drive(&mut f, &remote, step);
        assert!(remote.branch(BRANCH).is_none() && remote.git_calls().is_empty());
        assert_eq!(
            results(&f.host),
            [(
                f.implementer.id.clone(),
                "GitHub now reports allowed-org/app as other-org/app. Nothing was pushed. Call publish_ticket again: it will be decided on where the repository is now.".to_owned()
            )]
        );
        let repository = f.host.ticket_repository(&f.ticket()).unwrap();
        assert_eq!(owner_entry(&repository).as_deref(), Some("other-org"));
        // The owner it is under now is not allowed, so the next call waits for the human.
        let (status, step) = f.publish(&f.implementer.id.clone()).unwrap();
        assert!(step.is_none());
        assert_eq!(status["repository"], "other-org/app");
        assert_eq!(status["reason"], "other-org is not an allowed owner");
        let items = f.open_items();
        assert!(
            items[0]
                .prompt
                .starts_with("Push this branch to other-org/app?"),
            "{}",
            items[0].prompt
        );
    }

    #[test]
    fn a_repository_that_became_a_fork_is_not_pushed_to() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        remote.github.lock().unwrap().answer = Some(("allowed-org/app".into(), true));
        let (_, step) = f.publish(&f.coordinator.id.clone()).unwrap();
        drive(&mut f, &remote, step);
        assert!(remote.branch(BRANCH).is_none());
        assert!(
            results(&f.host)[0]
                .1
                .starts_with("GitHub now reports allowed-org/app as a fork. Nothing was pushed.")
        );
        let (status, _) = f.publish(&f.coordinator.id.clone()).unwrap();
        assert_eq!(
            status["reason"],
            "allowed-org/app is a fork that is not allowed"
        );
    }

    #[test]
    fn nothing_is_sent_when_github_cannot_confirm_the_repository() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        remote.github.lock().unwrap().answer = None;
        let (_, step) = f.publish(&f.coordinator.id.clone()).unwrap();
        drive(&mut f, &remote, step);
        assert!(remote.branch(BRANCH).is_none() && remote.git_calls().is_empty());
        assert_eq!(
            results(&f.host)[0].1,
            format!(
                "Publishing {BRANCH} to allowed-org/app failed: GitHub could not confirm the repository: error connecting to api.github.com. Nothing was pushed."
            )
        );
        // The earlier answer stands.
        let repository = f.host.ticket_repository(&f.ticket()).unwrap();
        assert_eq!(owner_entry(&repository).as_deref(), Some("allowed-org"));
    }

    /// Makes the remote reject every push, as GitHub does without credentials.
    fn reject_pushes(remote: &Remote, reject: bool) {
        let hook = remote.bare.join("hooks/pre-receive");
        if reject {
            fs::create_dir_all(hook.parent().unwrap()).unwrap();
            fs::write(&hook, "#!/bin/sh\necho 'Permission denied' >&2\nexit 1\n").unwrap();
            fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        } else {
            fs::remove_file(&hook).unwrap();
        }
    }

    #[test]
    fn a_failed_push_is_reported_with_gits_error_and_each_retry_gets_its_own_result() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        reject_pushes(&remote, true);
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        let (_, step) = f.publish(&implementer).unwrap();
        drive(&mut f, &remote, step);
        let failed = results(&f.host);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, implementer);
        assert!(
            failed[0].1.starts_with(&format!(
                "Publishing {BRANCH} to allowed-org/app failed: git push failed: error: failed to push some refs to"
            )) && failed[0].1.ends_with(". Nothing was pushed."),
            "{}",
            failed[0].1
        );
        assert!(f.ticket().publishing.is_none() && f.ticket().pushed.is_none());
        // The same commit again is a new request with its own result.
        let (_, step) = f.publish(&implementer).unwrap();
        drive(&mut f, &remote, step);
        assert_eq!(results(&f.host).len(), 2);
        // The coordinator retries once the cause is fixed, and gets the result of its own request.
        reject_pushes(&remote, false);
        let (_, step) = f.publish(&coordinator).unwrap();
        drive(&mut f, &remote, step);
        let all = results(&f.host);
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].0, coordinator);
        assert!(
            all[2].1.starts_with("Pushed ") && all[2].1.contains("opened PR #45"),
            "{}",
            all[2].1
        );
    }

    #[test]
    fn a_pull_request_that_cannot_be_opened_says_the_branch_was_pushed() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        remote.github.lock().unwrap().create_error = Some("pull request create failed: GraphQL: No commits between main and wiffletree/fix-contrast".into());
        let (_, step) = f.publish(&f.coordinator.id.clone()).unwrap();
        drive(&mut f, &remote, step);
        let head = f.head();
        assert_eq!(
            results(&f.host)[0].1,
            format!(
                "Publishing {BRANCH} to allowed-org/app failed: gh pr create failed: pull request create failed: GraphQL: No commits between main and wiffletree/fix-contrast. The branch was pushed at {}; no pull request was opened.",
                short_sha(&head)
            )
        );
        assert_eq!(f.ticket().pushed, Some(head));
    }

    #[test]
    fn a_step_interrupted_by_a_restart_is_reported_and_publishing_again_is_safe() {
        let mut f = fixture();
        let remote = remote(&f);
        f.allow("allowed-org");
        let caller = f.implementer.id.clone();
        let head = f.head();
        // The host stops after the push reached GitHub and before it recorded the result.
        let (_, step) = f.publish(&caller).unwrap();
        let _unrecorded = run(&step.unwrap(), &remote.tools);
        let mut f = restart(f);
        assert!(f.ticket().publishing.is_none());
        assert_eq!(
            results(&f.host),
            [(
                caller.clone(),
                format!(
                    "Publishing {BRANCH} to allowed-org/app was interrupted when Wiffletree stopped. The push may or may not have reached GitHub. Call publish_ticket again; repeating it is safe."
                )
            )]
        );
        // Calling again pushes nothing new, and this time the host stops after the pull request
        // was opened.
        let (_, step) = f.publish(&caller).unwrap();
        let next = f
            .host
            .record_published(run(&step.unwrap(), &remote.tools))
            .unwrap();
        assert_eq!(
            remote.git_calls().iter().filter(|c| c[0] == "push").count(),
            1
        );
        let _unrecorded = run(&next.unwrap(), &remote.tools);
        let mut f = restart(f);
        let interrupted = results(&f.host).pop().unwrap().1;
        assert!(
            interrupted.contains(&format!(
                "The branch was pushed at {}; the pull request may or may not have been opened.",
                short_sha(&head)
            )),
            "{interrupted}"
        );
        // GitHub refuses a second pull request for the branch, and the caller is told.
        remote.github.lock().unwrap().create_error = Some(format!(
            "a pull request for branch \"{BRANCH}\" into branch \"main\" already exists"
        ));
        let (_, step) = f.publish(&caller).unwrap();
        drive(&mut f, &remote, step);
        assert_eq!(remote.branch(BRANCH), Some(head));
        assert_eq!(
            remote.git_calls().iter().filter(|c| c[0] == "push").count(),
            1
        );
        let last = results(&f.host).pop().unwrap().1;
        assert!(
            last.contains("already exists") && last.contains("no pull request was opened"),
            "{last}"
        );
        // A restart with nothing running tells nobody anything.
        let count = results(&f.host).len();
        let f = restart(f);
        assert_eq!(results(&f.host).len(), count);
    }
}
