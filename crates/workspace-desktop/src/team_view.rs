//! Teams for a project, and tickets with their agents for a repository coordinator.
use super::*;
use gpui_component::button::ButtonVariants;
use inspector::copyable;
use ui::{
    card, empty_state, hint, humanize, icon, pill, section, session_icon, status_badge,
    ticket_state_color,
};

impl Workspace {
    pub(super) fn team_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let (Some(session), Some(snapshot)) = (self.selected_session(), self.snapshot.as_ref())
        else {
            return div();
        };
        if session.role == Role::ProjectOrchestrator {
            self.teams(session, snapshot, p, cx)
        } else {
            self.tickets(session, snapshot, p, cx)
        }
    }

    fn session_link(
        &self,
        session: &Session,
        label: String,
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
            .child(hint(format!("{:?}", session.provider), p))
            .child(status_badge(session.status, p))
            .child(icon("chevron-right").size(px(12.)).text_color(p.subtle))
    }

    fn teams(
        &self,
        project: &Session,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let teams: Vec<_> = self
            .shown(snapshot)
            .filter(|s| s.parent_id.as_ref() == Some(&project.id))
            .collect();
        let mut body =
            div().flex().flex_col().gap_3().child(
                section("REPOSITORY TEAMS", p).child(
                    button("add-team")
                        .primary()
                        .icon(icon("plus"))
                        .label("Add team")
                        .on_click(cx.listener(|v, _, w, c| {
                            v.open_creation(Creation::Coordinator, None, w, c)
                        })),
                ),
            );
        if teams.is_empty() {
            return body.child(empty_state(
                "team",
                "No teams yet",
                "Each repository gets one coordinator that plans its tickets and runs its agents. Your main coordinator can also create teams when you go live.",
                p,
            ));
        }
        for team in teams {
            let id = team.id.clone();
            let tickets: Vec<_> = snapshot
                .tickets
                .iter()
                .filter(|t| t.coordinator_id == team.id)
                .collect();
            let accepted = tickets.iter().filter(|t| t.state == "accepted").count();
            let repository = snapshot
                .repositories
                .iter()
                .find(|r| Some(&r.id) == team.repository_id.as_ref())
                .map_or(String::new(), |r| r.name.clone());
            body = body.child(
                card(p)
                    .id(SharedString::from(format!("team-{id}")))
                    .cursor_pointer()
                    .hover(move |d| d.border_color(p.focus))
                    .on_click(cx.listener(move |v, _, w, c| v.select(id.clone(), w, c)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon(session_icon(team.role)).text_color(p.subtle))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(team.name.clone()),
                            )
                            .child(status_badge(team.status, p)),
                    )
                    .child(hint(
                        format!(
                            "{repository} · {} {} · {accepted} accepted",
                            tickets.len(),
                            if tickets.len() == 1 {
                                "ticket"
                            } else {
                                "tickets"
                            }
                        ),
                        p,
                    )),
            );
        }
        body
    }

    fn tickets(
        &self,
        team: &Session,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let tickets: Vec<_> = snapshot
            .tickets
            .iter()
            .filter(|t| t.coordinator_id == team.id)
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
                                .child(ticket.title.clone()),
                        )
                        .child(pill(
                            humanize(&ticket.state),
                            ticket_state_color(&ticket.state, p),
                        )),
                )
                .child(hint(ticket.brief.clone(), p).line_clamp(4))
                .child(copyable("Branch", &ticket.branch, p));
            let agents = agents_of(ticket);
            if !agents.is_empty() {
                let mut list = div().flex().flex_col().mx(px(-8.));
                for agent in agents {
                    list = list.child(self.session_link(agent, agent.role.label().into(), p, cx));
                }
                entry = entry.child(list);
            }
            if ticket.state != "accepted" {
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
                s.parent_id.as_ref() == Some(&team.id)
                    && self.runtime(&s.id).is_none_or(|r| r.ticket_id.is_none())
            })
            .collect();
        if !unassigned.is_empty() {
            let mut list = div()
                .flex()
                .flex_col()
                .child(section("WITHOUT A TICKET", p));
            for agent in unassigned {
                list = list.child(self.session_link(agent, agent.name.clone(), p, cx));
            }
            body = body.child(list);
        }
        body
    }
}
