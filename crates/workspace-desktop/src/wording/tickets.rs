//! How tickets, their pull requests, the PR keeper and progress read in the sidebar, the
//! Tickets and Crew panels and the conversation's strip.
use super::{age, counted};
use workspace_core::*;

/// "repository · ticket", or the title alone when the repository is unknown.
pub fn ticket_label(snapshot: &Snapshot, ticket: &Ticket) -> String {
    match snapshot
        .repositories
        .iter()
        .find(|r| r.id == ticket.repository_id)
    {
        Some(repository) => format!("{} · {}", repository.name, ticket.title),
        None => ticket.title.clone(),
    }
}

fn cycle_line(verification: &Verification) -> String {
    let outcome = match verification.outcome {
        VerificationOutcome::Running => "running",
        VerificationOutcome::Passed => "passed",
        VerificationOutcome::Blocked => "blocked",
    };
    let rounds = verification.rounds.len();
    // A keeper review has one round, so "of 1" says nothing.
    let rounds = match verification.keeper {
        true => counted(rounds, "round"),
        false => format!("{rounds} of {} rounds", verification.max_rounds),
    };
    format!(
        "{}: {outcome}, {rounds}",
        verification.cycle_label("Verification cycle")
    )
}

/// One line for the latest cycle and one per round, with its commit and each verifier's result,
/// then one line per earlier cycle, newest first.
pub fn verification_lines(ticket: &Ticket) -> Vec<String> {
    let Some(verification) = &ticket.verification else {
        return vec![];
    };
    let mut lines = vec![cycle_line(verification)];
    for round in &verification.rounds {
        let results = round
            .verifiers
            .iter()
            .map(|v| {
                format!(
                    "{} {}",
                    v.role.agent_label(Some(&v.focus)),
                    v.result.label()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let commit: String = round.commit.chars().take(7).collect();
        lines.push(format!("Round {} at {commit}: {results}", round.round));
    }
    lines.extend(ticket.previous_cycles.iter().rev().map(cycle_line));
    lines
}

/// The line under a ticket while a step of its publish request waits for the human, such as
/// "push waits for your approval".
pub fn publish_waiting(ticket: &Ticket) -> Option<String> {
    let request = ticket.publishing.as_ref()?;
    request
        .waiting
        .as_ref()
        .map(|_| format!("{} waits for your approval", request.step.label()))
}

/// A ticket accepted with a waiver: its row's waived ring says this beside the label.
pub fn waiver_note(ticket: &Ticket) -> Option<String> {
    ticket
        .waiver
        .as_ref()
        .filter(|_| ticket.state == TicketState::Accepted)?;
    let open = ticket
        .ledger
        .iter()
        .filter(|e| e.status == EntryStatus::Open)
        .count();
    Some(format!(
        "Accepted with waiver · {}",
        counted(open, "open finding")
    ))
}

/// How a part of a ticket's PR line is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Good,
    Bad,
    Merged,
    Warn,
    /// The keeper's model at work.
    Busy,
}
#[derive(Debug, PartialEq)]
pub struct PrPart {
    pub text: String,
    pub tone: Tone,
}

/// A ticket row's PR line, such as "#142 · checks ✓ · 1 thread · behind", in parts. Only the
/// parts that apply appear, and an open PR ends with the keeper's part: the commits it adopted
/// that no review has checked, what it is doing, if anything, and the sorted comments waiting,
/// which replace "up to date"; a finished PR shows `merged` or `closed` instead of its checks.
pub fn pr_line(
    pr: &PullRequest,
    unreviewed: Option<&Unreviewed>,
    keeper: Option<KeeperPart>,
    comments: Option<&KeeperComments>,
    now: i64,
) -> Vec<PrPart> {
    let part = |text: &str, tone| PrPart {
        text: text.into(),
        tone,
    };
    let mut parts = vec![part(&format!("#{}", pr.number), Tone::Muted)];
    match pr.state {
        PrState::Merged => parts.push(part("merged", Tone::Merged)),
        PrState::Closed => parts.push(part("closed", Tone::Muted)),
        PrState::Open => {
            match pr.checks {
                CheckState::Success => parts.push(part("checks ✓", Tone::Good)),
                CheckState::Failure => parts.push(part("checks ✗", Tone::Bad)),
                CheckState::Pending => parts.push(part("checks …", Tone::Muted)),
                CheckState::None => {}
            }
            if pr.unresolved_threads > 0 {
                parts.push(part(
                    &counted(pr.unresolved_threads as usize, "thread"),
                    Tone::Muted,
                ));
            }
            // A conflict outranks the draft flag, so the error color is never hidden.
            let merge_word = match pr.merge_state {
                MergeState::Dirty => Some(("conflict", Tone::Bad)),
                _ if pr.draft => Some(("draft", Tone::Muted)),
                MergeState::Behind => Some(("behind", Tone::Muted)),
                MergeState::Blocked => Some(("blocked", Tone::Muted)),
                _ => None,
            };
            if let Some((word, tone)) = merge_word {
                parts.push(part(word, tone));
            }
            if let Some(unreviewed) = unreviewed {
                parts.push(part(&not_reviewed(unreviewed), Tone::Warn));
            }
            if let Some(keeper) =
                keeper.filter(|k| comments.is_none() || *k != KeeperPart::UpToDate)
            {
                let tone = match keeper {
                    KeeperPart::Resolving | KeeperPart::Fixing => Tone::Busy,
                    _ => Tone::Muted,
                };
                parts.push(part(&keeper_words(keeper, now), tone));
            }
            if let Some(comments) = comments {
                parts.extend(waiting_comments(comments).map(|(text, tone)| part(&text, tone)));
            }
        }
    }
    parts
}

const MINUTE: i64 = 60_000;

/// The keeper's part of a PR line, with minutes rounded up before a rebase and down after it.
fn keeper_words(part: KeeperPart, now: i64) -> String {
    match part {
        KeeperPart::RebaseAt(at) => {
            let minutes = ((at - now).max(1) as u64).div_ceil(MINUTE as u64);
            format!("rebase in {minutes} min")
        }
        KeeperPart::Waiting => "rebase waiting".into(),
        KeeperPart::Rebased(at) => format!("rebased {}", rebased_ago(at, now)),
        KeeperPart::UpToDate => "up to date".into(),
        KeeperPart::Resolving => "keeper resolving".into(),
        KeeperPart::Fixing => "keeper fixing".into(),
        KeeperPart::WithCoordinator => "with the coordinator".into(),
    }
}

/// How long ago the keeper rebased, in the same words on the PR line and the card: whole
/// minutes, at least one, within the hour; the app's usual age after that.
fn rebased_ago(at: i64, now: i64) -> String {
    match now - at {
        elapsed if elapsed < 60 * MINUTE => format!("{} min ago", (elapsed / MINUTE).max(1)),
        _ => age(at).to_lowercase(),
    }
}

/// The sorted comments waiting on a PR, as "1 needs a change" and "2 questions".
fn waiting_comments(comments: &KeeperComments) -> impl Iterator<Item = (String, Tone)> {
    [
        (comments.needs_change > 0).then(|| {
            (
                format!("{} needs a change", comments.needs_change),
                Tone::Warn,
            )
        }),
        (comments.questions > 0).then(|| {
            (
                counted(comments.questions as usize, "question"),
                Tone::Muted,
            )
        }),
    ]
    .into_iter()
    .flatten()
}

/// The sorted comments waiting on the ticket's PR, for its card.
pub fn comments_lines(ticket: &Ticket) -> Option<(String, String)> {
    let words: Vec<String> = waiting_comments(ticket.waiting_comments()?)
        .map(|(text, _)| text)
        .collect();
    Some((
        format!("{} in new comments", words.join(" · ")),
        "the coordinator was sent their links · counted until the next review starts".into(),
    ))
}

fn not_reviewed(unreviewed: &Unreviewed) -> String {
    format!(
        "{} not reviewed",
        counted(unreviewed.commits as usize, "commit")
    )
}

/// What the keeper last did to the ticket's branch, for its card: a line, and a detail line.
/// Adopted commits no review has checked come first, since they hold the ticket; then a try
/// newer than the keeper's last action, which a try that was thrown away leaves as it was,
/// shown once thrown away only while the PR's head is the try's.
pub fn keeper_lines(ticket: &Ticket) -> Option<(String, String)> {
    if let Some(unreviewed) = ticket.unreviewed() {
        return Some((
            format!("{}, taken from GitHub's branch", not_reviewed(unreviewed)),
            format!(
                "now at {} · nothing more is pushed or accepted until a review that starts after it ends",
                short_sha(&unreviewed.head)
            ),
        ));
    }
    let keeper = ticket.keeper.as_ref()?;
    let base = ticket
        .pull_request
        .as_ref()
        .map_or("its base", |pr| pr.base.as_str());
    let now = crate::ui::now();
    let last = keeper.last.as_ref();
    let head = ticket.pull_request.as_ref().map(|pr| pr.head.as_str());
    if let Some(tried) = keeper
        .tries
        .last()
        .filter(|t| last.is_none_or(|l| t.started >= l.at))
        .filter(|t| {
            !matches!(t.ended, Some(TryEnd::ThrownAway(_))) || head == Some(t.head.as_str())
        })
    {
        return try_lines(tried, base, now);
    }
    let last = last?;
    // A changed patch or a try's result is on the branch while its review runs.
    let review = ticket.running_cycle().filter(|v| v.keeper).is_some();
    let reviewed = match review {
        true => "being reviewed before the push",
        false => "reviewed before the push",
    };
    let pushed = match (review, last.pushed) {
        (true, _) => "and is having it reviewed",
        (false, true) => "and pushed",
        (false, false) => "but did not push it",
    };
    let ago = rebased_ago(last.at, now);
    let files = keeper
        .tries
        .iter()
        .rev()
        .find(|t| t.head == last.before)
        .map_or(&[][..], |t| &t.files);
    let (line, detail) = match last.kind {
        KeeperKind::Rebase => (
            format!(
                "rebased onto {base} at {} {pushed}, {ago}",
                short_sha(&last.base)
            ),
            "the same change as the reviewed commit, so the passed review stands".into(),
        ),
        KeeperKind::ReviewedRebase => (
            format!(
                "rebased onto {base} at {} {pushed}, {ago}",
                short_sha(&last.base)
            ),
            format!("a changed patch · the repository's checks passed · {reviewed}"),
        ),
        KeeperKind::Resolved => (
            format!(
                "resolved a conflict with {base} in {} {pushed}, {ago}",
                counted(files.len(), "file")
            ),
            format!(
                "changed only {} · the repository's checks passed · {reviewed}",
                files.join(", ")
            ),
        ),
        KeeperKind::Fixed => (
            format!("fixed failed checks {pushed}, {ago}"),
            format!(
                "only in files the pull request changes · the repository's checks passed · {reviewed}"
            ),
        ),
    };
    let commits = format!("{} → {}", short_sha(&last.before), short_sha(&last.after));
    Some((line, format!("{commits} · {detail}")))
}

/// A running try, or one thrown away, for the card.
fn try_lines(tried: &KeeperTry, base: &str, now: i64) -> Option<(String, String)> {
    let head = short_sha(&tried.head);
    let ago = rebased_ago(tried.started, now);
    let files = tried.files.join(", ");
    match &tried.ended {
        None if tried.problem == KeeperProblem::Checks => Some((
            format!("trying once to fix failed checks at head {head}, {ago}"),
            "only in files the pull request changes · nothing is pushed unless the repository's checks pass and a review passes".into(),
        )),
        None => Some((
            format!("trying once to resolve a conflict with {base} at head {head}, {ago}"),
            format!(
                "in {files} · nothing is pushed unless only those files change, the repository's checks pass and a review passes"
            ),
        )),
        Some(TryEnd::ThrownAway(why)) => Some((
            format!("tried once at head {head} and threw the try away, {ago}"),
            format!(
                "{} Nothing was pushed. Handed to the coordinator; the keeper will not try this head again.",
                capitalize_first(why)
            ),
        )),
        Some(TryEnd::Pushed | TryEnd::NotPushed) => None,
    }
}

fn capitalize_first(words: &str) -> String {
    let mut chars = words.chars();
    chars.next().map_or(String::new(), |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Each keeper review that was withdrawn, newest first, for the card: kept on the keeper's
/// record and not among the ticket's cycles, with its verdicts and its findings' decisions.
pub fn withdrawn_lines(ticket: &Ticket) -> Vec<(String, String)> {
    let withdrawn = ticket
        .keeper_reviews()
        .iter()
        .rev()
        .filter_map(|review| Some((review, review.withdrawn.as_ref()?)));
    withdrawn
        .map(|(review, withdrawn)| {
            let level = withdrawn
                .cycle
                .level
                .as_ref()
                .map_or("re-check".into(), CycleLevel::label);
            let branch = match withdrawn.putting_back {
                true => format!("putting the branch back to {}", short_sha(&review.before)),
                false => "nothing was pushed".into(),
            };
            let verdicts = withdrawn
                .cycle
                .latest_results()
                .into_iter()
                .map(|run| format!("{} {}", run.focus, run.result.label()));
            let findings = withdrawn.entries.iter().map(|entry| {
                let decision = match &entry.reason {
                    Some(reason) => format!("{} ({reason})", entry.status.label()),
                    None => entry.status.label().into(),
                };
                format!(
                    "{} {decision}: {} at {}",
                    entry.id, entry.finding.summary, entry.finding.location
                )
            });
            let detail: Vec<String> = [branch]
                .into_iter()
                .chain(verdicts)
                .chain(findings)
                .collect();
            (
                format!(
                    "withdrew its {level} of {}, cycle {}: it did not pass",
                    short_sha(&review.after),
                    review.cycle
                ),
                detail.join(" · "),
            )
        })
        .collect()
}

/// The PR in full words, for the row's tooltip and the ticket's card.
pub fn pr_sentence(pr: &PullRequest) -> String {
    let mut parts = vec![match pr.state {
        PrState::Merged => match &pr.merge_commit {
            Some(commit) => format!(
                "PR #{} merged into {} as {}",
                pr.number,
                pr.base,
                short_sha(commit)
            ),
            None => format!("PR #{} merged into {}", pr.number, pr.base),
        },
        PrState::Closed => format!("PR #{} closed without merging", pr.number),
        PrState::Open if pr.draft => format!("PR #{} draft into {}", pr.number, pr.base),
        PrState::Open => format!("PR #{} open into {}", pr.number, pr.base),
    }];
    if pr.state == PrState::Open {
        match pr.checks {
            CheckState::Success => parts.push("checks passed".into()),
            CheckState::Failure => parts.push("checks failed".into()),
            CheckState::Pending => parts.push("checks running".into()),
            CheckState::None => {}
        }
        if pr.unresolved_threads > 0 {
            parts.push(counted(pr.unresolved_threads as usize, "unresolved thread"));
        }
        match pr.merge_state {
            MergeState::Behind => parts.push(format!("behind {}", pr.base)),
            MergeState::Dirty => parts.push(format!("conflicts with {}", pr.base)),
            MergeState::Blocked => parts.push("blocked".into()),
            _ => {}
        }
    }
    parts.join(" · ")
}

/// When the ticket's repository last had its PRs checked, as "Checked 3m ago".
pub fn pr_checked(snapshot: &Snapshot, ticket: &Ticket) -> Option<String> {
    let at = snapshot
        .pull_request_checks
        .iter()
        .find(|c| c.repository_id == ticket.repository_id)?
        .checked_at?;
    Some(format!("Checked {}", age(at).to_lowercase()))
}

/// One decision-ledger entry as the Tickets panel lists it.
#[derive(Debug, PartialEq)]
pub struct LedgerRow {
    pub id: String,
    pub summary: String,
    /// Its location and source: "src/calls.rs:42 · Regressions · cycle 2, round 1".
    pub place: String,
    pub status: EntryStatus,
    /// Its severity, then the coordinator's reason once decided.
    pub note: String,
}

pub fn ledger_rows(ticket: &Ticket) -> Vec<LedgerRow> {
    ticket
        .ledger
        .iter()
        .map(|entry| {
            let severity = match entry.finding.severity {
                Severity::Blocking => "Blocking",
                Severity::NonBlocking => "Non-blocking",
                Severity::PreExisting => "Pre-existing",
            };
            LedgerRow {
                id: entry.id.clone(),
                summary: entry.finding.summary.clone(),
                place: format!(
                    "{} · {} · cycle {}, round {}",
                    entry.finding.location, entry.focus, entry.cycle, entry.round
                ),
                status: entry.status,
                note: match &entry.reason {
                    Some(reason) => format!("{severity} · {reason}"),
                    None => severity.into(),
                },
            }
        })
        .collect()
}

/// An agent's short figure: its word, or its checklist count such as "3 of 6". An idle agent
/// with no checklist has none.
pub fn agent_figure(agent: &AgentProgress) -> Option<String> {
    match (agent.word, &agent.plan) {
        (Some(word), _) => Some(word.label().into()),
        (None, Some(plan)) => Some(format!("{} of {}", plan.done, plan.total)),
        (None, None) => None,
    }
}

/// A sidebar row's figure for an agent's `blocked` reports its coordinator has not answered.
pub fn questions_figure(count: u32) -> Option<String> {
    (count > 0).then(|| counted(count as usize, "question"))
}

/// A status badge's text: the status, then any questions waiting beside it.
pub fn badge_label(status: &str, questions: u32) -> String {
    match questions_figure(questions) {
        Some(figure) => format!("{status} · {figure} waiting"),
        None => status.into(),
    }
}

/// A ticket's short figure: "blocked", or its percentage. A striped ticket has none.
pub fn ticket_figure(ticket: &TicketProgress) -> Option<String> {
    match ticket.state {
        BarState::Blocked => Some("blocked".into()),
        BarState::Indeterminate => None,
        _ => Some(format!("{}%", percent(ticket.position))),
    }
}

/// A project's percentage; none while it plans with no checklist to count.
pub fn project_figure(project: &ProjectProgress) -> Option<String> {
    project
        .position
        .map(|position| format!("{}%", percent(position)))
}

/// The coordinator's strip figure, such as "56% · 1 of 4 tickets accepted". Before any ticket
/// exists it is the percentage alone.
pub fn project_summary(project: &ProjectProgress) -> String {
    let tickets = project.tickets.len();
    let accepted = (tickets > 0).then(|| {
        format!(
            "{} of {} accepted",
            project.accepted,
            counted(tickets, "ticket")
        )
    });
    let parts = project_figure(project).into_iter().chain(accepted);
    parts.collect::<Vec<_>>().join(" · ")
}

/// "6 agents", the crew button's label.
pub fn crew_label(agents: usize) -> String {
    counted(agents, "agent")
}

/// The strip's label for an agent. Only a working agent reads "Step K of N" with its current
/// item; `None` when the agent has nothing of its own to show.
pub fn strip_label(agent: &AgentProgress) -> Option<String> {
    // The item being worked on, or the finished count when every item is done.
    let step = agent.plan.as_ref().map(|plan| {
        if plan.done < plan.total {
            format!("Step {} of {}", plan.done + 1, plan.total)
        } else {
            format!("{} of {}", plan.total, plan.total)
        }
    });
    match (agent.word, &agent.plan) {
        (Some(Word::Passed), _) => Some("Passed".into()),
        (Some(Word::Done), _) => Some("Done".into()),
        (Some(Word::Failed), _) => Some("Failed".into()),
        (Some(Word::Blocked), _) => Some(match step {
            Some(step) => format!("Blocked · {}", step.to_lowercase()),
            None => "Blocked".into(),
        }),
        (Some(Word::Working), _) => Some("Working".into()),
        (None, Some(plan)) if agent.working => Some(match &plan.current {
            Some(current) => format!("{} · {current}", step?),
            None => step?,
        }),
        (None, Some(plan)) => Some(format!("{} of {}", plan.done, plan.total)),
        (None, None) => None,
    }
}

/// The strip's figure for an agent: its own percentage, then its ticket's, such as
/// "50% · ticket 30%". A striped ticket is named by its stage.
pub fn strip_figure(agent: Option<&AgentProgress>, ticket: Option<&TicketProgress>) -> String {
    let own = agent
        .and_then(AgentProgress::position)
        .map(|position| format!("{}%", percent(position)));
    let ticket = ticket.map(|ticket| {
        let figure = ticket_figure(ticket).unwrap_or_else(|| ticket.stage.label().to_lowercase());
        format!("ticket {figure}")
    });
    own.into_iter()
        .chain(ticket)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A crew card's heading figure, such as "Verifying · 70%" or "Verifying · 80% · blocked".
pub fn stage_figure(ticket: &TicketProgress) -> String {
    let stage = ticket.stage.label();
    let position = percent(ticket.position);
    match ticket.state {
        BarState::Indeterminate => stage.into(),
        BarState::Blocked => format!("{stage} · {position}% · blocked"),
        _ => format!("{stage} · {position}%"),
    }
}

/// How long the agent's run has taken, in whole minutes: since it started while the agent
/// works, and otherwise the length of its latest run.
pub fn elapsed_label(agent: &AgentProgress, now: i64) -> Option<String> {
    let started = agent.run_started_at?;
    let end = if agent.working {
        now
    } else {
        agent
            .run_finished_at
            .filter(|&finished| finished >= started)?
    };
    Some(format!("{}m", (end - started).max(0) / 60_000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ticket_store as store;

    fn agent(word: Option<Word>, plan: Option<(u32, u32)>, working: bool) -> AgentProgress {
        let state = match word {
            Some(Word::Passed | Word::Done) => BarState::Finished,
            Some(Word::Failed | Word::Blocked) => BarState::Blocked,
            Some(Word::Working) => BarState::Indeterminate,
            None => BarState::InProgress,
        };
        AgentProgress {
            session_id: "agent".into(),
            state,
            plan: plan.map(|(done, total)| PlanCounts {
                done,
                total,
                current: (done < total).then(|| "Update the design README".into()),
            }),
            word,
            working,
            run_started_at: None,
            run_finished_at: None,
        }
    }
    fn ticket_at(stage: Stage, state: BarState, position: u16) -> TicketProgress {
        TicketProgress {
            ticket_id: "ticket".into(),
            stage,
            state,
            position,
            agents: vec![],
        }
    }
    fn project_at(
        state: BarState,
        position: Option<u16>,
        accepted: u32,
        tickets: usize,
    ) -> ProjectProgress {
        ProjectProgress {
            project_id: "project".into(),
            phase: Phase::Implementing,
            state,
            position,
            accepted,
            tickets: (0..tickets).map(|n| n.to_string()).collect(),
            coordinator_id: "coordinator".into(),
        }
    }

    #[test]
    fn an_agents_figure_is_its_word_or_its_checklist_count() {
        let figure = |word, plan, working| agent_figure(&agent(word, plan, working));
        assert_eq!(figure(None, Some((3, 5)), true).as_deref(), Some("3 of 5"));
        assert_eq!(figure(None, Some((4, 6)), false).as_deref(), Some("4 of 6"));
        assert_eq!(
            figure(Some(Word::Working), None, true).as_deref(),
            Some("working")
        );
        assert_eq!(
            figure(Some(Word::Done), Some((5, 6)), false).as_deref(),
            Some("done")
        );
        assert_eq!(
            figure(Some(Word::Passed), Some((3, 5)), false).as_deref(),
            Some("passed")
        );
        assert_eq!(
            figure(Some(Word::Failed), Some((4, 5)), false).as_deref(),
            Some("failed")
        );
        assert_eq!(
            figure(Some(Word::Blocked), Some((3, 7)), false).as_deref(),
            Some("blocked")
        );
        assert_eq!(figure(None, None, false), None);
    }

    #[test]
    fn questions_waiting_sit_beside_the_status_and_vanish_at_zero() {
        assert_eq!(badge_label("Working", 1), "Working · 1 question waiting");
        assert_eq!(badge_label("Blocked", 2), "Blocked · 2 questions waiting");
        assert_eq!(badge_label("Blocked", 0), "Blocked");
        assert_eq!(questions_figure(1).as_deref(), Some("1 question"));
        assert_eq!(questions_figure(2).as_deref(), Some("2 questions"));
        assert_eq!(questions_figure(0), None);
    }

    #[test]
    fn ticket_and_project_figures_show_a_percentage_only_where_one_is_defined() {
        use BarState::*;
        let figure = |state, position| ticket_figure(&ticket_at(Stage::Verifying, state, position));
        assert_eq!(figure(InProgress, 7_000).as_deref(), Some("70%"));
        assert_eq!(figure(Finished, 10_000).as_deref(), Some("100%"));
        assert_eq!(figure(Blocked, 2_571).as_deref(), Some("blocked"));
        assert_eq!(figure(Indeterminate, 0), None);
        let project = project_at(InProgress, Some(5_600), 1, 4);
        assert_eq!(project_figure(&project).as_deref(), Some("56%"));
        assert_eq!(project_summary(&project), "56% · 1 of 4 tickets accepted");
        assert_eq!(
            project_summary(&project_at(InProgress, Some(5_000), 0, 1)),
            "50% · 0 of 1 ticket accepted"
        );
        // Planning: the coordinator's own checklist, or stripes with no figure at all.
        assert_eq!(
            project_summary(&project_at(InProgress, Some(2_500), 0, 0)),
            "25%"
        );
        assert_eq!(project_summary(&project_at(Indeterminate, None, 0, 0)), "");
        assert_eq!(
            (crew_label(6), crew_label(1)),
            ("6 agents".into(), "1 agent".into())
        );
    }

    #[test]
    fn the_strip_says_step_k_of_n_only_for_a_working_agent() {
        let label = |word, plan, working| strip_label(&agent(word, plan, working));
        assert_eq!(
            label(None, Some((3, 6)), true).as_deref(),
            Some("Step 4 of 6 · Update the design README")
        );
        assert_eq!(label(None, Some((6, 6)), true).as_deref(), Some("6 of 6"));
        assert_eq!(
            label(Some(Word::Working), None, true).as_deref(),
            Some("Working")
        );
        // Finished and blocked agents are named by their state, never by a next step.
        assert_eq!(
            label(Some(Word::Done), Some((5, 6)), false).as_deref(),
            Some("Done")
        );
        assert_eq!(
            label(Some(Word::Passed), Some((3, 5)), false).as_deref(),
            Some("Passed")
        );
        assert_eq!(
            label(Some(Word::Failed), Some((4, 5)), false).as_deref(),
            Some("Failed")
        );
        assert_eq!(
            label(Some(Word::Blocked), Some((3, 7)), false).as_deref(),
            Some("Blocked · step 4 of 7")
        );
        assert_eq!(
            label(Some(Word::Blocked), None, false).as_deref(),
            Some("Blocked")
        );
        // An idle agent keeps its count, with no step and no current item.
        assert_eq!(label(None, Some((4, 6)), false).as_deref(), Some("4 of 6"));
        assert_eq!(label(None, None, false), None);
    }

    #[test]
    fn the_strip_figure_joins_the_agents_percentage_and_its_tickets() {
        use BarState::*;
        let at = |state, position| ticket_at(Stage::Implementing, state, position);
        let working = agent(None, Some((3, 6)), true);
        assert_eq!(
            strip_figure(Some(&working), Some(&at(InProgress, 3_000))),
            "50% · ticket 30%"
        );
        let finished = agent(None, Some((6, 6)), true);
        assert_eq!(
            strip_figure(Some(&finished), Some(&at(InProgress, 6_000))),
            "100% · ticket 60%"
        );
        let done = agent(Some(Word::Done), Some((5, 6)), false);
        assert_eq!(
            strip_figure(Some(&done), Some(&at(InProgress, 6_000))),
            "100% · ticket 60%"
        );
        let planless = agent(Some(Word::Working), None, true);
        let verifying = ticket_at(Stage::Verifying, InProgress, 7_000);
        assert_eq!(
            strip_figure(Some(&planless), Some(&verifying)),
            "ticket 70%"
        );
        assert_eq!(
            strip_figure(Some(&planless), Some(&at(Indeterminate, 0))),
            "ticket implementing"
        );
        assert_eq!(
            strip_figure(Some(&planless), Some(&at(Blocked, 2_571))),
            "ticket blocked"
        );
        assert_eq!(strip_figure(Some(&working), None), "50%");
        assert_eq!(strip_figure(None, None), "");
    }

    #[test]
    fn a_crew_card_names_the_stage_and_percentage() {
        use BarState::*;
        let figure = |stage, state, position| stage_figure(&ticket_at(stage, state, position));
        assert_eq!(
            figure(Stage::Verifying, InProgress, 7_000),
            "Verifying · 70%"
        );
        assert_eq!(figure(Stage::Accepted, Finished, 10_000), "Accepted · 100%");
        assert_eq!(
            figure(Stage::Verifying, Blocked, 8_000),
            "Verifying · 80% · blocked"
        );
        assert_eq!(
            figure(Stage::Implementing, Indeterminate, 0),
            "Implementing"
        );
    }

    #[test]
    fn elapsed_time_runs_while_working_and_is_the_runs_length_otherwise() {
        const MINUTE: i64 = 60_000;
        let mut running = agent(None, None, true);
        (running.run_started_at, running.run_finished_at) = (Some(MINUTE), Some(30_000));
        assert_eq!(
            elapsed_label(&running, 13 * MINUTE + 59_000).as_deref(),
            Some("12m")
        );
        let mut finished = agent(Some(Word::Done), None, false);
        (finished.run_started_at, finished.run_finished_at) = (Some(MINUTE), Some(42 * MINUTE));
        assert_eq!(
            elapsed_label(&finished, 500 * MINUTE).as_deref(),
            Some("41m")
        );
        assert_eq!(elapsed_label(&agent(None, None, false), MINUTE), None);
    }

    #[test]
    fn the_tickets_view_lists_each_round_with_its_commit_and_results() {
        let (_directory, mut host, _root, ticket) = store();
        host.set_verification(VerificationSettings::one_level(
            ["Claude", "Codex"]
                .map(|focus| VerifierConfig {
                    role: Role::Tester,
                    focus: focus.into(),
                    instruction: None,
                    provider: None,
                })
                .to_vec(),
            2,
            2,
        ))
        .unwrap();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        host.agent_tool(
            &implementer.id,
            "report",
            serde_json::json!({"message_id":"ready","kind":"ready_for_testing","body":"Done"}),
        )
        .unwrap();
        assert!(verification_lines(&host.ticket(&ticket.id).unwrap()).is_empty());
        let profile = host
            .model_selection()
            .unwrap()
            .prefill(Role::Tester)
            .unwrap();
        let choices = ["Claude", "Codex"].map(|focus| VerifierChoice {
            focus: focus.into(),
            profile: profile.clone(),
            reason: "Routine check".into(),
        });
        let verifying = host.verify_ticket(&ticket.id, "Full", &choices).unwrap();
        let round = &verifying.verification.as_ref().unwrap().rounds[0];
        // The verifier's turn takes its round input before it reports.
        host.advance_receipt(&round.verifiers[0].message_id, Receipt::Delivered)
            .unwrap();
        host.agent_tool(
            &round.verifiers[0].session_id,
            "report",
            serde_json::json!({"message_id":"ok","kind":"passed","body":"Green"}),
        )
        .unwrap();
        // Renaming the level later does not change the line of a cycle that ran under it.
        let mut renamed = host.settings().verification;
        renamed.levels[0].name = "Thorough".into();
        host.set_verification(renamed).unwrap();

        let lines = verification_lines(&host.ticket(&ticket.id).unwrap());

        let commit: String = round.commit.chars().take(7).collect();
        assert_eq!(
            lines,
            [
                "Verification cycle 1 · Full: running, 1 of 2 rounds".to_owned(),
                format!("Round 1 at {commit}: Tester · Claude passed, Tester · Codex pending"),
            ]
        );
    }

    fn entry(
        id: &str,
        severity: Severity,
        status: EntryStatus,
        reason: Option<&str>,
    ) -> LedgerEntry {
        LedgerEntry {
            id: id.into(),
            finding: Finding {
                severity,
                location: "live.rs:398".into(),
                summary: format!("Summary {id}"),
                trigger: "No base ref".into(),
                evidence: "Test fails".into(),
            },
            focus: "Regressions".into(),
            cycle: 2,
            round: 1,
            status,
            reason: reason.map(Into::into),
        }
    }

    fn verification(cycle: u32, outcome: VerificationOutcome, rounds: u32) -> Verification {
        Verification {
            cycle,
            level: None,
            max_rounds: 3,
            queued_at: None,
            keeper: false,
            outcome,
            rounds: (1..=rounds)
                .map(|round| VerificationRound {
                    round,
                    commit: format!("{round}abcdef0123"),
                    verifiers: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn each_cycles_line_names_its_level_and_says_when_it_was_a_re_check() {
        let (_directory, _host, _root, mut ticket) = store();
        let level = |name: &str, kind| {
            Some(CycleLevel {
                name: name.into(),
                kind,
            })
        };
        ticket.verification = Some(Verification {
            level: level("Light", LevelKind::Recheck),
            ..verification(3, VerificationOutcome::Passed, 1)
        });
        ticket.previous_cycles = vec![
            // Recorded before levels existed.
            verification(1, VerificationOutcome::Passed, 1),
            Verification {
                level: level("Full", LevelKind::FullReview),
                ..verification(2, VerificationOutcome::Passed, 2)
            },
        ];

        assert_eq!(
            verification_lines(&ticket),
            [
                "Verification cycle 3 · Light re-check: passed, 1 of 3 rounds",
                "Round 1 at 1abcdef: ",
                "Verification cycle 2 · Full: passed, 2 of 3 rounds",
                "Verification cycle 1: passed, 1 of 3 rounds",
            ]
        );
    }

    #[test]
    fn the_tickets_view_shows_every_cycle_and_the_ledger_with_its_decisions() {
        let (_directory, _host, _root, mut ticket) = store();
        ticket.verification = Some(verification(2, VerificationOutcome::Blocked, 3));
        ticket.previous_cycles = vec![verification(1, VerificationOutcome::Passed, 2)];
        ticket.ledger = vec![
            entry("F1", Severity::Blocking, EntryStatus::Fixed, None),
            entry(
                "F2",
                Severity::NonBlocking,
                EntryStatus::WontFix,
                Some("Matches the naming"),
            ),
            entry("F3", Severity::Blocking, EntryStatus::Open, None),
            entry(
                "F4",
                Severity::PreExisting,
                EntryStatus::FollowUp,
                Some("Ticket later"),
            ),
            entry("F5", Severity::NonBlocking, EntryStatus::Untriaged, None),
        ];

        let lines = verification_lines(&ticket);
        let rows = ledger_rows(&ticket);

        assert_eq!(lines[0], "Verification cycle 2: blocked, 3 of 3 rounds");
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], "Verification cycle 1: passed, 2 of 3 rounds");
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[1],
            LedgerRow {
                id: "F2".into(),
                summary: "Summary F2".into(),
                place: "live.rs:398 · Regressions · cycle 2, round 1".into(),
                status: EntryStatus::WontFix,
                note: "Non-blocking · Matches the naming".into(),
            }
        );
        assert_eq!(rows[2].note, "Blocking");
        assert_eq!(rows[3].note, "Pre-existing · Ticket later");
    }

    #[test]
    fn a_ticket_says_which_publish_step_waits_for_approval() {
        let (_directory, _host, _root, mut ticket) = store();
        assert_eq!(publish_waiting(&ticket), None);
        let mut request: PublishRequest = serde_json::from_value(serde_json::json!({
            "id": "r1", "caller": "c", "repository": "celadon-theme/website",
            "push_url": "git@github.com:celadon-theme/website.git", "branch": "wiffletree/t",
            "base": "main", "commit": "9e03c1b", "cycle": 1, "outcome": "passed",
            "size": "1 commit", "step": "push"
        }))
        .unwrap();
        // A step that is running without asking adds no line.
        ticket.publishing = Some(request.clone());
        assert_eq!(publish_waiting(&ticket), None);
        request.waiting = Some("item".into());
        ticket.publishing = Some(request.clone());
        assert_eq!(
            publish_waiting(&ticket).as_deref(),
            Some("push waits for your approval")
        );
        request.step = PublishStep::OpenPullRequest;
        ticket.publishing = Some(request);
        assert_eq!(
            publish_waiting(&ticket).as_deref(),
            Some("pull request waits for your approval")
        );
    }

    #[test]
    fn only_an_accepted_ticket_with_a_waiver_gets_the_waived_ring() {
        let (_directory, _host, _root, mut ticket) = store();
        ticket.ledger = vec![entry("F3", Severity::Blocking, EntryStatus::Open, None)];
        for state in [
            TicketState::Passed,
            TicketState::Blocked,
            TicketState::Accepted,
        ] {
            ticket.state = state;
            assert_eq!(waiver_note(&ticket), None, "{state}");
        }
        ticket.waiver = Some("F3 needs a fetched base".into());
        assert_eq!(
            waiver_note(&ticket).as_deref(),
            Some("Accepted with waiver · 1 open finding")
        );
        ticket.state = TicketState::Blocked;
        assert_eq!(waiver_note(&ticket), None);
    }

    fn pr(state: PrState, checks: CheckState, merge_state: MergeState) -> PullRequest {
        PullRequest {
            number: 142,
            url: "https://github.com/o/r/pull/142".into(),
            state,
            draft: false,
            base: "main".into(),
            head: "3daf610aa".into(),
            merge_state,
            checks,
            unresolved_threads: 0,
            comments: CommentCursors::default(),
            merge_commit: (state == PrState::Merged).then(|| "be5f484cc".into()),
        }
    }
    fn line(pr: &PullRequest) -> String {
        kept_line(pr, None)
    }
    /// The line of a PR with the keeper's part, a minute after the epoch.
    fn kept_line(pr: &PullRequest, keeper: Option<KeeperPart>) -> String {
        pr_line(pr, None, keeper, None, MINUTE)
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }
    fn tones(pr: &PullRequest) -> Vec<Tone> {
        pr_line(pr, None, None, None, 0)
            .iter()
            .map(|p| p.tone)
            .collect()
    }
    const MINUTE: i64 = 60_000;

    #[test]
    fn the_pr_line_shows_only_the_parts_that_apply() {
        use {CheckState as C, MergeState as M, PrState as S};
        let mut behind = pr(S::Open, C::Success, M::Behind);
        behind.unresolved_threads = 1;
        assert_eq!(line(&behind), "#142 · checks ✓ · 1 thread · behind");
        assert_eq!(
            tones(&behind),
            [Tone::Muted, Tone::Good, Tone::Muted, Tone::Muted]
        );
        assert_eq!(
            pr_sentence(&behind),
            "PR #142 open into main · checks passed · 1 unresolved thread · behind main"
        );
        let mut running = pr(S::Open, C::Pending, M::Clean);
        running.unresolved_threads = 2;
        assert_eq!(line(&running), "#142 · checks … · 2 threads");
        let failed = pr(S::Open, C::Failure, M::Unstable);
        assert_eq!(line(&failed), "#142 · checks ✗");
        assert_eq!(tones(&failed), [Tone::Muted, Tone::Bad]);
        assert_eq!(line(&pr(S::Open, C::Success, M::Clean)), "#142 · checks ✓");
        let conflict = pr(S::Open, C::Success, M::Dirty);
        assert_eq!(line(&conflict), "#142 · checks ✓ · conflict");
        assert_eq!(tones(&conflict)[2], Tone::Bad);
        assert_eq!(
            pr_sentence(&conflict),
            "PR #142 open into main · checks passed · conflicts with main"
        );
        let mut draft = pr(S::Open, C::None, M::Blocked);
        draft.draft = true;
        assert_eq!(line(&draft), "#142 · draft");
        assert_eq!(pr_sentence(&draft), "PR #142 draft into main · blocked");
        draft.merge_state = M::Dirty;
        assert_eq!(line(&draft), "#142 · conflict");
        assert_eq!(tones(&draft)[1], Tone::Bad);
        assert_eq!(
            pr_sentence(&draft),
            "PR #142 draft into main · conflicts with main"
        );
        assert_eq!(line(&pr(S::Open, C::None, M::Blocked)), "#142 · blocked");
        let mut merged = pr(S::Merged, C::Success, M::Unknown);
        merged.number = 141;
        merged.unresolved_threads = 1;
        assert_eq!(line(&merged), "#141 · merged");
        assert_eq!(tones(&merged), [Tone::Muted, Tone::Merged]);
        assert_eq!(pr_sentence(&merged), "PR #141 merged into main as be5f484");
        let closed = pr(S::Closed, C::Failure, M::Dirty);
        assert_eq!(line(&closed), "#142 · closed");
        assert_eq!(pr_sentence(&closed), "PR #142 closed without merging");
    }

    #[test]
    fn a_kept_pr_line_ends_with_what_the_keeper_is_doing() {
        use {CheckState as C, KeeperPart as K, MergeState as M, PrState as S};
        let mut rebased = pr(S::Open, C::Pending, M::Clean);
        rebased.number = 143;
        assert_eq!(
            kept_line(&rebased, Some(K::Rebased(0))),
            "#143 · checks … · rebased 1 min ago"
        );
        assert_eq!(
            pr_line(&rebased, None, Some(K::Rebased(0)), None, 0)[2].text,
            "rebased 1 min ago",
            "a moment ago"
        );
        // The base moved 40 seconds ago and the quiet period is 2 minutes: 80 seconds to go.
        let settling = pr(S::Open, C::Success, M::Behind);
        let due = MINUTE + 80_000;
        assert_eq!(
            kept_line(&settling, Some(K::RebaseAt(due))),
            "#142 · checks ✓ · behind · rebase in 2 min"
        );
        assert_eq!(
            kept_line(&settling, Some(K::Waiting)),
            "#142 · checks ✓ · behind · rebase waiting"
        );
        let current = pr(S::Open, C::Success, M::Clean);
        assert_eq!(
            kept_line(&current, Some(K::UpToDate)),
            "#142 · checks ✓ · up to date"
        );
        assert_eq!(
            pr_line(&current, None, Some(K::UpToDate), None, 0)[2].tone,
            Tone::Muted
        );
        // Not kept: no part. A finished PR has none either.
        assert_eq!(kept_line(&current, None), "#142 · checks ✓");
        let merged = pr(S::Merged, C::Success, M::Unknown);
        assert_eq!(kept_line(&merged, Some(K::UpToDate)), "#142 · merged");
    }

    #[test]
    fn sorted_comments_waiting_end_the_line_in_place_of_up_to_date() {
        use {CheckState as C, KeeperPart as K, MergeState as M, PrState as S};
        let mut waiting = pr(S::Open, C::Success, M::Clean);
        (waiting.number, waiting.unresolved_threads) = (88, 1);
        let comments = |needs_change, questions| KeeperComments {
            needs_change,
            questions,
            ..KeeperComments::default()
        };
        let text = |keeper, comments: &KeeperComments| {
            let parts = pr_line(&waiting, None, keeper, Some(comments), 0);
            parts
                .iter()
                .map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join(" · ")
        };
        assert_eq!(
            text(Some(K::UpToDate), &comments(1, 1)),
            "#88 · checks ✓ · 1 thread · 1 needs a change · 1 question"
        );
        assert_eq!(
            text(Some(K::Waiting), &comments(0, 2)),
            "#88 · checks ✓ · 1 thread · rebase waiting · 2 questions"
        );
        let parts = pr_line(&waiting, None, None, Some(&comments(1, 0)), 0);
        assert_eq!(parts.last().unwrap().tone, Tone::Warn);
        // Once the next review starts the ticket has none waiting, and the line names none.
        let mut ticket: Ticket = serde_json::from_value(serde_json::json!({
            "id":"t","coordinator_id":"c","repository_id":"r","title":"T","brief":"B",
            "worktree":"/w","branch":"wiffletree/t","state":"ready_for_testing"
        }))
        .unwrap();
        ticket.keeper = Some(Keeper {
            comments: comments(1, 1),
            ..Keeper::default()
        });
        assert_eq!(
            comments_lines(&ticket).unwrap().0,
            "1 needs a change · 1 question in new comments"
        );
        ticket.end_waiting_comments();
        assert!(ticket.waiting_comments().is_none() && comments_lines(&ticket).is_none());
    }

    #[test]
    fn adopted_commits_no_review_has_checked_show_as_a_warning_before_the_keepers_part() {
        use {CheckState as C, KeeperPart as K, MergeState as M, PrState as S};
        let unreviewed = |commits| Unreviewed {
            head: "5f2e7a0aa".into(),
            commits,
            cycle: 1,
        };
        let mut adopted = pr(S::Open, C::Pending, M::Clean);
        adopted.number = 141;
        let parts = pr_line(&adopted, Some(&unreviewed(1)), None, None, MINUTE);
        let texts: Vec<&str> = parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["#141", "checks …", "1 commit not reviewed"]);
        assert_eq!(parts[2].tone, Tone::Warn);
        let parts = pr_line(
            &adopted,
            Some(&unreviewed(2)),
            Some(K::UpToDate),
            None,
            MINUTE,
        );
        assert_eq!(parts[2].text, "2 commits not reviewed");
        assert_eq!(parts[3].text, "up to date");
        // The card says so first, whatever the keeper did before.
        let (_home, _host, _coordinator, mut ticket) = store();
        ticket.keeper = Some(Keeper {
            unreviewed: Some(unreviewed(1)),
            ..Default::default()
        });
        let (line, detail) = keeper_lines(&ticket).unwrap();
        assert_eq!(line, "1 commit not reviewed, taken from GitHub's branch");
        assert_eq!(
            detail,
            "now at 5f2e7a0 · nothing more is pushed or accepted until a review that starts after it ends"
        );
    }

    #[test]
    fn the_card_says_what_the_keeper_last_did() {
        let (_home, _host, _coordinator, mut ticket) = store();
        assert_eq!(keeper_lines(&ticket), None);
        ticket.pull_request = Some(pr(PrState::Open, CheckState::Pending, MergeState::Clean));
        let last = KeeperAction {
            kind: KeeperKind::Rebase,
            at: crate::ui::now() - 90_000,
            before: "e21f0aa11".into(),
            after: "77b0c13ff".into(),
            base: "4a1c2e9dd".into(),
            pushed: true,
        };
        ticket.keeper = Some(Keeper {
            last: Some(last),
            ..Default::default()
        });
        let (line, detail) = keeper_lines(&ticket).unwrap();
        assert_eq!(line, "rebased onto main at 4a1c2e9 and pushed, 1 min ago");
        // Older than the PR line shows, the usual age.
        let keeper = ticket.keeper.as_mut().unwrap();
        keeper.last.as_mut().unwrap().at = crate::ui::now() - 2 * 60 * MINUTE;
        let (line, _) = keeper_lines(&ticket).unwrap();
        assert_eq!(line, "rebased onto main at 4a1c2e9 and pushed, 2h ago");
        assert_eq!(
            detail,
            "e21f0aa → 77b0c13 · the same change as the reviewed commit, so the passed review stands"
        );
        let keeper = ticket.keeper.as_mut().unwrap();
        keeper.last.as_mut().unwrap().kind = KeeperKind::ReviewedRebase;
        let (_, detail) = keeper_lines(&ticket).unwrap();
        assert_eq!(
            detail,
            "e21f0aa → 77b0c13 · a changed patch · the repository's checks passed · reviewed before the push"
        );
    }

    #[test]
    fn a_try_shows_on_the_pr_line_and_the_card_until_the_head_changes() {
        let (_home, _host, _coordinator, mut ticket) = store();
        let mut conflicted = pr(PrState::Open, CheckState::None, MergeState::Dirty);
        conflicted.number = 139;
        ticket.pull_request = Some(conflicted.clone());
        let tried = KeeperTry {
            problem: KeeperProblem::Conflict,
            head: conflicted.head.clone(),
            base: "4a1c2e9dd".into(),
            files: vec!["src/sidebar.rs".into(), "src/tree.rs".into()],
            started: crate::ui::now() - 90_000,
            ended: None,
        };
        ticket.keeper = Some(Keeper {
            tries: vec![tried],
            ..Default::default()
        });
        // Being tried.
        let part = ticket.try_part();
        assert_eq!(
            kept_line(&conflicted, part),
            "#139 · conflict · keeper resolving"
        );
        assert_eq!(
            pr_line(&conflicted, None, part, None, 0)[2].tone,
            Tone::Busy
        );
        assert_eq!(
            keeper_lines(&ticket).unwrap(),
            (
                "trying once to resolve a conflict with main at head 3daf610, 1 min ago".into(),
                "in src/sidebar.rs, src/tree.rs · nothing is pushed unless only those files change, the repository's checks pass and a review passes".into()
            )
        );

        // Handed over.
        let thrown_away = TryEnd::ThrownAway("the Light review did not pass.".into());
        ticket.keeper.as_mut().unwrap().tries[0].ended = Some(thrown_away);
        conflicted.number = 140;
        let part = ticket.try_part();
        assert_eq!(
            kept_line(&conflicted, part),
            "#140 · conflict · with the coordinator"
        );
        assert_eq!(
            keeper_lines(&ticket).unwrap(),
            (
                "tried once at head 3daf610 and threw the try away, 1 min ago".into(),
                "The Light review did not pass. Nothing was pushed. Handed to the coordinator; the keeper will not try this head again.".into()
            )
        );

        ticket.pull_request.as_mut().unwrap().head = "5f2e7a0bb".into();
        assert_eq!(ticket.try_part(), None, "a new head");
        assert_eq!(keeper_lines(&ticket), None, "a new head");
    }

    #[test]
    fn a_try_at_failed_checks_reads_keeper_fixing_and_its_fix_reads_as_fixed() {
        let (_home, _host, _coordinator, mut ticket) = store();
        let failed = pr(PrState::Open, CheckState::Failure, MergeState::Clean);
        ticket.pull_request = Some(failed.clone());
        let at = crate::ui::now() - 90_000;
        let tried = KeeperTry {
            problem: KeeperProblem::Checks,
            head: failed.head.clone(),
            base: "4a1c2e9dd".into(),
            files: vec!["src/tree.rs".into()],
            started: at - MINUTE,
            ended: None,
        };
        ticket.keeper = Some(Keeper {
            tries: vec![tried],
            ..Default::default()
        });
        let part = ticket.try_part();
        assert_eq!(part, Some(KeeperPart::Fixing));
        assert_eq!(kept_line(&failed, part), "#142 · checks ✗ · keeper fixing");
        assert_eq!(pr_line(&failed, None, part, None, 0)[2].tone, Tone::Busy);
        assert_eq!(
            keeper_lines(&ticket).unwrap(),
            (
                "trying once to fix failed checks at head 3daf610, 2 min ago".into(),
                "only in files the pull request changes · nothing is pushed unless the repository's checks pass and a review passes".into()
            )
        );

        let keeper = ticket.keeper.as_mut().unwrap();
        keeper.tries[0].ended = Some(TryEnd::Pushed);
        keeper.last = Some(KeeperAction {
            kind: KeeperKind::Fixed,
            at,
            before: failed.head.clone(),
            after: "77b0c13ff".into(),
            base: "4a1c2e9dd".into(),
            pushed: true,
        });
        assert_eq!(
            keeper_lines(&ticket).unwrap(),
            (
                "fixed failed checks and pushed, 1 min ago".into(),
                "3daf610 → 77b0c13 · only in files the pull request changes · the repository's checks passed · reviewed before the push".into()
            )
        );
    }

    #[test]
    fn a_keeper_commit_reads_as_reviewed_only_once_its_review_ended() {
        let (_home, _host, _coordinator, mut ticket) = store();
        ticket.pull_request = Some(pr(PrState::Open, CheckState::Success, MergeState::Clean));
        let at = crate::ui::now() - 90_000;
        let tried = KeeperTry {
            problem: KeeperProblem::Conflict,
            head: "e21f0aa11".into(),
            base: "4a1c2e9dd".into(),
            files: vec!["src/sidebar.rs".into(), "src/tree.rs".into()],
            started: at - MINUTE,
            ended: None,
        };
        let last = KeeperAction {
            kind: KeeperKind::Resolved,
            at,
            before: "e21f0aa11".into(),
            after: "77b0c13ff".into(),
            base: "4a1c2e9dd".into(),
            pushed: false,
        };
        ticket.keeper = Some(Keeper {
            last: Some(last),
            tries: vec![tried.clone()],
            ..Default::default()
        });
        ticket.verification = Some(Verification {
            keeper: true,
            ..verification(2, VerificationOutcome::Running, 1)
        });
        assert_eq!(
            keeper_lines(&ticket).unwrap(),
            (
                "resolved a conflict with main in 2 files and is having it reviewed, 1 min ago".into(),
                "e21f0aa → 77b0c13 · changed only src/sidebar.rs, src/tree.rs · the repository's checks passed · being reviewed before the push".into()
            )
        );
        let keeper = ticket.keeper.as_mut().unwrap();
        keeper.last.as_mut().unwrap().kind = KeeperKind::ReviewedRebase;
        keeper.tries.clear();
        let (line, detail) = keeper_lines(&ticket).unwrap();
        assert_eq!(
            line,
            "rebased onto main at 4a1c2e9 and is having it reviewed, 1 min ago"
        );
        assert!(detail.ends_with(
            "a changed patch · the repository's checks passed · being reviewed before the push"
        ));

        ticket.verification.as_mut().unwrap().outcome = VerificationOutcome::Passed;
        let keeper = ticket.keeper.as_mut().unwrap();
        keeper.last.as_mut().unwrap().pushed = true;
        keeper.last.as_mut().unwrap().kind = KeeperKind::Resolved;
        keeper.tries = vec![KeeperTry {
            files: vec!["src/tree.rs".into()],
            ended: Some(TryEnd::Pushed),
            ..tried
        }];
        assert_eq!(
            keeper_lines(&ticket).unwrap(),
            (
                "resolved a conflict with main in 1 file and pushed, 1 min ago".into(),
                "e21f0aa → 77b0c13 · changed only src/tree.rs · the repository's checks passed · reviewed before the push".into()
            )
        );
    }

    #[test]
    fn a_keeper_review_reads_as_one_round_and_once_withdrawn_shows_on_the_keepers_record() {
        let (_home, _host, _coordinator, mut ticket) = store();
        let mut review = Verification {
            level: Some(CycleLevel {
                name: "Light".into(),
                kind: LevelKind::Recheck,
            }),
            max_rounds: 1,
            keeper: true,
            ..verification(2, VerificationOutcome::Passed, 1)
        };
        ticket.verification = Some(review.clone());
        assert_eq!(
            verification_lines(&ticket)[0],
            "Verification cycle 2 · Light re-check by the PR keeper: passed, 1 round"
        );

        review.outcome = VerificationOutcome::Blocked;
        review.rounds[0].verifiers = vec![VerifierRun {
            session_id: "v".into(),
            role: Role::Reviewer,
            focus: "Correctness".into(),
            message_id: "m".into(),
            result: VerifierResult::Failed,
            report_id: None,
            reason: None,
        }];
        ticket.verification = Some(verification(1, VerificationOutcome::Passed, 1));
        let decided = entry(
            "F5",
            Severity::Blocking,
            EntryStatus::FollowUp,
            Some("Later"),
        );
        let mut withdrawn = Withdrawn {
            cycle: review,
            entries: vec![decided],
            putting_back: true,
        };
        let record = |withdrawn: &Withdrawn| KeeperReview {
            cycle: 2,
            before: "3daf610aa".into(),
            after: "77b0c13ff".into(),
            base: "4a1c2e9dd".into(),
            state: TicketState::Passed,
            last: None,
            problem: None,
            withdrawn: Some(withdrawn.clone()),
            push_started: false,
        };
        ticket.keeper = Some(Keeper {
            reviews: vec![record(&withdrawn)],
            ..Default::default()
        });
        assert_eq!(
            withdrawn_lines(&ticket),
            [(
                "withdrew its Light re-check of 77b0c13, cycle 2: it did not pass".to_owned(),
                "putting the branch back to 3daf610 · Correctness failed · F5 follow-up (Later): Summary F5 at live.rs:398".to_owned()
            )]
        );
        // Not among the ticket's cycles.
        assert_eq!(verification_lines(&ticket).len(), 2);
        withdrawn.putting_back = false;
        ticket.keeper.as_mut().unwrap().reviews = vec![record(&withdrawn)];
        assert!(
            withdrawn_lines(&ticket)[0]
                .1
                .starts_with("nothing was pushed · ")
        );
    }
}
