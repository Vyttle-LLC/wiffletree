//! The selected session's header and progress strip. Their height never changes, so the
//! header is its own cached view: it draws again only when what it shows changes, not with every
//! keystroke in the composer or frame of another view's spinner.
use super::*;
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{HEADER_HEIGHT, icon, page_header, pill, progress_bar, segmented_bar};

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

/// A child project's parent and the parent's coordinator; none for a loose project.
fn parent_link<'a>(
    index: &'a WorkspaceIndex,
    project_id: &str,
) -> Option<(&'a Project, &'a Session)> {
    let parent = index.project(index.project(project_id)?.parent_id.as_ref()?)?;
    Some((parent, index.roots(&parent.id).next()?))
}

/// A header's breadcrumb: a link back to a coordinator, by its session id and the name shown,
/// then what follows the link; or the plain text above a coordinator with nothing above it.
#[derive(Debug, PartialEq)]
enum Breadcrumb {
    Back {
        id: String,
        name: String,
        rest: Option<String>,
    },
    Plain(String),
}

/// Where the session sits: the project and role for its coordinator, or for a child's
/// coordinator a link back to the parent's, then the child; and for any other agent a link back
/// to the coordinator, then the agent's ticket.
fn breadcrumb(shared: &Shared, session: &Session) -> Breadcrumb {
    let project = shared.project_name();
    if let Some(coordinator) = shared.coordinator().filter(|c| c.id != session.id) {
        let ticket = (shared.ticket_of(&session.id).zip(shared.snapshot()))
            .map(|(ticket, snapshot)| wording::ticket_label(snapshot, ticket));
        return Breadcrumb::Back {
            id: coordinator.id.clone(),
            name: project,
            rest: ticket,
        };
    }
    let index = shared.index.as_deref();
    match index.and_then(|index| parent_link(index, &session.project_id)) {
        Some((parent, coordinator)) => Breadcrumb::Back {
            id: coordinator.id.clone(),
            name: parent.name.clone(),
            rest: Some(project),
        },
        None => Breadcrumb::Plain(
            shared
                .host
                .breadcrumb([project.as_str(), session.role.label()]),
        ),
    }
}

/// A child coordinator's strip line, which says where its questions go; none for any other
/// project.
fn child_strip_line(index: &WorkspaceIndex, project_id: &str) -> Option<String> {
    let (parent, _) = parent_link(index, project_id)?;
    Some(wording::child_strip(&parent.name))
}

/// The project's apps, then its layers, for the coordinator's header; none before it has a
/// ticket.
fn tag_chips(snapshot: &Snapshot, project_id: &str) -> Vec<(TagKind, String)> {
    let Some(tags) = snapshot.project_tags.get(project_id) else {
        return vec![];
    };
    let apps = tags.apps.iter().map(|app| (TagKind::App, app.clone()));
    let layers = tags
        .layers
        .iter()
        .map(|layer| (TagKind::Layer, layer.clone()));
    apps.chain(layers).collect()
}

/// What a chip opens: its tag's bucket in the App or Layer lens, in the project's Space.
pub(super) fn chip_opens(
    index: &WorkspaceIndex,
    project_id: &str,
    kind: TagKind,
    value: String,
) -> Option<Intent> {
    let project = index.project(project_id)?;
    Some(Intent::OpenLensBucket {
        lens: match kind {
            TagKind::App => Lens::App,
            TagKind::Layer => Lens::Layer,
        },
        space: tree::section_of(index, project),
        bucket: lenses::Bucket::Tag(value),
    })
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
    fn breadcrumb(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let (id, name, rest) = match breadcrumb(&self.shared, session) {
            Breadcrumb::Back { id, name, rest } => (id, name, rest),
            Breadcrumb::Plain(text) => return div().text_ellipsis().child(text),
        };
        let host = self.shared.host.breadcrumb([]);
        div()
            .min_w_0()
            .flex()
            .gap_1()
            .when(!host.is_empty(), |d| d.child(format!("{host}  ›")))
            .child(
                div()
                    .id("back-to-coordinator")
                    .flex_none()
                    .cursor_pointer()
                    .text_color(p.focus)
                    .hover(|d| d.underline())
                    .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::Select(id.clone()))))
                    .child(format!("← {name}")),
            )
            .children(rest.map(|rest| div().min_w_0().text_ellipsis().child(format!("/  {rest}"))))
    }
    /// For one of the Assistant's work items, opened from the Assistant: Stop work while it
    /// runs, and Back to chat.
    fn assistant_work_controls(&self, cx: &mut Context<Self>) -> Vec<Button> {
        if !self.shared.assistant_work_open() {
            return vec![];
        }
        let project = (self.shared.project_id())
            .and_then(|id| self.shared.index.as_ref()?.project(&id).cloned());
        let running = (project.as_ref())
            .and_then(|p| p.assistant_work.as_ref())
            .is_some_and(|work| matches!(work.state, WorkState::Running | WorkState::Paused));
        let mut controls = vec![];
        if let (true, Some(project)) = (running, project) {
            controls.push(
                button("stop-work")
                    .outline()
                    .small()
                    .label("Stop work")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(cx.listener(move |v, _, w, c| {
                        v.ask(ask::stop_work(project.id.clone()), w, c, |_, done, w, c| {
                            if let Err(error) = done {
                                toast_error(error, w, c);
                            }
                        })
                    })),
            );
        }
        controls.push(
            button("back-to-chat")
                .small()
                .label("Back to chat")
                .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Assistant)))),
        );
        controls
    }
    /// Opens and closes Effective settings: what a ticket's agents get, or the project's from
    /// its coordinator, and where each value came from.
    fn settings_toggle(&self, cx: &mut Context<Self>) -> Option<Button> {
        self.shared.project_id()?;
        Some(
            button("effective-settings")
                .outline()
                .small()
                .icon(icon("settings"))
                .selected(self.shared.panel == Some(Panel::Settings))
                .tooltip("Effective settings · what this project's agents get, and where each value came from")
                .on_click(
                    cx.listener(|_, _, _, c| c.emit(Intent::TogglePanel(Panel::Settings))),
                ),
        )
    }
    fn header(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let live = self.shared.live_enabled();
        let paused = session.status == Status::Paused;
        let compacting = self.shared.compacting(&session.id);
        // An agent's ticket is in its breadcrumb, so its title names the agent.
        let title = match (self.shared.ticket_of(&session.id), &self.shared.index) {
            (Some(_), Some(index)) => index.agent_label(session),
            _ => session.name.clone(),
        };
        let chips = (self.shared.coordinator())
            .filter(|c| c.id == session.id)
            .zip(self.shared.snapshot())
            .map_or_else(Vec::new, |(_, snapshot)| {
                tag_chips(snapshot, &session.project_id)
            });
        page_header(
            self.breadcrumb(session, p, cx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(div().min_w_0().text_ellipsis().child(title))
                .children(chips.into_iter().enumerate().map(|(at, (kind, value))| {
                    let project = session.project_id.clone();
                    pill(value.clone(), p.subtle)
                        .id(("tag-chip", at))
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.text))
                        .on_click(cx.listener(move |v, _, _, c| {
                            let index = v.shared.index.as_deref();
                            let opens = index
                                .and_then(|index| chip_opens(index, &project, kind, value.clone()));
                            if let Some(opens) = opens {
                                c.emit(opens);
                            }
                        }))
                })),
            p,
        )
        .px_6()
        .gap_3()
        .children(self.assistant_work_controls(cx))
        .children(self.settings_toggle(cx))
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
        } else if live && self.project_working(session) && !self.shared.assistant_work_open() {
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
    fn progress_strip(&self, session: &Session, p: Palette) -> Div {
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
        // A child's strip says where its questions go instead of its phase.
        let label = (self.shared.index.as_deref())
            .and_then(|index| child_strip_line(index, &project.project_id))
            .unwrap_or_else(|| project.phase.label().into());
        (label, bar, wording::project_summary(project))
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
            .child(self.progress_strip(session, p))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{Breadcrumb, breadcrumb, child_strip_line, parent_link, tag_chips};
    use crate::{shared::Shared, test_support::ticket_store};
    use std::rc::Rc;
    use workspace_core::*;

    #[test]
    fn a_child_s_breadcrumb_and_strip_name_its_parent() {
        let (_directory, mut host, root, _) = ticket_store();
        let child = host.create_project("Contrast warnings").unwrap();
        host.set_project_parent(&child.id, Some(&root.project_id))
            .unwrap();
        let index = Rc::new(WorkspaceIndex::new(host.snapshot().unwrap(), Rc::default()));
        let coordinator = |project: &str| index.roots(project).next().unwrap().clone();
        let selecting = |session: &Session| Shared {
            index: Some(index.clone()),
            selected: Some(session.id.clone()),
            ..Default::default()
        };

        let child_coordinator = coordinator(&child.id);
        assert_eq!(
            breadcrumb(&selecting(&child_coordinator), &child_coordinator),
            Breadcrumb::Back {
                id: root.id.clone(),
                name: "Calls".into(),
                rest: Some("Contrast warnings".into()),
            }
        );
        assert_eq!(
            breadcrumb(&selecting(&root), &root),
            Breadcrumb::Plain("Calls  ›  Coordinator".into())
        );
        assert_eq!(
            child_strip_line(&index, &child.id).as_deref(),
            Some("Child of Calls · questions go to Calls first")
        );
        assert!(child_strip_line(&index, &root.project_id).is_none());
    }

    #[test]
    fn a_child_s_header_links_back_to_its_parent_s_coordinator() {
        let (_directory, mut host, root, _) = ticket_store();
        let child = host.create_project("Contrast warnings").unwrap();
        let loose = host.create_project("Recipes").unwrap();
        host.set_project_parent(&child.id, Some(&root.project_id))
            .unwrap();
        let index = WorkspaceIndex::new(host.snapshot().unwrap(), Rc::default());

        let (parent, coordinator) = parent_link(&index, &child.id).unwrap();

        assert_eq!(
            (parent.name.as_str(), coordinator.id.as_str()),
            ("Calls", root.id.as_str())
        );
        assert!(parent_link(&index, &loose.id).is_none());
        assert!(parent_link(&index, &root.project_id).is_none());
    }

    #[test]
    fn the_chips_name_the_apps_then_the_layers_and_none_before_a_ticket() {
        let (_directory, host, root, _) = ticket_store();
        let mut snapshot = host.snapshot().unwrap();
        snapshot.project_tags.insert(
            root.project_id.clone(),
            ProjectTags {
                started: true,
                apps: vec!["nvim".into()],
                layers: vec!["frontend".into(), "backend".into()],
            },
        );
        let chips: Vec<(TagKind, String)> = tag_chips(&snapshot, &root.project_id);
        let named: Vec<(TagKind, &str)> = chips.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            named,
            [
                (TagKind::App, "nvim"),
                (TagKind::Layer, "frontend"),
                (TagKind::Layer, "backend")
            ]
        );
        assert!(tag_chips(&snapshot, "not-started").is_empty());
    }
}
