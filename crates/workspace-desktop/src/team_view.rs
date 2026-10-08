//! A project's tickets with their repository, agents and verification rounds.
use super::*;
use gpui_component::button::ButtonVariants;
use inspector::copyable;
use ui::{
    card, empty_state, hint, humanize, icon, pill, section, session_icon, ticket_state_color,
};

impl Workspace {
    pub(super) fn team_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let (Some(session), Some(snapshot)) = (self.selected_session(), self.snapshot.as_ref())
        else {
            return div();
        };
        // Every ticket belongs to the project's coordinator; its agents show the same list.
        let owner = match session.parent_id.as_ref() {
            Some(parent) if session.role != Role::ProjectOrchestrator => snapshot
                .sessions
                .iter()
                .find(|s| &s.id == parent)
                .unwrap_or(session),
            _ => session,
        };
        self.tickets(owner, snapshot, p, cx)
    }

    fn session_link(
        &self,
        session: &Session,
        label: String,
        show_provider: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = session.id.clone();
        div()
            .id(SharedString::from(format!("team-{id}")))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(6.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |d| d.bg(p.overlay.opacity(0.7)))
            .on_click(cx.listener(move |v, _, w, c| v.select(id.clone(), w, c)))
            .child(
                icon(session_icon(session.role))
                    .size(px(14.))
                    .text_color(p.subtle),
            )
            .child(div().flex_1().min_w_0().text_ellipsis().child(label))
            .when(show_provider, |d| {
                d.child(hint(session.provider.label(), p))
            })
            .child(self.session_badge(session, p))
            .child(icon("chevron-right").size(px(12.)).text_color(p.subtle))
    }

    /// A ticket's agent with the model it runs and why that model was chosen.
    fn ticket_agent(
        &self,
        agent: &Session,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let runtime = self.runtime(&agent.id);
        let label = agent
            .role
            .agent_label(runtime.and_then(|r| r.focus.as_deref()));
        let mut entry = div()
            .flex()
            .flex_col()
            .child(self.session_link(agent, label, false, p, cx));
        if let Some(profile) = runtime.and_then(|r| r.profile.as_ref()) {
            let reason =
                runtime.and_then(|r| r.selection.as_ref()).map(|selection| {
                    match selection.chosen_by {
                        Chooser::Coordinator { .. } => selection.reason.clone(),
                        _ => models::chosen_by(selection, &snapshot.sessions),
                    }
                });
            entry = entry.child(
                div()
                    .pl(px(30.))
                    .pr_2()
                    .pb(px(6.))
                    .mt(px(-4.))
                    .flex()
                    .flex_col()
                    .child(hint(models::profile_label(&self.model_catalog, profile), p))
                    .children(reason.map(|reason| hint(reason, p).italic().line_clamp(2))),
            );
        }
        entry
    }

    fn tickets(
        &self,
        coordinator: &Session,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let tickets: Vec<_> = snapshot
            .tickets
            .iter()
            .filter(|t| t.coordinator_id == coordinator.id)
            .collect();
        let mut body = div().flex().flex_col().gap_3().child(
            section("TICKETS", p).child(
                button("new-ticket-panel")
                    .primary()
                    .icon(icon("plus"))
                    .label("New ticket")
                    .on_click(
                        cx.listener(|v, _, w, c| v.open_creation(Creation::Ticket, None, w, c)),
                    ),
            ),
        );
        if tickets.is_empty() {
            body = body.child(empty_state(
                "task",
                "No tickets yet",
                "A ticket is one bounded piece of work with acceptance criteria. It gets its own branch, worktree and agents.",
                p,
            ));
        }
        let agents_of = |ticket: &Ticket| {
            self.shown(snapshot)
                .filter(|s| {
                    self.runtime(&s.id)
                        .is_some_and(|r| r.ticket_id.as_ref() == Some(&ticket.id))
                })
                .collect::<Vec<_>>()
        };
        for ticket in tickets {
            let ticket_id = ticket.id.clone();
            let mut entry = card(p)
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tree::ticket_label(snapshot, ticket)),
                        )
                        .child(pill(
                            humanize(&ticket.state),
                            ticket_state_color(&ticket.state, p),
                        )),
                )
                .child(hint(ticket.brief.clone(), p).line_clamp(4))
                .child(copyable("Branch", &ticket.branch, p));
            let rounds = tree::verification_lines(ticket);
            if !rounds.is_empty() {
                let mut list = div().flex().flex_col().gap_1();
                for line in rounds {
                    list = list.child(hint(line, p));
                }
                entry = entry.child(list);
            }
            let agents = agents_of(ticket);
            if !agents.is_empty() {
                let mut list = div().flex().flex_col().mx(px(-8.));
                for agent in agents {
                    list = list.child(self.ticket_agent(agent, snapshot, p, cx));
                }
                entry = entry.child(list);
            }
            if ticket.is_open() {
                entry = entry.child(
                    div().flex().child(
                        button(SharedString::from(format!("assign-{ticket_id}")))
                            .ghost()
                            .icon(icon("plus"))
                            .label("Assign agent")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.open_creation(Creation::Agent, Some(&ticket_id), w, c)
                            })),
                    ),
                );
            }
            body = body.child(entry);
        }
        // Older stores can contain workers created before ticket ownership existed.
        let unassigned: Vec<_> = self
            .shown(snapshot)
            .filter(|s| {
                s.parent_id.as_ref() == Some(&coordinator.id)
                    && s.role != Role::TaskOrchestrator
                    && self.runtime(&s.id).is_none_or(|r| r.ticket_id.is_none())
            })
            .collect();
        if !unassigned.is_empty() {
            let mut list = div()
                .flex()
                .flex_col()
                .child(section("WITHOUT A TICKET", p));
            for agent in unassigned {
                list = list.child(self.session_link(agent, agent.name.clone(), true, p, cx));
            }
            body = body.child(list);
        }
        body
    }
}
