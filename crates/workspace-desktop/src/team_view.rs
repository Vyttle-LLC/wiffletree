//! A project's tickets with their repository, agents, verification cycles and decision ledger.
use super::*;
use gpui_component::button::ButtonVariants;
use inspector::copyable;
use ui::{
    card, empty_state, hint, humanize, icon, mono, pill, section, session_icon, ticket_state_color,
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
            let (state, state_color) = match tree::waiver_note(ticket) {
                Some(_) => ("Accepted with waiver".to_owned(), p.yellow),
                None => (
                    humanize(&ticket.state),
                    ticket_state_color(&ticket.state, p),
                ),
            };
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
                        .children(warning_badge(snapshot, ticket, p))
                        .child(pill(state, state_color)),
                )
                .child(hint(ticket.brief.clone(), p).line_clamp(4))
                .child(copyable("Branch", &ticket.branch, p))
                .children(
                    ticket
                        .waiver
                        .as_ref()
                        .map(|reason| waiver_block(ticket, reason, p)),
                );
            let rounds = tree::verification_lines(ticket);
            if !rounds.is_empty() {
                let mut list = div().flex().flex_col().gap_1();
                for line in rounds {
                    list = list.child(hint(line, p));
                }
                entry = entry.child(list);
            }
            let ledger = tree::ledger_rows(ticket);
            if !ledger.is_empty() {
                let untriaged = ledger
                    .iter()
                    .filter(|row| row.status == EntryStatus::Untriaged)
                    .count();
                entry = entry
                    .child(
                        section(format!("DECISION LEDGER · {}", ledger.len()), p)
                            .child(hint(format!("{untriaged} untriaged"), p).text_size(px(11.))),
                    )
                    .child(ledger_list(ledger, p));
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

/// Why the coordinator accepted the ticket despite a blocked cycle, and what it left open.
fn waiver_block(ticket: &Ticket, reason: &str, p: Palette) -> Div {
    let open: Vec<&str> = ticket
        .ledger
        .iter()
        .filter(|e| e.status == EntryStatus::Open)
        .map(|e| e.id.as_str())
        .collect();
    let left = if open.is_empty() {
        "Verdict stays blocked".to_owned()
    } else {
        format!(
            "Open at acceptance: {} · verdict stays blocked",
            open.join(", ")
        )
    };
    div()
        .border_l_2()
        .border_color(p.yellow)
        .rounded_r(px(6.))
        .bg(p.yellow.opacity(0.06))
        .px(px(10.))
        .py(px(6.))
        .text_size(px(12.))
        .child(
            div()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Waived by the coordinator."),
                )
                .child(reason.to_owned()),
        )
        .child(hint(left, p).text_size(px(11.)))
}

/// Each entry's id, summary, place and decision, with its status as a pill.
fn ledger_list(rows: Vec<tree::LedgerRow>, p: Palette) -> Div {
    let line = p.edge.opacity(0.5);
    let mut list = div().flex().flex_col().border_t_1().border_color(line);
    for row in rows {
        let color = match row.status {
            EntryStatus::Open => p.red,
            EntryStatus::Fixed => p.green,
            EntryStatus::Untriaged => p.focus,
            EntryStatus::FixNow => p.yellow,
            EntryStatus::FollowUp => p.blue,
            EntryStatus::WontFix => p.subtle,
        };
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .py(px(9.))
                .border_b_1()
                .border_color(line)
                .child(mono(row.id, p).w(px(28.)).flex_none())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(12.)).child(row.summary))
                        .child(
                            div()
                                .font_family(".SystemMonoFont")
                                .text_size(px(10.5))
                                .text_color(p.subtle)
                                .child(row.place),
                        )
                        .child(hint(row.note, p).text_size(px(11.))),
                )
                .child(pill(humanize(row.status.label()), color)),
        );
    }
    list
}

/// Flags an open ticket whose branch has fallen behind its base or overlaps another open
/// ticket's, or whose check could not run; hovering lists the warnings.
fn warning_badge(snapshot: &Snapshot, ticket: &Ticket, p: Palette) -> Option<impl IntoElement> {
    let warnings = snapshot
        .branch_warnings
        .iter()
        .find(|w| w.ticket_id == ticket.id)
        .filter(|_| ticket.is_open())?;
    let label = warning_label(warnings);
    let color = match label {
        "Conflicts" => p.red,
        "Behind" => p.subtle,
        _ => p.yellow,
    };
    let lines = warnings.lines().join("\n");
    Some(
        pill(label, color)
            .id(SharedString::from(format!("warnings-{}", ticket.id)))
            .tooltip(move |w, c| gpui_component::tooltip::Tooltip::new(lines.clone()).build(w, c)),
    )
}

/// The most pressing warning: a check that could not run outranks overlaps and drift.
fn warning_label(warnings: &BranchWarnings) -> &'static str {
    if warnings.has_conflicts() {
        "Conflicts"
    } else if !warnings.failures.is_empty() || warnings.overlaps.iter().any(|o| o.failure.is_some())
    {
        "Check failed"
    } else if !warnings.overlaps.is_empty() {
        "Overlaps"
    } else {
        "Behind"
    }
}

#[cfg(test)]
mod tests {
    use super::warning_label;
    use workspace_core::{BranchOverlap, BranchWarnings};

    #[test]
    fn a_failed_check_outranks_overlaps_and_drift_but_not_conflicts() {
        let mut warnings = BranchWarnings {
            base_ahead: 1,
            overlaps: vec![BranchOverlap::default()],
            failures: vec!["last fetch failed".into()],
            ..Default::default()
        };
        assert_eq!(warning_label(&warnings), "Check failed");
        warnings.base_conflicts.push("a.rs".into());
        assert_eq!(warning_label(&warnings), "Conflicts");
    }
}
