//! When a step of a ticket's publish request waits for the human, and what the Inbox item then
//! shows. The host decides with this and writes the item's text from it; the desktop draws the
//! same card, so the two cannot disagree.
use crate::{
    EntryStatus, PrState, PublishRequest, PublishStep, Repository, Ticket, VerificationOutcome,
    is_allowed, origin_branch, owner_entry, short_sha,
};

/// Why a step of a publish request waits for the human.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishAsk {
    /// The repository does not publish without asking, so each step asks.
    NotAllowed,
    /// The review ended blocked in an allowed owner's repository: one approval covers the
    /// commit's push and pull request.
    BlockedReview,
    /// The coordinator waived commits no review checked: one approval covers the commit's push
    /// and pull request, even in an allowed owner's repository after a passed review.
    UnreviewedCommits,
    /// The ticket's earlier pull request was closed or merged, so opening another asks.
    EndedPullRequest,
}

/// Whether the request's next step must wait for the human, and why. `None` runs it without
/// asking: the repository publishes without asking, the review passed and no unreviewed commits
/// are waived, or the human already approved this commit's steps.
pub fn publish_ask(
    ticket: &Ticket,
    request: &PublishRequest,
    repository: &Repository,
    allowed: &[String],
) -> Option<PublishAsk> {
    let passed = ticket
        .verification
        .as_ref()
        .is_some_and(|v| v.outcome == VerificationOutcome::Passed);
    if !is_allowed(repository, allowed) {
        Some(PublishAsk::NotAllowed)
    } else if request.blocked_review_approved {
        None
    } else if request.waived.is_some() {
        Some(PublishAsk::UnreviewedCommits)
    } else if !passed {
        Some(PublishAsk::BlockedReview)
    } else if request.step == PublishStep::OpenPullRequest && ticket.ended_pull_request().is_some()
    {
        Some(PublishAsk::EndedPullRequest)
    } else {
        None
    }
}

/// Why a repository does not publish without asking, such as "celadon-theme is not an allowed
/// owner".
pub fn not_allowed_reason(repository: &Repository) -> String {
    match owner_entry(repository) {
        None => "GitHub has not answered for this repository".into(),
        Some(_) if origin_branch(&repository.base).is_none() => {
            format!("its base {} is not on origin", repository.base)
        }
        Some(entry) if entry.contains('/') => format!("{entry} is a fork that is not allowed"),
        Some(entry) => format!("{entry} is not an allowed owner"),
    }
}

/// A publish approval as the human sees it, built from the frozen request, the ticket's review
/// and ledger, and the repository's facts. The agent's only words in it are the pull request's
/// title and description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishCard {
    pub title: String,
    /// Each row's label and value, in order.
    pub rows: Vec<(&'static str, String)>,
    /// After a blocked review: each open finding's heading and its summary.
    pub findings: Vec<(String, String)>,
    /// What one approval after a blocked review would send.
    pub would_send: Option<String>,
    /// The pull request's description, in full.
    pub description: Option<String>,
    pub note: String,
    /// The approve button's label.
    pub approve: &'static str,
}
impl PublishCard {
    /// The card as the Inbox item's text. The description is left to the card, which shows it in
    /// full.
    pub fn text(&self) -> String {
        let mut lines = vec![self.title.clone()];
        lines.extend(
            self.rows
                .iter()
                .map(|(label, value)| format!("{label}: {value}")),
        );
        if !self.findings.is_empty() {
            lines.push("Open findings:".into());
        }
        lines.extend(
            self.findings
                .iter()
                .map(|(heading, summary)| format!("{heading} · {summary}")),
        );
        lines.extend(
            self.would_send
                .iter()
                .map(|sent| format!("Would send: {sent}")),
        );
        lines.join("\n")
    }
}

/// Such as "PR #45 was closed without merging".
pub fn ended_pull_request_notice(ticket: &Ticket) -> Option<String> {
    ticket.ended_pull_request().map(|pr| match pr.state {
        PrState::Merged => format!("PR #{} was merged", pr.number),
        _ => format!("PR #{} was closed without merging", pr.number),
    })
}

/// The card for the step that waits on its Inbox item, as the rules stand now. An owner allowed
/// since the item was raised no longer asks; the step still waits for this answer.
pub fn waiting_card(
    ticket: &Ticket,
    request: &PublishRequest,
    repository: &Repository,
    allowed: &[String],
) -> PublishCard {
    let ask =
        publish_ask(ticket, request, repository, allowed).unwrap_or(PublishAsk::EndedPullRequest);
    publish_card(ticket, request, repository, ask)
}

/// The card for the request's next step, which waits for `ask`.
pub fn publish_card(
    ticket: &Ticket,
    request: &PublishRequest,
    repository: &Repository,
    ask: PublishAsk,
) -> PublishCard {
    let commit = short_sha(&request.commit);
    let facts = repository.github.as_ref().and_then(|g| g.facts.as_ref());
    let visibility = match facts {
        Some(facts) if facts.public => "public",
        Some(_) => "private",
        None => "visibility unknown",
    };
    let rule = match (ask, owner_entry(repository)) {
        (PublishAsk::NotAllowed, _) | (_, None) => not_allowed_reason(repository),
        (_, Some(fork)) if fork.contains('/') => format!("{fork} is an allowed fork"),
        (_, Some(owner)) => format!("{owner} is an allowed owner"),
    };
    let open_pr = ticket.open_pull_request();
    let covers_commit = matches!(
        ask,
        PublishAsk::BlockedReview | PublishAsk::UnreviewedCommits
    );
    let mut rows = vec![(
        "Repository",
        format!("{} · {visibility} · {rule}", request.repository),
    )];
    match (request.step, open_pr) {
        (PublishStep::Push, Some(pr)) => {
            rows.push(("Updates", format!("PR #{} · {}", pr.number, request.branch)));
        }
        (PublishStep::Push, None) if ticket.pushed.is_none() && ticket.pull_request.is_none() => {
            rows.push(("Branch", format!("{} · not on GitHub yet", request.branch)));
        }
        (PublishStep::Push, None) => rows.push(("Branch", request.branch.clone())),
        (PublishStep::OpenPullRequest, _) => {
            rows.push(("Branches", format!("{} → {}", request.branch, request.base)))
        }
    }
    rows.push(("Commit", format!("{commit} · {}", request.size)));
    if request.step == PublishStep::OpenPullRequest {
        rows.push(("Title", request.title.clone().unwrap_or_default()));
    }
    let mut findings = vec![];
    if let Some(verification) = &ticket.verification {
        rows.push(("Review", verification.review_line(&request.commit)));
        if !verification.is_full_review()
            && let Some(full_review) = ticket.last_full_review()
        {
            rows.push(("Last full review", full_review.passed_line()));
        }
        if verification.outcome == VerificationOutcome::Blocked {
            let unchecked = verification.unchecked();
            if !unchecked.is_empty() {
                rows.push(("Did not check", unchecked.join(", ")));
            }
            findings = ticket
                .open_findings()
                .into_iter()
                .map(|entry| {
                    let untriaged = match entry.status {
                        EntryStatus::Untriaged => " Not triaged yet.",
                        _ => "",
                    };
                    (
                        entry.heading(),
                        format!("{}{untriaged}", entry.finding.summary),
                    )
                })
                .collect();
            if findings.is_empty() {
                rows.push(("Open findings", "None".into()));
            }
        }
    }
    if let Some(waived) = &request.waived {
        let commits: Vec<&str> = waived.commits.iter().map(|c| short_sha(c)).collect();
        rows.push(("Unreviewed commits", commits.join(", ")));
        rows.push(("Waiver", waived.reason.clone()));
    }
    if let Some(notice) = ended_pull_request_notice(ticket) {
        rows.push(("Earlier pull request", notice));
    }
    let pushes = format!("Push {} at {commit}", request.branch);
    let (title, note, approve) = match (ask, request.step) {
        (PublishAsk::BlockedReview, _) => (
            "Publish this ticket although its review ended blocked?".to_owned(),
            "Wiffletree asks because the review did not pass, even though the owner is allowed. Approving covers this commit only.".to_owned(),
            "Publish anyway",
        ),
        (PublishAsk::UnreviewedCommits, _) => (
            "Publish this ticket with commits no review checked?".to_owned(),
            "Wiffletree asks because the coordinator waived review of these commits after the ticket used all its full reviews, even if the owner is allowed. Approving covers this commit only.".to_owned(),
            "Publish anyway",
        ),
        (_, PublishStep::Push) => (
            match (open_pr, request.size.split_once(' ')) {
                (Some(_), Some(("1", _))) => format!("Push 1 new commit to {}?", request.repository),
                (Some(_), Some((count, _))) => {
                    format!("Push {count} new commits to {}?", request.repository)
                }
                _ => format!("Push this branch to {}?", request.repository),
            },
            format!(
                "Wiffletree runs the push itself, for exactly this commit. If the branch has moved by the time you answer, nothing is pushed.{}",
                match (
                    request.opens_pull_request(),
                    ask == PublishAsk::NotAllowed || ticket.ended_pull_request().is_some(),
                ) {
                    (false, _) => "",
                    (true, true) => " Opening the pull request is a separate approval.",
                    (true, false) => " Wiffletree then opens the pull request without asking again.",
                }
            ),
            "Push once",
        ),
        (_, PublishStep::OpenPullRequest) => (
            format!("Open this pull request in {}?", request.repository),
            "The title and description are the agent’s words, shown in full. Anyone who can see the repository will see them.".to_owned(),
            "Open pull request",
        ),
    };
    PublishCard {
        title,
        rows,
        findings,
        would_send: covers_commit.then(|| match &request.title {
            Some(title) => format!("{pushes} and open its pull request, “{title}”"),
            None => pushes,
        }),
        // One approval that covers the commit also opens the pull request, so it shows its words.
        description: match covers_commit || request.step == PublishStep::OpenPullRequest {
            true => request.body.clone(),
            false => None,
        },
        note,
        approve,
    }
}
