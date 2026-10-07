//! Project tree, account quota and client appearance.
use super::*;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use ui::{
    brand_image, card, eyebrow, hint, humanize, icon, needs_you_badge, pill, session_icon,
    status_glyph, status_label, ticket_state_color,
};

const INDENT: f32 = 14.;

impl Appearance {
    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::System => "monitor",
            Self::Light => "sun",
            Self::Dark => "moon",
        }
    }
}

impl Workspace {
    pub(super) fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let dark = self.is_dark(cx);
        let roots = || {
            self.snapshot
                .iter()
                .flat_map(|s| &s.sessions)
                .filter(|s| s.parent_id.is_none())
        };
        let projects = roots().filter(|s| !s.archived).count();
        let archived = self
            .snapshot
            .iter()
            .flat_map(|s| &s.sessions)
            .filter(|s| s.archived)
            .count();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .pl_4()
                    .pr_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(brand_image(
                                if dark {
                                    "brand/symbol-dark.svg"
                                } else {
                                    "brand/symbol-light.svg"
                                },
                                28.,
                                14.,
                            ))
                            .child(brand_image(
                                if dark {
                                    "brand/wordmark-dark.svg"
                                } else {
                                    "brand/wordmark-light.svg"
                                },
                                112.,
                                21.,
                            ))
                            .when(super::data_dir::is_beta(), |d| {
                                d.child(pill("BETA", p.accent))
                            }),
                    )
                    .child(
                        button("new-project")
                            .ghost()
                            .icon(icon("plus"))
                            .tooltip("New project · ⌘N")
                            .disabled(self.creating_project)
                            .on_click(cx.listener(|v, _, w, c| v.new_project(w, c))),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_1()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(eyebrow(format!("PROJECTS · {projects}"), p))
                    .when(archived > 0, |d| {
                        d.child(
                            button("show-archived")
                                .ghost()
                                .xsmall()
                                .icon(icon("archive").size(px(13.)))
                                .label(if self.show_archived {
                                    "Hide archived"
                                } else {
                                    "Archived"
                                })
                                .tooltip(format!("{archived} archived sessions"))
                                .on_click(cx.listener(|v, _, _, c| {
                                    v.show_archived = !v.show_archived;
                                    c.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("tree-scroll")
                            .track_scroll(&self.tree_scroll)
                            .size_full()
                            .overflow_y_scroll()
                            .px_2()
                            .pb_3()
                            .child(self.tree(p, cx)),
                    )
                    .vertical_scrollbar(&self.tree_scroll),
            )
            .child(self.workspace_nav(p, cx))
            .child(self.quota_strip(p, cx))
            .child(
                self.sidebar_footer(p, cx)
                    .relative()
                    .children(self.update_card(p, cx)),
            )
    }

    /// Pages about the whole workspace, kept apart from the per-session tools on the right.
    fn workspace_nav(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut nav = div()
            .flex_none()
            .px_2()
            .py_2()
            .flex()
            .flex_col()
            .gap(px(1.))
            .border_t_1()
            .border_color(p.edge.opacity(0.35))
            .child(div().px_2().pb_1().child(eyebrow("WORKSPACE", p)));
        for (page, label, glyph) in [
            (Page::Repositories, "Repositories", "folder"),
            (Page::Usage, "Usage and limits", "activity"),
            (Page::Models, "Models", "settings"),
        ] {
            let selected = self.view == page;
            nav = nav.child(
                self.row(SharedString::from(label), 0, selected, p)
                    .on_click(cx.listener(move |v, _, w, c| v.open_page(page, w, c)))
                    .child(
                        div()
                            .flex_none()
                            .text_color(if selected { p.text } else { p.subtle })
                            .child(icon(glyph).size(px(14.))),
                    )
                    .child(label),
            );
        }
        nav
    }

    fn sidebar_footer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .pl_4()
            .pr_3()
            .py_2()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(p.edge.opacity(0.35))
            .child(
                match self.update.as_ref().filter(|_| self.update_card_dismissed) {
                    Some(staged) => button("restart-to-update")
                        .primary()
                        .xsmall()
                        .icon(icon("refresh").size(px(13.)))
                        .label("Restart to update")
                        .tooltip(format!("Wiffletree {} is ready", staged.version))
                        .on_click(cx.listener(|v, _, w, c| v.restart_to_update(w, c)))
                        .into_any_element(),
                    None => div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(11.))
                        .text_color(p.subtle)
                        .child(div().size(px(6.)).rounded_full().bg(p.green))
                        .child("Local workspace")
                        .into_any_element(),
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("version")
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(p.subtle)
                            .hover(|d| d.text_color(p.text))
                            .child(if self.checking_update {
                                "Checking…".to_owned()
                            } else {
                                version_label()
                            })
                            .tooltip(|w, c| Tooltip::new("Check for updates").build(w, c))
                            .on_click(cx.listener(|v, _, w, c| v.check_for_updates(true, w, c))),
                    )
                    .child(self.appearance_menu(cx)),
            )
    }

    /// What the downloaded release brings, with its changelog and the restart that installs it.
    /// It floats above the footer rather than taking room from the sidebar above it.
    fn update_card(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let staged = self
            .update
            .as_ref()
            .filter(|_| !self.update_card_dismissed)?;
        let page = staged.page.clone();
        Some(
            div()
                .absolute()
                .bottom(relative(1.))
                .left_0()
                .right_0()
                .px_3()
                .pb_2()
                .child(
                    card(p)
                        .bg(p.surface)
                        .shadow_lg()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("Wiffletree {} is ready", staged.version)),
                                )
                                .child(
                                    button("dismiss-update")
                                        .ghost()
                                        .xsmall()
                                        .icon(icon("close").size(px(12.)))
                                        .tooltip("Later")
                                        .on_click(cx.listener(|v, _, _, c| {
                                            v.update_card_dismissed = true;
                                            c.notify();
                                        })),
                                ),
                        )
                        .when_some(staged.summary.clone(), |d, summary| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(relative(1.45))
                                    .text_color(p.subtle)
                                    .line_clamp(4)
                                    .child(summary),
                            )
                        })
                        // A row keeps the link's hit area to its text rather than the card's width.
                        .child(
                            div().flex().child(
                                div()
                                    .id("changelog")
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.accent)
                                    .hover(|d| d.underline())
                                    .child("Changelog →")
                                    .on_click(move |_, _, cx| cx.open_url(&page)),
                            ),
                        )
                        .child(
                            div().flex().child(
                                button("install-update")
                                    .primary()
                                    .small()
                                    .flex_1()
                                    .icon(icon("refresh").size(px(13.)))
                                    .label("Restart to update")
                                    .on_click(cx.listener(|v, _, w, c| v.restart_to_update(w, c))),
                            ),
                        ),
                ),
        )
    }

    fn appearance_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.appearance;
        let weak = cx.weak_entity();
        button("appearance")
            .ghost()
            .icon(icon(current.icon()))
            .tooltip("Appearance")
            .dropdown_menu(move |mut menu, _, _| {
                for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(appearance.label())
                            .icon(icon(appearance.icon()))
                            .checked(appearance == current)
                            .on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |view, cx| {
                                    view.set_appearance(appearance, window, cx)
                                });
                            }),
                    );
                }
                menu
            })
    }

    fn tree(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut tree = div().flex().flex_col().gap(px(1.));
        let Some(snapshot) = &self.snapshot else {
            return tree;
        };
        if snapshot.projects.is_empty() {
            return tree.child(div().px_2().py_3().child(hint(
                "No projects yet. Create one to brief a coordinator.",
                p,
            )));
        }
        for project in snapshot.projects.iter().rev() {
            let Some(root) = self
                .shown(snapshot)
                .find(|s| s.project_id == project.id && s.parent_id.is_none())
            else {
                continue;
            };
            tree = tree.child(self.project_row(root, p, cx));
            if self.collapsed_projects.contains(&project.id) {
                continue;
            }
            let teams: Vec<_> = self
                .shown(snapshot)
                .filter(|s| s.parent_id.as_ref() == Some(&root.id))
                .collect();
            for team in &teams {
                tree = tree.child(self.session_row(team, 1, team.name.clone(), p, cx));
                for ticket in snapshot
                    .tickets
                    .iter()
                    .filter(|t| t.coordinator_id == team.id)
                {
                    let on_ticket = |s: &Session| {
                        self.runtime(&s.id)
                            .is_some_and(|r| r.ticket_id.as_ref() == Some(&ticket.id))
                    };
                    let workers: Vec<_> = self.shown(snapshot).filter(|s| on_ticket(s)).collect();
                    // A ticket whose agents are all archived, such as a closed one, goes with them.
                    if workers.is_empty() && snapshot.sessions.iter().any(on_ticket) {
                        continue;
                    }
                    tree = tree.child(self.ticket_row(ticket, p, cx));
                    for worker in workers {
                        // The ticket row above already names the work.
                        let focus = self.runtime(&worker.id).and_then(|r| r.focus.as_deref());
                        let label = worker.role.agent_label(focus);
                        tree = tree.child(self.session_row(worker, 3, label, p, cx));
                    }
                }
                // Older stores can contain workers created before ticket ownership existed.
                for worker in self.shown(snapshot).filter(|s| {
                    s.parent_id.as_ref() == Some(&team.id)
                        && self.runtime(&s.id).is_none_or(|r| r.ticket_id.is_none())
                }) {
                    tree = tree.child(self.session_row(worker, 2, worker.name.clone(), p, cx));
                }
            }
            if teams.is_empty() && self.selected.as_ref() == Some(&root.id) {
                tree = tree.child(
                    button(SharedString::from(format!("add-team-{}", root.id)))
                        .ghost()
                        .icon(icon("plus"))
                        .label("Add repository team")
                        .ml(px(INDENT + 8.))
                        .mr_auto()
                        .on_click(cx.listener(|v, _, w, c| {
                            v.open_creation(Creation::Coordinator, None, w, c)
                        })),
                );
            }
            tree = tree.child(div().h_2());
        }
        tree
    }

    pub(super) fn shown_status(&self, session: &Session) -> Status {
        let sessions = self.snapshot.as_ref().map_or(&[][..], |s| &s.sessions);
        shown_status(session, sessions)
    }

    fn status_detail(&self, session: &Session) -> String {
        let label = status_label(session.status);
        if self.shown_status(session) == session.status {
            label.to_owned()
        } else {
            format!("{label} · Team working")
        }
    }

    fn row(&self, id: SharedString, depth: usize, selected: bool, p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .min_h(px(30.))
            .flex()
            .items_center()
            .gap_2()
            .pl(px(8. + depth as f32 * INDENT))
            .pr_2()
            .rounded_md()
            .text_size(px(12.5))
            .when(selected, |d| d.bg(p.overlay))
            .text_color(if depth >= 2 && !selected {
                p.subtle
            } else {
                p.text
            })
            .cursor_pointer()
            .hover(move |d| d.bg(p.overlay.opacity(if selected { 1. } else { 0.6 })))
    }

    fn project_row(&self, root: &Session, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = root.id.clone();
        let project_id = root.project_id.clone();
        let selected = self.view == Page::Conversation && self.selected.as_ref() == Some(&id);
        let collapsed = self.collapsed_projects.contains(&project_id);
        let edit = self
            .project_edit
            .as_ref()
            .filter(|e| e.project_id == project_id);
        let editing = edit.is_some();
        let live = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.live_projects.contains(&project_id));
        let attention = self
            .snapshot
            .iter()
            .flat_map(|s| &s.attention)
            .filter(|a| a.project_id == project_id)
            .count();
        let detail = format!(
            "{} · {}{}{}",
            root.name,
            self.status_detail(root),
            if live { " · Running" } else { " · Stopped" },
            match attention {
                0 => String::new(),
                1 => " · 1 item needs you".to_owned(),
                n => format!(" · {n} items need you"),
            }
        );
        let archived = root.archived;
        let archive_id = id.clone();
        let toggle_id = project_id.clone();
        let rename_id = project_id.clone();
        let click_project = project_id.clone();
        self.row(SharedString::from(format!("tree-{id}")), 0, selected, p)
            .pl_1()
            .text_size(px(13.))
            .when(root.archived, |d| d.opacity(0.55))
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .on_click(cx.listener(move |v, event: &ClickEvent, w, c| {
                if !editing {
                    v.select(id.clone(), w, c);
                    if event.click_count() == 2 {
                        v.begin_project_rename(click_project.clone(), w, c);
                    }
                }
            }))
            .child(
                div()
                    .id(SharedString::from(format!("collapse-{project_id}")))
                    .flex_none()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_color(p.subtle)
                    .hover(move |d| d.bg(p.edge.opacity(0.5)))
                    .on_click(cx.listener(move |v, _, _, c| {
                        c.stop_propagation();
                        if !v.collapsed_projects.remove(&toggle_id) {
                            v.collapsed_projects.insert(toggle_id.clone());
                        }
                        c.notify();
                    }))
                    .child(
                        icon(if collapsed {
                            "chevron-right"
                        } else {
                            "chevron-down"
                        })
                        .size(px(12.)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(if selected { p.text } else { p.subtle })
                    .child(icon("project")),
            )
            .child(div().flex_1().min_w_0().child(if let Some(edit) = edit {
                div()
                    .w_full()
                    .py_1()
                    .on_mouse_down_out(cx.listener(|v, _, w, c| {
                        v.save_project_name(w, c);
                    }))
                    .on_action(cx.listener(|v, _: &gpui_component::input::Escape, w, c| {
                        v.cancel_project_name(w, c);
                        c.stop_propagation();
                    }))
                    .child(
                        Input::new(&self.project_name)
                            .small()
                            .w_full()
                            .disabled(edit.saving),
                    )
                    .when(!edit.error.is_empty(), |d| {
                        d.child(hint(edit.error.clone(), p).text_color(p.red))
                    })
                    .into_any_element()
            } else {
                div()
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(root.name.clone())
                    .into_any_element()
            }))
            .when(selected && !editing, |d| {
                let weak = cx.weak_entity();
                d.child(
                    button(SharedString::from(format!("project-actions-{project_id}")))
                        .ghost()
                        .xsmall()
                        .icon(icon("ellipsis").size(px(14.)))
                        .tooltip("Project actions")
                        .dropdown_menu(move |menu, _, _| {
                            let rename = weak.clone();
                            let rename_id = rename_id.clone();
                            let add_repositories = weak.clone();
                            let add_team = weak.clone();
                            let archive = weak.clone();
                            let archive_id = archive_id.clone();
                            menu.item(PopupMenuItem::new("Rename").icon(icon("rename")).on_click(
                                move |_, window, cx| {
                                    let _ = rename.update(cx, |view, cx| {
                                        view.begin_project_rename(rename_id.clone(), window, cx)
                                    });
                                },
                            ))
                            .item(
                                PopupMenuItem::new("Add repositories…")
                                    .icon(icon("folder"))
                                    .on_click(move |_, window, cx| {
                                        let _ = add_repositories.update(cx, |view, cx| {
                                            view.open_creation(Creation::Import, None, window, cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Add repository team")
                                    .icon(icon("plus"))
                                    .on_click(move |_, window, cx| {
                                        let _ = add_team.update(cx, |view, cx| {
                                            view.open_creation(
                                                Creation::Coordinator,
                                                None,
                                                window,
                                                cx,
                                            )
                                        });
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new(if archived {
                                    "Restore project"
                                } else {
                                    "Archive project"
                                })
                                .icon(icon("archive"))
                                .on_click(move |_, window, cx| {
                                    let _ = archive.update(cx, |view, cx| {
                                        view.set_archived(archive_id.clone(), !archived, window, cx)
                                    });
                                }),
                            )
                        }),
                )
            })
            // Needs you outranks every other state, so it takes the status slot.
            .child(if attention > 0 {
                needs_you_badge(attention, p)
            } else {
                div()
                    .flex_none()
                    .child(status_glyph(self.shown_status(root), 12., p))
            })
    }

    fn session_row(
        &self,
        session: &Session,
        depth: usize,
        label: String,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = session.id.clone();
        let selected = self.view == Page::Conversation && self.selected.as_ref() == Some(&id);
        let status = self.shown_status(session);
        let detail = format!(
            "{} · {} · {:?} · {}",
            session.name,
            session.role.label(),
            session.provider,
            self.status_detail(session)
        );
        self.row(SharedString::from(format!("tree-{id}")), depth, selected, p)
            .when(session.archived, |d| d.opacity(0.55))
            // Working agents keep full-strength text; idle ones recede.
            .when(status == Status::Working, |d| d.text_color(p.text))
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .on_click(cx.listener(move |v, _, w, c| v.select(id.clone(), w, c)))
            .child(
                div()
                    .flex_none()
                    .text_color(if selected { p.text } else { p.subtle })
                    .child(icon(session_icon(session.role)).size(px(14.))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .when(depth == 1, |d| d.font_weight(FontWeight::MEDIUM))
                    .child(label),
            )
            .child(div().flex_none().child(status_glyph(status, 12., p)))
    }

    fn ticket_row(&self, ticket: &Ticket, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let coordinator = ticket.coordinator_id.clone();
        let state = humanize(&ticket.state);
        let detail = format!("{} · {state}", ticket.title);
        self.row(
            SharedString::from(format!("ticket-{}", ticket.id)),
            2,
            false,
            p,
        )
        .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
        .on_click(cx.listener(move |v, _, w, c| {
            v.select(coordinator.clone(), w, c);
            v.show_panel(Panel::Team, w, c);
        }))
        .child(div().flex_none().child(icon("task").size(px(14.))))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .text_color(p.text)
                .child(ticket.title.clone()),
        )
        .child(
            div()
                .flex_none()
                .size(px(6.))
                .mr(px(3.))
                .rounded_full()
                .bg(ticket_state_color(&ticket.state, p)),
        )
    }
}

/// An idle coordinator shows its team's work, so a collapsed or quiet parent never hides it.
fn shown_status(session: &Session, sessions: &[Session]) -> Status {
    let idle = matches!(session.status, Status::Ready | Status::Done);
    if idle && team_working(&session.id, sessions) {
        Status::Working
    } else {
        session.status
    }
}

fn team_working(coordinator_id: &str, sessions: &[Session]) -> bool {
    let parent_of = |id: &str| {
        sessions
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.parent_id.as_deref())
    };
    let reports_to = |session: &Session| {
        std::iter::successors(session.parent_id.as_deref(), |id| parent_of(id))
            .any(|id| id == coordinator_id)
    };
    sessions
        .iter()
        .any(|s| !s.archived && s.status == Status::Working && reports_to(s))
}

#[cfg(test)]
mod tests {
    use super::shown_status;
    use workspace_core::{Provider, Role, Session, Status};

    fn session(id: &str, parent: Option<&str>, status: Status) -> Session {
        Session {
            id: id.into(),
            project_id: "project".into(),
            parent_id: parent.map(Into::into),
            repository_id: None,
            name: id.into(),
            role: Role::Implementer,
            provider: Provider::Claude,
            status,
            archived: false,
        }
    }

    #[test]
    fn idle_coordinators_show_work_anywhere_below_them() {
        let sessions = [
            session("main", None, Status::Ready),
            session("team", Some("main"), Status::Done),
            session("implementer", Some("team"), Status::Working),
            session("other-team", Some("main"), Status::Ready),
        ];
        assert_eq!(shown_status(&sessions[0], &sessions), Status::Working);
        assert_eq!(shown_status(&sessions[1], &sessions), Status::Working);
        assert_eq!(shown_status(&sessions[3], &sessions), Status::Ready);
    }

    #[test]
    fn a_coordinators_own_attention_state_is_not_masked() {
        let mut sessions = [
            session("team", None, Status::Blocked),
            session("implementer", Some("team"), Status::Working),
        ];
        assert_eq!(shown_status(&sessions[0], &sessions), Status::Blocked);

        sessions[0].status = Status::Ready;
        sessions[1].archived = true;
        assert_eq!(shown_status(&sessions[0], &sessions), Status::Ready);
    }
}
