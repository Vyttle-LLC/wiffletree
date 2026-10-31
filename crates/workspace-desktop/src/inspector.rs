//! The right-edge rail and the inspector panel it opens for the selected session.
use super::*;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use ui::{
    card, empty_state, eyebrow, hint, icon, mono, page_header, pill, property, section,
    session_icon,
};
use wording::{from_now, local_time};

const MOVE_TO_SPACE: &str = "Its later entries go to that Space’s brain; earlier ones stay where they are. Its repositories don’t change.";

fn scope(role: Role) -> &'static str {
    match role {
        Role::ProjectOrchestrator => "Project",
        Role::TaskOrchestrator => "Retired repository coordinator",
        _ => "Agent",
    }
}

pub(super) fn copy_button(id: impl Into<ElementId>, text: String) -> Button {
    button(id)
        .ghost()
        .xsmall()
        .icon(icon("copy").size(px(13.)))
        .tooltip("Copy")
        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())))
}

/// A labelled value in monospace with a copy affordance, for paths and branches.
pub(super) fn copyable(label: &'static str, value: &str, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(px(56.))
                .text_size(px(font::CAPTION))
                .text_color(p.subtle)
                .child(label),
        )
        .child(mono(value.to_owned(), p).flex_1())
        .child(copy_button(
            SharedString::from(format!("copy-{label}-{value}")),
            value.to_owned(),
        ))
}

/// A stacked ticket's parent branch, which its branch started from and its PR opens into.
pub(super) fn parent_branch(index: &WorkspaceIndex, ticket: &Ticket, p: Palette) -> Option<Div> {
    let parent = index.ticket(ticket.stacked_on.as_ref()?)?;
    Some(copyable("Parent", &parent.branch, p))
}

impl Panel {
    pub(super) fn available(self, role: Role) -> bool {
        match self {
            Self::Attention => role == Role::ProjectOrchestrator,
            Self::Team => !role.is_worker(),
            Self::Git => role != Role::ProjectOrchestrator,
            _ => true,
        }
    }
    /// The panel shown once a session of `role` is selected: this one where that session
    /// has it, Overview otherwise.
    pub(super) fn kept_for(self, role: Role) -> Self {
        if self.available(role) {
            self
        } else {
            Self::Overview
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Team => "team",
            Self::Memory => "memory",
            Self::Events => "activity",
            Self::Attention => "inbox",
            Self::Git => "git",
            Self::Files => "folder",
            Self::Processes => "terminal",
            Self::Agents => "team",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Team => "Tickets",
            Self::Memory => "Memory",
            Self::Events => "Events",
            Self::Attention => "Inbox",
            Self::Git => "Git",
            Self::Files => "Files",
            Self::Processes => "Processes",
            Self::Agents => "Agents",
        }
    }
}

/// The inspector panel the rail opens beside the conversation, for the selected session.
pub(super) struct Inspector {
    pub(super) bridge: Bridge,
    pub(super) shared: Shared,
    pub(super) inbox: Entity<inbox::Inbox>,
    pub(super) contexts: Entity<context_view::Contexts>,
    /// The Inbox lists the Skills page's questions.
    pub(super) skills: Entity<skills_view::SkillsPage>,
    pub(super) panel_data: PanelData,
    pub(super) memory_title: Entity<InputState>,
    pub(super) memory_body: Entity<TextareaState>,
    pub(super) memory_kind: LogKind,
    /// What the Files panel shows; fetched only while that panel is open.
    pub(super) files: files_view::Files,
    /// What the Processes panel shows; read only while that panel is open.
    pub(super) processes: processes_view::Processes,
    pub(super) agents_clock: bool,
    /// The Agents panel's "Previous agents" section is unfolded.
    pub(super) previous_open: bool,
    /// The Compact form of each session that has it open, and the field they share.
    pub(super) compact_forms: BTreeMap<String, context_view::CompactForm>,
    pub(super) compact_note: Entity<TextareaState>,
}
impl EventEmitter<Intent> for Inspector {}
impl Inspector {
    pub(super) fn new(
        bridge: Bridge,
        inbox: Entity<inbox::Inbox>,
        contexts: Entity<context_view::Contexts>,
        skills: Entity<skills_view::SkillsPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&inbox, |_, _, cx| cx.notify()).detach();
        cx.observe(&contexts, |_, _, cx| cx.notify()).detach();
        cx.observe(&skills, |_, _, cx| cx.notify()).detach();
        Self {
            bridge,
            shared: Shared::default(),
            inbox,
            contexts,
            skills,
            panel_data: PanelData::Loading,
            memory_title: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Title, e.g. Palette stays compatible")
            }),
            memory_body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 8)
                    .placeholder("What was decided or observed, and why it matters")
            }),
            memory_kind: LogKind::Decision,
            files: Default::default(),
            processes: Default::default(),
            agents_clock: false,
            previous_open: false,
            compact_forms: BTreeMap::new(),
            compact_note: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 6)
                    .placeholder("For example: the failing test names")
            }),
        }
    }
    /// The open panel follows the selected session: it loads again for a new selection, a new
    /// panel and each snapshot.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.shared, shared);
        let (selection, moved) = self.shared.moved_from(&old);
        let panel = self.shared.panel != old.panel;
        let page = self.shared.page != old.page;
        let progress = !Rc::ptr_eq(&self.shared.progress, &old.progress);
        if selection && let Some(id) = self.shared.selected.clone() {
            self.swap_compact_note(old.selected.as_deref(), &id, window, cx);
        }
        if selection || panel {
            self.panel_data = PanelData::Loading;
        }
        if selection || panel || moved {
            self.load_panel(window, cx);
        }
        if page {
            self.sync_process_readings(window, cx);
        }
        if selection || panel || progress {
            self.sync_agents_clock(window, cx);
        }
        if selection || moved || panel || page || progress {
            cx.notify();
        }
    }
    fn accept_panel_data(
        &mut self,
        data: Result<PanelData, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match data {
            Ok(data) => self.panel_data = data,
            Err(error) => toast_error(error, window, cx),
        }
    }
    fn load_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project_id) = self.shared.project_id() else {
            return;
        };
        match self.shared.panel {
            Some(Panel::Memory) => {
                let ask = ask::logs(project_id.clone(), None, 30);
                self.ask(ask, window, cx, move |view, logs, window, cx| {
                    let shown = view.shared.panel == Some(Panel::Memory)
                        && view.shared.project_id() == Some(project_id);
                    if shown {
                        view.accept_panel_data(logs.map(PanelData::Memory), window, cx);
                    }
                })
            }
            Some(Panel::Events) => {
                let ask = ask::activity(project_id.clone(), None, 50);
                self.ask(ask, window, cx, move |view, events, window, cx| {
                    let shown = view.shared.panel == Some(Panel::Events)
                        && view.shared.project_id() == Some(project_id);
                    if shown {
                        view.accept_panel_data(events.map(PanelData::Events), window, cx);
                    }
                })
            }
            Some(Panel::Git) => {
                let repository =
                    (self.shared.selected_session()).and_then(|s| s.repository_id.clone());
                if let Some(repository_id) = repository {
                    let ask = ask::git_history(repository_id.clone(), 0, 30);
                    self.ask(ask, window, cx, move |view, history, window, cx| {
                        let shown = view.shared.panel == Some(Panel::Git)
                            && (view.shared.selected_session())
                                .and_then(|s| s.repository_id.as_ref())
                                == Some(&repository_id);
                        if shown {
                            let commits = history.map(|history| PanelData::Git(history.commits));
                            view.accept_panel_data(commits, window, cx);
                        }
                    })
                }
            }
            Some(Panel::Files) => self.fetch_files(window, cx),
            Some(Panel::Processes) => self.sync_process_readings(window, cx),
            _ => {}
        }
    }
    pub(super) fn capture_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.shared.selected.clone() else {
            return;
        };
        let title = self.memory_title.read(cx).value().trim().to_string();
        let changed = self.memory_body.read(cx).value().trim().to_string();
        if title.is_empty() || changed.is_empty() {
            toast_error("Add a title and a description first.", window, cx);
            return;
        }
        self.request(
            Command::AppendLog {
                input: LogInput {
                    session_id,
                    milestone: None,
                    kind: self.memory_kind.clone(),
                    title,
                    repository: None,
                    ticket_id: None,
                    changed,
                    why: "Captured directly in native prototype".into(),
                    state: "Source report; evidence not independently verified".into(),
                },
            },
            window,
            cx,
        );
    }

    pub(super) fn panel_view(&self, panel: Panel, p: Palette, cx: &mut Context<Self>) -> Div {
        let role = self.shared.selected_role();
        let body = match panel {
            Panel::Overview => self.overview(p, cx),
            Panel::Team => self.team_panel(p, cx),
            Panel::Memory => self.memory_panel(p, cx),
            Panel::Events => self.events_panel(p),
            Panel::Attention => self.attention_panel(p, cx),
            Panel::Git => self.git_panel(p),
            Panel::Files => self.files_message(p),
            Panel::Processes => self.processes_panel(p, cx),
            Panel::Agents => self.agents_panel(p, cx),
        };
        // The Files panel lists its tree a screenful at a time, so the tree scrolls itself.
        let tree = (panel == Panel::Files)
            .then(|| self.files_tree(p, cx))
            .flatten();
        // The Files panel keeps its counts and what is live now in view while its tree scrolls.
        let (head, foot) = match panel {
            Panel::Files => (self.files_head(p, cx), self.files_live(p)),
            Panel::Processes => (self.processes_head(p, cx), None),
            _ => (None, None),
        };
        // The Agents panel is the project's, whichever of its agents is selected.
        let (scope, subtitle) = match panel {
            Panel::Agents => ("Project", self.shared.project_name()),
            _ => (scope(role), self.shared.owner_name()),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(
                page_header(
                    div()
                        .text_ellipsis()
                        .child(format!("{scope}  ·  {subtitle}")),
                    div().child(panel.title()),
                    p,
                )
                .pl_5()
                .pr_3()
                .gap_2()
                .child(
                    button("close-inspector")
                        .ghost()
                        .icon(icon("close"))
                        .tooltip("Close")
                        .on_click(cx.listener(|_, _, _, c| c.emit(Intent::ClosePanel))),
                ),
            )
            .children(head)
            .map(|d| match tree {
                Some(tree) => d.child(tree),
                None => d.child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_5()
                        .text_size(px(font::TEXT))
                        .child(body.w_full()),
                ),
            })
            .children(foot)
    }

    fn stat(value: usize, label: &'static str, p: Palette) -> Div {
        card(p)
            .flex_1()
            .gap_0()
            .child(
                div()
                    .text_size(px(font::FIGURE_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .child(value.to_string()),
            )
            .child(hint(label, p))
    }

    /// The project's Space and the brain it logs to, with Move to another Space, which asks
    /// first.
    fn space_section(
        &self,
        project: &Project,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let space = snapshot.spaces.iter().find(|s| s.id == project.space);
        let others: Vec<(SpaceId, String)> = (snapshot.spaces.iter())
            .filter(|s| s.id != project.space)
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let (project_id, project_name) = (project.id.clone(), project.name.clone());
        let weak = cx.weak_entity();
        let mut section = div().flex().flex_col().gap_1().child(
            section("SPACE", p).child(
                ui::gated(
                    button("move-to-space")
                        .ghost()
                        .label("Move to another Space")
                        .dropdown_caret(true)
                        .disabled(others.is_empty()),
                    &self.shared.host,
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for (id, name) in &others {
                        let (weak, id, name) = (weak.clone(), id.clone(), name.clone());
                        let (project_id, project_name) = (project_id.clone(), project_name.clone());
                        menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                            move |_, window, cx| {
                                let move_to = Command::MoveProjectToSpace {
                                    project_id: project_id.clone(),
                                    space_id: id.clone(),
                                };
                                let weak = weak.clone();
                                let title = format!("Move {project_name} to {name}?");
                                window.open_alert_dialog(cx, move |dialog, _, _| {
                                    let (weak, move_to) = (weak.clone(), move_to.clone());
                                    dialog
                                        .title(title.clone())
                                        .confirm()
                                        .ok_text("Move")
                                        .child(MOVE_TO_SPACE)
                                        .on_ok(move |_, window, cx| {
                                            let move_to = move_to.clone();
                                            let _ = weak.update(cx, |view, cx| {
                                                view.request(move_to, window, cx)
                                            });
                                            true
                                        })
                                });
                            },
                        ));
                    }
                    menu
                }),
            ),
        );
        let Some(space) = space else {
            return section.child(hint("No Space", p));
        };
        let local = snapshot.space_locals.get(&space.id);
        section = section.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(spaces_view::space_mark(spaces_view::space_color(
                    &snapshot.spaces,
                    &space.id,
                    p,
                )))
                .child(space.name.clone()),
        );
        section.child(hint(
            format!("Logs to {}.", spaces_view::brain_label(space, local)),
            p,
        ))
    }

    fn repository_row(repository: &Repository, p: Palette) -> Stateful<Div> {
        let path = repository.path.clone();
        div()
            .id(SharedString::from(format!("repository-{}", repository.id)))
            .flex()
            .items_center()
            .gap_3()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::HAIRLINE))
            .tooltip(move |w, c| Tooltip::new(path.clone()).build(w, c))
            .child(icon("folder").text_color(p.subtle))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .child(repository.name.clone()),
                    )
                    .child(mono(repository.path.clone(), p)),
            )
            .child(pill(repository.base.clone(), p.subtle))
    }

    /// A coordinator's timer: what it is, whose it is, its cadence and when it fires next.
    fn schedule_row(&self, schedule: &Schedule, p: Palette) -> Div {
        let local = |at| local_time(at, "America/New_York").unwrap_or_default();
        let owner = self
            .shared
            .session(&schedule.session_id)
            .map_or("Unknown session".into(), |s| s.name.clone());
        let mut cadence = schedule
            .every_ms
            .map_or("Once".into(), |ms| format!("Every {}", duration_label(ms)));
        if let Some(until) = schedule.until {
            cadence = format!("{cadence} until {}", local(until));
        }
        let next = schedule.next_fire_at.unwrap_or_default();
        div()
            .flex()
            .items_center()
            .gap_3()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::HAIRLINE))
            .child(icon("clock").text_color(p.subtle))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .child(schedule.label.clone()),
                    )
                    .child(hint(format!("{owner} · {cadence}"), p)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(div().font_weight(FontWeight::MEDIUM).child(from_now(next)))
                    .child(hint(local(next), p)),
            )
    }

    fn overview(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let (Some(session), Some(index)) =
            (self.shared.selected_session(), self.shared.index.as_ref())
        else {
            return div();
        };
        let snapshot = index.snapshot();
        let live = self.shared.live_enabled();
        let profile = self.shared.selected_profile();
        let mut body = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(38.))
                            .flex_none()
                            .rounded_lg()
                            .bg(p.accent.opacity(0.1))
                            .text_color(p.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(session_icon(session.role)).size(px(19.))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(font::BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_ellipsis()
                                    .child(session.name.clone()),
                            )
                            .child(hint(session.role.label(), p)),
                    )
                    .child(ui::session_badge(&self.shared, session, p)),
            )
            .child(
                card(p)
                    .when_some(profile, |d, profile| {
                        d.child(property(
                            "Model",
                            format!(
                                "{} · {} · {}",
                                profile.provider.label(),
                                profile.model,
                                profile.effort
                            ),
                            p,
                        ))
                    })
                    .when_some(
                        session
                            .parent_id
                            .as_deref()
                            .and_then(|id| self.shared.session(id)),
                        |d, parent| d.child(property("Reports to", parent.name.clone(), p)),
                    )
                    .child(property(
                        "Project",
                        if live { "Running" } else { "Stopped" },
                        p,
                    )),
            );
        body = body.child(self.context_card(session, p, cx));
        if session.role == Role::ProjectOrchestrator {
            let in_project = |s: &&Session| s.project_id == session.project_id;
            // Open tickets are live; the snapshot counts the others.
            let tickets = (index.snapshot().tickets_owned.get(&session.id))
                .copied()
                .unwrap_or(0);
            let open = (index.tickets_owned_by(&session.id))
                .filter(|t| t.is_open())
                .count();
            let agents = (index.sessions())
                .filter(|s| self.shared.listed(s))
                .filter(in_project)
                .filter(|s| s.role.is_worker())
                .count();
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(Self::stat(tickets as usize, "Tickets", p))
                    .child(Self::stat(open, "Open", p))
                    .child(Self::stat(agents, "Agents", p)),
            );
            if let Some(project) = index.project(&session.project_id) {
                body = body.child(self.space_section(project, snapshot, p, cx));
            }
            let weak = cx.weak_entity();
            let mut repositories = div().flex().flex_col().child(
                section("REPOSITORIES", p).child(
                    button("add-repository")
                        .ghost()
                        .icon(icon("plus"))
                        .label("Add")
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu;
                            for (label, kind) in [
                                ("Choose repositories…", Creation::ProjectRepositories),
                                ("Add repositories…", Creation::Import),
                                ("Attach one with a custom base…", Creation::Repository),
                            ] {
                                let weak = weak.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |_, cx| {
                                            cx.emit(Intent::Create { kind, ticket: None })
                                        });
                                    },
                                ));
                            }
                            menu
                        }),
                ),
            );
            let used = snapshot.project_repositories(&session.project_id);
            let uses_all = snapshot
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
                .is_some_and(|project| project.repositories.is_none());
            repositories = repositories.child(hint(
                if snapshot.repositories.is_empty() {
                    "No repositories in the workspace yet. Add them one at a time or from parent folders such as ~/dev/*; each gets its own team.".to_owned()
                } else if uses_all {
                    "Uses every workspace repository, including ones added later.".to_owned()
                } else {
                    format!(
                        "Uses {} of {} workspace repositories.",
                        used.len(),
                        snapshot.repositories.len()
                    )
                },
                p,
            ));
            for repository in used {
                repositories = repositories.child(Self::repository_row(repository, p));
            }
            body = body.child(repositories);
            let schedules: Vec<_> = snapshot
                .schedules
                .iter()
                .filter(|s| s.project_id == session.project_id)
                .collect();
            if !schedules.is_empty() {
                let mut list = div().flex().flex_col().child(section("SCHEDULES", p));
                for schedule in schedules {
                    list = list.child(self.schedule_row(schedule, p));
                }
                body = body.child(list);
            }
        } else {
            if let Some(ticket) = self.shared.ticket_of(&session.id) {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(eyebrow("TICKET", p))
                        .child(
                            card(p)
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(ticket.title.clone()),
                                )
                                .child(copyable("Branch", &ticket.branch, p))
                                .children(
                                    (self.shared.index.as_deref())
                                        .and_then(|index| parent_branch(index, ticket, p)),
                                )
                                .child(copyable("Worktree", &ticket.worktree, p)),
                        ),
                );
            }
            if let Some(repository) = self.shared.repository() {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(eyebrow("REPOSITORY", p))
                        .child(Self::repository_row(repository, p)),
                );
            }
        }
        let session_id = session.id.clone();
        let archived = session.archived;
        let noun = match session.role {
            Role::ProjectOrchestrator => "project",
            _ => "agent",
        };
        // A retired repository coordinator stays archived: its conversation is history only.
        if session.role == Role::TaskOrchestrator {
            return body.child(hint(
                "Retired when tickets moved to the project coordinator. Its conversation stays readable here; it cannot be restored.",
                p,
            ));
        }
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    button("archive-session")
                        .outline()
                        .icon(icon("archive"))
                        .label(if archived {
                            format!("Restore {noun}")
                        } else {
                            format!("Archive {noun}")
                        })
                        .on_click(cx.listener(move |v, _, w, c| {
                            v.set_archived(session_id.clone(), !archived, w, c)
                        })),
                )
                .child(
                    hint(
                        if archived {
                            "Brings it back with everything it owns."
                        } else {
                            "Hides it and everything it owns. Nothing is deleted."
                        },
                        p,
                    )
                    .flex_1()
                    .min_w_0(),
                ),
        );
        body.child(hint(
            "Agents run in YOLO mode with your signed-in CLIs. Ticket work stays in isolated worktrees. A project runs once you message it; Stop interrupts its active turns.",
            p,
        ))
    }

    fn attention_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let items = self.shared.project_attention();
        let notes = self.shared.workspace_notes();
        if items.is_empty() && notes.is_empty() {
            return empty_state(
                "check",
                "You’re all caught up",
                "Your coordinator brings decisions and approvals here when it needs you.",
                p,
            );
        }
        let answering = self.inbox.read(cx).answering.clone().or_else(|| {
            items
                .iter()
                .find(|item| !item.is_permission())
                .map(|item| item.id.clone())
        });
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .children(notes.iter().map(|note| self.note_card(note, p, cx)));
        for item in items {
            let open = answering.as_ref() == Some(&item.id);
            body = body.child(self.inbox.update(cx, |inbox, cx| {
                inbox.attention_card(item, open, None, p, cx)
            }));
        }
        body
    }

    fn note_card(&self, note: &WorkspaceNote, p: Palette, cx: &mut Context<Self>) -> Div {
        match note {
            WorkspaceNote::Paused(setting) => self
                .inbox
                .update(cx, |inbox, cx| inbox.paused_card(setting, p, cx)),
            WorkspaceNote::SkillQuestion(question) => self
                .skills
                .update(cx, |skills, cx| skills.question_card(question, p, cx)),
            WorkspaceNote::Held(held) => self.inbox.read(cx).held_card(held, p),
        }
    }

    fn git_panel(&self, p: Palette) -> Div {
        let mut body = div().flex().flex_col().gap_4();
        if let Some(repository) = self.shared.repository() {
            body = body.child(Self::repository_row(repository, p));
        }
        let PanelData::Git(commits) = &self.panel_data else {
            return body.child(hint("Loading local history…", p));
        };
        if commits.is_empty() {
            return body.child(empty_state(
                "git",
                "No commits yet",
                "This repository’s base has no history to show.",
                p,
            ));
        }
        let mut history = div()
            .flex()
            .flex_col()
            .child(section("LOCAL HISTORY · READ ONLY", p));
        for commit in commits {
            let sha: String = commit.sha.chars().take(7).collect();
            history = history.child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .py_2()
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::HAIRLINE))
                    .child(
                        div()
                            .flex_none()
                            .mt(px(5.))
                            .size(px(7.))
                            .rounded_full()
                            .border_1()
                            .border_color(p.focus),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(font::MESSAGE))
                            .child(commit.subject.clone()),
                    )
                    .child(mono(sha, p).flex_none().text_color(p.accent)),
            );
        }
        body.child(history)
    }
}

impl Requests for Inspector {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = reply {
            // A refused compaction shows in its form.
            if let Command::CompactContext { session_id, .. } = &command {
                return self.compact_refused(session_id, &error);
            }
            return toast_error(error, window, cx);
        }
        match command {
            Command::CompactContext { session_id, .. } => self.compact_accepted(&session_id),
            Command::AppendLog { .. } => {
                self.memory_title
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.memory_body
                    .update(cx, |input, cx| input.set_value("", window, cx));
                toast("Saved to project memory", window, cx);
                self.load_panel(window, cx);
            }
            _ => {}
        }
    }
}
impl Render for Inspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Inspector");
        let p = palette(is_dark(cx));
        match self.shared.panel {
            Some(panel) => self.panel_view(panel, p, cx),
            None => div(),
        }
    }
}

impl Workspace {
    pub(super) fn inspector_rail(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let role = self.shared.selected_role();
        let attention = self.shared.workspace_notes().len() + self.shared.project_attention().len();
        let mut rail = div()
            .w(px(64.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .py_2()
            .bg(p.surface)
            .border_l_1()
            .border_color(p.edge.opacity(edge::LIGHT));
        if self.shared.selected_session().is_none() {
            return rail;
        }
        for panel in [
            Panel::Overview,
            Panel::Team,
            Panel::Attention,
            Panel::Git,
            Panel::Files,
            Panel::Processes,
            Panel::Memory,
            Panel::Events,
        ]
        .into_iter()
        .filter(|panel| panel.available(role))
        {
            let selected = self.shared.panel == Some(panel);
            let badge = match panel {
                Panel::Attention => (attention > 0).then_some((attention, p.accent, p.on_accent)),
                Panel::Processes => self
                    .inspector
                    .read(cx)
                    .processes_badge()
                    .map(|count| (count, p.red, p.base)),
                _ => None,
            };
            let tooltip = format!("{} · {}", panel.title(), self.shared.owner_name());
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("inspector-{panel:?}")))
                    .relative()
                    .w(px(54.))
                    .h(px(50.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .rounded_lg()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(p.accent.opacity(0.10)))
                    .text_color(if selected { p.accent } else { p.subtle })
                    .hover(move |d| d.bg(p.overlay).text_color(p.text))
                    .tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c))
                    .on_click(cx.listener(move |v, _, w, c| v.toggle_panel(panel, w, c)))
                    .child(icon(panel.icon()).size(px(18.)))
                    .child(
                        div()
                            .text_size(px(font::TINY))
                            .when(selected, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .child(panel.title()),
                    )
                    .when_some(badge, |d, (count, fill, text)| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .right(px(8.))
                                .min_w(px(15.))
                                .h(px(15.))
                                .px(px(4.))
                                .rounded_full()
                                .bg(fill)
                                .text_color(text)
                                .text_size(px(font::MICRO))
                                .font_weight(FontWeight::SEMIBOLD)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(count.to_string()),
                        )
                    }),
            );
        }
        rail
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::Panel;
    use crate::test_support::git;
    use workspace_core::{Role, ask};
    use workspace_host::Host;

    #[test]
    fn the_agents_panel_stays_open_whichever_agent_is_selected() {
        for role in Role::ALL {
            assert!(Panel::Agents.kept_for(role) == Panel::Agents);
        }
        // A panel the selected session does not have still gives way to Overview.
        assert!(Panel::Attention.kept_for(Role::Implementer) == Panel::Overview);
        assert!(Panel::Git.kept_for(Role::ProjectOrchestrator) == Panel::Overview);
    }

    #[test]
    fn the_git_panel_reads_the_hosts_history_reply() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("web");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "Base"]);
        git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "Add the toolbar"],
        );
        let mut host = Host::open(directory.path().join("home")).unwrap();
        let repository = host.add_repository(repo.to_str().unwrap(), "HEAD").unwrap();

        let ask = ask::git_history(repository.id, 0, 30);
        let reply = host.execute(ask.command().clone()).unwrap();
        let history = ask.decode(reply).unwrap().commits;

        let subjects: Vec<&str> = history.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["Add the toolbar", "Base"]);
        assert!(history.iter().all(|c| c.sha.len() == 40));
    }
}
