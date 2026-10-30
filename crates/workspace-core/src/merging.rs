//! What a merge approval shows. The host writes the Inbox item's text from it when it asks the
//! human, and the desktop draws the same card from the stored request, so the two cannot
//! disagree.
use crate::{
    ApprovalCard, CheckState, MergeRequest, Repository, Ticket, VerificationOutcome, capitalized,
    owner_entry, short_sha,
};

/// The acceptance waiver a merge after a blocked review records when the coordinator gave none.
pub const APPROVED_WAIVER: &str = "Merged after a blocked review with the human's approval";

/// The card for a merge request waiting for the human: the repository, the branches, the
/// method and head, the review, the waiver and the findings still open.
pub fn merge_card(
    ticket: &Ticket,
    request: &MergeRequest,
    repository: &Repository,
) -> ApprovalCard {
    let facts = repository.github.as_ref().and_then(|g| g.facts.as_ref());
    let visibility = match facts {
        Some(facts) if facts.public => "public",
        Some(_) => "private",
        None => "visibility unknown",
    };
    let mut repository_row = format!("{} · {visibility}", request.repository);
    match owner_entry(repository) {
        Some(fork) if fork.contains('/') => {
            repository_row.push_str(&format!(" · {fork} is an allowed fork"))
        }
        Some(owner) => repository_row.push_str(&format!(" · {owner} is an allowed owner")),
        None => {}
    }
    let head = short_sha(&request.head);
    let verification = ticket.verification.as_ref();
    let mut method = vec![match request.method {
        Some(method) => format!("{} · head {head}", capitalized(method.label())),
        None => format!("Head {head}"),
    }];
    if verification.and_then(|v| v.verified_commit()) == Some(request.head.as_str()) {
        method[0].push_str(", the reviewed commit");
    }
    if let Some(read) = request.read {
        let checks = match read.checks {
            CheckState::Success => "checks passed",
            CheckState::Failure => "checks failed",
            CheckState::Pending => "checks running",
            CheckState::None => "no checks",
        };
        let conflict = match read.mergeable {
            true => "no conflict",
            false => "whether it conflicts not worked out yet",
        };
        method.push(format!("GitHub last read: {checks}, {conflict}"));
    }
    let mut rows = vec![
        ("Repository", repository_row),
        (
            "Branches",
            format!("{} → {} · not protected", request.branch, request.base),
        ),
        ("Method", method.join(" · ")),
    ];
    let blocked = verification.is_some_and(|v| v.outcome == VerificationOutcome::Blocked);
    if let Some(verification) = verification {
        rows.push(("Review", verification.review_line(&request.head)));
        let unchecked = verification.unchecked();
        if !unchecked.is_empty() {
            rows.push(("Did not check", unchecked.join(", ")));
        }
    }
    let waiver = coordinator_waiver(request);
    rows.push((
        "Waiver",
        waiver.map_or("None given".into(), |reason| {
            format!("“{reason}” · the coordinator")
        }),
    ));
    if !request.unreviewed.is_empty() {
        let commits: Vec<&str> = request.unreviewed.iter().map(|c| short_sha(c)).collect();
        rows.push(("Unreviewed commits", commits.join(", ")));
    }
    let findings = ticket
        .open_findings()
        .into_iter()
        .map(|entry| (entry.heading(), entry.finding.summary.clone()))
        .collect();
    let (title, why) = match blocked {
        true => (
            format!(
                "Merge PR #{} into {} although its review ended blocked?",
                request.number, request.base
            ),
            match waiver {
                Some(_) => {
                    "Wiffletree asks because the review did not pass, even though the coordinator waived it and the owner is allowed."
                }
                None => "Wiffletree asks because the review did not pass.",
            },
        ),
        false => (
            format!(
                "Merge PR #{} into {} over commits no review checked?",
                request.number, request.base
            ),
            "Wiffletree asks because the merge takes commits no review checked.",
        ),
    };
    ApprovalCard {
        title,
        rows,
        findings,
        would_send: None,
        description: None,
        note: format!(
            "{why} Approving merges this head only. If the head moves, the checks fail or the base becomes protected before the merge, nothing is merged."
        ),
        approve: "Merge",
    }
}

/// The coordinator's own waiver, not the reason the host records when it gave none.
fn coordinator_waiver(request: &MergeRequest) -> Option<&str> {
    request
        .waived
        .as_deref()
        .filter(|reason| *reason != APPROVED_WAIVER)
}
