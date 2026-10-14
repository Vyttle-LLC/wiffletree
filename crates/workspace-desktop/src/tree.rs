//! The sidebar's shape: projects in the host's order, gathered into groups by first word, each
//! with its coordinator, its ticket workspaces, and their agents.
use crate::ui::age;
use std::collections::BTreeSet;
use workspace_core::*;

pub enum Row<'a> {
    Group(Group<'a>),
    Project {
        root: &'a Session,
        /// Its name; inside a group, without the first word the group already shows.
        label: String,
        grouped: bool,
    },
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
        "{}: {outcome}, {} of {} rounds",
        verification.cycle_label("Verification cycle"),
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
/// parts that apply appear, and an open PR ends with the keeper's part, if it has one; a
/// finished PR shows `merged` or `closed` instead of its checks.
pub fn pr_line(pr: &PullRequest, keeper: Option<KeeperPart>, now: i64) -> Vec<PrPart> {
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
            if let Some(keeper) = keeper {
                parts.push(part(&keeper_words(keeper, now), Tone::Muted));
            }
        }
    }
    parts
}

/// The keeper's part of a PR line, with minutes rounded up before a rebase and down after it.
fn keeper_words(part: KeeperPart, now: i64) -> String {
    const MINUTE: i64 = 60_000;
    match part {
        KeeperPart::RebaseAt(at) => {
            let minutes = ((at - now).max(1) as u64).div_ceil(MINUTE as u64);
            format!("rebase in {minutes} min")
        }
        KeeperPart::Waiting => "rebase waiting".into(),
        KeeperPart::Rebased(at) => format!("rebased {} min ago", ((now - at) / MINUTE).max(1)),
        KeeperPart::UpToDate => "up to date".into(),
    }
}

/// What the keeper last did to the ticket's branch, for its card: a line, and a detail line.
pub fn keeper_lines(ticket: &Ticket) -> Option<(String, String)> {
    let last = ticket.keeper.as_ref()?.last.as_ref()?;
    let base = ticket
        .pull_request
        .as_ref()
        .map_or("its base", |pr| pr.base.as_str());
    let pushed = match last.pushed {
        true => "and pushed",
        false => "but did not push it",
    };
    Some((
        format!(
            "rebased onto {base} at {} {pushed}, {}",
            short_sha(&last.base),
            age(last.at).to_lowercase()
        ),
        format!(
            "{} → {} · the same change as the reviewed commit, so the passed review stands",
            short_sha(&last.before),
            short_sha(&last.after)
        ),
    ))
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

/// Projects whose names share a first word, drawn under one row while grouping is on.
#[derive(Debug)]
pub struct Group<'a> {
    /// The first word in lowercase: what the members share.
    pub key: String,
    /// The first word as the topmost member writes it.
    pub label: String,
    /// Their coordinators, in the host's order.
    pub members: Vec<&'a Session>,
}

/// A top-level row of the project list.
#[derive(Debug)]
pub enum Entry<'a> {
    Project(&'a Session),
    Group(Group<'a>),
}

/// A name's first word without trailing punctuation: "wiffletree" in "wiffletree: Sidebar".
fn first_word(name: &str) -> &str {
    let word = name.split_whitespace().next().unwrap_or_default();
    word.trim_end_matches(|c: char| !c.is_alphanumeric())
}

/// What projects in one group share: their first word, whatever its case.
pub fn group_key(name: &str) -> String {
    first_word(name).to_lowercase()
}

/// A grouped project's name without the first word and the separator after it, or the full
/// name when nothing else remains.
fn member_label(name: &str) -> &str {
    let rest = name
        .trim_start()
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim_start());
    // A separator standing on its own, as in "watch - Relay".
    let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let separator = |c: char| c.is_ascii_punctuation() || matches!(c, '–' | '—' | '·');
    let rest = if word.chars().all(separator) {
        after.trim_start()
    } else {
        rest
    };
    if rest.is_empty() { name } else { rest }
}

/// The shown projects in the host's order. With `grouped`, two or more that share a first word,
/// whatever its case, gather where the topmost of them is. Archived projects count only when
/// `show_archived`.
pub fn entries(snapshot: &Snapshot, show_archived: bool, grouped: bool) -> Vec<Entry<'_>> {
    let mut groups: Vec<Group> = vec![];
    for project in &snapshot.projects {
        let Some(root) = snapshot.sessions.iter().find(|s| {
            s.project_id == project.id && s.parent_id.is_none() && (show_archived || !s.archived)
        }) else {
            continue;
        };
        let key = group_key(&root.name);
        match groups
            .iter_mut()
            .find(|group| grouped && !key.is_empty() && group.key == key)
        {
            Some(group) => group.members.push(root),
            None => groups.push(Group {
                key,
                label: first_word(&root.name).into(),
                members: vec![root],
            }),
        }
    }
    groups
        .into_iter()
        .map(|group| match group.members[..] {
            [root] => Entry::Project(root),
            _ => Entry::Group(group),
        })
        .collect()
}

/// The most urgent state inside a collapsed group.
#[derive(Debug, PartialEq, Eq)]
pub enum Rollup {
    NeedsYou(usize),
    Blocked,
    Working,
}

/// A collapsed group's state, from the items in it that need the human and the status each
/// member's row shows.
pub fn rollup(needs_you: usize, shown: &[Status]) -> Option<Rollup> {
    if needs_you > 0 {
        Some(Rollup::NeedsYou(needs_you))
    } else if shown.contains(&Status::Blocked) {
        Some(Rollup::Blocked)
    } else if shown.contains(&Status::Working) {
        Some(Rollup::Working)
    } else {
        None
    }
}

/// What a drag moves as one: a project by its id, or a group, by its key, with all its members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Movable {
    Project(String),
    Group(String),
}

/// Where a dragged row lands among the rows it can be reordered with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drop {
    Before(Movable),
    /// After the last member of the group with this key.
    EndOfGroup(String),
    /// After the last top-level row.
    End,
}

/// The host command's arguments for one drop.
#[derive(Debug, PartialEq, Eq)]
pub struct ProjectMove {
    pub project_ids: Vec<String>,
    pub before: Option<String>,
}

impl Entry<'_> {
    pub fn movable(&self) -> Movable {
        match self {
            Entry::Project(root) => Movable::Project(root.project_id.clone()),
            Entry::Group(group) => Movable::Group(group.key.clone()),
        }
    }
    fn project_ids(&self) -> Vec<String> {
        match self {
            Entry::Project(root) => vec![root.project_id.clone()],
            Entry::Group(group) => group.project_ids(),
        }
    }
}
impl Group<'_> {
    fn project_ids(&self) -> Vec<String> {
        let ids = self.members.iter().map(|root| root.project_id.clone());
        ids.collect()
    }
}

/// The rows one row is reordered among: the top-level rows, or the members of one group.
struct Siblings<'a> {
    /// The group's key, when they are its members.
    group: Option<&'a str>,
    /// Each row with the projects it moves.
    rows: Vec<(Movable, Vec<String>)>,
}

impl<'a> Siblings<'a> {
    fn of(entries: &'a [Entry], row: &Movable) -> Option<Self> {
        if entries.iter().any(|entry| entry.movable() == *row) {
            let rows = entries
                .iter()
                .map(|entry| (entry.movable(), entry.project_ids()));
            return Some(Self {
                group: None,
                rows: rows.collect(),
            });
        }
        let Movable::Project(id) = row else {
            return None;
        };
        let group = entries.iter().find_map(|entry| match entry {
            Entry::Group(group) if group.members.iter().any(|m| m.project_id == *id) => Some(group),
            _ => None,
        })?;
        let rows = group
            .project_ids()
            .into_iter()
            .map(|id| (Movable::Project(id.clone()), vec![id]));
        Some(Self {
            group: Some(&group.key),
            rows: rows.collect(),
        })
    }
    fn position(&self, row: &Movable) -> Option<usize> {
        self.rows.iter().position(|(sibling, _)| sibling == row)
    }
}

/// Where a drop below `row` lands: before the row after it, or at the end of the rows it is
/// reordered among.
fn after(entries: &[Entry], row: &Movable) -> Option<Drop> {
    let siblings = Siblings::of(entries, row)?;
    let next = siblings.rows.get(siblings.position(row)? + 1);
    Some(match (next, siblings.group) {
        (Some((next, _)), _) => Drop::Before(next.clone()),
        (None, Some(key)) => Drop::EndOfGroup(key.into()),
        (None, None) => Drop::End,
    })
}

/// Where `dragged` lands when released over the upper and the lower half of the row `over`;
/// none when it may not land there. A grouped project stays among its group's members. A group
/// or an ungrouped project stays among the top-level rows, and over a group's member it lands
/// below that group.
pub fn drops(entries: &[Entry], dragged: &Movable, over: &Movable) -> Option<(Drop, Drop)> {
    let siblings = Siblings::of(entries, dragged)?;
    if siblings.position(over).is_some() {
        return Some((Drop::Before(over.clone()), after(entries, over)?));
    }
    let group = Movable::Group(Siblings::of(entries, over)?.group?.into());
    siblings.position(&group)?;
    let below = after(entries, &group)?;
    Some((below.clone(), below))
}

/// The move that puts `dragged` at `drop`; none when it is already there or may not land there.
pub fn project_move(entries: &[Entry], dragged: &Movable, drop: &Drop) -> Option<ProjectMove> {
    let siblings = Siblings::of(entries, dragged)?;
    let (group, rows) = (siblings.group, &siblings.rows);
    let from = siblings.position(dragged)?;
    let to = match drop {
        Drop::Before(target) => siblings.position(target)?,
        Drop::EndOfGroup(key) if group == Some(key.as_str()) => rows.len(),
        Drop::End if group.is_none() => rows.len(),
        _ => return None,
    };
    if to == from || to == from + 1 {
        return None;
    }
    let topmost = |at: usize| Some(rows.get(at)?.1[0].clone());
    if group.is_some() && to > from {
        // A group sits where its topmost member is, so that member moving down would take the
        // group with it. The members it passes move up instead, and the group stays put.
        let passed = rows[from + 1..to].iter().flat_map(|(_, ids)| ids.clone());
        return Some(ProjectMove {
            project_ids: passed.collect(),
            before: topmost(from),
        });
    }
    Some(ProjectMove {
        project_ids: rows[from].1.clone(),
        before: topmost(to),
    })
}

/// How near an edge of the list a dragged row starts scrolling it, and the most one tick scrolls.
const SCROLL_ZONE: f32 = 32.;
const SCROLL_STEP: f32 = 12.;

/// How far one tick scrolls the list while a row is dragged with the pointer at `pointer`
/// between the list's `top` and `bottom` edges: up when negative, down when positive, and
/// further the nearer the edge. Zero away from both edges and outside the list.
pub fn edge_scroll(pointer: f32, top: f32, bottom: f32) -> f32 {
    if pointer < top || pointer > bottom {
        return 0.;
    }
    let step = |from_edge: f32| ((SCROLL_ZONE - from_edge) / SCROLL_ZONE * SCROLL_STEP).ceil();
    if pointer - top < SCROLL_ZONE {
        -step(pointer - top)
    } else if bottom - pointer < SCROLL_ZONE {
        step(bottom - pointer)
    } else {
        0.
    }
}

/// How the press that may become a drag began on its row.
#[derive(Clone, Copy, Default)]
pub struct RowPress {
    pub on_chevron: bool,
    pub click_count: usize,
}

/// What releasing a dragged row over itself does.
#[derive(Debug, PartialEq, Eq)]
pub enum OwnRelease {
    /// The drag left its row and came back: it is cancelled.
    Nothing,
    Click,
    /// The second click of a double-click.
    Rename,
    /// Collapses or expands, as a click on the chevron does.
    Toggle,
}

/// A drag that never left its row is a click that moved, so it does what that click would have
/// done still; `on_chevron` is whether it was released over the row's chevron.
pub fn own_release(left_row: bool, press: RowPress, on_chevron: bool) -> OwnRelease {
    if left_row {
        OwnRelease::Nothing
    } else if press.on_chevron && on_chevron {
        OwnRelease::Toggle
    } else if press.click_count == 2 {
        OwnRelease::Rename
    } else {
        OwnRelease::Click
    }
}

/// Projects in the host's order, under their group's row while `grouped`; a collapsed group
/// hides its projects. Archived sessions appear only when `show_archived`; a ticket whose
/// agents are all archived, such as a closed one, goes with them.
pub fn rows<'a>(
    snapshot: &'a Snapshot,
    show_archived: bool,
    grouped: bool,
    collapsed: &BTreeSet<String>,
    collapsed_groups: &BTreeSet<String>,
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
    for entry in entries(snapshot, show_archived, grouped) {
        let projects = match entry {
            Entry::Project(root) => vec![(root, root.name.clone(), false)],
            Entry::Group(group) => {
                let members = if collapsed_groups.contains(&group.key) {
                    vec![]
                } else {
                    let labelled = |root: &&'a Session| {
                        let label = member_label(&root.name);
                        (*root, label.to_owned(), true)
                    };
                    group.members.iter().map(labelled).collect()
                };
                rows.push(Row::Group(group));
                members
            }
        };
        for (root, label, grouped) in projects {
            rows.push(Row::Project {
                root,
                label,
                grouped,
            });
            if collapsed.contains(&root.project_id) {
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
            for session in
                snapshot.sessions.iter().filter(shown).filter(|s| {
                    s.parent_id.as_ref() == Some(&root.id) && ticket_of(&s.id).is_none()
                })
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
        let mut host = Host::open_set_up(directory.path().join("home")).unwrap();
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
                Row::Group(group) => (0, format!("{} ({})", group.label, group.members.len())),
                Row::Project { label, .. } => (0, label.clone()),
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

        let rows = rows(&snapshot, false, true, &BTreeSet::new(), &BTreeSet::new());

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
        let none = BTreeSet::new();
        host.assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        host.close_ticket(&ticket.id).unwrap();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            shape(&rows(&snapshot, false, true, &none, &none)),
            [(0, "Calls".to_owned())]
        );
        assert_eq!(shape(&rows(&snapshot, true, true, &none, &none)).len(), 3);
        let collapsed = BTreeSet::from([root.project_id.clone()]);
        assert_eq!(rows(&snapshot, true, true, &collapsed, &none).len(), 1);
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
        let mut host = Host::open_set_up(directory.path().join("home")).unwrap();
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
        let none = BTreeSet::new();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            shape(&rows(&snapshot, false, true, &none, &none)),
            [
                (0, "Calls".to_owned()),
                (1, "sagechat-sms · Fix inbound calls".into()),
                (1, older.name.clone()),
            ]
        );
        let archive = shape(&rows(&snapshot, true, true, &none, &none));
        assert!(
            archive.contains(&(1, "sagechat-sms (retired)".to_owned())),
            "{archive:?}"
        );
    }

    /// A store whose projects read, top to bottom, as `names`.
    fn listed(names: &[&str]) -> (tempfile::TempDir, Host) {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(directory.path()).unwrap();
        for name in names.iter().rev() {
            host.create_project(name).unwrap();
        }
        (directory, host)
    }

    /// The project list as drawn: group rows with their count, their projects indented.
    fn drawn(snapshot: &Snapshot, show_archived: bool, grouped: bool) -> Vec<String> {
        let none = BTreeSet::new();
        rows(snapshot, show_archived, grouped, &none, &none)
            .iter()
            .map(|row| match row {
                Row::Group(group) => format!("{} ({})", group.label, group.members.len()),
                Row::Project {
                    label,
                    grouped: true,
                    ..
                } => format!("  {label}"),
                Row::Project { label, .. } => label.clone(),
                Row::Ticket { .. } | Row::Agent { .. } => panic!("no tickets here"),
            })
            .collect()
    }

    const MOCKUP: [&str; 9] = [
        "Trial reminder copy",
        "wiffletree: Need to reorder projects",
        "infra: Terraform state split",
        "watch: Call relay reconnect",
        "wiffletree: Repository owners",
        "ci-runners: Autoscaling",
        "Watch complications refresh",
        "infra: Neptune upgrade",
        "wiffletree: Compact context",
    ];

    #[test]
    fn projects_sharing_a_first_word_gather_where_the_topmost_of_them_is() {
        let (_directory, host) = listed(&MOCKUP);
        let snapshot = host.snapshot().unwrap();

        // Case and the colon do not matter, a lone first word stays an ordinary row, and
        // members keep the host's order under the word as their topmost member writes it.
        assert_eq!(
            drawn(&snapshot, false, true),
            [
                "Trial reminder copy",
                "wiffletree (3)",
                "  Need to reorder projects",
                "  Repository owners",
                "  Compact context",
                "infra (2)",
                "  Terraform state split",
                "  Neptune upgrade",
                "watch (2)",
                "  Call relay reconnect",
                "  complications refresh",
                "ci-runners: Autoscaling",
            ]
        );
        assert_eq!(drawn(&snapshot, false, false), MOCKUP, "grouping off");
    }

    #[test]
    fn a_first_word_ignores_case_and_trailing_punctuation() {
        let key = group_key;
        assert_eq!(key("Wiffletree: a"), key("wiffletree b"));
        assert_eq!(key("WATCH — relay"), key("watch, relay"));
        assert_eq!(first_word("ci-runners: Autoscaling"), "ci-runners");
        assert_ne!(key("wiffletree: a"), key("wiffle tree"));
        // Only whitespace ends the word, and a name with no word in front never groups.
        assert_eq!(first_word("infra:Neptune upgrade"), "infra:Neptune");
        assert_eq!(first_word("— untitled"), "");
        let (_directory, host) = listed(&["— one", "— two", "Wiffletree: a", "wiffletree b"]);
        assert_eq!(
            drawn(&host.snapshot().unwrap(), false, true),
            ["— one", "— two", "Wiffletree (2)", "  a", "  b"]
        );
    }

    #[test]
    fn a_grouped_row_drops_the_word_and_its_separator_unless_nothing_remains() {
        assert_eq!(
            member_label("wiffletree: Repository owners"),
            "Repository owners"
        );
        assert_eq!(
            member_label("Watch complications refresh"),
            "complications refresh"
        );
        assert_eq!(member_label("watch - Relay (beta)"), "Relay (beta)");
        assert_eq!(member_label("watch — Relay"), "Relay");
        assert_eq!(member_label("infra #48 state"), "#48 state");
        // Only punctuation separates: what follows the word is kept, letters or not.
        assert_eq!(member_label("watch: 🚀"), "🚀");
        assert_eq!(member_label("watch 🚀 launch"), "🚀 launch");
        assert_eq!(member_label("infra: 2026"), "2026");
        for bare in ["wiffletree", "wiffletree:", "wiffletree -"] {
            assert_eq!(member_label(bare), bare);
        }
    }

    #[test]
    fn groups_form_over_the_projects_shown_so_archived_ones_join_only_when_shown() {
        let (_directory, mut host) = listed(&["infra: State split", "Billing", "infra: Neptune"]);
        let neptune = host.sessions().unwrap().remove(0);
        assert_eq!(neptune.name, "infra: Neptune");
        host.set_archived(&neptune.id, true).unwrap();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            drawn(&snapshot, false, true),
            ["infra: State split", "Billing"]
        );
        assert_eq!(
            drawn(&snapshot, true, true),
            ["infra (2)", "  State split", "  Neptune", "Billing"]
        );
        assert_eq!(
            drawn(&snapshot, true, false),
            ["infra: State split", "Billing", "infra: Neptune"]
        );
    }

    #[test]
    fn a_collapsed_group_hides_its_projects_and_rolls_up_the_most_urgent_state() {
        let (_directory, host) = listed(&["infra: a", "Billing", "infra: b"]);
        let snapshot = host.snapshot().unwrap();
        let (none, infra) = (BTreeSet::new(), BTreeSet::from(["infra".to_owned()]));

        let collapsed = rows(&snapshot, false, true, &none, &infra);

        assert_eq!(
            shape(&collapsed),
            [(0, "infra (2)".to_owned()), (0, "Billing".into())]
        );
        use Status::*;
        assert_eq!(
            rollup(2, &[Blocked, Working]),
            Some(Rollup::NeedsYou(2)),
            "needs you first"
        );
        assert_eq!(rollup(0, &[Working, Blocked, Ready]), Some(Rollup::Blocked));
        assert_eq!(rollup(0, &[Done, Working, Paused]), Some(Rollup::Working));
        assert_eq!(rollup(0, &[Ready, Done, Paused, Disconnected]), None);
    }

    /// The order the host would hold after `dragged` is dropped at `drop`.
    fn dropped(host: &mut Host, grouped: bool, dragged: &Movable, drop: &Drop) -> Option<String> {
        let snapshot = host.snapshot().unwrap();
        let entries = entries(&snapshot, false, grouped);
        let moved = project_move(&entries, dragged, drop)?;
        let order = host
            .move_projects(&moved.project_ids, moved.before.as_deref())
            .unwrap();
        Some(order.iter().map(|p| p.name.as_str()).collect())
    }

    fn project(host: &Host, name: &str) -> Movable {
        let projects = host.projects().unwrap();
        Movable::Project(projects.into_iter().find(|p| p.name == name).unwrap().id)
    }

    #[test]
    fn with_grouping_off_any_project_drops_anywhere_in_the_flat_list() {
        let (_directory, mut host) = listed(&["a", "b", "c", "d"]);
        let [a, b, d] = ["a", "b", "d"].map(|name| project(&host, name));

        assert_eq!(
            dropped(&mut host, false, &d, &Drop::Before(a.clone())).as_deref(),
            Some("dabc")
        );
        assert_eq!(
            dropped(&mut host, false, &d, &Drop::End).as_deref(),
            Some("abcd")
        );
        // Onto itself or the place it already has: nothing to send.
        assert_eq!(dropped(&mut host, false, &d, &Drop::End), None);
        assert_eq!(
            dropped(&mut host, false, &a, &Drop::Before(a.clone())),
            None
        );
        assert_eq!(
            dropped(&mut host, false, &a, &Drop::Before(b.clone())),
            None
        );
        // The upper half of a row means before it; the lower half, before the next one, or
        // the end below the last.
        let snapshot = host.snapshot().unwrap();
        let flat = entries(&snapshot, false, false);
        let before = |row: &Movable| Drop::Before(row.clone());
        assert_eq!(drops(&flat, &d, &a), Some((before(&a), before(&b))));
        assert_eq!(drops(&flat, &a, &d), Some((before(&d), Drop::End)));
    }

    #[test]
    fn a_grouped_project_reorders_inside_its_group_and_the_group_stays_put() {
        let (_directory, mut host) = listed(&["w: 1", "x", "w: 2", "y", "w: 3"]);
        let [first, third, x] = ["w: 1", "w: 3", "x"].map(|name| project(&host, name));
        let end = Drop::EndOfGroup("w".into());

        // Up: it lands before the member it was dropped above.
        assert_eq!(
            dropped(&mut host, true, &third, &Drop::Before(first.clone())).as_deref(),
            Some("w: 3w: 1xw: 2y")
        );
        // Down from the top: the members it passes move up, so the group stays above "x".
        assert_eq!(
            dropped(&mut host, true, &third, &end).as_deref(),
            Some("w: 1w: 2w: 3xy")
        );
        assert_eq!(
            dropped(&mut host, true, &first, &Drop::Before(third.clone())).as_deref(),
            Some("w: 2w: 1w: 3xy")
        );
        assert_eq!(dropped(&mut host, true, &third, &end), None, "already last");

        // It cannot leave its group, and nothing from outside can land inside: over a member,
        // a top-level row lands below the whole group.
        let snapshot = host.snapshot().unwrap();
        let grouped = entries(&snapshot, false, true);
        let (group, before) = (Movable::Group("w".into()), |row: &Movable| {
            Drop::Before(row.clone())
        });
        assert_eq!(
            drops(&grouped, &third, &first),
            Some((before(&first), before(&third)))
        );
        assert_eq!(
            drops(&grouped, &first, &third),
            Some((before(&third), end.clone()))
        );
        assert_eq!(drops(&grouped, &third, &x), None);
        assert_eq!(drops(&grouped, &third, &group), None);
        assert_eq!(
            drops(&grouped, &x, &group),
            Some((before(&group), before(&x)))
        );
        assert_eq!(drops(&grouped, &x, &first), Some((before(&x), before(&x))));
        assert_eq!(project_move(&grouped, &third, &before(&x)), None);
        assert_eq!(project_move(&grouped, &third, &Drop::End), None);
        assert_eq!(project_move(&grouped, &x, &before(&first)), None);
        assert_eq!(project_move(&grouped, &x, &end), None);
    }

    #[test]
    fn drops_reckon_with_the_rows_shown_while_archived_projects_keep_their_place() {
        let (_directory, mut host) = listed(&["x", "w: 1", "old", "w: 2", "y", "gone"]);
        for session in host.sessions().unwrap() {
            if ["old", "gone"].contains(&session.name.as_str()) {
                host.set_archived(&session.id, true).unwrap();
            }
        }
        let [x, y] = ["x", "y"].map(|name| project(&host, name));
        let (group, hidden) = (Movable::Group("w".into()), project(&host, "old"));
        let snapshot = host.snapshot().unwrap();
        let shown = entries(&snapshot, false, true);
        // A hidden project is no row: nothing lands beside it, and it is not dragged.
        assert_eq!(drops(&shown, &x, &hidden), None);
        assert_eq!(project_move(&shown, &hidden, &Drop::End), None);
        assert_eq!(
            drops(&shown, &y, &x),
            Some((Drop::Before(x.clone()), Drop::Before(group.clone())))
        );

        // A top-level drop gathers the group above "x"; the archived project between its
        // members stays where it was.
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::Before(x.clone())).as_deref(),
            Some("w: 1w: 2xoldygone")
        );
        assert_eq!(
            dropped(&mut host, true, &y, &Drop::Before(group)).as_deref(),
            Some("yw: 1w: 2xoldgone")
        );
        // The very end is the end of the stored order, below the hidden project too.
        assert_eq!(
            dropped(&mut host, true, &y, &Drop::End).as_deref(),
            Some("w: 1w: 2xoldgoney")
        );
        assert_eq!(
            dropped(&mut host, false, &x, &Drop::End).as_deref(),
            Some("w: 1w: 2oldgoneyx")
        );
    }

    #[test]
    fn a_dragged_group_takes_all_its_members_as_one_block() {
        let (_directory, mut host) = listed(&["x", "w: 1", "y", "w: 2", "z"]);
        let group = Movable::Group("w".into());

        let x = project(&host, "x");
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::Before(x)).as_deref(),
            Some("w: 1w: 2xyz")
        );
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::End).as_deref(),
            Some("xyzw: 1w: 2")
        );
        // An ungrouped project lands above the group's topmost member.
        let y = project(&host, "y");
        assert_eq!(
            dropped(&mut host, true, &y, &Drop::Before(group.clone())).as_deref(),
            Some("xzyw: 1w: 2")
        );
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::Before(y)).as_deref(),
            Some("xzw: 1w: 2y")
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
        kept_line(pr, None)
    }
    /// The line of a PR with the keeper's part, a minute after the epoch.
    fn kept_line(pr: &PullRequest, keeper: Option<KeeperPart>) -> String {
        pr_line(pr, keeper, MINUTE)
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }
    fn tones(pr: &PullRequest) -> Vec<Tone> {
        pr_line(pr, None, 0).iter().map(|p| p.tone).collect()
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
            pr_line(&rebased, Some(K::Rebased(0)), 0)[2].text,
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
        assert_eq!(pr_line(&current, Some(K::UpToDate), 0)[2].tone, Tone::Muted);
        // Not kept: no part. A finished PR has none either.
        assert_eq!(kept_line(&current, None), "#142 · checks ✓");
        let merged = pr(S::Merged, C::Success, M::Unknown);
        assert_eq!(kept_line(&merged, Some(K::UpToDate)), "#142 · merged");
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
        assert_eq!(line, "rebased onto main at 4a1c2e9 and pushed, 1m ago");
        assert_eq!(
            detail,
            "e21f0aa → 77b0c13 · the same change as the reviewed commit, so the passed review stands"
        );
    }

    #[test]
    fn a_dragged_row_scrolls_the_list_only_near_its_edges_and_faster_nearer_them() {
        let (top, bottom) = (100., 500.);
        assert_eq!(edge_scroll(300., top, bottom), 0.);
        assert_eq!(edge_scroll(100. + SCROLL_ZONE, top, bottom), 0.);
        assert_eq!(edge_scroll(500. - SCROLL_ZONE, top, bottom), 0.);
        // Just inside a zone still moves, so a resting pointer reaches the end.
        assert_eq!(edge_scroll(131., top, bottom), -1.);
        assert_eq!(edge_scroll(469., top, bottom), 1.);
        assert_eq!(edge_scroll(116., top, bottom), -SCROLL_STEP / 2.);
        assert_eq!(edge_scroll(100., top, bottom), -SCROLL_STEP);
        assert_eq!(edge_scroll(500., top, bottom), SCROLL_STEP);
        // Past either edge the pointer has left the list.
        assert_eq!(edge_scroll(99., top, bottom), 0.);
        assert_eq!(edge_scroll(501., top, bottom), 0.);
    }

    #[test]
    fn a_release_on_the_drags_own_row_is_the_click_it_would_have_been_or_a_cancel() {
        let press = |on_chevron, click_count| RowPress {
            on_chevron,
            click_count,
        };
        assert_eq!(
            own_release(false, press(false, 1), false),
            OwnRelease::Click
        );
        assert_eq!(
            own_release(false, press(false, 2), false),
            OwnRelease::Rename
        );
        assert_eq!(own_release(false, press(true, 1), true), OwnRelease::Toggle);
        // The chevron takes a double-click as two clicks.
        assert_eq!(own_release(false, press(true, 2), true), OwnRelease::Toggle);
        // Pressed on the chevron and released beside it, or the reverse, is a click on the row.
        assert_eq!(own_release(false, press(true, 1), false), OwnRelease::Click);
        assert_eq!(own_release(false, press(false, 1), true), OwnRelease::Click);
        // A third click is not another rename.
        assert_eq!(
            own_release(false, press(false, 3), false),
            OwnRelease::Click
        );
        for (on_chevron, click_count) in [(false, 1), (false, 2), (true, 1)] {
            let press = press(on_chevron, click_count);
            assert_eq!(own_release(true, press, on_chevron), OwnRelease::Nothing);
        }
    }
}
