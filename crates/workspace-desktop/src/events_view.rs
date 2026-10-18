//! The Events panel: what happened in the selected project, or to the selected agent.
use super::*;
use ui::{empty_state, hint, section};
use wording::{age, humanize};

impl Inspector {
    /// Events for the selected session: everything for a project, a team's own and its workers'.
    pub(super) fn events_panel(&self, p: Palette) -> Div {
        let Some(selected) = self.shared.selected_session() else {
            return div();
        };
        let PanelData::Events(events) = &self.panel_data else {
            return hint("Loading events…", p);
        };
        let in_scope = |event: &Activity| {
            selected.role == Role::ProjectOrchestrator
                || event
                    .session_id
                    .as_deref()
                    .is_some_and(|id| id == selected.id)
        };
        let mut body = div().flex().flex_col().child(section(
            format!("RECENT · {}", selected.name.to_uppercase()),
            p,
        ));
        let mut shown = 0;
        for event in events.iter().filter(|event| in_scope(event)) {
            shown += 1;
            let source = event
                .session_id
                .as_deref()
                .and_then(|id| self.shared.session(id))
                .map(|s| s.name.clone());
            body = body.child(
                div()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::HAIRLINE))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(font::MESSAGE))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(humanize(&event.kind)),
                            )
                            .child(hint(age(event.created_at), p).flex_none()),
                    )
                    .when_some(source, |d, source| d.child(hint(source, p)))
                    .child(
                        // A rejection's allowed list is the part a reader needs.
                        hint(event.detail.clone(), p)
                            .when(event.kind != "model_rejected", |d| d.line_clamp(3)),
                    ),
            );
        }
        if shown == 0 {
            body = body.child(empty_state(
                "activity",
                "Nothing yet",
                "Messages, status changes and reports appear here as they happen.",
                p,
            ));
        }
        body
    }
}
