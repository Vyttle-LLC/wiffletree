//! Merging a ticket's pull request with `merge_ticket`: the host itself runs `gh pr merge` for
//! the ticket's coordinator, only into a branch that is not protected, after every check before
//! a merge, with GitHub read again at the time of the merge. A merge after a blocked review, or
//! over commits no review checked, waits for the human's approval. The merge accepts the ticket
//! and retargets the pull requests of the tickets stacked on it. Local checks run on the host
//! thread; GitHub is read, and merged, off it.
use crate::*;
use publish::Tools;

/// One decision about a merge request, with everything it needs off the host thread.
pub(crate) struct Job {
    pub ticket_id: String,
    pub request: MergeRequest,
    /// The store's database, read again on the job's own thread just before it merges.
    pub store: PathBuf,
    /// The repository as stored: its protected branches and what GitHub said in the daily read.
    pub repository: Repository,
    /// The open pull requests of the tickets stacked on this one, retargeted after the merge.
    pub children: Vec<Child>,
    /// Only reads whether the pull request merged, after a restart cut `gh pr merge` short.
    pub recover: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct Child {
    pub ticket_id: String,
    pub title: String,
    pub number: u64,
}

/// How a job ended, reported back to the host thread.
pub(crate) struct Outcome {
    pub ticket_id: String,
    pub request_id: String,
    /// The request's stage when the job started.
    pub stage: MergeStage,
    pub result: Decided,
    pub children: Vec<(Child, Retarget)>,
}
pub(crate) enum Decided {
    /// Why nothing was merged: a check failed.
    Refused(String),
    /// GitHub could not be read, so nothing was merged.
    Unread(String),
    /// GitHub reports another head than the request's.
    HeadMoved(String),
    /// Every check holds, but the checks are running or GitHub has not worked out whether the
    /// pull request can be merged.
    Waiting(MergeMethod),
    /// Every check holds, and the request waits for the human's approval before it merges.
    Ready(MergeMethod),
    /// GitHub merged it; the merge commit when it was read back.
    Merged {
        method: Option<MergeMethod>,
        commit: Option<String>,
    },
    /// `gh pr merge` was cut short, and the pull request is still open.
    Unfinished,
}
pub(crate) enum Retarget {
    Moved,
    /// GitHub had already moved it, as when the repository deletes merged branches.
    AlreadyMoved,
    Failed(String),
}

const PULL_REQUEST_FIELDS: &str =
    "state,baseRefName,headRefOid,isDraft,mergeable,mergeStateStatus,statusCheckRollup,mergeCommit";

/// The pull request as GitHub reports it now.
struct PullRequestNow {
    state: PrState,
    base: String,
    head: String,
    draft: bool,
    mergeable: String,
    merge_state: String,
    checks: CheckState,
    merge_commit: Option<String>,
}
/// What protects the base on GitHub now, and how it allows merging.
struct GitHubNow {
    default_branch: String,
    methods: Vec<MergeMethod>,
    /// Classic branch protection applies to the base.
    protected: bool,
    /// The ids of the active rulesets whose rules apply to the base.
    rulesets: Vec<u64>,
}

/// Runs one decision off the host thread: GitHub read again, and the merge when every check
/// holds and the request may merge without asking.
pub(crate) fn run(job: &Job, tools: &Tools) -> Outcome {
    let result = match job.recover {
        true => recover(job, tools),
        false => decide_and_merge(job, tools),
    };
    let children = match result {
        Decided::Merged { .. } => retarget(job, tools),
        _ => vec![],
    };
    Outcome {
        ticket_id: job.ticket_id.clone(),
        request_id: job.request.id.clone(),
        stage: job.request.stage.clone(),
        result,
        children,
    }
}

fn decide_and_merge(job: &Job, tools: &Tools) -> Decided {
    let read = read_pull_request(job, tools, job.request.number)
        .and_then(|pr| Ok((pr, read_github(job, tools)?)));
    let decided = match read {
        Ok((pr, github)) => decide(job, &pr, &github),
        Err(error) => Decided::Unread(format!(
            "GitHub could not be read, so nothing was merged: {error:#}"
        )),
    };
    // Only a `Merging` request sends, with the method the host thread recorded when an earlier
    // read found it ready, so a restart can tell a merge that may have been sent, and its method.
    let method = match (decided, job.request.stage == MergeStage::Merging) {
        (Decided::Ready(method), true) if job.request.method == Some(method) => method,
        (Decided::Ready(method), true) => {
            return Decided::Refused(format!(
                "GitHub's first allowed merge method is now {}, not {}. Nothing was merged",
                method.label(),
                job.request.method.map_or("none", MergeMethod::label)
            ));
        }
        (other, _) => return other,
    };
    if let Err(changed) = recheck_stored(job) {
        return Decided::Refused(format!("{changed:#}. Nothing was merged."));
    }
    merge(job, tools, method)
}

/// Where `gh` runs: the repository's own folder, which outlives the ticket's worktree, as a
/// recovery after acceptance needs.
fn gh_folder(job: &Job) -> &Path {
    Path::new(&job.repository.path)
}

fn gh_repository(job: &Job) -> String {
    format!("{}/{}", repositories::GITHUB_HOST, job.request.repository)
}

fn read_pull_request(job: &Job, tools: &Tools, number: u64) -> Result<PullRequestNow> {
    let number = number.to_string();
    let repository = gh_repository(job);
    let args = [
        "pr",
        "view",
        &number,
        "--repo",
        &repository,
        "--json",
        PULL_REQUEST_FIELDS,
    ];
    let answer = (tools.gh)(gh_folder(job), &args)?;
    ensure!(
        answer.status.success(),
        "gh pr view failed: {}",
        answer.error
    );
    let pr: Value = serde_json::from_str(&answer.output)?;
    let text = |field: &str| {
        pr[field]
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("GitHub's answer is missing {field}"))
    };
    Ok(PullRequestNow {
        state: match pr["state"].as_str() {
            Some("OPEN") => PrState::Open,
            Some("MERGED") => PrState::Merged,
            Some("CLOSED") => PrState::Closed,
            other => bail!("PR #{number} has the unknown state {other:?}"),
        },
        base: text("baseRefName")?,
        head: text("headRefOid")?,
        draft: pr["isDraft"].as_bool().unwrap_or(false),
        mergeable: text("mergeable")?,
        merge_state: text("mergeStateStatus")?,
        checks: rollup(
            pr["statusCheckRollup"]
                .as_array()
                .map_or(&[], Vec::as_slice),
        ),
        merge_commit: pr["mergeCommit"]["oid"].as_str().map(str::to_owned),
    })
}

/// The checks' state from `gh pr view`'s check runs and commit statuses: any failure fails, any
/// still running is pending, and none at all is `None`.
fn rollup(checks: &[Value]) -> CheckState {
    let mut state = match checks.is_empty() {
        true => CheckState::None,
        false => CheckState::Success,
    };
    for check in checks {
        let conclusion = check["conclusion"].as_str().unwrap_or_default();
        let status = check["state"].as_str().unwrap_or_default();
        let failed = matches!(
            conclusion,
            "FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "STALE"
        ) || matches!(status, "FAILURE" | "ERROR");
        if failed {
            return CheckState::Failure;
        }
        let running = check["status"].as_str().is_some_and(|s| s != "COMPLETED")
            || matches!(status, "PENDING" | "EXPECTED");
        if running {
            state = CheckState::Pending;
        }
    }
    state
}

/// The repository's default branch and merge methods, and the base's protection and rulesets,
/// read now: the merge never relies on the daily read.
fn read_github(job: &Job, tools: &Tools) -> Result<GitHubNow> {
    let read = |endpoint: &str| github_access::gh_json(tools, gh_folder(job), endpoint);
    let repository = read(&format!("repos/{}", job.request.repository))?;
    let methods = [
        ("allow_squash_merge", MergeMethod::Squash),
        ("allow_merge_commit", MergeMethod::Merge),
        ("allow_rebase_merge", MergeMethod::Rebase),
    ];
    let base = &job.request.base;
    let branch = read(&format!("repos/{}/branches/{base}", job.request.repository))?;
    let rules = read(&format!(
        "repos/{}/rules/branches/{base}",
        job.request.repository
    ))?;
    let rules = rules.as_array().context("GitHub's rules are not a list")?;
    let mut rulesets: Vec<u64> = rules
        .iter()
        .filter_map(|rule| rule["ruleset_id"].as_u64())
        .collect();
    rulesets.dedup();
    if rulesets.is_empty() && !rules.is_empty() {
        // A rule GitHub reports without its ruleset still protects the branch.
        rulesets.push(0);
    }
    Ok(GitHubNow {
        default_branch: repository["default_branch"]
            .as_str()
            .context("GitHub did not report the default branch")?
            .to_owned(),
        methods: (methods.into_iter())
            .filter(|(field, _)| repository[field].as_bool() == Some(true))
            .map(|(_, method)| method)
            .collect(),
        protected: branch["protected"]
            .as_bool()
            .context("GitHub did not say whether the base is protected")?,
        rulesets,
    })
}

/// The checks before a merge that need GitHub's answer, in order.
fn decide(job: &Job, pr: &PullRequestNow, github: &GitHubNow) -> Decided {
    let request = &job.request;
    let number = request.number;
    if pr.state != PrState::Open {
        let state = match pr.state {
            PrState::Merged => "merged",
            _ => "closed",
        };
        return Decided::Refused(format!(
            "PR #{number} is now {state} on GitHub, so Wiffletree merged nothing"
        ));
    }
    if pr.base != request.base {
        return Decided::Refused(format!(
            "PR #{number} now opens into {}, not {}, the ticket's base. Nothing was merged",
            pr.base, request.base
        ));
    }
    let stored = ProtectedBranches::of(&job.repository);
    let protection = (stored.with_default_branch(Some(&github.default_branch)))
        .why(&pr.base)
        .or_else(|| (github.protected).then(|| "GitHub branch protection protects it".to_owned()))
        .or_else(|| {
            let ids: Vec<String> = github.rulesets.iter().map(u64::to_string).collect();
            (!ids.is_empty())
                .then(|| format!("a GitHub ruleset protects it (id {})", ids.join(", ")))
        });
    if let Some(why) = protection {
        return Decided::Refused(format!(
            "{} is protected: {why}. Only the human merges into it; nothing was merged",
            pr.base
        ));
    }
    if pr.head != request.head {
        return Decided::HeadMoved(pr.head.clone());
    }
    if pr.draft {
        return Decided::Refused(format!("PR #{number} is a draft. Nothing was merged"));
    }
    if pr.mergeable == "CONFLICTING" || pr.merge_state == "DIRTY" {
        return Decided::Refused(format!(
            "PR #{number} conflicts with {}. Resolve the conflict, verify and publish again; nothing was merged",
            pr.base
        ));
    }
    if pr.merge_state == "BLOCKED" {
        return Decided::Refused(format!(
            "GitHub reports PR #{number} blocked from merging, such as by a required review. Nothing was merged"
        ));
    }
    if pr.checks == CheckState::Failure {
        return Decided::Refused(format!(
            "PR #{number}'s checks failed at {}. Nothing was merged",
            short_sha(&pr.head)
        ));
    }
    let Some(&method) = github.methods.iter().min() else {
        return Decided::Refused(format!(
            "GitHub allows no merge method in {}. Nothing was merged",
            request.repository
        ));
    };
    if pr.checks == CheckState::Pending || pr.mergeable == "UNKNOWN" || pr.merge_state == "UNKNOWN"
    {
        return Decided::Waiting(method);
    }
    Decided::Ready(method)
}

/// The local checks once more, on the job's own thread and just before `gh pr merge`: against
/// the ticket as the store has it now and its worktree as it is now.
fn recheck_stored(job: &Job) -> Result<()> {
    let store =
        Connection::open_with_flags(&job.store, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    store.busy_timeout(std::time::Duration::from_secs(2))?;
    let ticket: Ticket = store
        .query_row(
            "SELECT data FROM tickets WHERE id=?1",
            [&job.ticket_id],
            |r| decode(r, 0),
        )
        .context("Ticket not found")?;
    let running = ticket.merging.as_ref().is_some_and(|request| {
        request.id == job.request.id
            && request.stage == MergeStage::Merging
            && request.head == job.request.head
    });
    ensure!(running, "This merge request was withdrawn");
    ensure!(ticket.is_open(), "The ticket is now {}", ticket.state);
    ensure!(
        ticket.running_cycle().is_none()
            && ticket.verification.as_ref().map(|v| v.cycle) == Some(job.request.cycle),
        "A new verification cycle started"
    );
    let head = verification::head(&ticket)?;
    ensure!(
        head == job.request.head && ticket.pushed.as_deref() == Some(head.as_str()),
        "The worktree's HEAD moved to {}",
        short_sha(&head)
    );
    // The human may have protected the base or revoked the owner while GitHub was read.
    let repository: Repository = store
        .query_row(
            "SELECT data FROM repositories WHERE id=?1",
            [&ticket.repository_id],
            |r| decode(r, 0),
        )
        .context("Repository not found")?;
    let home = job.store.parent().context("No data folder")?;
    ensure!(
        is_allowed(&repository, &settings::allowed_owners(home)),
        "{} no longer publishes without asking",
        repository.name
    );
    if let Some(why) = ProtectedBranches::of(&repository).why(&job.request.base) {
        bail!("{} is now protected: {why}", job.request.base);
    }
    Ok(())
}

/// `gh pr merge`, pinned to the checked head. Never `--auto`, `--admin` or `--delete-branch`.
fn merge(job: &Job, tools: &Tools, method: MergeMethod) -> Decided {
    let number = job.request.number.to_string();
    let repository = gh_repository(job);
    let flag = match method {
        MergeMethod::Squash => "--squash",
        MergeMethod::Merge => "--merge",
        MergeMethod::Rebase => "--rebase",
    };
    let args = [
        "pr",
        "merge",
        &number,
        "--repo",
        &repository,
        flag,
        "--match-head-commit",
        &job.request.head,
    ];
    match (tools.gh)(gh_folder(job), &args) {
        Ok(answer) if answer.status.success() => Decided::Merged {
            method: Some(method),
            commit: merge_commit(job, tools),
        },
        Ok(answer) => Decided::Refused(format!(
            "GitHub refused the merge: {}. Nothing was merged",
            answer.error
        )),
        // `gh` was started and gave no answer: it was stopped at its deadline.
        Err(_) => match recover(job, tools) {
            Decided::Merged { commit, .. } => Decided::Merged {
                method: Some(method),
                commit,
            },
            other => other,
        },
    }
}

/// The merge commit GitHub reports for the merged pull request, read a few times since one
/// failed read would otherwise lose it; `None` when every read failed.
fn merge_commit(job: &Job, tools: &Tools) -> Option<String> {
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        if let Ok(pr) = read_pull_request(job, tools, job.request.number)
            && pr.merge_commit.is_some()
        {
            return pr.merge_commit;
        }
    }
    None
}

/// Whether a merge cut short reached GitHub: merged at the request's head finishes it, with the
/// method recorded before it was sent.
fn recover(job: &Job, tools: &Tools) -> Decided {
    let number = job.request.number;
    match read_pull_request(job, tools, number) {
        Ok(pr) if pr.state == PrState::Merged && pr.head == job.request.head => Decided::Merged {
            method: job.request.method,
            commit: pr.merge_commit,
        },
        Ok(pr) if pr.state == PrState::Open => Decided::Unfinished,
        Ok(pr) => Decided::Refused(format!(
            "PR #{number} is {} at {} on GitHub, so Wiffletree merged nothing",
            match pr.state {
                PrState::Merged => "merged",
                _ => "closed",
            },
            short_sha(&pr.head)
        )),
        Err(error) => Decided::Refused(format!(
            "Whether PR #{number} merged could not be read ({error:#}). The PR watcher reports a merge when it reads one"
        )),
    }
}

/// Moves each stacked child's open pull request from the merged branch to the merged pull
/// request's base.
fn retarget(job: &Job, tools: &Tools) -> Vec<(Child, Retarget)> {
    let repository = gh_repository(job);
    let (from, to) = (&job.request.branch, &job.request.base);
    let moved = |child: &Child| -> Result<Retarget> {
        let pr = read_pull_request(job, tools, child.number)?;
        ensure!(pr.state == PrState::Open, "it is no longer open");
        if pr.base == *to {
            return Ok(Retarget::AlreadyMoved);
        }
        ensure!(pr.base == *from, "it opens into {}", pr.base);
        let number = child.number.to_string();
        let args = ["pr", "edit", &number, "--repo", &repository, "--base", to];
        let answer = (tools.gh)(gh_folder(job), &args)?;
        ensure!(
            answer.status.success(),
            "gh pr edit failed: {}",
            answer.error
        );
        Ok(Retarget::Moved)
    };
    (job.children.iter())
        .map(|child| {
            let result = moved(child).unwrap_or_else(|e| Retarget::Failed(format!("{e:#}")));
            (child.clone(), result)
        })
        .collect()
}

/// What the checks before a merge found on this machine, short of GitHub's answer now.
struct Checked {
    repository: Repository,
    name: String,
    number: u64,
    base: String,
    head: String,
    cycle: u32,
    waived: Option<String>,
    needs_approval: bool,
}

/// Such as `PR #180 for ticket "Space storage" (t1)`.
fn named(ticket: &Ticket, request: &MergeRequest) -> String {
    format!(
        "PR #{} for ticket \"{}\" ({})",
        request.number, ticket.title, ticket.id
    )
}

impl Host {
    /// Accepts the coordinator's `merge_ticket` call once the checks this machine can make
    /// hold, records the request and returns the job that reads GitHub and decides it.
    pub(crate) fn merge_ticket(
        &mut self,
        caller: &str,
        ticket_id: &str,
        waived: Option<&str>,
    ) -> Result<Job> {
        let mut ticket = self.ticket(ticket_id)?;
        let coordinates = self.session(caller)?.role == Role::ProjectOrchestrator
            && ticket.coordinator_id == caller;
        ensure!(
            coordinates,
            "Only the project coordinator that owns the ticket can merge it; nothing was merged"
        );
        if let Some(request) = &ticket.merging {
            bail!(
                "A merge of PR #{} for this ticket {}; its result will arrive as a message from Wiffletree",
                request.number,
                match request.stage {
                    MergeStage::WaitingForChecks => "waits for its checks",
                    MergeStage::WaitingForHuman { .. } => "waits for the human's approval",
                    _ => "is running",
                }
            );
        }
        let waived = waived.map(one_line_reason).transpose()?;
        let checked = self.merge_checks(&ticket, waived)?;
        ticket.merging = Some(MergeRequest {
            id: new_id(),
            caller: caller.into(),
            repository: checked.name,
            number: checked.number,
            branch: ticket.branch.clone(),
            base: checked.base,
            head: checked.head,
            cycle: checked.cycle,
            waived: checked.waived,
            needs_approval: checked.needs_approval,
            approved: false,
            method: None,
            unreviewed: vec![],
            stage: MergeStage::Checking,
        });
        self.save_ticket(&ticket)?;
        self.merge_job(&ticket, &checked.repository)
    }

    /// The checks before a merge that need no GitHub read, in order. No answer from the human
    /// overrides them.
    fn merge_checks(&self, ticket: &Ticket, waived: Option<&str>) -> Result<Checked> {
        ensure!(ticket.is_open(), "Ticket is already {}", ticket.state);
        let pr = ticket.open_pull_request().context(
            "The ticket has no open pull request; publish it with publish_ticket first. Nothing was merged",
        )?;
        let blocked = (ticket.verification.as_ref())
            .is_some_and(|v| v.outcome == VerificationOutcome::Blocked);
        // After a blocked review without a waiver, the reason the merge records stands in, so
        // the request can reach the human.
        let waived = waived.or(blocked.then_some(APPROVED_WAIVER));
        let accepted = self.acceptance(ticket, waived)?;
        self.check_removable(std::slice::from_ref(ticket))?;
        let repository = self.ticket_repository(ticket)?;
        ensure!(
            is_allowed(&repository, &self.settings().allowed_owners),
            "{} does not publish without asking: {}. Only the human merges there; nothing was merged",
            repository.name,
            not_allowed_reason(&repository)
        );
        let name = (repository.github.as_ref())
            .and_then(|github| github.facts.as_ref())
            .map(|facts| facts.name_with_owner.clone())
            .context("GitHub has not answered for this repository")?;
        let base = self.ticket_base(ticket)?.pull_request_base()?;
        ensure!(
            pr.base == base,
            "PR #{} opens into {}, not {base}, the ticket's base. Nothing was merged",
            pr.number,
            pr.base
        );
        if let Some(why) = ProtectedBranches::of(&repository).why(&base) {
            bail!("{base} is protected: {why}. Only the human merges into it; nothing was merged");
        }
        let head = verification::head(ticket)?;
        ensure!(
            ticket.pushed.as_deref() == Some(head.as_str()),
            "The worktree's HEAD {} is not the commit Wiffletree last pushed ({}). Publish it with publish_ticket first; nothing was merged",
            short_sha(&head),
            ticket.pushed.as_deref().map_or("none", short_sha)
        );
        Ok(Checked {
            name,
            number: pr.number,
            base,
            head,
            cycle: ticket.verification.as_ref().map_or(0, |v| v.cycle),
            waived: accepted.waiver,
            needs_approval: blocked || !accepted.waived_commits.is_empty(),
            repository,
        })
    }

    /// The checks again for a request: they hold, on the review, pull request and head it was
    /// decided on.
    fn recheck_merge(&self, ticket: &Ticket, request: &MergeRequest) -> Result<Repository> {
        let checked = self.merge_checks(ticket, request.waived.as_deref())?;
        ensure!(
            checked.cycle == request.cycle,
            "the ticket's latest review is now cycle {}",
            checked.cycle
        );
        ensure!(
            checked.number == request.number && checked.base == request.base,
            "the ticket's pull request is now #{} into {}",
            checked.number,
            checked.base
        );
        ensure!(
            checked.head == request.head,
            "the worktree's HEAD moved from {} to {}",
            short_sha(&request.head),
            short_sha(&checked.head)
        );
        Ok(checked.repository)
    }

    fn merge_job(&self, ticket: &Ticket, repository: &Repository) -> Result<Job> {
        let request = ticket.merging.clone().context("No merge request")?;
        let children = match request.stage {
            MergeStage::Merging => self.stacked_children(ticket)?,
            _ => vec![],
        };
        Ok(Job {
            ticket_id: ticket.id.clone(),
            request,
            store: self.home.join(STORE_FILE),
            repository: repository.clone(),
            children,
            recover: false,
        })
    }

    /// The open tickets stacked on `ticket` whose pull request is open into its branch.
    fn stacked_children(&self, ticket: &Ticket) -> Result<Vec<Child>> {
        Ok((self.tickets()?.into_iter())
            .filter(|child| child.is_open() && child.stacked_on.as_ref() == Some(&ticket.id))
            .filter_map(|child| {
                let pr = child.open_pull_request()?;
                (pr.base == ticket.branch).then(|| Child {
                    number: pr.number,
                    ticket_id: child.id.clone(),
                    title: child.title.clone(),
                })
            })
            .collect())
    }

    /// Records a job's decision, and returns the job that sends the merge when a request that
    /// may merge is ready. With `answering`, the tool call waits for it, through that job too: a
    /// request that ends is its error and sends no message. Otherwise the request's caller gets
    /// one message when it ends.
    pub(crate) fn record_merge(
        &mut self,
        outcome: Outcome,
        answering: bool,
    ) -> Result<(Value, Option<Job>)> {
        if let Some(job) = self.start_merge(&outcome)? {
            return Ok((json!({"status": "merging"}), Some(job)));
        }
        self.record_decision(outcome, answering)
            .map(|status| (status, None))
    }

    /// For a request that may merge whose read found every check holding: records the method
    /// and that a merge may now be sent, so a restart finishes or ends it on GitHub's word, and
    /// returns the job that checks GitHub once more and sends it.
    fn start_merge(&mut self, outcome: &Outcome) -> Result<Option<Job>> {
        let Decided::Ready(method) = outcome.result else {
            return Ok(None);
        };
        let mut ticket = self.ticket(&outcome.ticket_id)?;
        let ready = (ticket.merging.clone()).filter(|request| {
            // A read made at another stage, such as a re-read while the human decided, is stale.
            request.id == outcome.request_id
                && request.stage == outcome.stage
                && matches!(request.stage, MergeStage::Checking | MergeStage::Rechecking)
                && request.may_merge()
                && ticket.is_open()
                && ticket.verification.as_ref().map(|v| v.cycle) == Some(request.cycle)
        });
        let Some(request) = ready else {
            return Ok(None);
        };
        ticket.merging = Some(MergeRequest {
            method: Some(method),
            stage: MergeStage::Merging,
            ..request
        });
        self.save_ticket(&ticket)?;
        let repository = self.ticket_repository(&ticket)?;
        self.merge_job(&ticket, &repository).map(Some)
    }

    fn record_decision(&mut self, outcome: Outcome, answering: bool) -> Result<Value> {
        let mut ticket = self.ticket(&outcome.ticket_id)?;
        let current = (ticket.merging.clone()).filter(|request| request.id == outcome.request_id);
        let Some(mut request) = current else {
            return Ok(json!({"status": "ended"}));
        };
        // A re-read while the human decides only ends the request on what would refuse it. Once
        // the human answered, that answer's own reading of GitHub decides instead.
        if let MergeStage::WaitingForHuman { .. } = outcome.stage {
            let still_waiting = request.stage == outcome.stage;
            let stands = matches!(
                outcome.result,
                Decided::Waiting(_) | Decided::Ready(_) | Decided::Unread(_)
            );
            if !still_waiting || stands {
                return Ok(json!({"status": "unchanged"}));
            }
        }
        let ended = match outcome.result {
            Decided::Merged { method, commit } => {
                return self.merged(ticket, request, method, commit, outcome.children, answering);
            }
            _ if !ticket.is_open() => format!("the ticket is now {}", ticket.state),
            _ if ticket.verification.as_ref().map(|v| v.cycle) != Some(request.cycle) => {
                "a new verification cycle started".into()
            }
            Decided::HeadMoved(head) => match self.follow_head(&ticket, &request, &head) {
                Ok(()) => {
                    request.head = head;
                    return self.wait_for_checks(&mut ticket, request);
                }
                Err(why) => why,
            },
            Decided::Waiting(method) | Decided::Ready(method) if !request.may_merge() => {
                return self.ask_merge(&mut ticket, request, method);
            }
            Decided::Waiting(_) | Decided::Ready(_) => {
                return self.wait_for_checks(&mut ticket, request);
            }
            Decided::Refused(why) | Decided::Unread(why) => why,
            // A re-decision cut short by a restart sent no merge: the next pass decides it anew.
            Decided::Unfinished if request.stage == MergeStage::Rechecking => {
                return self.wait_for_checks(&mut ticket, request);
            }
            Decided::Unfinished => format!(
                "gh pr merge was cut short, by its time limit or a restart, and GitHub reports PR #{} still open. Call merge_ticket again",
                request.number
            ),
        };
        let ended = format!("{} was not merged: {ended}.", named(&ticket, &request));
        if answering {
            self.drop_merge(&mut ticket)?;
            bail!("{ended}");
        }
        self.end_merge(&mut ticket, &ended)?;
        Ok(json!({"status": "ended"}))
    }

    /// Whether a request may follow the pull request's new `head`: only a request that needs no
    /// approval, to a head the host itself pushed with the same change. Otherwise why it ends.
    fn follow_head(
        &self,
        ticket: &Ticket,
        request: &MergeRequest,
        head: &str,
    ) -> std::result::Result<(), String> {
        let moved = format!(
            "its head moved from {} to {}",
            short_sha(&request.head),
            short_sha(head)
        );
        if request.needs_approval {
            return Err(format!(
                "{moved}, and the human's approval covers {} only. Call merge_ticket again for the new head",
                short_sha(&request.head)
            ));
        }
        if ticket.pushed.as_deref() != Some(head) {
            return Err(format!(
                "{moved}, which Wiffletree did not push. Verify it and call merge_ticket again"
            ));
        }
        let followed = MergeRequest {
            head: head.into(),
            ..request.clone()
        };
        (self.recheck_merge(ticket, &followed))
            .map(drop)
            .map_err(|why| format!("{moved}, and {why:#}"))
    }

    fn wait_for_checks(&mut self, ticket: &mut Ticket, request: MergeRequest) -> Result<Value> {
        let status = json!({"status": "waiting_for_checks", "pull_request": request.number,
            "base": request.base, "head": request.head,
            "next": "Finish your turn. Wiffletree merges once GitHub's checks pass and it can be merged, and messages you the result."});
        ticket.merging = Some(MergeRequest {
            stage: MergeStage::WaitingForChecks,
            ..request
        });
        self.save_ticket(ticket)?;
        Ok(status)
    }

    /// Raises the request's Inbox item: every check holds, and the human approves this head.
    fn ask_merge(
        &mut self,
        ticket: &mut Ticket,
        request: MergeRequest,
        method: MergeMethod,
    ) -> Result<Value> {
        let repository = self.ticket_repository(ticket)?;
        let accepted = self.acceptance(ticket, request.waived.as_deref())?;
        let request = MergeRequest {
            method: Some(method),
            unreviewed: accepted.waived_commits,
            ..request
        };
        let prompt = approval_prompt(ticket, &request, &repository);
        let item = self.request_attention(
            &request.caller,
            "local",
            &format!("{MERGE_PREFIX}{}", request.id),
            &prompt,
            &[],
        )?;
        let status = json!({"status": "waiting_for_human", "pull_request": request.number,
            "base": request.base, "head": request.head, "method": method.label(),
            "next": "Finish your turn. Wiffletree merges if the human approves, and messages you the result."});
        ticket.merging = Some(MergeRequest {
            stage: MergeStage::WaitingForHuman { item: item.id },
            ..request
        });
        self.save_ticket(ticket)?;
        Ok(status)
    }

    /// Records the merge the host made, accepts the ticket and tells the caller. The request is
    /// kept until its message is queued, so a restart in between finishes it again, and while
    /// GitHub has not reported the merge commit, so the next start reads it again. Acceptance and
    /// the result's stable message id make repeating a finished step harmless.
    fn merged(
        &mut self,
        mut ticket: Ticket,
        request: MergeRequest,
        method: Option<MergeMethod>,
        commit: Option<String>,
        children: Vec<(Child, Retarget)>,
        answering: bool,
    ) -> Result<Value> {
        let named = named(&ticket, &request);
        // A restart re-enters here; each step skips what an earlier entry already did.
        let mut late_commit = None;
        let pr = (ticket.pull_request.as_mut()).filter(|pr| pr.number == request.number);
        if let Some(pr) = pr.filter(|pr| pr.merged_by_host.is_some()) {
            // Recorded before, but perhaps without the merge commit this read found.
            if pr.merge_commit.is_none() && commit.is_some() {
                pr.merge_commit = commit.clone();
                late_commit = commit.clone();
                self.save_ticket(&ticket)?;
            }
        } else if let Some(pr) =
            (ticket.pull_request.as_mut()).filter(|pr| pr.number == request.number)
        {
            pr.state = PrState::Merged;
            pr.head = request.head.clone();
            pr.merge_commit = commit.clone();
            pr.merged_by_host = Some(HostMerge {
                method,
                head: request.head.clone(),
                approved: request.approved,
            });
            self.save_ticket(&ticket)?;
            let project = self.session(&ticket.coordinator_id)?.project_id;
            let detail = format!(
                "{}: PR #{} into {} at {}",
                ticket.id,
                request.number,
                request.base,
                short_sha(&request.head)
            );
            Self::event(
                &self.db,
                &project,
                Some(&request.caller),
                "pull_request_merged",
                &detail,
            )?;
        }
        let commit = (ticket.pull_request.as_ref())
            .filter(|pr| pr.number == request.number)
            .and_then(|pr| pr.merge_commit.clone())
            .or(commit);
        let accepted = self.accept_ticket(&ticket.id, request.waived.as_deref());
        let stacked = self.record_retargets(&request, &children)?;
        let method_words = method.map_or(String::new(), |m| format!(" with {}", m.label()));
        let commit_words = commit
            .as_deref()
            .map_or(String::new(), |c| format!(" as {}", short_sha(c)));
        let mut result = format!(
            "Merged {named} into {}{method_words} at {}{commit_words}.",
            request.base,
            short_sha(&request.head)
        );
        if request.approved {
            result.push_str(" The human approved the merge.");
        }
        match &accepted {
            Ok(_) => result.push_str(" The ticket was accepted."),
            Err(why) => result.push_str(&format!(
                " The ticket was not accepted: {why:#}. Fix that, then accept it with accept_ticket."
            )),
        }
        for line in &stacked {
            result.push(' ');
            result.push_str(line);
        }
        if commit.is_none() {
            result.push_str(
                " GitHub did not report the merge commit; Wiffletree reads it again when it next starts.",
            );
        }
        let status = json!({"status": "merged", "pull_request": request.number,
            "base": request.base, "method": method.map(MergeMethod::label),
            "head": request.head, "merge_commit": commit, "accepted": accepted.is_ok(),
            "result": result});
        // The result already went out without the commit: one follow-up names it.
        if let Some(late) = late_commit
            && self.message_exists(&format!("{MERGE_PREFIX}{}", request.id))?
        {
            let notice = format!(
                "{named} merged into {} as {}; GitHub had not reported its merge commit before.",
                request.base,
                short_sha(&late)
            );
            let id = format!("{MERGE_PREFIX}{}:commit", request.id);
            if let Err(error) = self.send_publish_result(&id, &request.caller, &notice)
                && !self.message_exists(&id)?
            {
                return Err(error);
            }
        }
        let mut ticket = self.ticket(&ticket.id)?;
        match (answering, commit.is_some()) {
            (true, true) => self.drop_merge(&mut ticket)?,
            (false, true) => self.end_merge(&mut ticket, &result)?,
            // Kept, as `Merging`, for the next start's read of the merge commit.
            (true, false) => {}
            (false, false) => {
                let id = format!("{MERGE_PREFIX}{}", request.id);
                if let Err(error) = self.send_publish_result(&id, &request.caller, &result)
                    && !self.message_exists(&id)?
                {
                    return Err(error);
                }
            }
        }
        Ok(status)
    }

    /// Records each retargeted child's new base, and says for each what happened and what
    /// happens next.
    fn record_retargets(
        &mut self,
        request: &MergeRequest,
        children: &[(Child, Retarget)],
    ) -> Result<Vec<String>> {
        let mut lines = vec![];
        for (child, retarget) in children {
            let pr = format!(
                "PR #{} of ticket \"{}\" ({})",
                child.number, child.title, child.ticket_id
            );
            let moved = match retarget {
                Retarget::Moved => format!("{pr} now opens into {}", request.base),
                Retarget::AlreadyMoved => {
                    format!("GitHub had already moved {pr} to {}", request.base)
                }
                Retarget::Failed(why) => {
                    lines.push(format!(
                        "{pr} was not retargeted: {why}. It still opens into {}; ask the human to change its base to {}.",
                        request.branch, request.base
                    ));
                    continue;
                }
            };
            let mut ticket = self.ticket(&child.ticket_id)?;
            if let Some(pr) = (ticket.pull_request.as_mut()).filter(|pr| pr.number == child.number)
            {
                pr.base = request.base.clone();
                self.save_ticket(&ticket)?;
            }
            let repository = self.ticket_repository(&ticket)?;
            let settings = self.settings();
            let kept = settings.keeper.rebase && is_allowed(&repository, &settings.allowed_owners);
            lines.push(match kept {
                true => format!("{moved}; the PR keeper will rebase it onto {}.", request.base),
                false => format!(
                    "{moved}; it needs a rebase onto {} of only its own commits, which the PR keeper does not do here.",
                    request.base
                ),
            });
        }
        Ok(lines)
    }

    /// Acts on the human's answer to a merge item, and only for the item a ticket's waiting
    /// request names: the ticket and head come from that stored request. Approval checks
    /// everything again and returns the job that merges; any other answer ends the request.
    pub(crate) fn answer_merge(&mut self, item: &str, approved: bool) -> Result<Option<Job>> {
        let waiting = self.tickets()?.into_iter().find(|ticket| {
            (ticket.merging.as_ref()).is_some_and(|request| {
                request.stage
                    == MergeStage::WaitingForHuman {
                        item: item.to_owned(),
                    }
            })
        });
        let Some(mut ticket) = waiting else {
            return Ok(None);
        };
        let request = ticket.merging.clone().context("No merge request")?;
        let named = named(&ticket, &request);
        if !approved {
            let denied = format!(
                "The human denied merging {named}. Nothing was merged. Do not merge it another way."
            );
            self.end_merge(&mut ticket, &denied)?;
            return Ok(None);
        }
        let repository = match self.recheck_merge(&ticket, &request) {
            Ok(repository) => repository,
            Err(changed) => {
                let notice = format!(
                    "{named} was not merged although the human approved it: {changed:#}. Verify it and call merge_ticket again."
                );
                self.end_merge(&mut ticket, &notice)?;
                return Ok(None);
            }
        };
        ticket.merging = Some(MergeRequest {
            approved: true,
            stage: MergeStage::Checking,
            ..request
        });
        self.save_ticket(&ticket)?;
        self.merge_job(&ticket, &repository).map(Some)
    }

    /// Decides again each waiting request of the repository after a PR watcher pass, and
    /// returns the jobs that read GitHub for those still standing. A request waiting for the
    /// human is read only when `read_waiting_for_human`, once per pass that left GitHub's rate
    /// limit above the watcher's floor: the read sends no merge and starts no pass.
    pub(crate) fn merges_due(
        &mut self,
        repository_id: &str,
        read_waiting_for_human: bool,
    ) -> Result<Vec<Job>> {
        let mut jobs = vec![];
        for mut ticket in self.tickets()? {
            let Some(mut request) = ticket.merging.clone() else {
                continue;
            };
            let waiting = matches!(
                request.stage,
                MergeStage::WaitingForChecks | MergeStage::WaitingForHuman { .. }
            );
            if ticket.repository_id != repository_id || !ticket.is_open() || !waiting {
                continue;
            }
            let named = named(&ticket, &request);
            let pr = (ticket.open_pull_request())
                .filter(|pr| pr.number == request.number)
                .cloned();
            let Some(pr) = pr else {
                let ended = format!("{named} was not merged: it is no longer open.");
                self.end_merge(&mut ticket, &ended)?;
                continue;
            };
            if pr.head != request.head {
                if let Err(why) = self.follow_head(&ticket, &request, &pr.head) {
                    self.end_merge(&mut ticket, &format!("{named} was not merged: {why}."))?;
                    continue;
                }
                request.head = pr.head.clone();
            }
            let repository = match self.recheck_merge(&ticket, &request) {
                Ok(repository) => repository,
                Err(why) => {
                    let ended = format!("{named} was not merged: {why:#}.");
                    self.end_merge(&mut ticket, &ended)?;
                    continue;
                }
            };
            if !request.may_merge() {
                // It waits for the human, unless the pass found what would refuse it anyway, or
                // GitHub, read again, does; GitHub is read in full once more on approval.
                let refused = match (pr.checks, pr.merge_state) {
                    (CheckState::Failure, _) => Some("its checks failed"),
                    (_, MergeState::Dirty) => Some("it conflicts with its base"),
                    _ => None,
                };
                if let Some(why) = refused {
                    self.end_merge(&mut ticket, &format!("{named} was not merged: {why}."))?;
                } else if read_waiting_for_human {
                    jobs.push(self.merge_job(&ticket, &repository)?);
                }
                continue;
            }
            ticket.merging = Some(MergeRequest {
                stage: MergeStage::Rechecking,
                ..request
            });
            self.save_ticket(&ticket)?;
            jobs.push(self.merge_job(&ticket, &repository)?);
        }
        Ok(jobs)
    }

    /// Withdraws the ticket's waiting merge request, if it has one, telling its caller. A job
    /// already running checks the stored request before it merges.
    pub(crate) fn withdraw_merge(&mut self, ticket: &mut Ticket, because: &str) -> Result<()> {
        let Some(request) = ticket.merging.clone() else {
            return Ok(());
        };
        if !matches!(
            request.stage,
            MergeStage::WaitingForChecks | MergeStage::WaitingForHuman { .. }
        ) {
            return Ok(());
        }
        let withdrawn = format!(
            "The merge of {} was withdrawn, because {because}. Nothing was merged.",
            named(ticket, &request)
        );
        self.end_merge(ticket, &withdrawn)
    }

    /// Ends the ticket's request: sends its caller the request's one message and withdraws its
    /// Inbox item if one still waits. A request whose message cannot be queued is kept.
    fn end_merge(&mut self, ticket: &mut Ticket, result: &str) -> Result<()> {
        let Some(request) = ticket.merging.clone() else {
            return Ok(());
        };
        // Sent before the request is cleared: after a crash in between, the repeated id sends
        // nothing more.
        let id = format!("{MERGE_PREFIX}{}", request.id);
        let result: String = result.chars().take(MAX_TEXT_BYTES / 4).collect();
        if let Err(error) = self.send_publish_result(&id, &request.caller, &result) {
            let told = self.message_exists(&id)?;
            let nobody_to_tell = self.session(&request.caller).is_ok_and(|s| s.archived);
            if !told && !nobody_to_tell {
                return Err(error.context(format!(
                    "The result of merge request {} could not be queued, so the request is kept",
                    request.id
                )));
            }
        }
        self.drop_merge(ticket)
    }

    /// Clears the ticket's request with no message, withdrawing its Inbox item if one waits.
    fn drop_merge(&mut self, ticket: &mut Ticket) -> Result<()> {
        if let Some(MergeRequest {
            stage: MergeStage::WaitingForHuman { item },
            ..
        }) = ticket.merging.take()
            && self.is_unanswered(&item)?
        {
            self.resolve_attention(&item, "Withdrawn: this merge request ended")?;
        }
        self.save_ticket(ticket)
    }

    /// At start: ends each request whose GitHub read was running when the host stopped, which
    /// merged nothing, or whose answered item was never acted on. A request that may have sent
    /// `gh pr merge` is left for `interrupted_merges`.
    pub(crate) fn end_interrupted_merges(&mut self) -> Result<()> {
        for mut ticket in self.tickets()? {
            let Some(request) = ticket.merging.clone() else {
                continue;
            };
            let interrupted = match &request.stage {
                MergeStage::Checking => true,
                MergeStage::WaitingForHuman { item } => !self.is_unanswered(item)?,
                MergeStage::Merging | MergeStage::Rechecking | MergeStage::WaitingForChecks => {
                    false
                }
            };
            if interrupted {
                let notice = format!(
                    "Deciding the merge of {} was interrupted when Wiffletree stopped. Nothing was merged. Call merge_ticket again.",
                    named(&ticket, &request)
                );
                self.end_merge(&mut ticket, &notice)?;
            }
        }
        Ok(())
    }

    /// The jobs that read whether each merge running when the host stopped reached GitHub.
    pub(crate) fn interrupted_merges(&self) -> Result<Vec<Job>> {
        let mut jobs = vec![];
        for ticket in self.tickets()? {
            let running =
                |r: &MergeRequest| matches!(r.stage, MergeStage::Merging | MergeStage::Rechecking);
            if ticket.merging.as_ref().is_some_and(running) {
                let repository = self.ticket_repository(&ticket)?;
                jobs.push(Job {
                    recover: true,
                    ..self.merge_job(&ticket, &repository)?
                });
            }
        }
        Ok(jobs)
    }
}

/// The Inbox item's text, written by the host from the stored request; the desktop draws the
/// same card.
fn approval_prompt(ticket: &Ticket, request: &MergeRequest, repository: &Repository) -> String {
    let card = merge_card(ticket, request, repository);
    let mut prompt = format!("{}\n{}", card.text(), card.note);
    if prompt.len() > 4000 {
        let end = (0..=4000).rev().find(|&i| prompt.is_char_boundary(i));
        prompt.truncate(end.unwrap_or_default());
        prompt.push('…');
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::{Fixture, exit, fixture, git, restart, watch_open_pr};
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    const BASE: &str = "feature/spaces";
    const MERGE_COMMIT: &str = "6e1f0c2d9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d";

    /// Stands in for GitHub: its pull requests, its repository settings and protection, and
    /// what was asked of it.
    struct GitHub {
        prs: BTreeMap<u64, Value>,
        repository: Value,
        /// Branches classic branch protection covers.
        protected: Vec<String>,
        /// The rules GitHub reports for each branch.
        rules: BTreeMap<String, Value>,
        unreadable: bool,
        merge_error: Option<String>,
        /// `gh pr merge` merges and is then stopped at its deadline.
        stalls: bool,
        /// `gh pr merge` is stopped at its deadline before GitHub merged.
        times_out: bool,
        /// How many reads of a merged pull request fail before one succeeds.
        failing_reads: usize,
        merges: Vec<Vec<String>>,
        edits: Vec<Vec<String>>,
    }
    struct Fake {
        github: Arc<Mutex<GitHub>>,
        tools: Tools,
    }
    impl Fake {
        fn pr(&self, number: u64) -> Value {
            self.github.lock().unwrap().prs[&number].clone()
        }
        fn set(&self, number: u64, field: &str, value: Value) {
            self.github.lock().unwrap().prs.get_mut(&number).unwrap()[field] = value;
        }
        fn merges(&self) -> Vec<Vec<String>> {
            self.github.lock().unwrap().merges.clone()
        }
    }
    fn pr_json(head: &str, base: &str) -> Value {
        json!({"state": "OPEN", "baseRefName": base, "headRefOid": head, "isDraft": false,
            "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN", "statusCheckRollup": [],
            "mergeCommit": null})
    }
    fn fake(f: &Fixture) -> Fake {
        let github = Arc::new(Mutex::new(GitHub {
            prs: BTreeMap::from([(180, pr_json(&f.head(), BASE))]),
            repository: json!({"default_branch": "main", "allow_squash_merge": true,
                "allow_merge_commit": true, "allow_rebase_merge": false}),
            protected: vec!["main".into()],
            rules: BTreeMap::new(),
            unreadable: false,
            merge_error: None,
            stalls: false,
            times_out: false,
            failing_reads: 0,
            merges: vec![],
            edits: vec![],
        }));
        let state = github.clone();
        let gh = move |folder: &Path, args: &[&str]| -> Result<runtime::GhAnswer> {
            // As the real `gh`, which cannot start in a folder that is gone.
            ensure!(folder.is_dir(), "gh could not run in {}", folder.display());
            let mut gh = state.lock().unwrap();
            let ok = |output: String| {
                Ok(runtime::GhAnswer {
                    status: exit(0),
                    output,
                    error: String::new(),
                })
            };
            let fail = |error: &str| {
                Ok(runtime::GhAnswer {
                    status: exit(1),
                    output: String::new(),
                    error: error.into(),
                })
            };
            let owned: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            assert!(
                !args
                    .iter()
                    .any(|a| matches!(*a, "--auto" | "--admin" | "--delete-branch")),
                "{args:?}"
            );
            match args {
                ["api", "--hostname", "github.com", endpoint] => {
                    if gh.unreadable {
                        return fail("HTTP 502: Bad Gateway");
                    }
                    let endpoint = endpoint.strip_prefix("repos/allowed-org/app").unwrap();
                    if endpoint.is_empty() {
                        return ok(gh.repository.to_string());
                    }
                    if let Some(branch) = endpoint.strip_prefix("/rules/branches/") {
                        return ok(gh
                            .rules
                            .get(branch)
                            .cloned()
                            .unwrap_or(json!([]))
                            .to_string());
                    }
                    let branch = endpoint.strip_prefix("/branches/").unwrap();
                    let protected = gh.protected.iter().any(|p| p == branch);
                    ok(json!({"name": branch, "protected": protected}).to_string())
                }
                [
                    "pr",
                    "view",
                    number,
                    "--repo",
                    "github.com/allowed-org/app",
                    "--json",
                    _,
                ] => {
                    let pr = gh.prs[&number.parse().unwrap()].clone();
                    if pr["state"] == "MERGED" && gh.failing_reads > 0 {
                        gh.failing_reads -= 1;
                        return fail("HTTP 502: Bad Gateway");
                    }
                    ok(pr.to_string())
                }
                [
                    "pr",
                    "merge",
                    number,
                    "--repo",
                    "github.com/allowed-org/app",
                    _,
                    "--match-head-commit",
                    head,
                ] => {
                    gh.merges.push(owned);
                    if let Some(error) = gh.merge_error.clone() {
                        return fail(&error);
                    }
                    if gh.times_out {
                        bail!("gh was stopped at its deadline");
                    }
                    let stalls = gh.stalls;
                    let pr = gh.prs.get_mut(&number.parse().unwrap()).unwrap();
                    if pr["headRefOid"] != *head {
                        return fail("Head branch was modified. Review and try the merge again.");
                    }
                    pr["state"] = json!("MERGED");
                    pr["mergeCommit"] = json!({"oid": MERGE_COMMIT});
                    if stalls {
                        bail!("gh was stopped at its deadline");
                    }
                    ok(String::new())
                }
                [
                    "pr",
                    "edit",
                    number,
                    "--repo",
                    "github.com/allowed-org/app",
                    "--base",
                    base,
                ] => {
                    gh.edits.push(owned);
                    let pr = gh.prs.get_mut(&number.parse().unwrap()).unwrap();
                    pr["baseRefName"] = json!(base);
                    ok(String::new())
                }
                _ => panic!("the host runs no other gh command: {args:?}"),
            }
        };
        Fake {
            github,
            tools: Tools {
                gh: Arc::new(gh),
                git: Tools::default().git,
            },
        }
    }

    /// A passed ticket on `feature/spaces`, pushed at its head, whose PR #180 is open into it
    /// with no checks, in an allowed owner's repository. GitHub allows squash and merge
    /// commits and protects only `main`.
    fn ready() -> (Fixture, Fake) {
        let mut f = fixture();
        f.allow("allowed-org");
        git(
            &f.repository,
            &["update-ref", "refs/remotes/origin/feature/spaces", "HEAD"],
        );
        let mut ticket = f.ticket();
        ticket.base = Some(BASE.into());
        ticket.pushed = Some(f.head());
        f.host.save_ticket(&ticket).unwrap();
        watch(&mut f, 180, BASE);
        let fake = fake(&f);
        (f, fake)
    }
    /// Records the ticket's PR open into `base` at the worktree's HEAD, as the watcher would.
    fn watch(f: &mut Fixture, number: u64, base: &str) {
        let mut pr = watch_open_pr(f, number);
        pr.base = base.into();
        let mut ticket = f.ticket();
        ticket.pull_request = Some(pr);
        f.host.save_ticket(&ticket).unwrap();
    }
    fn coordinator(f: &Fixture) -> String {
        f.coordinator.id.clone()
    }
    /// Calls `merge_ticket` as `caller` and records the first decision as the tool's answer.
    fn merge_as(f: &mut Fixture, fake: &Fake, caller: &str, waived: Option<&str>) -> Result<Value> {
        let job = f.host.merge_ticket(caller, &f.ticket.clone(), waived)?;
        settle(f, fake, &job, true)
    }
    fn merge(f: &mut Fixture, fake: &Fake, waived: Option<&str>) -> Result<Value> {
        merge_as(f, fake, &coordinator(f), waived)
    }
    /// Runs `job` and records it, then each merge job that follows, as the service does.
    fn settle(f: &mut Fixture, fake: &Fake, job: &Job, answering: bool) -> Result<Value> {
        let (mut status, mut next) = f.host.record_merge(run(job, &fake.tools), answering)?;
        while let Some(job) = next {
            (status, next) = f.host.record_merge(run(&job, &fake.tools), answering)?;
        }
        Ok(status)
    }
    /// Calls `merge_ticket` and records its first read, which finds it ready: the request is now
    /// `Merging` with its method, and the returned job sends the merge.
    fn ready_to_send(f: &mut Fixture, fake: &Fake) -> Job {
        let job = (f.host)
            .merge_ticket(&coordinator(f), &f.ticket.clone(), None)
            .unwrap();
        let (_, sending) = f.host.record_merge(run(&job, &fake.tools), true).unwrap();
        assert_eq!(f.ticket().merging.unwrap().stage, MergeStage::Merging);
        sending.unwrap()
    }
    /// Runs the decisions due after a PR watcher pass with rate limit to spare, as the service
    /// does.
    fn pass(f: &mut Fixture, fake: &Fake) {
        let repository = f.ticket().repository_id;
        for job in f.host.merges_due(&repository, true).unwrap() {
            settle(f, fake, &job, false).unwrap();
        }
    }
    /// Every merge message sent so far.
    fn messages(host: &Host) -> Vec<String> {
        host.db
            .prepare("SELECT body FROM messages WHERE id LIKE 'merge:%' ORDER BY sequence")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
    /// The call is refused for `reason`, and nothing is merged or left waiting.
    fn refused(f: &Fixture, fake: &Fake, result: Result<Value>, reason: &str) {
        let error = format!("{:#}", result.expect_err("the merge was not refused"));
        assert!(error.contains(reason), "{error}");
        assert!(fake.merges().is_empty(), "{:?}", fake.merges());
        assert!(f.ticket().merging.is_none());
        assert!(f.ticket().is_open());
        assert!(f.open_items().is_empty());
    }
    fn finding(id: &str, status: EntryStatus) -> LedgerEntry {
        LedgerEntry {
            id: id.into(),
            finding: serde_json::from_value(json!({"severity": "non_blocking",
                "location": "src/a.rs:4", "summary": "Contrast is low"}))
            .unwrap(),
            focus: "Correctness".into(),
            cycle: 1,
            round: 1,
            status,
            reason: None,
        }
    }
    /// What GitHub said about the repository's branches in the daily read.
    fn daily_read(f: &mut Fixture, branches: GitHubBranches) {
        let repository = f.host.ticket_repository(&f.ticket()).unwrap();
        let mut github = repository.github.unwrap();
        github.facts.as_mut().unwrap().branches = Some(branches);
        let read = repositories::GitHubRead {
            id: repository.id,
            github: Some(github),
            origin_problem: None,
        };
        f.host
            .record_github(repositories::GitHubReading {
                repositories: vec![read],
                error: None,
            })
            .unwrap();
    }

    #[test]
    fn only_the_tickets_project_coordinator_merges_it() {
        let (mut f, fake) = ready();
        let ticket = f.ticket.clone();
        let reviewer = (f.host)
            .assign_ticket(&ticket, Role::Reviewer, Provider::Claude, "Review", None)
            .unwrap();
        for caller in [f.implementer.id.clone(), reviewer.id] {
            let result = merge_as(&mut f, &fake, &caller, None);
            refused(&f, &fake, result, "Only the project coordinator");
        }
    }

    #[test]
    fn a_pr_with_no_checks_merges_on_the_review_alone_and_accepts_the_ticket() {
        let (mut f, fake) = ready();
        let head = f.head();
        let merged = merge(&mut f, &fake, None).unwrap();
        assert_eq!(merged["status"], "merged");
        assert_eq!(merged["method"], "squash");
        assert_eq!(merged["merge_commit"], MERGE_COMMIT);
        assert_eq!(merged["accepted"], true);
        let result = merged["result"].as_str().unwrap();
        for part in [
            "PR #180",
            BASE,
            "squash",
            short_sha(&head),
            "6e1f0c2",
            "accepted",
        ] {
            assert!(result.contains(part), "{result}");
        }
        assert_eq!(
            fake.merges(),
            [[
                "pr",
                "merge",
                "180",
                "--repo",
                "github.com/allowed-org/app",
                "--squash",
                "--match-head-commit",
                &head
            ]]
        );
        let ticket = f.ticket();
        assert_eq!(ticket.state, TicketState::Accepted);
        assert!(ticket.merging.is_none());
        assert!(
            !Path::new(&ticket.worktree).exists(),
            "the worktree is removed"
        );
        let pr = ticket.pull_request.unwrap();
        assert_eq!(pr.state, PrState::Merged);
        assert_eq!(pr.merge_commit.as_deref(), Some(MERGE_COMMIT));
        let by_host = pr.merged_by_host.unwrap();
        assert_eq!(by_host.method, Some(MergeMethod::Squash));
        assert_eq!(by_host.head, head);
        // The tool's answer was the result; no message repeats it.
        assert!(messages(&f.host).is_empty());
    }

    #[test]
    fn the_host_never_merges_into_a_protected_base_whoever_asks() {
        // Into main, the default branch and the repository's base.
        let (mut f, fake) = ready();
        let mut ticket = f.ticket();
        ticket.base = None;
        f.host.save_ticket(&ticket).unwrap();
        watch(&mut f, 180, "main");
        fake.set(180, "baseRefName", json!("main"));
        let result = merge(&mut f, &fake, None);
        refused(
            &f,
            &fake,
            result,
            "main is protected: it is this repository’s base",
        );

        // An entry of the human's list.
        let (mut f, fake) = ready();
        let repository = f.ticket().repository_id;
        (f.host)
            .set_protected_branch(&repository, "feature/*", true)
            .unwrap();
        let result = merge(&mut f, &fake, None);
        refused(&f, &fake, result, "“feature/*” matches it");

        // A ruleset the daily read named.
        let (mut f, fake) = ready();
        daily_read(
            &mut f,
            GitHubBranches {
                default_branch: Some("main".into()),
                merge_methods: vec![MergeMethod::Squash],
                rulesets: Some(vec![BranchRuleset {
                    name: "Features".into(),
                    include: vec!["refs/heads/feature/**".into()],
                    exclude: vec![],
                }]),
                protection_rules: Some(vec![]),
            },
        );
        let result = merge(&mut f, &fake, None);
        refused(
            &f,
            &fake,
            result,
            "the GitHub ruleset “Features” protects it",
        );

        // What GitHub reports at the time of the merge, though the daily read had none of it.
        type Protect = fn(&mut GitHub);
        let live: [(&str, Protect); 3] = [
            ("a GitHub ruleset protects it (id 7)", |gh| {
                gh.rules.insert(
                    BASE.into(),
                    json!([{"type": "non_fast_forward", "ruleset_id": 7}]),
                );
            }),
            ("GitHub branch protection protects it", |gh| {
                gh.protected.push(BASE.into())
            }),
            ("it is the default branch on GitHub", |gh| {
                gh.repository["default_branch"] = json!(BASE)
            }),
        ];
        for (reason, protect) in live {
            let (mut f, fake) = ready();
            protect(&mut fake.github.lock().unwrap());
            let result = merge(&mut f, &fake, None);
            refused(&f, &fake, result, reason);
        }
    }

    #[test]
    fn an_approved_merge_whose_base_became_protected_merges_nothing() {
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        let waiting = merge(&mut f, &fake, Some("F4 is cosmetic")).unwrap();
        assert_eq!(waiting["status"], "waiting_for_human");
        fake.github.lock().unwrap().protected.push(BASE.into());
        let item = f.open_items().remove(0);
        f.host.resolve_attention(&item.id, "approved").unwrap();
        let job = f.host.answer_merge(&item.id, true).unwrap().unwrap();
        settle(&mut f, &fake, &job, false).unwrap();
        assert!(fake.merges().is_empty());
        assert!(f.ticket().merging.is_none());
        let told = messages(&f.host);
        assert_eq!(told.len(), 1);
        assert!(told[0].contains("feature/spaces is protected"), "{told:?}");
    }

    #[test]
    fn every_other_check_refuses_and_merges_nothing() {
        let (mut f, fake) = ready();
        f.host.set_owner_allowed("allowed-org", false).unwrap();
        let result = merge(&mut f, &fake, None);
        refused(&f, &fake, result, "allowed-org is not an allowed owner");

        let (mut f, fake) = ready();
        let mut ticket = f.ticket();
        ticket.ledger.push(finding("F6", EntryStatus::Untriaged));
        f.host.save_ticket(&ticket).unwrap();
        let result = merge(&mut f, &fake, None);
        refused(&f, &fake, result, "Triage F6");

        let (mut f, fake) = ready();
        let mut ticket = f.ticket();
        ticket.pull_request = None;
        f.host.save_ticket(&ticket).unwrap();
        let result = merge(&mut f, &fake, None);
        refused(&f, &fake, result, "no open pull request");

        // A head with another patch than the reviewed one.
        let (mut f, fake) = ready();
        let reviewed = f.head();
        let head = f.commit("c.txt", "other\n");
        let mut ticket = f.ticket();
        ticket.pushed = Some(head.clone());
        f.host.save_ticket(&ticket).unwrap();
        let result = merge(&mut f, &fake, None);
        let reason = format!("verified {reviewed}, current {head}; verify again");
        refused(&f, &fake, result, &reason);

        let (mut f, fake) = ready();
        let mut ticket = f.ticket();
        ticket.pushed = None;
        f.host.save_ticket(&ticket).unwrap();
        let result = merge(&mut f, &fake, None);
        refused(
            &f,
            &fake,
            result,
            "is not the commit Wiffletree last pushed",
        );

        let on_github: [(&str, &str, Value); 5] = [
            (
                "conflicts with feature/spaces",
                "mergeable",
                json!("CONFLICTING"),
            ),
            ("is a draft", "isDraft", json!(true)),
            ("blocked from merging", "mergeStateStatus", json!("BLOCKED")),
            (
                "checks failed",
                "statusCheckRollup",
                json!([{"status": "COMPLETED", "conclusion": "SUCCESS"},
                    {"status": "COMPLETED", "conclusion": "FAILURE"}]),
            ),
            (
                "which Wiffletree did not push",
                "headRefOid",
                json!("5f2e7a0"),
            ),
        ];
        for (reason, field, value) in on_github {
            let (mut f, fake) = ready();
            fake.set(180, field, value);
            let result = merge(&mut f, &fake, None);
            refused(&f, &fake, result, reason);
        }

        let (mut f, fake) = ready();
        fake.github.lock().unwrap().unreadable = true;
        let result = merge(&mut f, &fake, None);
        refused(
            &f,
            &fake,
            result,
            "GitHub could not be read, so nothing was merged",
        );

        // A read that leaves out the default branch is a failed read, not the stored one.
        let (mut f, fake) = ready();
        (fake.github.lock().unwrap().repository)
            .as_object_mut()
            .unwrap()
            .remove("default_branch");
        let result = merge(&mut f, &fake, None);
        refused(
            &f,
            &fake,
            result,
            "GitHub did not report the default branch",
        );
    }

    #[test]
    fn a_pr_behind_its_base_merges_with_the_first_method_github_allows() {
        let (mut f, fake) = ready();
        fake.set(180, "mergeStateStatus", json!("BEHIND"));
        fake.github.lock().unwrap().repository["allow_squash_merge"] = json!(false);
        let merged = merge(&mut f, &fake, None).unwrap();
        assert_eq!(merged["method"], "merge commit");
        assert_eq!(fake.merges()[0][5], "--merge");

        let (mut f, fake) = ready();
        {
            let methods = &mut fake.github.lock().unwrap().repository;
            methods["allow_squash_merge"] = json!(false);
            methods["allow_merge_commit"] = json!(false);
            methods["allow_rebase_merge"] = json!(true);
        }
        merge(&mut f, &fake, None).unwrap();
        assert_eq!(fake.merges()[0][5], "--rebase");
    }

    #[test]
    fn a_merge_waits_for_running_checks_and_merges_once_they_pass() {
        let (mut f, fake) = ready();
        fake.set(
            180,
            "statusCheckRollup",
            json!([{"status": "IN_PROGRESS", "conclusion": ""}]),
        );
        let waiting = merge(&mut f, &fake, None).unwrap();
        assert_eq!(waiting["status"], "waiting_for_checks");
        assert_eq!(
            f.ticket().merging.unwrap().stage,
            MergeStage::WaitingForChecks
        );
        let context = f.host.agent_context(&coordinator(&f)).unwrap();
        let shown = &context["tickets"][0]["merging"];
        assert_eq!(shown["stage"]["kind"], "waiting_for_checks", "{shown}");
        let again = merge(&mut f, &fake, None).unwrap_err();
        assert!(format!("{again:#}").contains("waits for its checks"));

        // A waiting merge survives a restart, and a pass with the checks still running waits on.
        let mut f = restart(f);
        pass(&mut f, &fake);
        assert_eq!(
            f.ticket().merging.unwrap().stage,
            MergeStage::WaitingForChecks
        );
        // Checks passed, but GitHub has not worked out whether it can be merged.
        fake.set(
            180,
            "statusCheckRollup",
            json!([{"status": "COMPLETED", "conclusion": "SUCCESS"}, {"state": "SUCCESS"}]),
        );
        fake.set(180, "mergeable", json!("UNKNOWN"));
        pass(&mut f, &fake);
        assert!(fake.merges().is_empty());
        assert!(messages(&f.host).is_empty());

        fake.set(180, "mergeable", json!("MERGEABLE"));
        pass(&mut f, &fake);
        assert_eq!(fake.merges().len(), 1);
        assert_eq!(f.ticket().state, TicketState::Accepted);
        let told = messages(&f.host);
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].starts_with("Merged PR #180"), "{told:?}");
        assert!(told[0].contains("The ticket was accepted."), "{told:?}");
    }

    #[test]
    fn a_waiting_merge_ends_once_when_its_checks_fail() {
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        fake.set(180, "statusCheckRollup", json!([{"state": "ERROR"}]));
        pass(&mut f, &fake);
        pass(&mut f, &fake);
        assert!(fake.merges().is_empty());
        assert!(f.ticket().merging.is_none());
        let told = messages(&f.host);
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].contains("checks failed"), "{told:?}");

        // A new verification cycle ends one too.
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        f.review(VerificationOutcome::Passed);
        pass(&mut f, &fake);
        assert!(f.ticket().merging.is_none());
        assert!(messages(&f.host)[0].contains("latest review is now cycle 2"));

        // Accepting the ticket withdraws it.
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        f.host.accept_ticket(&f.ticket.clone(), None).unwrap();
        assert!(messages(&f.host)[0].contains("was withdrawn, because the ticket was accepted"));
        assert!(fake.merges().is_empty());
    }

    /// Replaces the ticket's head with a commit of the same change, pushed by the host and read
    /// by the watcher, as the PR keeper's rebase leaves it.
    fn keeper_rebase(f: &mut Fixture, fake: &Fake) -> String {
        git(
            &f.worktree,
            &[
                "commit",
                "-q",
                "--amend",
                "--no-edit",
                "--date=2001-01-01T00:00:00",
            ],
        );
        let head = f.head();
        let mut ticket = f.ticket();
        ticket.pushed = Some(head.clone());
        ticket.pull_request.as_mut().unwrap().head = head.clone();
        f.host.save_ticket(&ticket).unwrap();
        fake.set(180, "headRefOid", json!(head));
        head
    }

    #[test]
    fn a_waiting_merge_follows_the_keepers_same_change_rebase_and_merges_that_head() {
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        let rebased = keeper_rebase(&mut f, &fake);
        pass(&mut f, &fake);
        assert_eq!(f.ticket().merging.unwrap().head, rebased);
        fake.set(180, "statusCheckRollup", json!([{"state": "SUCCESS"}]));
        pass(&mut f, &fake);
        assert_eq!(fake.merges()[0][7], rebased);
        assert_eq!(f.ticket().state, TicketState::Accepted);

        // A head the host did not push ends it.
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        fake.set(180, "headRefOid", json!("5f2e7a0"));
        fake.set(180, "statusCheckRollup", json!([{"state": "SUCCESS"}]));
        pass(&mut f, &fake);
        assert!(fake.merges().is_empty());
        assert!(messages(&f.host)[0].contains("which Wiffletree did not push"));
    }

    #[test]
    fn a_blocked_review_without_a_waiver_reaches_the_humans_approval_and_merges_on_it() {
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        let waiting = merge(&mut f, &fake, None).unwrap();
        assert_eq!(waiting["status"], "waiting_for_human");
        assert!(fake.merges().is_empty());
        let items = f.open_items();
        assert_eq!(items.len(), 1);
        assert!(items[0].operation_id.starts_with(MERGE_PREFIX));
        assert!(items[0].is_permission(), "drawn with Approve and Deny");
        for part in [
            "Merge PR #180 into feature/spaces although its review ended blocked?",
            "allowed-org/app · private · allowed-org is an allowed owner",
            "wiffletree/fix-contrast → feature/spaces · not protected",
            "Method: Squash · head",
            "Waiver: None given",
            "Approving merges this head only.",
        ] {
            assert!(items[0].prompt.contains(part), "{}", items[0].prompt);
        }
        f.host.resolve_attention(&items[0].id, "approved").unwrap();
        let job = f.host.answer_merge(&items[0].id, true).unwrap().unwrap();
        settle(&mut f, &fake, &job, false).unwrap();
        let ticket = f.ticket();
        assert_eq!(ticket.state, TicketState::Accepted);
        assert_eq!(ticket.waiver.as_deref(), Some(APPROVED_WAIVER));
        assert!(
            ticket
                .pull_request
                .unwrap()
                .merged_by_host
                .unwrap()
                .approved
        );
        let told = messages(&f.host);
        assert_eq!(told.len(), 1);
        assert!(
            told[0].contains("The human approved the merge."),
            "{told:?}"
        );
    }

    #[test]
    fn a_waived_blocked_review_asks_and_a_denial_merges_nothing() {
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        let mut ticket = f.ticket();
        ticket.ledger.push(finding("F4", EntryStatus::Open));
        f.host.save_ticket(&ticket).unwrap();
        merge(&mut f, &fake, Some("F4 is cosmetic")).unwrap();
        let item = f.open_items().remove(0);
        assert!(
            item.prompt
                .contains("Waiver: “F4 is cosmetic” · the coordinator"),
            "{}",
            item.prompt
        );
        assert!(item.prompt.contains("F4 · non-blocking"), "{}", item.prompt);
        f.host.resolve_attention(&item.id, "denied").unwrap();
        assert!(f.host.answer_merge(&item.id, false).unwrap().is_none());
        assert!(fake.merges().is_empty());
        assert!(f.ticket().merging.is_none());
        let told = messages(&f.host);
        assert_eq!(told.len(), 1);
        assert!(
            told[0].starts_with("The human denied merging PR #180"),
            "{told:?}"
        );
    }

    #[test]
    fn an_approval_covers_one_head_only() {
        // The head moved before the human answered.
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        merge(&mut f, &fake, None).unwrap();
        let item = f.open_items().remove(0);
        f.commit("c.txt", "later\n");
        f.host.resolve_attention(&item.id, "approved").unwrap();
        assert!(f.host.answer_merge(&item.id, true).unwrap().is_none());
        assert!(fake.merges().is_empty());
        let told = messages(&f.host);
        assert!(
            told[0].contains("not merged although the human approved it"),
            "{told:?}"
        );

        // The keeper rebased before the human answered: the item is withdrawn.
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        merge(&mut f, &fake, None).unwrap();
        keeper_rebase(&mut f, &fake);
        pass(&mut f, &fake);
        assert!(f.open_items().is_empty(), "the Inbox item is withdrawn");
        assert!(f.ticket().merging.is_none());
        assert!(messages(&f.host)[0].contains("Call merge_ticket again for the new head"));

        // The keeper rebased an approved merge that waits for its checks.
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        let item = f.open_items().remove(0);
        f.host.resolve_attention(&item.id, "approved").unwrap();
        let job = f.host.answer_merge(&item.id, true).unwrap().unwrap();
        settle(&mut f, &fake, &job, false).unwrap();
        assert_eq!(
            f.ticket().merging.unwrap().stage,
            MergeStage::WaitingForChecks
        );
        keeper_rebase(&mut f, &fake);
        fake.set(180, "statusCheckRollup", json!([{"state": "SUCCESS"}]));
        pass(&mut f, &fake);
        assert!(fake.merges().is_empty());
        assert!(messages(&f.host)[0].contains("covers"));
    }

    #[test]
    fn an_agent_cannot_raise_a_merge_approval() {
        let (mut f, _) = ready();
        let args = json!({"request_id": "merge:forged", "question": "Merge?"});
        let error = f
            .host
            .agent_tool(&coordinator(&f), "ask_user", args)
            .unwrap_err();
        assert!(format!("{error:#}").contains("reserved for the workspace"));
        assert!(f.open_items().is_empty());
    }

    #[test]
    fn the_head_moving_at_the_last_moment_merges_nothing() {
        let (mut f, fake) = ready();
        fake.github.lock().unwrap().merge_error =
            Some("Head branch was modified. Review and try the merge again.".into());
        let result = merge(&mut f, &fake, None);
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("GitHub refused the merge"), "{error}");
        assert_eq!(fake.pr(180)["state"], "OPEN");
        assert!(f.ticket().is_open() && f.ticket().merging.is_none());
    }

    #[test]
    fn acceptance_failing_after_the_merge_leaves_the_ticket_open_and_says_why() {
        let (mut f, fake) = ready();
        let job = f
            .host
            .merge_ticket(&coordinator(&f), &f.ticket.clone(), None)
            .unwrap();
        fs::write(f.worktree.join("stray.txt"), "untracked\n").unwrap();
        let merged = settle(&mut f, &fake, &job, true).unwrap();
        assert_eq!(merged["accepted"], false);
        let result = merged["result"].as_str().unwrap();
        assert!(result.contains("The ticket was not accepted"), "{result}");
        assert!(result.contains("untracked"), "{result}");
        let ticket = f.ticket();
        assert!(ticket.is_open());
        assert!(ticket.merging.is_none());
        assert!(ticket.pull_request.unwrap().merged_by_host.is_some());
    }

    #[test]
    fn a_stacked_childs_pr_is_retargeted_to_the_merged_prs_base() {
        let (mut f, fake) = ready();
        let parent = f.ticket();
        let repository = parent.repository_id.clone();
        let child = (f.host)
            .create_stacked_ticket(
                &coordinator(&f),
                &repository,
                "Space tracker",
                "Do",
                Some(&parent.id),
            )
            .unwrap();
        let mut stacked = child.clone();
        let mut pr = parent.pull_request.clone().unwrap();
        pr.number = 181;
        pr.base = parent.branch.clone();
        stacked.pull_request = Some(pr);
        f.host.save_ticket(&stacked).unwrap();
        (fake.github.lock().unwrap().prs).insert(181, pr_json("c81f04d", &parent.branch));
        let merged = merge(&mut f, &fake, None).unwrap();
        let result = merged["result"].as_str().unwrap();
        assert!(
            result.contains("PR #181 of ticket \"Space tracker\"")
                && result.contains("now opens into feature/spaces"),
            "{result}"
        );
        let edits = fake.github.lock().unwrap().edits.clone();
        assert_eq!(
            edits,
            [[
                "pr",
                "edit",
                "181",
                "--repo",
                "github.com/allowed-org/app",
                "--base",
                BASE
            ]]
        );
        let child = f.host.ticket(&child.id).unwrap();
        assert_eq!(child.pull_request.as_ref().unwrap().base, BASE);
        assert_eq!(
            f.host
                .ticket_base(&child)
                .unwrap()
                .pull_request_base()
                .unwrap(),
            BASE
        );

        // GitHub already moved it: nothing is sent.
        let (mut f, fake) = ready();
        let parent = f.ticket();
        let child = (f.host)
            .create_stacked_ticket(
                &coordinator(&f),
                &repository_of(&f),
                "Space tracker",
                "Do",
                Some(&parent.id),
            )
            .unwrap();
        let mut stacked = child;
        let mut pr = parent.pull_request.clone().unwrap();
        pr.number = 181;
        pr.base = parent.branch.clone();
        stacked.pull_request = Some(pr);
        f.host.save_ticket(&stacked).unwrap();
        (fake.github.lock().unwrap().prs).insert(181, pr_json("c81f04d", BASE));
        let merged = merge(&mut f, &fake, None).unwrap();
        assert!(
            merged["result"]
                .as_str()
                .unwrap()
                .contains("GitHub had already moved PR #181")
        );
        assert!(fake.github.lock().unwrap().edits.is_empty());
    }
    fn repository_of(f: &Fixture) -> String {
        f.ticket().repository_id
    }

    #[test]
    fn a_merge_cut_short_is_finished_or_ended_on_githubs_word() {
        // Stopped after gh pr merge was sent, which GitHub carried out.
        let (mut f, fake) = ready();
        ready_to_send(&mut f, &fake);
        fake.set(180, "state", json!("MERGED"));
        fake.set(180, "mergeCommit", json!({"oid": MERGE_COMMIT}));
        let mut f = restart(f);
        let jobs = f.host.interrupted_merges().unwrap();
        assert_eq!(jobs.len(), 1);
        f.host
            .record_merge(run(&jobs[0], &fake.tools), false)
            .unwrap();
        assert_eq!(f.ticket().state, TicketState::Accepted);
        assert!(fake.merges().is_empty(), "nothing is sent again");
        // The method recorded before the merge was sent survives the restart.
        let by_host = f.ticket().pull_request.unwrap().merged_by_host.unwrap();
        assert_eq!(by_host.method, Some(MergeMethod::Squash));
        let told = messages(&f.host);
        assert_eq!(told.len(), 1);
        assert!(told[0].starts_with("Merged PR #180"), "{told:?}");
        assert!(told[0].contains("with squash"), "{told:?}");
        assert!(f.host.interrupted_merges().unwrap().is_empty());

        // Stopped before GitHub merged it.
        let (mut f, fake) = ready();
        ready_to_send(&mut f, &fake);
        let mut f = restart(f);
        let jobs = f.host.interrupted_merges().unwrap();
        f.host
            .record_merge(run(&jobs[0], &fake.tools), false)
            .unwrap();
        assert!(f.ticket().is_open() && f.ticket().merging.is_none());
        assert!(messages(&f.host)[0].contains("Call merge_ticket again"));

        // A read that was deciding a request awaiting approval merged nothing; it simply ends.
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        f.host
            .merge_ticket(&coordinator(&f), &f.ticket.clone(), None)
            .unwrap();
        let f = restart(f);
        assert!(f.ticket().merging.is_none());
        assert!(messages(&f.host)[0].contains("interrupted when Wiffletree stopped"));
        assert!(fake.merges().is_empty());

        // gh pr merge stopped at its time limit after GitHub merged.
        let (mut f, fake) = ready();
        fake.github.lock().unwrap().stalls = true;
        let merged = merge(&mut f, &fake, None).unwrap();
        assert_eq!(merged["status"], "merged");
        assert_eq!(merged["method"], "squash");
        assert_eq!(f.ticket().state, TicketState::Accepted);
    }

    #[test]
    fn protecting_the_base_or_revoking_the_owner_while_github_is_read_merges_nothing() {
        type Change = fn(&mut Fixture);
        let revoke: [(&str, Change); 2] = [
            ("feature/spaces is now protected", |f| {
                let repository = f.ticket().repository_id;
                (f.host)
                    .set_protected_branch(&repository, "feature/*", true)
                    .unwrap();
            }),
            ("no longer publishes without asking", |f| {
                f.host.set_owner_allowed("allowed-org", false).unwrap();
            }),
        ];
        for (reason, change) in revoke {
            let (mut f, fake) = ready();
            let sending = ready_to_send(&mut f, &fake);
            change(&mut f);
            let error = settle(&mut f, &fake, &sending, true).unwrap_err();
            assert!(format!("{error:#}").contains(reason), "{error:#}");
            assert!(fake.merges().is_empty());
        }
    }

    #[test]
    fn a_merge_waiting_for_the_human_ends_on_failed_checks_a_conflict_or_a_protected_base() {
        type Change = fn(&mut Fixture);
        let changes: [(&str, Change); 3] = [
            ("its checks failed", |f| {
                let mut ticket = f.ticket();
                ticket.pull_request.as_mut().unwrap().checks = CheckState::Failure;
                f.host.save_ticket(&ticket).unwrap();
            }),
            ("it conflicts with its base", |f| {
                let mut ticket = f.ticket();
                ticket.pull_request.as_mut().unwrap().merge_state = MergeState::Dirty;
                f.host.save_ticket(&ticket).unwrap();
            }),
            ("feature/spaces is protected", |f| {
                let repository = f.ticket().repository_id;
                (f.host)
                    .set_protected_branch(&repository, "feature/*", true)
                    .unwrap();
            }),
        ];
        for (reason, change) in changes {
            let (mut f, fake) = ready();
            f.review(VerificationOutcome::Blocked);
            merge(&mut f, &fake, None).unwrap();
            assert_eq!(f.open_items().len(), 1);
            change(&mut f);
            pass(&mut f, &fake);
            assert!(f.open_items().is_empty(), "the Inbox item is withdrawn");
            assert!(f.ticket().merging.is_none());
            let told = messages(&f.host);
            assert_eq!(told.len(), 1);
            assert!(told[0].contains(reason), "{told:?}");
        }
    }

    #[test]
    fn a_merge_waiting_for_the_human_ends_when_github_reads_its_base_protected() {
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        merge(&mut f, &fake, None).unwrap();
        let repository = f.ticket().repository_id;
        // Nothing changed, or GitHub could not be read: the card stays.
        pass(&mut f, &fake);
        fake.github.lock().unwrap().unreadable = true;
        pass(&mut f, &fake);
        fake.github.lock().unwrap().unreadable = false;
        assert_eq!(f.open_items().len(), 1);
        assert!(messages(&f.host).is_empty());

        fake.github.lock().unwrap().protected.push(BASE.into());
        // A pass that left the rate limit near its floor reads nothing more.
        assert!(f.host.merges_due(&repository, false).unwrap().is_empty());
        assert_eq!(f.open_items().len(), 1);
        pass(&mut f, &fake);
        assert!(f.open_items().is_empty(), "the Inbox item is withdrawn");
        assert!(f.ticket().merging.is_none());
        assert!(fake.merges().is_empty());
        let told = messages(&f.host);
        assert_eq!(told.len(), 1);
        assert!(told[0].contains("feature/spaces is protected"), "{told:?}");
    }

    #[test]
    fn a_re_read_finishing_after_the_human_answered_leaves_the_answer_to_decide() {
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        merge(&mut f, &fake, None).unwrap();
        let repository = f.ticket().repository_id;
        let reread = f.host.merges_due(&repository, true).unwrap().remove(0);
        fake.github.lock().unwrap().protected.push(BASE.into());
        let refused = run(&reread, &fake.tools);
        let item = f.open_items().remove(0);
        f.host.resolve_attention(&item.id, "approved").unwrap();
        let approved = f.host.answer_merge(&item.id, true).unwrap().unwrap();
        let (status, next) = f.host.record_merge(refused, false).unwrap();
        assert_eq!(status["status"], "unchanged");
        assert!(next.is_none());
        assert_eq!(f.ticket().merging.unwrap().stage, MergeStage::Checking);
        // The approval's own reading of GitHub refuses it, once.
        settle(&mut f, &fake, &approved, false).unwrap();
        assert!(fake.merges().is_empty());
        assert!(f.ticket().merging.is_none());
        assert_eq!(messages(&f.host).len(), 1);

        // A re-read that found it ready starts no merge of its own: the approval's read does.
        let (mut f, fake) = ready();
        f.review(VerificationOutcome::Blocked);
        merge(&mut f, &fake, None).unwrap();
        let reread = f
            .host
            .merges_due(&repository_of(&f), true)
            .unwrap()
            .remove(0);
        let ready = run(&reread, &fake.tools);
        let item = f.open_items().remove(0);
        f.host.resolve_attention(&item.id, "approved").unwrap();
        let approved = f.host.answer_merge(&item.id, true).unwrap().unwrap();
        let (_, next) = f.host.record_merge(ready, false).unwrap();
        assert!(next.is_none());
        settle(&mut f, &fake, &approved, false).unwrap();
        assert_eq!(fake.merges().len(), 1);
    }

    #[test]
    fn a_restart_after_acceptance_still_finishes_the_merge_with_its_result() {
        let (mut f, fake) = ready();
        ready_to_send(&mut f, &fake);
        fake.set(180, "state", json!("MERGED"));
        fake.set(180, "mergeCommit", json!({"oid": MERGE_COMMIT}));
        // The host stopped after merged() accepted the ticket, which removed its worktree.
        f.host.accept_ticket(&f.ticket.clone(), None).unwrap();
        assert!(!f.worktree.exists());
        let mut f = restart(f);
        let jobs = f.host.interrupted_merges().unwrap();
        f.host
            .record_merge(run(&jobs[0], &fake.tools), false)
            .unwrap();
        let told = messages(&f.host);
        assert_eq!(told.len(), 1);
        assert!(told[0].starts_with("Merged PR #180"), "{told:?}");
        assert!(f.ticket().merging.is_none());
    }

    #[test]
    fn a_restart_after_the_merge_was_recorded_still_accepts_tells_and_clears() {
        for accepted_before in [false, true] {
            let (mut f, fake) = ready();
            ready_to_send(&mut f, &fake);
            fake.set(180, "state", json!("MERGED"));
            fake.set(180, "mergeCommit", json!({"oid": MERGE_COMMIT}));
            // The host stopped after merged() saved the merge, and perhaps accepted the ticket.
            let mut ticket = f.ticket();
            let pr = ticket.pull_request.as_mut().unwrap();
            pr.state = PrState::Merged;
            pr.merge_commit = Some(MERGE_COMMIT.into());
            pr.merged_by_host = Some(HostMerge {
                method: Some(MergeMethod::Squash),
                head: pr.head.clone(),
                approved: false,
            });
            f.host.save_ticket(&ticket).unwrap();
            if accepted_before {
                f.host.accept_ticket(&f.ticket.clone(), None).unwrap();
            }
            let mut f = restart(f);
            let jobs = f.host.interrupted_merges().unwrap();
            settle(&mut f, &fake, &jobs[0], false).unwrap();
            let ticket = f.ticket();
            assert_eq!(ticket.state, TicketState::Accepted);
            assert!(ticket.merging.is_none());
            let told = messages(&f.host);
            assert_eq!(told.len(), 1, "{told:?}");
            assert!(told[0].starts_with("Merged PR #180"), "{told:?}");
            assert!(f.host.interrupted_merges().unwrap().is_empty());
        }
    }

    #[test]
    fn a_waiting_merge_decided_again_when_the_host_stopped_waits_on_after_the_restart() {
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        // A pass starts its decision, and the host stops before it reports back.
        let repository = repository_of(&f);
        assert_eq!(f.host.merges_due(&repository, true).unwrap().len(), 1);
        let mut f = restart(f);
        let jobs = f.host.interrupted_merges().unwrap();
        f.host
            .record_merge(run(&jobs[0], &fake.tools), false)
            .unwrap();
        assert_eq!(
            f.ticket().merging.unwrap().stage,
            MergeStage::WaitingForChecks
        );
        assert!(messages(&f.host).is_empty());
        assert!(fake.merges().is_empty());

        // The next pass merges once the checks pass.
        fake.set(180, "statusCheckRollup", json!([{"state": "SUCCESS"}]));
        pass(&mut f, &fake);
        assert_eq!(f.ticket().state, TicketState::Accepted);
    }

    #[test]
    fn a_waited_merge_cut_short_after_it_was_sent_ends_once_and_is_not_sent_again() {
        // gh pr merge times out with the pull request still open.
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        fake.set(180, "statusCheckRollup", json!([{"state": "SUCCESS"}]));
        fake.github.lock().unwrap().times_out = true;
        pass(&mut f, &fake);
        pass(&mut f, &fake);
        assert_eq!(fake.merges().len(), 1, "sent once");
        assert!(f.ticket().merging.is_none());
        let told = messages(&f.host);
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].contains("Call merge_ticket again"), "{told:?}");

        // The host stops once the merge may have been sent: the restart ends it.
        let (mut f, fake) = ready();
        fake.set(180, "statusCheckRollup", json!([{"state": "PENDING"}]));
        merge(&mut f, &fake, None).unwrap();
        fake.set(180, "statusCheckRollup", json!([{"state": "SUCCESS"}]));
        let repository = repository_of(&f);
        let rechecking = f.host.merges_due(&repository, true).unwrap().remove(0);
        let (_, sending) = (f.host)
            .record_merge(run(&rechecking, &fake.tools), false)
            .unwrap();
        assert!(sending.is_some(), "the merge job follows the re-decision");
        assert_eq!(f.ticket().merging.unwrap().stage, MergeStage::Merging);
        let mut f = restart(f);
        let jobs = f.host.interrupted_merges().unwrap();
        settle(&mut f, &fake, &jobs[0], false).unwrap();
        assert!(f.ticket().merging.is_none());
        assert!(messages(&f.host)[0].contains("Call merge_ticket again"));
        assert!(fake.merges().is_empty());
    }

    #[test]
    fn a_merge_commit_github_does_not_report_at_once_is_read_again() {
        // One failed read is retried.
        let (mut f, fake) = ready();
        fake.github.lock().unwrap().failing_reads = 1;
        let merged = merge(&mut f, &fake, None).unwrap();
        assert_eq!(merged["merge_commit"], MERGE_COMMIT);

        // Every read fails: the request stays, and the next start records the commit.
        let (mut f, fake) = ready();
        fake.github.lock().unwrap().failing_reads = 3;
        let merged = merge(&mut f, &fake, None).unwrap();
        assert!(merged["merge_commit"].is_null());
        let result = merged["result"].as_str().unwrap();
        assert!(
            result.contains("did not report the merge commit"),
            "{result}"
        );
        assert_eq!(f.ticket().state, TicketState::Accepted);
        assert_eq!(f.ticket().merging.unwrap().stage, MergeStage::Merging);
        let mut f = restart(f);
        let jobs = f.host.interrupted_merges().unwrap();
        settle(&mut f, &fake, &jobs[0], false).unwrap();
        let ticket = f.ticket();
        assert!(ticket.merging.is_none());
        assert_eq!(
            ticket.pull_request.unwrap().merge_commit.as_deref(),
            Some(MERGE_COMMIT)
        );
        let told = messages(&f.host);
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].contains("as 6e1f0c2"), "{told:?}");
    }

    #[test]
    fn checks_roll_up_as_github_reports_them() {
        let none: [Value; 0] = [];
        assert_eq!(rollup(&none), CheckState::None);
        let passed = [
            json!({"status": "COMPLETED", "conclusion": "SKIPPED"}),
            json!({"state": "SUCCESS"}),
        ];
        assert_eq!(rollup(&passed), CheckState::Success);
        let running = [
            json!({"status": "QUEUED", "conclusion": ""}),
            json!({"state": "SUCCESS"}),
        ];
        assert_eq!(rollup(&running), CheckState::Pending);
        let failed = [
            json!({"status": "QUEUED"}),
            json!({"status": "COMPLETED", "conclusion": "TIMED_OUT"}),
        ];
        assert_eq!(rollup(&failed), CheckState::Failure);
    }
}
