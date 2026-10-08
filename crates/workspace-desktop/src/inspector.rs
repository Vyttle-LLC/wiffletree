//! The right-edge rail and the inspector panel it opens for the selected session.
use super::*;
use conversation::markdown;
use gpui_component::{
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use ui::{
    card, empty_state, eyebrow, from_now, hint, icon, mono, pill, property, section, session_icon,
};
use workspace_host::usage::local_time;

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
                .text_size(px(11.))
                .text_color(p.subtle)
                .child(label),
        )
        .child(mono(value.to_owned(), p).flex_1())
        .child(copy_button(
            SharedString::from(format!("copy-{label}-{value}")),
            value.to_owned(),
        ))
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
    fn icon(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Team => "team",
            Self::Memory => "memory",
            Self::Events => "activity",
            Self::Attention => "inbox",
            Self::Git => "git",
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
        }
    }
}

impl Workspace {
    fn selected_role(&self) -> Role {
        self.selected_session()
            .map_or(Role::ProjectOrchestrator, |s| s.role)
    }

    pub(super) fn inspector_rail(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let role = self.selected_role();
        let attention = self.project_attention().len();
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
            .border_color(p.edge.opacity(0.35));
        if self.selected_session().is_none() {
            return rail;
        }
        for panel in [
            Panel::Overview,
            Panel::Team,
            Panel::Attention,
            Panel::Git,
            Panel::Memory,
            Panel::Events,
        ]
        .into_iter()
        .filter(|panel| panel.available(role))
        {
            let selected = self.panel == Some(panel);
            let badge = (panel == Panel::Attention && attention > 0).then_some(attention);
            let tooltip = format!("{} · {}", panel.title(), self.owner_name());
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
                            .text_size(px(10.))
                            .when(selected, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .child(panel.title()),
                    )
                    .when_some(badge, |d, count| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .right(px(8.))
                                .min_w(px(15.))
                                .h(px(15.))
                                .px(px(4.))
                                .rounded_full()
                                .bg(p.accent)
                                .text_color(p.on_accent)
                                .text_size(px(9.5))
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

    pub(super) fn panel_view(&self, panel: Panel, p: Palette, cx: &mut Context<Self>) -> Div {
        let role = self.selected_role();
        let body = match panel {
            Panel::Overview => self.overview(p, cx),
            Panel::Team => self.team_panel(p, cx),
            Panel::Memory => self.memory_panel(p, cx),
            Panel::Events => self.events_panel(p),
            Panel::Attention => self.attention_panel(p, cx),
            Panel::Git => self.git_panel(p),
        };
        let (scope, subtitle) = (scope(role), self.owner_name());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(
                div()
                    .flex_none()
                    .h(px(64.))
                    .pl_5()
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(p.edge.opacity(0.3))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(p.subtle)
                                    .text_ellipsis()
                                    .child(format!("{scope}  ·  {subtitle}")),
                            )
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(panel.title()),
                            ),
                    )
                    .child(
                        button("close-inspector")
                            .ghost()
                            .icon(icon("close"))
                            .tooltip("Close")
                            .on_click(cx.listener(|v, _, _, c| {
                                v.panel = None;
                                c.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_5()
                    .text_size(px(13.))
                    .child(body.w_full()),
            )
    }

    fn stat(value: usize, label: &'static str, p: Palette) -> Div {
        card(p)
            .flex_1()
            .gap_0()
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(value.to_string()),
            )
            .child(hint(label, p))
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
            .border_color(p.edge.opacity(0.25))
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
            .border_color(p.edge.opacity(0.25))
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
        let (Some(session), Some(snapshot)) = (self.selected_session(), self.snapshot.as_ref())
        else {
            return div();
        };
        let live = self.live_enabled();
        let profile = self.selected_profile();
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
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_ellipsis()
                                    .child(session.name.clone()),
                            )
                            .child(hint(session.role.label(), p)),
                    )
                    .child(self.session_badge(session, p)),
            )
            .child(
                card(p)
                    .when_some(profile, |d, profile| {
                        d.child(property(
                            "Model",
                            format!(
                                "{:?} · {} · {}",
                                profile.provider, profile.model, profile.effort
                            ),
                            p,
                        ))
                    })
                    .when_some(
                        session.parent_id.as_deref().and_then(|id| self.session(id)),
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
            let tickets: Vec<_> = snapshot
                .tickets
                .iter()
                .filter(|t| t.coordinator_id == session.id)
                .collect();
            let open = tickets.iter().filter(|t| t.is_open()).count();
            let agents = self
                .shown(snapshot)
                .filter(in_project)
                .filter(|s| s.role.is_worker())
                .count();
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(Self::stat(tickets.len(), "Tickets", p))
                    .child(Self::stat(open, "Open", p))
                    .child(Self::stat(agents, "Agents", p)),
            );
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
                                    move |_, window, cx| {
                                        let _ = weak.update(cx, |view, cx| {
                                            view.open_creation(kind, None, window, cx)
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
            if let Some(ticket) = self.ticket_of(&session.id) {
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
                                .child(copyable("Worktree", &ticket.worktree, p)),
                        ),
                );
            }
            if let Some(repository) = self.repository() {
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
        let items = self.project_attention();
        if items.is_empty() {
            return empty_state(
                "check",
                "You’re all caught up",
                "Your coordinator brings decisions and approvals here when it needs you.",
                p,
            );
        }
        let answering = self.answering.clone().or_else(|| {
            items
                .iter()
                .find(|item| !item.is_permission())
                .map(|item| item.id.clone())
        });
        let mut body = div().flex().flex_col().gap_3();
        for item in items {
            let open = answering.as_ref() == Some(&item.id);
            body = body.child(self.attention_card(item, open, p, cx));
        }
        body
    }

    /// One inbox item with everything needed to settle it. `answering` opens the answer box.
    pub(super) fn attention_card(
        &self,
        item: &Attention,
        answering: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let asker = self
            .session(&item.session_id)
            .map_or("Agent".to_owned(), |s| s.name.clone());
        let id = item.id.clone();
        let entry = card(p).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(hint(asker, p).text_ellipsis())
                .child(if item.is_permission() {
                    pill("Approval", p.yellow)
                } else {
                    pill("Question", p.focus)
                }),
        );
        if item.is_permission() {
            let approve = id.clone();
            return entry
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(relative(1.5))
                        .child(item.prompt.clone()),
                )
                .child(hint(format!("{} · {}", item.host, item.operation_id), p))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button(SharedString::from(format!("deny-{id}")))
                                .ghost()
                                .label("Deny")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    v.resolve_attention(&id, "denied", w, c)
                                })),
                        )
                        .child(
                            button(SharedString::from(format!("approve-{approve}")))
                                .primary()
                                .label("Approve once")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    v.resolve_attention(&approve, "approved", w, c)
                                })),
                        ),
                );
        }
        let mut choices = div().flex().flex_col().gap_1p5();
        for (ix, option) in item.options.iter().enumerate() {
            let (id, option) = (id.clone(), option.clone());
            choices = choices.child(
                div()
                    .id(SharedString::from(format!("choice-{id}-{ix}")))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(p.edge.opacity(0.5))
                    .text_size(px(13.))
                    .line_height(relative(1.4))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.overlay).border_color(p.accent))
                    .child(option.clone())
                    .on_click(
                        cx.listener(move |v, _, w, c| v.resolve_attention(&id, &option, w, c)),
                    ),
            );
        }
        let dismiss = id.clone();
        let actions = div().flex().justify_end().gap_2().child(
            button(SharedString::from(format!("dismiss-{id}")))
                .ghost()
                .label("Dismiss")
                .on_click(cx.listener(move |v, _, w, c| {
                    v.request(
                        Command::DismissAttention {
                            id: dismiss.clone(),
                        },
                        w,
                        c,
                    )
                })),
        );
        let entry = entry
            .child(
                div()
                    .max_h(px(280.))
                    .overflow_y_scrollbar()
                    .id(SharedString::from(format!("question-{id}")))
                    .child(markdown(&format!("attention-{id}"), &item.prompt)),
            )
            .when(!item.options.is_empty(), |d| d.child(choices));
        if answering {
            entry.child(Textarea::new(&self.answer).w_full()).child(
                actions.child(
                    button(SharedString::from(format!("answer-{id}")))
                        .primary()
                        .label("Send answer")
                        .on_click(cx.listener(move |v, _, w, c| {
                            let answer = v.answer.read(c).value().trim().to_string();
                            if !answer.is_empty() {
                                v.resolve_attention(&id, &answer, w, c);
                            }
                        })),
                ),
            )
        } else {
            entry.child(
                actions.child(
                    button(SharedString::from(format!("start-answer-{id}")))
                        .label("Answer")
                        .on_click(cx.listener(move |v, _, w, c| v.start_answer(&id, w, c))),
                ),
            )
        }
    }

    /// Opens the answer box for one question, empty and focused.
    pub(super) fn start_answer(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.answering = Some(id.to_owned());
        self.answer.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn resolve_attention(
        &mut self,
        id: &str,
        answer: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request(
            Command::ResolveAttention {
                id: id.into(),
                answer: answer.into(),
            },
            window,
            cx,
        );
    }

    fn git_panel(&self, p: Palette) -> Div {
        let mut body = div().flex().flex_col().gap_4();
        if let Some(repository) = self.repository() {
            body = body.child(Self::repository_row(repository, p));
        }
        let Some(commits) = self.panel_data["commits"].as_array() else {
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
            let sha: String = commit["sha"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(7)
                .collect();
            history = history.child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .py_2()
                    .border_b_1()
                    .border_color(p.edge.opacity(0.25))
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
                            .text_size(px(12.5))
                            .child(commit["subject"].as_str().unwrap_or_default().to_owned()),
                    )
                    .child(mono(sha, p).flex_none().text_color(p.accent)),
            );
        }
        body.child(history)
    }
}
