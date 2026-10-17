//! The crew: every counted ticket of a project with its stage, and under each open ticket the
//! agents working on it, as the progress board lists them.
use super::*;
use std::time::Duration;
use ui::{card, empty_state, hint, icon, progress_bar, session_icon};

/// How often the elapsed times redraw while a listed agent works.
const CREW_TICK: Duration = Duration::from_secs(30);

/// The elapsed-time timer runs only while the crew panel is open and an agent it lists is
/// working; `sync_crew_clock` starts it on this and each tick stops it when this ends.
fn clock_runs(panel: Option<Panel>, crew: &[&AgentProgress]) -> bool {
    panel == Some(Panel::Crew) && crew.iter().any(|agent| agent.working)
}

impl Workspace {
    /// The selected project's board entry.
    pub(super) fn project_progress(&self) -> Option<&ProjectProgress> {
        self.progress.project(&self.selected_session()?.project_id)
    }

    pub(super) fn crew_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let (Some(project), Some(index)) = (self.project_progress(), self.index.as_ref()) else {
            return div();
        };
        let tickets = self.progress.tickets_of(project);
        if tickets.is_empty() {
            return empty_state(
                "team",
                "No tickets yet",
                "Tickets and their agents appear here once the coordinator creates them.",
                p,
            );
        }
        let mut body = div().flex().flex_col().gap_3();
        for progress in tickets {
            let Some(ticket) = index.ticket(&progress.ticket_id) else {
                continue;
            };
            let mut ticket_card = card(p)
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
                                .child(tree::ticket_label(index.snapshot(), ticket)),
                        )
                        .child(hint(tree::stage_figure(progress), p).flex_none()),
                )
                .child(progress_bar(Some(progress.position), progress.state, 3., p));
            for agent in progress
                .agents
                .iter()
                .filter_map(|id| self.progress.agent(id))
            {
                ticket_card = ticket_card.children(self.crew_row(agent, p, cx));
            }
            body = body.child(ticket_card);
        }
        body
    }

    /// One agent of a ticket: role and focus, model, figure, elapsed time and bar. Selecting
    /// it opens its conversation, leaving the panel open with the row marked; an archived
    /// verifier's conversation is read-only, as elsewhere.
    fn crew_row(
        &self,
        agent: &AgentProgress,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let session = self.session(&agent.session_id)?;
        let runtime = self.runtime(&session.id);
        let label = session
            .role
            .agent_label(runtime.and_then(|r| r.focus.as_deref()));
        let model = runtime
            .and_then(|r| r.profile.as_ref())
            .map(|profile| models::profile_label(&self.model_catalog, profile));
        let figure = [
            tree::agent_figure(agent),
            tree::elapsed_label(agent, ui::now()),
        ];
        let figure = figure.into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let id = session.id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        Some(
            div()
                .id(SharedString::from(format!("crew-{id}")))
                .flex()
                .flex_col()
                .gap(px(5.))
                .px_2()
                .py(px(6.))
                .rounded_md()
                .cursor_pointer()
                .when(session.archived, |d| d.opacity(0.7))
                .when(selected, |d| d.bg(p.overlay))
                .hover(move |d| d.bg(p.overlay.opacity(if selected { 1. } else { 0.7 })))
                .on_click(cx.listener(move |v, _, w, c| v.select(id.clone(), w, c)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            icon(session_icon(session.role))
                                .size(px(14.))
                                .text_color(if selected { p.text } else { p.subtle }),
                        )
                        .child(div().flex_1().min_w_0().text_ellipsis().child(label))
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(ui::bar_color(agent.state, p.subtle, p))
                                .child(figure),
                        ),
                )
                // Under the label, clear of the icon.
                .children(model.map(|model| hint(model, p).pl(px(22.)).text_ellipsis()))
                .child(progress_bar(agent.position(), agent.state, 3., p)),
        )
    }

    fn crew_working(&self) -> bool {
        let crew = self.project_progress().map(|p| self.progress.crew(p));
        clock_runs(self.panel, &crew.unwrap_or_default())
    }

    /// Redraws the elapsed times every half minute, only while the panel is open and a
    /// listed agent works.
    pub(super) fn sync_crew_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.crew_clock || !self.crew_working() {
            return;
        }
        self.crew_clock = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(CREW_TICK).await;
                let ticking = this.update(cx, |view, cx| {
                    view.crew_clock = view.crew_working();
                    if view.crew_clock {
                        cx.notify();
                    }
                    view.crew_clock
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
    use super::{Panel, clock_runs};
    use crate::progress::ProgressBoard;
    use workspace_core::*;

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

    #[test]
    fn the_crew_timer_runs_only_while_the_panel_is_open_and_a_listed_agent_works() {
        let (working, idle) = (agent(true), agent(false));
        assert!(clock_runs(Some(Panel::Crew), &[&idle, &working]));
        // An idle panel: open, with no listed agent working.
        assert!(!clock_runs(Some(Panel::Crew), &[&idle]));
        assert!(!clock_runs(Some(Panel::Crew), &[]));
        // Closed, or another panel shown, while an agent works.
        assert!(!clock_runs(None, &[&working]));
        assert!(!clock_runs(Some(Panel::Team), &[&working]));
    }

    #[test]
    fn the_crew_timer_follows_the_project_of_the_selected_conversation() {
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
        // The open panel lists the selected conversation's project, so a selection in
        // another project changes whether the timer runs.
        let runs = |project| {
            let crew = board.crew(board.project(project).unwrap());
            clock_runs(Some(Panel::Crew), &crew)
        };
        assert!(runs("working") && !runs("quiet"));
    }
}
