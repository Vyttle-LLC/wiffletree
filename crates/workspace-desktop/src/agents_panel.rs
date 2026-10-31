//! The Agents panel: each open ticket of a project with its stage and the agents working on it,
//! as the progress board lists them. Accepted and closed tickets fold into "Finished tickets",
//! and every other agent the project has had folds into "Previous agents" at the bottom.
use super::*;
use progress::ProgressBoard;
use std::{collections::HashSet, time::Duration};
use ui::{card, empty_state, hint, icon, progress_bar, session_icon};

/// How often the elapsed times redraw while a listed agent works.
const AGENTS_TICK: Duration = Duration::from_secs(30);

/// Why an agent needs a look that nobody is asking for. A blocked agent is not stuck: its
/// coordinator answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stuck {
    Disconnected,
    LastTurnFailed,
}
impl Stuck {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::LastTurnFailed => "Last turn failed",
        }
    }
}

/// An agent is stuck when it is disconnected, or idle after a turn that ended in an error.
/// Blocked, paused and working agents are not, nor are archived ones, which receive nothing.
pub(crate) fn stuck(session: &Session, runtime: Option<&SessionRuntime>) -> Option<Stuck> {
    let failed = runtime.is_some_and(|r| r.last_error.is_some());
    match session.status {
        _ if session.archived => None,
        Status::Disconnected => Some(Stuck::Disconnected),
        Status::Ready | Status::Done if failed => Some(Stuck::LastTurnFailed),
        _ => None,
    }
}

/// A ticket the panel draws as a card, with its unarchived agents.
pub(crate) struct TicketCard<'a> {
    pub(crate) ticket_id: &'a str,
    /// `None` for a closed ticket, which the board leaves out.
    pub(crate) progress: Option<&'a TicketProgress>,
    pub(crate) agents: Vec<&'a AgentProgress>,
}

/// Everyone the panel lists for a project: a card for each open ticket, then for each finished
/// ticket an unarchived agent still works on, such as one restored from the archive; the other
/// finished tickets, accepted or closed, whichever half of the snapshot holds them; then the
/// project's previous agents. Those are the rest of the coordinator's agents: archived agents,
/// finished verifiers among them, workers from before tickets existed and retired repository
/// coordinators. An archived project has no board entry, so all of its agents are previous.
pub(crate) struct Roster<'a> {
    pub(crate) tickets: Vec<TicketCard<'a>>,
    pub(crate) finished: Vec<&'a Ticket>,
    pub(crate) previous: Vec<&'a Session>,
}
impl<'a> Roster<'a> {
    pub(crate) fn new(
        board: &'a ProgressBoard,
        index: &'a WorkspaceIndex,
        project_id: &str,
        coordinator: &str,
    ) -> Self {
        let counted = board.project(project_id).map(|p| board.tickets_of(p));
        let live = |id: &&String| index.session(id).is_some_and(|s| !s.archived);
        let mut tickets: Vec<_> = (counted.unwrap_or_default().into_iter())
            .filter(|ticket| ticket.stage != Stage::Accepted)
            .map(|ticket| {
                let agents = ticket.agents.iter().filter(live);
                TicketCard {
                    ticket_id: &ticket.ticket_id,
                    progress: Some(ticket),
                    agents: agents.filter_map(|id| board.agent(id)).collect(),
                }
            })
            .collect();
        let mut finished = vec![];
        for ticket in index.tickets_owned_by(coordinator).filter(|t| !t.is_open()) {
            let agents = index.agents(&ticket.id).filter(|s| !s.archived);
            let agents: Vec<_> = agents.filter_map(|s| board.agent(&s.id)).collect();
            match agents.is_empty() {
                true => finished.push(ticket),
                false => tickets.push(TicketCard {
                    ticket_id: &ticket.id,
                    progress: board.ticket(&ticket.id),
                    agents,
                }),
            }
        }
        let listed: HashSet<&str> = (tickets.iter())
            .flat_map(|card| card.agents.iter().map(|a| a.session_id.as_str()))
            .collect();
        let ticket_agents = (index.tickets_owned_by(coordinator)).flat_map(|t| index.agents(&t.id));
        let without_ticket = index.children(coordinator).filter(|s| {
            index
                .runtime(&s.id)
                .and_then(|r| r.ticket_id.as_ref())
                .is_none()
        });
        let previous = (ticket_agents.chain(without_ticket))
            .filter(|s| !listed.contains(s.id.as_str()))
            .collect();
        Self {
            tickets,
            finished,
            previous,
        }
    }
    /// Each ticket's agents in turn.
    pub(crate) fn ticket_agents(&self) -> Vec<&'a AgentProgress> {
        let agents = self.tickets.iter().flat_map(|card| &card.agents);
        agents.copied().collect()
    }
    pub(crate) fn working(&self) -> usize {
        let previous = self.previous.iter().filter(|s| s.status == Status::Working);
        let on_tickets = self.ticket_agents().into_iter().filter(|a| a.working);
        on_tickets.count() + previous.count()
    }
    pub(crate) fn busy_tickets(&self) -> usize {
        let busy = |card: &&TicketCard| card.agents.iter().any(|a| a.working);
        self.tickets.iter().filter(busy).count()
    }
    pub(crate) fn stuck(&self, index: &WorkspaceIndex) -> usize {
        let on_tickets = self.ticket_agents().into_iter();
        let on_tickets = on_tickets.filter_map(|a| index.session(&a.session_id));
        (on_tickets.chain(self.previous.iter().copied()))
            .filter(|s| stuck(s, index.runtime(&s.id)).is_some())
            .count()
    }
}

/// A previous agent's name, and the ticket it worked on, if any.
fn previous_label(index: &WorkspaceIndex, session: &Session) -> (String, Option<String>) {
    if let Some(ticket) = index.ticket_of(&session.id) {
        let ticket = wording::ticket_label(index.snapshot(), ticket);
        (index.agent_label(session), Some(ticket))
    } else if session.role == Role::TaskOrchestrator {
        (format!("{} (retired)", session.name), None)
    } else {
        (session.name.clone(), None)
    }
}

/// A ticket card's PR line: muted except for passed or failed checks, a conflict, a merge,
/// commits no review has checked and a merge that waits, with the keeper's part near its end.
fn pr_line(
    pr: &PullRequest,
    unreviewed: Option<&Unreviewed>,
    keeper: Option<KeeperPart>,
    comments: Option<&KeeperComments>,
    merge: Option<wording::MergePart>,
    p: Palette,
) -> Div {
    let glyph = if pr.state == PrState::Merged {
        "git-merge"
    } else {
        "git-pull-request"
    };
    let mut line = div()
        .flex()
        .items_center()
        .gap(px(5.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(font::CAPTION))
        .text_color(p.subtle)
        .child(icon(glyph).size(px(11.)));
    for (i, part) in wording::pr_line(pr, unreviewed, keeper, comments, merge, ui::now())
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            line = line.child(div().text_color(p.subtle.opacity(0.6)).child("·"));
        }
        line = line.child(div().text_color(tone_color(part.tone, p)).child(part.text));
    }
    line
}

fn tone_color(tone: wording::Tone, p: Palette) -> Hsla {
    match tone {
        wording::Tone::Muted => p.subtle,
        wording::Tone::Good => p.green,
        wording::Tone::Bad => p.red,
        wording::Tone::Merged => p.focus,
        wording::Tone::Warn => p.yellow,
        wording::Tone::Busy => p.focus,
    }
}

/// What the agent's one quiet line says: an agent idle with nothing running, and not stuck,
/// waits for something its ticket names. `None` keeps its full row.
fn waiting_line(
    ticket: &Ticket,
    session: &Session,
    runtime: Option<&SessionRuntime>,
) -> Option<String> {
    let idle = matches!(session.status, Status::Ready | Status::Done)
        && stuck(session, runtime).is_none()
        && !context_view::compacting(runtime);
    idle.then(|| wording::waiting_for(ticket, &session.id))
        .flatten()
}

/// The elapsed-time timer runs only while the Agents panel is open and an agent it lists under
/// a ticket is working; `sync_agents_clock` starts it on this and each tick stops it when this
/// ends.
fn clock_runs(panel: Option<Panel>, agents: &[&AgentProgress]) -> bool {
    panel == Some(Panel::Agents) && agents.iter().any(|agent| agent.working)
}

/// A fold's one subtle line, with its chevron.
fn fold_toggle(id: &'static str, open: bool, label: String, p: Palette) -> Stateful<Div> {
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .cursor_pointer()
        .text_size(px(font::SMALL))
        .text_color(p.subtle)
        .hover(|d| d.text_color(p.text))
        .child(icon(chevron).size(px(12.)))
        .child(label)
}

impl Inspector {
    pub(super) fn agents_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let (Some(roster), Some(index)) = (self.shared.roster(), self.shared.index.as_ref()) else {
            return div();
        };
        if roster.tickets.is_empty() && roster.finished.is_empty() && roster.previous.is_empty() {
            return empty_state(
                "team",
                "No tickets yet",
                "Tickets and their agents appear here once the coordinator creates them.",
                p,
            );
        }
        let mut body = div().flex().flex_col().gap_3();
        for ticket_card in &roster.tickets {
            let Some(ticket) = index.ticket(ticket_card.ticket_id) else {
                continue;
            };
            body = body.child(self.ticket_card(index, ticket, ticket_card, p, cx));
        }
        body.children(self.finished_tickets(&roster.finished, p, cx))
            .children(self.previous_agents(index, &roster.previous, p, cx))
    }

    /// The tickets drawn, for the panel's heading: "9 open · 32 finished".
    pub(super) fn agents_counts(&self, p: Palette) -> Option<Div> {
        let roster = self.shared.roster()?;
        let (open, finished) = (roster.tickets.len(), roster.finished.len());
        (open + finished > 0).then(|| hint(wording::ticket_counts(open, finished), p).flex_none())
    }

    /// One ticket with its agents. A closed ticket has no stage or bar on the board, and a
    /// finished ticket's PR says only how it ended.
    fn ticket_card(
        &self,
        index: &WorkspaceIndex,
        ticket: &Ticket,
        ticket_card: &TicketCard,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (progress, agents) = (ticket_card.progress, &ticket_card.agents);
        let stuck = (agents.iter())
            .filter_map(|a| index.session(&a.session_id))
            .any(|s| stuck(s, index.runtime(&s.id)).is_some());
        let figure = progress.map_or("Closed".into(), wording::stage_figure);
        let mut drawn = card(p)
            .when(stuck, |d| d.border_color(p.yellow))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(wording::ticket_label(index.snapshot(), ticket)),
                    )
                    .child(hint(figure, p).flex_none()),
            )
            .children(
                progress
                    .map(|progress| progress_bar(Some(progress.position), progress.state, 3., p)),
            )
            .children(ticket.pull_request.as_ref().map(|pr| {
                if !ticket.is_open() {
                    let outcome = wording::finished_outcome(ticket);
                    return hint(outcome.text, p).text_color(tone_color(outcome.tone, p));
                }
                let keeper = (self.shared.snapshot())
                    .and_then(|s| s.keeper_parts.get(&ticket.id))
                    .copied();
                let keeper = ticket.try_part().or(keeper);
                let repository =
                    (index.snapshot().repositories.iter()).find(|r| r.id == ticket.repository_id);
                pr_line(
                    pr,
                    ticket.unreviewed(),
                    keeper,
                    ticket.waiting_comments(),
                    wording::merge_part(ticket, repository),
                    p,
                )
            }))
            // A waived acceptance and a publish step waiting for the human both need them.
            .children(
                [
                    wording::waiver_note(ticket),
                    wording::publish_waiting(ticket),
                ]
                .into_iter()
                .flatten()
                .map(|note| hint(note, p).text_color(p.yellow).text_ellipsis()),
            );
        for agent in agents {
            drawn = drawn.children(self.agent_row(index, ticket, agent, p, cx));
        }
        drawn
    }

    /// One agent of a ticket: role and focus, figure, elapsed time, current step, model and
    /// bar, and why it is stuck. Selecting it opens its conversation, leaving the panel open
    /// with the row marked; an archived verifier's conversation is read-only, as elsewhere. An
    /// agent that is only waiting gets one quiet line instead.
    fn agent_row(
        &self,
        index: &WorkspaceIndex,
        ticket: &Ticket,
        agent: &AgentProgress,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let session = index.session(&agent.session_id)?;
        let runtime = index.runtime(&session.id);
        if let Some(waiting) = waiting_line(ticket, session, runtime) {
            return Some(
                self.agent_link(session, p, cx)
                    .text_size(px(font::SMALL))
                    .text_color(p.subtle)
                    .child(
                        self.row_head(session, index.agent_label(session), false, p)
                            .child(div().flex_none().child(waiting)),
                    ),
            );
        }
        let model = runtime
            .and_then(|r| r.profile.as_ref())
            .map(|profile| models::profile_label(&self.shared.catalog, profile));
        // The item being worked on, only while the agent works.
        let current = (agent.plan.as_ref())
            .and_then(|plan| plan.current.clone())
            .filter(|_| agent.working);
        let step = [current, model];
        let step = step.into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let stuck = stuck(session, runtime);
        let figure = [
            stuck.map(|s| s.label().to_owned()),
            wording::agent_figure(agent),
            wording::elapsed_label(agent, ui::now()),
        ];
        let figure = figure.into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let figure_color = match stuck {
            Some(_) => p.yellow,
            None => ui::bar_color(agent.state, p.subtle, p),
        };
        let head = self
            .row_head(session, index.agent_label(session), stuck.is_some(), p)
            .child(
                div()
                    .flex_none()
                    .text_size(px(font::CAPTION))
                    .text_color(figure_color)
                    .child(figure),
            );
        Some(
            self.agent_link(session, p, cx)
                .gap(px(5.))
                .child(head)
                // Under the label, clear of the icon.
                .when(!step.is_empty(), |d| {
                    d.child(hint(step, p).pl(px(22.)).text_ellipsis())
                })
                .child(progress_bar(agent.position(), agent.state, 3., p)),
        )
    }

    /// The folded "Finished tickets (N)" section: each ticket's title and how it ended, on one
    /// line, with no bar, checks or conflict.
    fn finished_tickets(
        &self,
        finished: &[&Ticket],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if finished.is_empty() {
            return None;
        }
        let open = self.finished_open;
        let toggle = fold_toggle(
            "finished-tickets",
            open,
            format!("Finished tickets ({})", finished.len()),
            p,
        )
        .on_click(cx.listener(|view, _, _, cx| {
            view.finished_open = !view.finished_open;
            cx.notify();
        }));
        let rows = finished.iter().filter(|_| open).map(|ticket| {
            let outcome = wording::finished_outcome(ticket);
            div()
                .flex()
                .items_baseline()
                .gap_2()
                .px_1()
                .py(px(2.))
                .rounded_md()
                .text_size(px(font::SMALL))
                .hover(|d| d.bg(p.overlay.opacity(0.7)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .child(ticket.title.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(tone_color(outcome.tone, p))
                        .child(outcome.text),
                )
        });
        Some(div().flex().flex_col().gap_1().child(toggle).children(rows))
    }

    /// The folded "Previous agents (N)" section: each agent's name, the ticket it worked on,
    /// and its figure or why it is stuck.
    fn previous_agents(
        &self,
        index: &WorkspaceIndex,
        previous: &[&Session],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if previous.is_empty() {
            return None;
        }
        let open = self.previous_open;
        let toggle = fold_toggle(
            "previous-agents",
            open,
            format!("Previous agents ({})", previous.len()),
            p,
        )
        .on_click(cx.listener(|view, _, _, cx| {
            view.previous_open = !view.previous_open;
            cx.notify();
        }));
        let rows = previous.iter().filter(|_| open).map(|&session| {
            let (label, ticket) = previous_label(index, session);
            let stuck = stuck(session, index.runtime(&session.id));
            // A finished verifier of the latest cycle keeps its word, such as "passed".
            let agent = self.shared.progress.agent(&session.id);
            let figure = match stuck {
                Some(stuck) => Some((stuck.label().to_owned(), p.yellow)),
                None => agent.and_then(|agent| {
                    let color = ui::bar_color(agent.state, p.subtle, p);
                    Some((wording::agent_figure(agent)?, color))
                }),
            };
            let head = self
                .row_head(session, label, stuck.is_some(), p)
                .children(figure.map(|(figure, color)| {
                    div()
                        .flex_none()
                        .text_size(px(font::CAPTION))
                        .text_color(color)
                        .child(figure)
                }));
            self.agent_link(session, p, cx)
                .gap(px(2.))
                .child(head)
                .children(ticket.map(|ticket| hint(ticket, p).pl(px(22.)).text_ellipsis()))
        });
        Some(div().flex().flex_col().gap_1().child(toggle).children(rows))
    }

    /// An agent row's clickable frame. Selecting it opens the agent's conversation, leaving the
    /// panel open with the row marked.
    fn agent_link(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = session.id.clone();
        let selected = self.shared.selected.as_ref() == Some(&id);
        div()
            .id(SharedString::from(format!("agent-{id}")))
            .flex()
            .flex_col()
            .px_2()
            .py(px(6.))
            .rounded_md()
            .cursor_pointer()
            .when(session.archived, |d| d.opacity(0.7))
            .when(selected, |d| d.bg(p.overlay))
            .hover(move |d| d.bg(p.overlay.opacity(if selected { 1. } else { 0.7 })))
            .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::Select(id.clone()))))
    }

    /// An agent row's first line: its icon, a warning in its place when the agent is stuck,
    /// and its label. Callers add the figure.
    fn row_head(&self, session: &Session, label: String, stuck: bool, p: Palette) -> Div {
        let selected = self.shared.selected.as_ref() == Some(&session.id);
        let glyph = match stuck {
            true => icon("triangle-alert").text_color(p.yellow),
            false => icon(session_icon(session.role)).text_color(if selected {
                p.text
            } else {
                p.subtle
            }),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(glyph.size(px(14.)))
            .child(div().flex_1().min_w_0().text_ellipsis().child(label))
    }

    fn agents_working(&self) -> bool {
        let agents = self.shared.roster().map(|roster| roster.ticket_agents());
        clock_runs(self.shared.panel, &agents.unwrap_or_default())
    }

    /// Redraws the elapsed times every half minute, only while the panel is open and a
    /// listed agent works.
    pub(super) fn sync_agents_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agents_clock || !self.agents_working() {
            return;
        }
        self.agents_clock = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENTS_TICK).await;
                let ticking = this.update(cx, |view, cx| {
                    view.agents_clock = view.agents_working();
                    if view.agents_clock {
                        cx.notify();
                    }
                    view.agents_clock
                });
                if !matches!(ticking, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{Panel, Roster, Stuck, clock_runs, previous_label, stuck, waiting_line};
    use crate::progress::ProgressBoard;
    use crate::test_support::ticket_store;
    use std::rc::Rc;
    use workspace_core::*;
    use workspace_host::Host;

    fn agent(working: bool) -> AgentProgress {
        named("agent", working)
    }
    fn named(id: &str, working: bool) -> AgentProgress {
        AgentProgress {
            session_id: id.into(),
            state: BarState::InProgress,
            plan: None,
            word: None,
            working,
            run_started_at: Some(0),
            run_finished_at: None,
        }
    }
    fn session(status: Status) -> Session {
        named_session("agent", status)
    }
    fn named_session(id: &str, status: Status) -> Session {
        Session {
            id: id.into(),
            project_id: "project".into(),
            parent_id: Some("coordinator".into()),
            repository_id: None,
            name: "Implementer".into(),
            role: Role::Implementer,
            provider: Provider::Claude,
            status,
            archived: false,
            owner: None,
        }
    }

    #[test]
    fn an_agent_is_stuck_when_disconnected_or_idle_after_a_failed_turn() {
        let failed = SessionRuntime {
            last_error: Some("The provider exited".into()),
            ..Default::default()
        };
        let clean = SessionRuntime::default();
        let disconnected = session(Status::Disconnected);
        assert_eq!(stuck(&disconnected, None), Some(Stuck::Disconnected));
        for idle in [Status::Ready, Status::Done] {
            assert_eq!(
                stuck(&session(idle), Some(&failed)),
                Some(Stuck::LastTurnFailed)
            );
            assert_eq!(stuck(&session(idle), Some(&clean)), None);
        }
        // The coordinator answers a blocked agent; a paused one was stopped on purpose.
        for handled in [Status::Blocked, Status::Paused, Status::Working] {
            assert_eq!(stuck(&session(handled), Some(&failed)), None, "{handled:?}");
        }
        let archived = Session {
            archived: true,
            ..disconnected
        };
        assert_eq!(stuck(&archived, Some(&failed)), None);
    }

    #[test]
    fn the_agents_timer_runs_only_while_the_panel_is_open_and_a_listed_agent_works() {
        let (working, idle) = (agent(true), agent(false));
        assert!(clock_runs(Some(Panel::Agents), &[&idle, &working]));
        // An idle panel: open, with no listed agent working.
        assert!(!clock_runs(Some(Panel::Agents), &[&idle]));
        assert!(!clock_runs(Some(Panel::Agents), &[]));
        // Closed, or another panel shown, while an agent works.
        assert!(!clock_runs(None, &[&working]));
        assert!(!clock_runs(Some(Panel::Team), &[&working]));
    }

    #[test]
    fn the_agents_timer_follows_the_project_of_the_selected_conversation() {
        let ticket = |id: &str, agent: &str| TicketProgress {
            ticket_id: id.into(),
            stage: Stage::Implementing,
            state: BarState::InProgress,
            position: 3_000,
            agents: vec![agent.into()],
        };
        let project = |id: &str, ticket: &str| ProjectProgress {
            project_id: id.into(),
            phase: Phase::Implementing,
            state: BarState::InProgress,
            position: Some(3_000),
            accepted: 0,
            tickets: vec![ticket.into()],
            coordinator_id: format!("{id}-coordinator"),
        };
        let mut board = ProgressBoard::default();
        board.merge(ProgressPage {
            revision: 1,
            complete: true,
            agents: vec![named("busy", true), named("idle", false)],
            tickets: vec![ticket("one", "busy"), ticket("two", "idle")],
            projects: vec![project("working", "one"), project("quiet", "two")],
        });
        let snapshot = Snapshot {
            sessions: ["busy", "idle"]
                .map(|id| named_session(id, Status::Ready))
                .into(),
            ..Default::default()
        };
        let index = WorkspaceIndex::new(snapshot, Default::default());
        // The open panel lists the selected conversation's project, so a selection in
        // another project changes whether the timer runs.
        let runs = |project: &str| {
            let roster = Roster::new(&board, &index, project, &format!("{project}-coordinator"));
            clock_runs(Some(Panel::Agents), &roster.ticket_agents())
        };
        assert!(runs("working") && !runs("quiet"));
    }

    #[test]
    fn every_agent_of_the_project_is_on_a_ticket_card_or_under_previous_agents() {
        let (directory, mut host, root, ticket) = ticket_store();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        let reviewer = host
            .assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        host.set_archived(&reviewer.id, true).unwrap();
        let repository = host.repositories().unwrap().remove(0);
        let finished = host
            .create_ticket(&root.id, &repository.id, "Old calls", "Done")
            .unwrap();
        let earlier = host
            .assign_ticket(
                &finished.id,
                Role::Implementer,
                Provider::Codex,
                "Old",
                None,
            )
            .unwrap();
        host.close_ticket(&finished.id).unwrap();
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
        // A repository coordinator kept from a store made before they were removed.
        drop(host);
        let retired = Session {
            id: "retired".into(),
            name: "sagechat-sms".into(),
            role: Role::TaskOrchestrator,
            repository_id: Some(ticket.repository_id.clone()),
            ..named_session("retired", Status::Ready)
        };
        let db =
            rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO sessions VALUES (?1,?2,?3,'task_orchestrator',?4)",
            (
                &retired.id,
                &root.project_id,
                &root.id,
                serde_json::to_string(&Session {
                    project_id: root.project_id.clone(),
                    parent_id: Some(root.id.clone()),
                    ..retired.clone()
                })
                .unwrap(),
            ),
        )
        .unwrap();
        drop(db);
        let mut host = Host::open_set_up(directory.path().join("home")).unwrap();
        let mut page = host.progress(0).unwrap();
        // The board lists the latest cycle's verifiers, archived or not.
        page.tickets[0].agents.push(reviewer.id.clone());
        let mut board = ProgressBoard::default();
        board.merge(page);
        let index = WorkspaceIndex::new(host.snapshot().unwrap(), Rc::new(host.archive().unwrap()));

        let roster = Roster::new(&board, &index, &root.project_id, &root.id);

        let on_tickets: Vec<&str> = roster
            .ticket_agents()
            .iter()
            .map(|a| a.session_id.as_str())
            .collect();
        assert_eq!(on_tickets, [implementer.id.as_str()]);
        let previous: Vec<&str> = roster.previous.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            previous,
            [&reviewer.id, &earlier.id, &older.id, &retired.id]
        );
        let retired = index.session(&retired.id).unwrap();
        assert_eq!(previous_label(&index, retired).0, "sagechat-sms (retired)");
    }

    #[test]
    fn open_tickets_are_cards_and_finished_ones_fold_unless_an_agent_still_works_on_them() {
        let (_directory, mut host, root, open) = ticket_store();
        let repository = host.repositories().unwrap().remove(0);
        let mut ticket = |title: &str| {
            let created = host
                .create_ticket(&root.id, &repository.id, title, "Brief")
                .unwrap();
            let agent = host
                .assign_ticket(&created.id, Role::Implementer, Provider::Codex, "Fix", None)
                .unwrap();
            (created, agent)
        };
        let (accepted, accepted_agent) = ticket("Accepted");
        let (closed, _) = ticket("Closed");
        let (restored, agent) = ticket("Restored");
        host.set_archived(&accepted_agent.id, true).unwrap();
        host.close_ticket(&closed.id).unwrap();
        host.close_ticket(&restored.id).unwrap();
        host.set_archived(&agent.id, false).unwrap();
        // Accepting needs a passed review, so the records are moved by hand.
        let mut page = host.progress(0).unwrap();
        let entry = page.tickets.iter_mut();
        let entry = entry.into_iter().find(|t| t.ticket_id == accepted.id);
        entry.unwrap().stage = Stage::Accepted;
        let mut snapshot = host.snapshot().unwrap();
        let held = snapshot.tickets.iter_mut();
        let held = held.into_iter().find(|t| t.id == accepted.id).unwrap();
        held.state = TicketState::Accepted;
        let mut board = ProgressBoard::default();
        board.merge(page);
        let index = WorkspaceIndex::new(snapshot, Rc::new(host.archive().unwrap()));

        let roster = Roster::new(&board, &index, &root.project_id, &root.id);

        let cards: Vec<&str> = roster.tickets.iter().map(|c| c.ticket_id).collect();
        assert_eq!(cards, [open.id.as_str(), restored.id.as_str()]);
        assert!(
            roster.tickets[1].progress.is_none(),
            "the board has no entry"
        );
        let on_card = &roster.tickets[1].agents;
        assert_eq!(on_card[0].session_id, agent.id);
        let finished: Vec<&str> = roster.finished.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(finished, ["Accepted", "Closed"]);
        assert!(roster.previous.iter().all(|s| s.id != agent.id));
    }

    #[test]
    fn a_project_whose_tickets_all_finished_still_has_something_to_list() {
        let (_directory, mut host, root, ticket) = ticket_store();
        host.close_ticket(&ticket.id).unwrap();
        let mut board = ProgressBoard::default();
        board.merge(host.progress(0).unwrap());
        let index = WorkspaceIndex::new(host.snapshot().unwrap(), Rc::new(host.archive().unwrap()));

        let roster = Roster::new(&board, &index, &root.project_id, &root.id);

        assert!(roster.tickets.is_empty() && roster.previous.is_empty());
        assert_eq!(roster.finished.len(), 1);
    }

    #[test]
    fn only_an_idle_agent_with_nothing_wrong_gets_the_quiet_line() {
        let (_directory, _host, _root, mut ticket) = ticket_store();
        ticket.state = TicketState::Assigned;
        let clean = SessionRuntime::default();
        for idle in [Status::Ready, Status::Done] {
            assert_eq!(
                waiting_line(&ticket, &session(idle), Some(&clean)).as_deref(),
                Some("Waiting on coordinator"),
                "{idle:?}"
            );
        }
        for full_row in [
            Status::Working,
            Status::Blocked,
            Status::Paused,
            Status::Disconnected,
        ] {
            assert_eq!(
                waiting_line(&ticket, &session(full_row), Some(&clean)),
                None,
                "{full_row:?}"
            );
        }
        let failed = SessionRuntime {
            last_error: Some("The provider exited".into()),
            ..Default::default()
        };
        assert_eq!(
            waiting_line(&ticket, &session(Status::Ready), Some(&failed)),
            None,
            "a stuck agent is never quieted"
        );
    }

    #[test]
    fn an_archived_projects_agents_are_all_previous() {
        let (_directory, mut host, root, ticket) = ticket_store();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        host.set_archived(&root.id, true).unwrap();
        let mut board = ProgressBoard::default();
        board.merge(host.progress(0).unwrap());
        let index = WorkspaceIndex::new(host.snapshot().unwrap(), Rc::new(host.archive().unwrap()));

        let roster = Roster::new(&board, &index, &root.project_id, &root.id);

        assert!(board.project(&root.project_id).is_none() && roster.tickets.is_empty());
        let previous: Vec<&str> = roster.previous.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(previous, [&implementer.id]);
    }

    #[test]
    fn the_working_count_covers_every_listed_agent_and_the_tickets_they_work_on() {
        let (_directory, mut host, root, ticket) = ticket_store();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        let tester = host
            .assign_ticket(&ticket.id, Role::Tester, Provider::Codex, "Test", None)
            .unwrap();
        let mut board = ProgressBoard::default();
        board.merge(host.progress(0).unwrap());
        let busy = |id: &str| AgentProgress {
            working: true,
            ..board.agent(id).unwrap().clone()
        };
        let (implementer, tester) = (busy(&implementer.id), busy(&tester.id));
        board.merge(ProgressPage {
            revision: 2,
            complete: false,
            agents: vec![implementer, tester],
            tickets: vec![],
            projects: vec![],
        });
        let index = WorkspaceIndex::new(host.snapshot().unwrap(), Default::default());

        let roster = Roster::new(&board, &index, &root.project_id, &root.id);

        assert_eq!((roster.working(), roster.busy_tickets()), (2, 1));
        assert_eq!(roster.stuck(&index), 0);
    }
}
