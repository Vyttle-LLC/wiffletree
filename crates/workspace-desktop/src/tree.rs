//! The sidebar's shape: each project's coordinator, its ticket workspaces, and their agents.
use crate::ui::age;
use std::collections::BTreeSet;
use workspace_core::*;

pub enum Row<'a> {
    Project(&'a Session),
    Ticket {
        ticket: &'a Ticket,
        label: String,
    },
    Agent {
        session: &'a Session,
        depth: usize,
        label: String,
    },
}

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
    format!(
        "Verification cycle {}: {outcome}, {} of {} rounds",
        verification.cycle,
        verification.rounds.len(),
        verification.max_rounds
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

/// A ticket accepted with a waiver: its row's waived ring says this beside the label.
pub fn waiver_note(ticket: &Ticket) -> Option<String> {
    ticket
        .waiver
        .as_ref()
        .filter(|_| ticket.state == "accepted")?;
    let open = ticket
        .ledger
        .iter()
        .filter(|e| e.status == EntryStatus::Open)
        .count();
    let plural = if open == 1 { "" } else { "s" };
    Some(format!(
        "Accepted with waiver · {open} open finding{plural}"
    ))
}

/// How a part of a ticket's PR line is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Good,
    Bad,
    Merged,
}
#[derive(Debug, PartialEq)]
pub struct PrPart {
    pub text: String,
    pub tone: Tone,
}

/// A ticket row's PR line, such as "#142 · checks ✓ · 1 thread · behind", in parts. Only the
/// parts that apply appear; a finished PR shows `merged` or `closed` instead of its checks.
pub fn pr_line(pr: &PullRequest) -> Vec<PrPart> {
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
                let plural = if pr.unresolved_threads == 1 { "" } else { "s" };
                parts.push(part(
                    &format!("{} thread{plural}", pr.unresolved_threads),
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
        }
    }
    parts
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
            let plural = if pr.unresolved_threads == 1 { "" } else { "s" };
            parts.push(format!(
                "{} unresolved thread{plural}",
                pr.unresolved_threads
            ));
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
    (count > 0).then(|| format!("{count} question{}", if count == 1 { "" } else { "s" }))
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
        let plural = if tickets == 1 { "" } else { "s" };
        format!("{} of {tickets} ticket{plural} accepted", project.accepted)
    });
    let parts = project_figure(project).into_iter().chain(accepted);
    parts.collect::<Vec<_>>().join(" · ")
}

/// "6 agents", the crew button's label.
pub fn crew_label(agents: usize) -> String {
    format!("{agents} agent{}", if agents == 1 { "" } else { "s" })
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

/// Newest project first. Archived sessions appear only when `show_archived`; a ticket whose
/// agents are all archived, such as a closed one, goes with them.
pub fn rows<'a>(
    snapshot: &'a Snapshot,
    show_archived: bool,
    collapsed: &BTreeSet<String>,
) -> Vec<Row<'a>> {
    let shown = |s: &&Session| show_archived || !s.archived;
    let ticket_of = |id: &str| {
        snapshot
            .runtimes
            .iter()
            .find(|r| r.session_id == id)
            .and_then(|r| r.ticket_id.as_deref())
    };
    let focus_of = |id: &str| {
        snapshot
            .runtimes
            .iter()
            .find(|r| r.session_id == id)
            .and_then(|r| r.focus.as_deref())
    };
    let mut rows = vec![];
    for project in snapshot.projects.iter().rev() {
        let Some(root) = snapshot
            .sessions
            .iter()
            .filter(shown)
            .find(|s| s.project_id == project.id && s.parent_id.is_none())
        else {
            continue;
        };
        rows.push(Row::Project(root));
        if collapsed.contains(&project.id) {
            continue;
        }
        for ticket in snapshot
            .tickets
            .iter()
            .filter(|t| t.coordinator_id == root.id)
        {
            let on_ticket = |s: &&Session| ticket_of(&s.id) == Some(ticket.id.as_str());
            let agents: Vec<&Session> = snapshot
                .sessions
                .iter()
                .filter(shown)
                .filter(on_ticket)
                .collect();
            if agents.is_empty() && snapshot.sessions.iter().any(|s| on_ticket(&s)) {
                continue;
            }
            rows.push(Row::Ticket {
                ticket,
                label: ticket_label(snapshot, ticket),
            });
            for session in agents {
                rows.push(Row::Agent {
                    session,
                    depth: 2,
                    // The ticket row above already names the work.
                    label: session.role.agent_label(focus_of(&session.id)),
                });
            }
        }
        // Older stores can contain workers created before ticket ownership existed, and
        // retired repository coordinators stay readable from the archive.
        for session in snapshot
            .sessions
            .iter()
            .filter(shown)
            .filter(|s| s.parent_id.as_ref() == Some(&root.id) && ticket_of(&s.id).is_none())
        {
            let label = if session.role == Role::TaskOrchestrator {
                format!("{} (retired)", session.name)
            } else {
                session.name.clone()
            };
            rows.push(Row::Agent {
                session,
                depth: 1,
                label,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use workspace_host::Host;

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

    fn git(path: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    /// A temporary store with a project using `sagechat-sms` and one ticket in it.
    fn store() -> (tempfile::TempDir, Host, Session, Ticket) {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("sagechat-sms");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "Base"]);
        let mut host = Host::open(directory.path().join("home")).unwrap();
        host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Calls").unwrap();
        let root = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let ticket = host
            .create_ticket(&root.id, &repository.id, "Fix inbound calls", "Fix them")
            .unwrap();
        (directory, host, root, ticket)
    }

    fn shape(rows: &[Row]) -> Vec<(usize, String)> {
        rows.iter()
            .map(|row| match row {
                Row::Project(s) => (0, s.name.clone()),
                Row::Ticket { label, .. } => (1, label.clone()),
                Row::Agent { depth, label, .. } => (*depth, label.clone()),
            })
            .collect()
    }

    #[test]
    fn a_project_shows_its_coordinator_then_ticket_workspaces_then_their_agents() {
        let (_directory, mut host, _root, ticket) = store();
        host.assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        for focus in ["Correctness", "Style"] {
            host.assign_ticket(
                &ticket.id,
                Role::Reviewer,
                Provider::Codex,
                "Review",
                Some(focus),
            )
            .unwrap();
        }
        let snapshot = host.snapshot().unwrap();

        let rows = rows(&snapshot, false, &BTreeSet::new());

        assert_eq!(
            shape(&rows),
            [
                (0, "Calls".to_owned()),
                (1, "sagechat-sms · Fix inbound calls".into()),
                (2, "Implementer".into()),
                (2, "Reviewer · Correctness".into()),
                (2, "Reviewer · Style".into()),
            ]
        );
        let Row::Ticket { ticket: shown, .. } = &rows[1] else {
            panic!("ticket row")
        };
        assert_eq!(
            shown.state, "assigned",
            "the row carries the ticket's state"
        );
    }

    #[test]
    fn a_closed_ticket_hides_with_its_agents_and_a_collapsed_project_hides_everything() {
        let (_directory, mut host, root, ticket) = store();
        host.assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        host.close_ticket(&ticket.id).unwrap();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            shape(&rows(&snapshot, false, &BTreeSet::new())),
            [(0, "Calls".to_owned())]
        );
        assert_eq!(shape(&rows(&snapshot, true, &BTreeSet::new())).len(), 3);
        let collapsed = BTreeSet::from([root.project_id.clone()]);
        assert_eq!(rows(&snapshot, true, &collapsed).len(), 1);
    }

    #[test]
    fn the_tickets_view_lists_each_round_with_its_commit_and_results() {
        let (_directory, mut host, _root, ticket) = store();
        host.set_verification(VerificationSettings {
            verifiers: ["Claude", "Codex"]
                .map(|focus| VerifierConfig {
                    role: Role::Tester,
                    focus: focus.into(),
                    instruction: None,
                    provider: None,
                })
                .to_vec(),
            max_rounds: 2,
            max_cycles: 2,
        })
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
        let verifying = host.verify_ticket(&ticket.id, &choices).unwrap();
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

        let lines = verification_lines(&host.ticket(&ticket.id).unwrap());

        let commit: String = round.commit.chars().take(7).collect();
        assert_eq!(
            lines,
            [
                "Verification cycle 1: running, 1 of 2 rounds".to_owned(),
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
            max_rounds: 3,
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
    fn only_an_accepted_ticket_with_a_waiver_gets_the_waived_ring() {
        let (_directory, _host, _root, mut ticket) = store();
        ticket.ledger = vec![entry("F3", Severity::Blocking, EntryStatus::Open, None)];
        for state in ["passed", "blocked", "accepted"] {
            ticket.state = state.into();
            assert_eq!(waiver_note(&ticket), None, "{state}");
        }
        ticket.waiver = Some("F3 needs a fetched base".into());
        assert_eq!(
            waiver_note(&ticket).as_deref(),
            Some("Accepted with waiver · 1 open finding")
        );
        ticket.state = "blocked".into();
        assert_eq!(waiver_note(&ticket), None);
    }

    #[test]
    fn retired_coordinators_and_older_workers_sit_under_the_coordinator() {
        let (directory, host, root, ticket) = store();
        drop(host);
        // A repository coordinator as the previous release stored it, then migrated on open.
        let retired = Session {
            id: "retired".into(),
            project_id: root.project_id.clone(),
            parent_id: Some(root.id.clone()),
            repository_id: Some(ticket.repository_id.clone()),
            name: "sagechat-sms".into(),
            role: Role::TaskOrchestrator,
            provider: Provider::Codex,
            status: Status::Ready,
            archived: false,
        };
        let db =
            rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
        db.execute_batch(&format!(
            "INSERT INTO sessions VALUES ('retired','{}','{}','task_orchestrator','{}'); \
             UPDATE tickets SET coordinator_id='retired',data=json_set(data,'$.coordinator_id','retired'); \
             ALTER TABLE messages DROP COLUMN quiet; PRAGMA user_version = 5;",
            root.project_id,
            root.id,
            serde_json::to_string(&retired).unwrap()
        ))
        .unwrap();
        drop(db);
        let mut host = Host::open(directory.path().join("home")).unwrap();
        let older = host
            .create_session(
                &root.project_id,
                &root.id,
                None,
                "Older worker",
                Role::Implementer,
                Provider::Codex,
            )
            .unwrap();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            shape(&rows(&snapshot, false, &BTreeSet::new())),
            [
                (0, "Calls".to_owned()),
                (1, "sagechat-sms · Fix inbound calls".into()),
                (1, older.name.clone()),
            ]
        );
        let archive = shape(&rows(&snapshot, true, &BTreeSet::new()));
        assert!(
            archive.contains(&(1, "sagechat-sms (retired)".to_owned())),
            "{archive:?}"
        );
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
        pr_line(pr)
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }
    fn tones(pr: &PullRequest) -> Vec<Tone> {
        pr_line(pr).iter().map(|p| p.tone).collect()
    }

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
}
