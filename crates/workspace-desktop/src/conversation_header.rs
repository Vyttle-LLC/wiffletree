//! The selected session's header and progress strip. Their height never changes, so the
//! header is its own cached view: it draws again only when what it shows changes, not with every
//! keystroke in the composer or frame of another view's spinner.
use super::*;
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{HEADER_HEIGHT, icon, page_header, progress_bar, segmented_bar};

/// The progress strip never changes height, so the transcript below it never moves.
const STRIP_HEIGHT: f32 = 36.;
const STRIP_BAR: f32 = 6.;

/// The header's slot in the conversation: the header above the strip.
pub(super) fn style() -> StyleRefinement {
    StyleRefinement::default()
        .flex_none()
        .w_full()
        .h(px(HEADER_HEIGHT + STRIP_HEIGHT))
}

pub(super) struct ConversationHeader {
    bridge: Bridge,
    shared: Shared,
    contexts: Entity<context_view::Contexts>,
}
impl EventEmitter<Intent> for ConversationHeader {}
impl ConversationHeader {
    pub(super) fn new(
        bridge: Bridge,
        contexts: Entity<context_view::Contexts>,
        cx: &mut Context<Self>,
    ) -> Self {
        // The context chip draws the selected session's measurement.
        cx.observe(&contexts, |_, _, cx| cx.notify()).detach();
        Self {
            bridge,
            shared: Shared::default(),
            contexts,
        }
    }
    /// Draws again for a new selection, workspace, progress or panel.
    pub(super) fn share(&mut self, shared: Shared, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.shared, shared);
        let (selection, moved) = self.shared.moved_from(&old);
        let progress = !Rc::ptr_eq(&old.progress, &self.shared.progress);
        if selection || moved || progress || old.panel != self.shared.panel {
            cx.notify();
        }
    }
    fn toggle_pause(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.shared.selected_session() {
            let status = if session.status == Status::Paused {
                Status::Ready
            } else {
                Status::Paused
            };
            self.request(
                Command::SetStatus {
                    session_id: session.id.clone(),
                    status,
                },
                window,
                cx,
            );
        }
    }
    /// Stop is offered only while some agent in the project is mid-turn.
    fn project_working(&self, session: &Session) -> bool {
        self.shared
            .index
            .iter()
            .flat_map(|index| index.sessions())
            .any(|s| s.project_id == session.project_id && s.status == Status::Working)
    }
    fn breadcrumb(&self, session: &Session) -> String {
        let mut parts = vec![self.shared.project_name()];
        if session.role.is_worker() {
            if let Some(team) = session
                .parent_id
                .as_deref()
                .and_then(|id| self.shared.session(id))
            {
                parts.push(team.name.clone());
            }
            if let Some(ticket) = self.shared.ticket_of(&session.id) {
                parts.push(ticket.title.clone());
            }
        }
        parts.push(session.role.label().into());
        self.shared
            .host
            .breadcrumb(parts.iter().map(String::as_str))
    }
    fn header(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let live = self.shared.live_enabled();
        let paused = session.status == Status::Paused;
        let compacting = self.shared.compacting(&session.id);
        let title = if session.role.is_worker() {
            self.shared
                .ticket_of(&session.id)
                .map_or(session.name.clone(), |t| t.title.clone())
        } else {
            session.name.clone()
        };
        let eyebrow = div().text_ellipsis().child(self.breadcrumb(session));
        page_header(eyebrow, div().text_ellipsis().child(title), p)
            .px_6()
            .gap_3()
            .children((self.contexts.read(cx).chip(session, p)).map(|chip| {
                chip.on_click(cx.listener(|_, _, _, c| c.emit(Intent::ShowPanel(Panel::Overview))))
            }))
            .child(ui::session_badge(&self.shared, session, p))
            .when(!session.archived, |d| {
                d.child(
                    button("pause-session")
                        .ghost()
                        .icon(icon(if paused { "play" } else { "pause" }))
                        // A compaction that has started cannot be taken back.
                        .disabled(compacting)
                        .tooltip(if compacting {
                            "Compacting · it cannot be interrupted"
                        } else if paused {
                            "Resume this session"
                        } else {
                            "Pause this session · interrupts an active turn"
                        })
                        .map(|b| ui::gated(b, &self.shared.host))
                        .on_click(cx.listener(|v, _, w, c| v.toggle_pause(w, c))),
                )
            })
            .when(
                session.role == Role::ProjectOrchestrator && !session.archived,
                |d| {
                    d.child(
                        button("new-ticket")
                            .ghost()
                            .icon(icon("plus"))
                            .label("New ticket")
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(|_, _, _, c| {
                                c.emit(Intent::Create {
                                    kind: Creation::Ticket,
                                    ticket: None,
                                })
                            })),
                    )
                },
            )
            // A project runs once you message it, so the only control it needs is Stop.
            .children(if session.archived {
                Some(
                    button("restore-session")
                        .outline()
                        .icon(icon("archive"))
                        .label("Restore")
                        .map(|b| ui::gated(b, &self.shared.host))
                        .on_click(cx.listener(|v, _, w, c| {
                            if let Some(id) = v.shared.selected.clone() {
                                v.set_archived(id, false, w, c);
                            }
                        })),
                )
            } else if live && self.project_working(session) {
                Some(
                    button("stop-project")
                        .outline()
                        .icon(icon("stop").text_color(p.red))
                        .label("Stop")
                        .tooltip("Interrupt this project’s active turns and hold its queue")
                        .map(|b| ui::gated(b, &self.shared.host))
                        .on_click(cx.listener(|v, _, w, c| {
                            if let Some(project_id) = v.shared.project_id() {
                                v.set_live(project_id, false, w, c)
                            }
                        })),
                )
            } else {
                None
            })
    }
    /// The progress strip pinned under the title: the project for a coordinator, and for any
    /// other agent its own checklist beside its ticket's figure. Its height is fixed and it
    /// is always drawn, so a progress change never moves the transcript.
    fn progress_strip(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let project = self
            .shared
            .project_progress()
            .filter(|project| project.coordinator_id == session.id);
        let strip = div()
            .flex_none()
            .h(px(STRIP_HEIGHT))
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .bg(p.surface)
            .border_b_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .text_size(px(font::SMALL));
        let (label, bar, figure) = match project {
            Some(project) => self.project_strip(project, p),
            None => self.agent_strip(session, p),
        };
        strip
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.55))
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(div().flex_1().min_w(px(80.)).child(bar))
            .child(div().flex_none().text_color(p.subtle).child(figure))
            .children(project.map(|project| {
                let crew = self.shared.progress.crew(project).len();
                button("crew")
                    .outline()
                    .xsmall()
                    .label(wording::crew_label(crew))
                    .selected(self.shared.panel == Some(Panel::Crew))
                    .tooltip("Every ticket and the agents working on it")
                    .on_click(cx.listener(|_, _, _, c| c.emit(Intent::TogglePanel(Panel::Crew))))
            }))
    }
    /// A project's phase, its bar with a segment per ticket, and its figure. Before any
    /// ticket exists the bar is the coordinator's own.
    fn project_strip(&self, project: &ProjectProgress, p: Palette) -> (String, Div, String) {
        let tickets = self.shared.progress.tickets_of(project);
        let bar = if tickets.is_empty() {
            progress_bar(project.position, project.state, STRIP_BAR, p)
        } else {
            let segments: Vec<_> = tickets.iter().map(|t| (t.position, t.state)).collect();
            segmented_bar(&segments, STRIP_BAR, p)
        };
        (
            project.phase.label().into(),
            bar,
            wording::project_summary(project),
        )
    }
    /// An agent's own label and bar, or its ticket's stage and bar when it has nothing of
    /// its own to show.
    fn agent_strip(&self, session: &Session, p: Palette) -> (String, Div, String) {
        let agent = self.shared.progress.agent(&session.id);
        let ticket = self
            .shared
            .ticket_of(&session.id)
            .and_then(|ticket| self.shared.progress.ticket(&ticket.id));
        let figure = wording::strip_figure(agent, ticket);
        let own = agent.and_then(|agent| Some((wording::strip_label(agent)?, agent)));
        match (own, ticket) {
            (Some((label, agent)), _) => {
                let bar = progress_bar(agent.position(), agent.state, STRIP_BAR, p);
                (label, bar, figure)
            }
            (None, Some(ticket)) => {
                let bar = progress_bar(Some(ticket.position), ticket.state, STRIP_BAR, p);
                (ticket.stage.label().into(), bar, figure)
            }
            (None, None) => {
                let empty = progress_bar(None, BarState::InProgress, STRIP_BAR, p);
                (String::new(), empty, figure)
            }
        }
    }
}
impl Requests for ConversationHeader {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        _: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = reply {
            toast_error(error, window, cx);
        }
    }
}
impl Render for ConversationHeader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("ConversationHeader");
        let p = palette(is_dark(cx));
        let Some(session) = self.shared.selected_session() else {
            return div();
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(session, p, cx))
            .child(self.progress_strip(session, p, cx))
    }
}
