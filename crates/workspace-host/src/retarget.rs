//! Moving a ticket to another base with `set_ticket_base`: the host fetches the branch from
//! origin and, when the ticket's open pull request opens elsewhere, moves it there on GitHub
//! with `gh pr edit --base`. Moving a pull request is outward-facing, so it follows
//! `publish_ticket`'s rules: it runs without asking only where the repository publishes without
//! asking and the Publishing Behavior does not always ask, and never into a protected branch
//! without the human's approval. Nothing here merges. Local checks run on the host thread; the
//! fetch and GitHub run off it.
use crate::*;
use publish::Tools;

/// One request's fetch and move, with everything it needs off the host thread.
#[derive(Debug)]
pub(crate) struct Job {
    pub ticket_id: String,
    pub request: RetargetRequest,
    /// The repository's checkout, which fetches the base and runs `gh`.
    pub checkout: PathBuf,
}

/// How a job ended, reported back to the host thread.
pub(crate) struct Outcome {
    pub ticket_id: String,
    pub request_id: String,
    pub result: Moved,
}
pub(crate) enum Moved {
    /// The base was fetched, and the pull request, if any, moved to it.
    Done,
    /// GitHub protects the base now, though the daily read did not say so, and the human has not
    /// approved the move: nothing was moved.
    Protected(String),
    /// Why nothing changed.
    Failed(String),
}

/// Fetches the base, then moves the pull request to it. A move the human did not approve reads
/// the base's protection from GitHub first, as the daily read may be a day old.
pub(crate) fn run(job: &Job, tools: &Tools) -> Outcome {
    let request = &job.request;
    let result = checkout::fetch_base(&tools.git, &job.checkout, &request.base).and_then(|()| {
        let Some(number) = request.number else {
            return Ok(Moved::Done);
        };
        if request.asks.is_none()
            && let Some(why) = protected_now(job, tools)?
        {
            return Ok(Moved::Protected(why));
        }
        // The host is named, so `GH_HOST` cannot move a pull request on another GitHub.
        let repository = format!("{}/{}", repositories::GITHUB_HOST, request.repository);
        let number = number.to_string();
        let args = [
            "pr",
            "edit",
            &number,
            "--repo",
            &repository,
            "--base",
            &request.base,
        ];
        let answer = (tools.gh)(&job.checkout, &args)?;
        ensure!(
            answer.status.success(),
            "gh pr edit failed: {}",
            answer.error
        );
        Ok(Moved::Done)
    });
    Outcome {
        ticket_id: job.ticket_id.clone(),
        request_id: request.id.clone(),
        result: result.unwrap_or_else(|error| Moved::Failed(format!("{error:#}"))),
    }
}

/// Why GitHub protects the base now, by branch protection or a ruleset, if it does.
fn protected_now(job: &Job, tools: &Tools) -> Result<Option<String>> {
    let (repository, base) = (&job.request.repository, &job.request.base);
    let read = |endpoint: String| github_access::gh_json(tools, &job.checkout, &endpoint);
    let branch = read(format!("repos/{repository}/branches/{base}"))?;
    let protected = (branch["protected"].as_bool())
        .context("GitHub did not say whether the base is protected")?;
    let rules = read(format!("repos/{repository}/rules/branches/{base}"))?;
    let ruled = !(rules.as_array())
        .context("GitHub's rules are not a list")?
        .is_empty();
    Ok(match (protected, ruled) {
        (true, _) => Some(format!(
            "{base} is protected: GitHub branch protection protects it"
        )),
        (false, true) => Some(format!("{base} is protected: a GitHub ruleset protects it")),
        (false, false) => None,
    })
}

/// The outcome of a job whose thread stopped before it reported, such as by a panic.
pub(crate) fn stopped(job: &Job) -> Outcome {
    Outcome {
        ticket_id: job.ticket_id.clone(),
        request_id: job.request.id.clone(),
        result: Moved::Failed("the job stopped unexpectedly".into()),
    }
}

impl Host {
    /// Accepts the coordinator's `set_ticket_base` call: records the request and returns the job
    /// that fetches the base and moves the pull request, or raises the human's Inbox item when
    /// moving the pull request needs approval first. Returns the tool's result too.
    pub(crate) fn set_ticket_base(
        &mut self,
        caller: &str,
        ticket_id: &str,
        base: &str,
    ) -> Result<(Value, Option<Job>)> {
        let mut ticket = self.ticket(ticket_id)?;
        let coordinates = self.session(caller)?.role == Role::ProjectOrchestrator
            && ticket.coordinator_id == caller;
        ensure!(
            coordinates,
            "Only the project coordinator that owns the ticket can change its base"
        );
        ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
        text(base, 255)?;
        ensure!(
            ticket.retargeting.is_none(),
            "A change of this ticket's base is running or waits for the human; its result will arrive as a message from Wiffletree"
        );
        ensure!(
            ticket.publishing.is_none() && ticket.merging.is_none(),
            "A publish or merge of this ticket is running or waits for the human; change its base once it ends"
        );
        let repository = self.ticket_repository(&ticket)?;
        let current = self.ticket_base(&ticket)?;
        let open = ticket.open_pull_request();
        let from = match open {
            Some(pr) => pr.base.clone(),
            None => (current.pull_request_base()).unwrap_or_else(|_| current.measured().into()),
        };
        if from == base && ticket.stacked_on.is_none() {
            return Ok((json!({"status": "unchanged", "base": base}), None));
        }
        let moves = open.filter(|pr| pr.base != base);
        let asks = match moves {
            Some(_) => self.retarget_ask(&ticket, &repository, base)?,
            None => None,
        };
        let request = RetargetRequest {
            id: new_id(),
            caller: caller.into(),
            repository: match moves {
                Some(_) => (repository.github.as_ref())
                    .and_then(|github| github.facts.as_ref())
                    .map(|facts| facts.name_with_owner.clone())
                    .context("GitHub has not answered for this repository; nothing changed")?,
                None => repository.name.clone(),
            },
            number: moves.map(|pr| pr.number),
            from,
            base: base.into(),
            asks,
            waiting: None,
        };
        ticket.retargeting = Some(request.clone());
        if let Some(why) = &request.asks {
            self.ask_human(&mut ticket, request.clone())?;
            return Ok((
                json!({"status": "waiting_for_human", "base": base, "reason": why,
                    "next": "Finish your turn. Wiffletree moves the pull request once the human approves and messages you the result."}),
                None,
            ));
        }
        self.save_ticket(&ticket)?;
        let job = self.retarget_job(&ticket, &request)?;
        Ok((
            json!({"status": "setting_base", "base": base,
                "next": "Finish your turn. Wiffletree messages you the result."}),
            Some(job),
        ))
    }

    /// Why moving the ticket's pull request to `base` waits for the human, if it does.
    fn retarget_ask(
        &self,
        ticket: &Ticket,
        repository: &Repository,
        base: &str,
    ) -> Result<Option<String>> {
        Ok(
            if let Some(why) = ProtectedBranches::of(repository).why(base) {
                Some(format!("{base} is protected: {why}"))
            } else if !is_allowed(repository, &self.settings().allowed_owners) {
                Some(not_allowed_reason(repository))
            } else if self.always_asks_before_publishing(ticket)? {
                Some(ALWAYS_ASK_REASON.to_owned())
            } else {
                None
            },
        )
    }

    /// Raises the Inbox item for `request`, which says why it asks, and records that the
    /// request waits on it.
    fn ask_human(&mut self, ticket: &mut Ticket, request: RetargetRequest) -> Result<()> {
        let why = request.asks.as_deref().unwrap_or_default();
        let item = self.request_attention(
            &request.caller,
            "local",
            &format!("{RETARGET_PREFIX}{}", request.id),
            &retarget_prompt(ticket, &request, why),
            &[],
        )?;
        ticket.retargeting = Some(RetargetRequest {
            waiting: Some(item.id),
            ..request
        });
        self.save_ticket(ticket)
    }

    fn retarget_job(&self, ticket: &Ticket, request: &RetargetRequest) -> Result<Job> {
        Ok(Job {
            ticket_id: ticket.id.clone(),
            request: request.clone(),
            checkout: self.ticket_repository(ticket)?.path.into(),
        })
    }

    /// Acts on the human's answer to a retarget item, and only for the item a ticket's waiting
    /// request names. Approval returns the job that moves the pull request; any other answer
    /// ends the request with nothing changed.
    pub(crate) fn answer_retarget(&mut self, item: &str, approved: bool) -> Result<Option<Job>> {
        let waiting = self.tickets()?.into_iter().find(|ticket| {
            (ticket.retargeting.as_ref()).is_some_and(|r| r.waiting.as_deref() == Some(item))
        });
        let Some(mut ticket) = waiting else {
            return Ok(None);
        };
        let request = ticket.retargeting.clone().context("No retarget request")?;
        if !approved || !ticket.is_open() {
            let result = format!(
                "The human did not approve moving {} to {}. Nothing changed.",
                named(&ticket, &request),
                request.base
            );
            self.end_retarget(&mut ticket, &result)?;
            return Ok(None);
        }
        let request = RetargetRequest {
            waiting: None,
            ..request
        };
        ticket.retargeting = Some(request.clone());
        self.save_ticket(&ticket)?;
        self.retarget_job(&ticket, &request).map(Some)
    }

    /// Records a finished job: the ticket's new base and its pull request's, or why nothing
    /// changed. Returns the repository whose pull requests changed, for the PR watcher.
    pub(crate) fn record_retarget(&mut self, outcome: Outcome) -> Result<Option<String>> {
        let mut ticket = self.ticket(&outcome.ticket_id)?;
        let Some(request) = (ticket.retargeting.clone())
            .filter(|r| r.id == outcome.request_id && r.waiting.is_none())
        else {
            return Ok(None);
        };
        let named = named(&ticket, &request);
        let result = match outcome.result {
            Moved::Protected(why) => {
                // The coordinator hears the outcome once the human answers.
                let asks = RetargetRequest {
                    asks: Some(why),
                    ..request
                };
                self.ask_human(&mut ticket, asks)?;
                return Ok(None);
            }
            Moved::Failed(why) => format!(
                "Changing the base of {named} to {} failed: {why}. Nothing changed.",
                request.base
            ),
            Moved::Done => {
                let repository = self.ticket_repository(&ticket)?;
                stacks::move_base(&mut ticket, &repository, &request.base);
                if let Some(pr) =
                    (ticket.pull_request.as_mut()).filter(|pr| Some(pr.number) == request.number)
                {
                    pr.base = request.base.clone();
                }
                let project = self.session(&ticket.coordinator_id)?.project_id;
                let detail = format!("{}: base {}", ticket.id, request.base);
                Self::event(&self.db, &project, None, "ticket_base", &detail)?;
                let moved = match request.number {
                    Some(number) => format!(" PR #{number} now opens into {}.", request.base),
                    None => String::new(),
                };
                format!(
                    "{named} is now on {}: reviews, sizes, overlap warnings, publishes and merge_ticket measure against it, and it is not stacked.{moved}",
                    request.base
                )
            }
        };
        self.end_retarget(&mut ticket, &result)?;
        Ok(Some(ticket.repository_id))
    }

    /// Ends the ticket's request: sends its caller the one result message and withdraws its
    /// Inbox item if one still waits. A request whose message cannot be queued is kept.
    fn end_retarget(&mut self, ticket: &mut Ticket, result: &str) -> Result<()> {
        let Some(request) = ticket.retargeting.clone() else {
            return Ok(());
        };
        // Sent before the request is cleared: after a crash in between, the repeated id sends
        // nothing more.
        let id = format!("{RETARGET_PREFIX}{}", request.id);
        if let Err(error) = self.send_publish_result(&id, &request.caller, result) {
            let told = self.message_exists(&id)?;
            let nobody_to_tell = self.session(&request.caller).is_ok_and(|s| s.archived);
            if !told && !nobody_to_tell {
                return Err(error.context(format!(
                    "The result of retarget request {} could not be queued, so the request is kept",
                    request.id
                )));
            }
        }
        ticket.retargeting = None;
        if let Some(item) = &request.waiting
            && self.is_unanswered(item)?
        {
            self.resolve_attention(item, "Withdrawn: this change of base ended")?;
        }
        self.save_ticket(ticket)
    }

    /// At start: ends each request whose job was running when the host stopped, or whose
    /// answered item was never acted on. The PR watcher follows a move that reached GitHub.
    pub(crate) fn end_interrupted_retargets(&mut self) -> Result<()> {
        for mut ticket in self.tickets()? {
            let Some(request) = ticket.retargeting.clone() else {
                continue;
            };
            let interrupted = match &request.waiting {
                Some(item) => !self.is_unanswered(item)?,
                None => true,
            };
            if interrupted {
                let notice = format!(
                    "Changing the base of {} to {} was interrupted when Wiffletree stopped. If the pull request moved on GitHub, the PR watcher makes that its base; otherwise call set_ticket_base again.",
                    named(&ticket, &request),
                    request.base
                );
                self.end_retarget(&mut ticket, &notice)?;
            }
        }
        Ok(())
    }
}

/// Such as `PR #180 for ticket "Space storage" (t1)`, or the ticket alone without a PR.
fn named(ticket: &Ticket, request: &RetargetRequest) -> String {
    let ticket_named = format!("ticket \"{}\" ({})", ticket.title, ticket.id);
    match request.number {
        Some(number) => format!("PR #{number} for {ticket_named}"),
        None => ticket_named,
    }
}

/// The Inbox item's text, written by the host from the stored request.
fn retarget_prompt(ticket: &Ticket, request: &RetargetRequest, why: &str) -> String {
    format!(
        "Move {} in {} from {} to {}?\nIt asks because {why}. The pull request opens into {} from then on; nothing is merged.",
        named(ticket, request),
        request.repository,
        request.from,
        request.base,
        request.base
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::{Fixture, Remote, fixture, git, remote, watch_open_pr};

    /// The fixture's ticket with its PR #45 open into main, and a `release` branch on GitHub.
    fn on_github() -> (Fixture, Remote) {
        let f = fixture();
        let r = remote(&f);
        git(&f.repository, &["branch", "release"]);
        let bare = r.bare.to_str().unwrap();
        git(&f.repository, &["push", "-q", bare, "release"]);
        git(&f.repository, &["branch", "-q", "-D", "release"]);
        (f, r)
    }
    fn set(f: &mut Fixture, ticket: &str, base: &str) -> Result<(Value, Option<Job>)> {
        let coordinator = f.coordinator.id.clone();
        f.host.set_ticket_base(&coordinator, ticket, base)
    }
    /// Runs the job and records it, as the service does.
    fn finish(f: &mut Fixture, r: &Remote, job: Option<Job>) {
        f.host
            .record_retarget(run(&job.unwrap(), &r.tools))
            .unwrap();
    }
    fn edits(r: &Remote) -> Vec<Vec<String>> {
        r.github.lock().unwrap().edited.clone()
    }
    fn results(f: &Fixture) -> Vec<String> {
        (f.host.db)
            .prepare("SELECT body FROM messages WHERE id LIKE 'retarget:%' ORDER BY sequence")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    fn moved_to(base: &str) -> Vec<String> {
        let args = ["pr", "edit", "45", "--repo", "github.com/allowed-org/app"];
        (args.into_iter().chain(["--base", base]))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn an_allowed_owners_pr_moves_to_an_unprotected_branch_without_asking() {
        let (mut f, r) = on_github();
        f.allow("allowed-org");
        watch_open_pr(&mut f, 45);
        let ticket = f.ticket.clone();
        let (status, job) = set(&mut f, &ticket, "release").unwrap();
        assert_eq!(status["status"], "setting_base");
        finish(&mut f, &r, job);
        assert_eq!(edits(&r), [moved_to("release")]);
        let moved = f.ticket();
        assert_eq!(moved.base.as_deref(), Some("release"));
        assert_eq!(moved.pull_request.as_ref().unwrap().base, "release");
        assert!(moved.retargeting.is_none());
        let base = f.host.ticket_base(&moved).unwrap();
        assert_eq!(base.measured(), "origin/release");
        // The base was fetched, so reviews can measure against it.
        git(&f.repository, &["rev-parse", "--verify", "origin/release"]);
        assert!(results(&f)[0].contains("PR #45 now opens into release"));
        let (status, job) = set(&mut f, &ticket, "release").unwrap();
        assert_eq!(status["status"], "unchanged");
        assert!(job.is_none());
    }

    #[test]
    fn a_branch_github_protects_since_the_daily_read_waits_for_the_human() {
        let (mut f, r) = on_github();
        f.allow("allowed-org");
        watch_open_pr(&mut f, 45);
        r.github.lock().unwrap().protected.push("release".into());
        let ticket = f.ticket.clone();
        let (status, job) = set(&mut f, &ticket, "release").unwrap();
        assert_eq!(
            status["status"], "setting_base",
            "the daily read says unprotected"
        );
        finish(&mut f, &r, job);
        assert!(edits(&r).is_empty(), "nothing was moved");
        assert!(
            results(&f).is_empty(),
            "the coordinator hears once the human answers"
        );
        let items = f.open_items();
        assert!(items[0].is_retarget());
        assert!(
            (items[0].prompt).contains("GitHub branch protection protects it"),
            "{}",
            items[0].prompt
        );
        // Approved, the move runs without reading again.
        let job = f.host.answer_retarget(&items[0].id, true).unwrap();
        finish(&mut f, &r, job);
        assert_eq!(edits(&r), [moved_to("release")]);
        assert_eq!(f.ticket().base.as_deref(), Some("release"));
    }

    #[test]
    fn moving_a_pr_into_a_protected_branch_waits_for_the_human() {
        let (mut f, r) = on_github();
        f.allow("allowed-org");
        watch_open_pr(&mut f, 45);
        let ticket = f.ticket.clone();
        let (_, job) = set(&mut f, &ticket, "release").unwrap();
        finish(&mut f, &r, job);
        // Back into main, the repository's base, which Wiffletree protects.
        let ask = |f: &mut Fixture| {
            let (status, job) = set(f, &ticket, "main").unwrap();
            assert_eq!(status["status"], "waiting_for_human");
            assert!(
                status["reason"]
                    .as_str()
                    .unwrap()
                    .contains("main is protected")
            );
            assert!(job.is_none());
            let items = f.open_items();
            assert!(items[0].is_retarget() && items[0].is_permission());
            assert!(items[0].prompt.contains("from release to main"));
            items[0].id.clone()
        };
        let item = ask(&mut f);
        let refused = set(&mut f, &ticket, "main").unwrap_err();
        assert!(format!("{refused:#}").contains("waits for the human"));
        assert!(f.host.answer_retarget(&item, false).unwrap().is_none());
        assert_eq!(f.ticket().base.as_deref(), Some("release"));
        assert!(results(&f).last().unwrap().contains("did not approve"));
        assert_eq!(edits(&r).len(), 1, "nothing was moved");

        let item = ask(&mut f);
        let job = f.host.answer_retarget(&item, true).unwrap();
        finish(&mut f, &r, job);
        assert_eq!(edits(&r).last().unwrap(), &moved_to("main"));
        let moved = f.ticket();
        assert_eq!(moved.base, None);
        assert_eq!(moved.pull_request.unwrap().base, "main");
    }

    #[test]
    fn a_ticket_without_a_pr_is_unstacked_without_asking_and_only_its_coordinator_moves_it() {
        let (mut f, r) = on_github();
        let repository = f.ticket().repository_id;
        let coordinator = f.coordinator.id.clone();
        let child = (f.host)
            .create_stacked_ticket(&coordinator, &repository, "Child", "Do", Some(&f.ticket))
            .unwrap();
        let implementer = f.implementer.id.clone();
        let refused = f.host.set_ticket_base(&implementer, &child.id, "release");
        assert!(format!("{:#}", refused.unwrap_err()).contains("Only the project coordinator"));
        // Nothing reaches GitHub, so a repository that asks before publishing does not ask.
        let (status, job) = set(&mut f, &child.id, "release").unwrap();
        assert_eq!(status["status"], "setting_base");
        finish(&mut f, &r, job);
        let moved = f.host.ticket(&child.id).unwrap();
        assert_eq!(
            (moved.stacked_on, moved.base.as_deref()),
            (None, Some("release"))
        );
        assert!(edits(&r).is_empty());

        // A branch that is not on origin changes nothing.
        let (_, job) = set(&mut f, &child.id, "missing").unwrap();
        finish(&mut f, &r, job);
        assert_eq!(
            f.host.ticket(&child.id).unwrap().base.as_deref(),
            Some("release")
        );
        assert!(results(&f).last().unwrap().contains("Nothing changed"));
    }

    #[test]
    fn a_pr_in_a_repository_that_asks_before_publishing_waits_for_the_human() {
        let (mut f, _) = on_github();
        watch_open_pr(&mut f, 45);
        let ticket = f.ticket.clone();
        let (status, job) = set(&mut f, &ticket, "release").unwrap();
        assert_eq!(status["status"], "waiting_for_human");
        assert!(
            status["reason"]
                .as_str()
                .unwrap()
                .contains("not an allowed owner")
        );
        assert!(job.is_none());
    }
}
