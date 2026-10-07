//! The selected session's conversation: header, transcript, notices and composer.
use super::*;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    text::TextView,
};
use ui::{
    age, banner, brand_image, effort_label, hint, humanize, icon, pill, session_icon, status_badge,
};

/// Wide enough for the composer, narrow enough that message lines stay readable.
const COLUMN: f32 = 720.;

/// Kinds an agent can pass to the host's `report` tool.
const REPORT_KINDS: [&str; 6] = [
    "progress",
    "blocked",
    "ready_for_testing",
    "passed",
    "failed",
    "completed",
];

/// How a refreshed transcript page lines up with the one on screen: how many leading messages
/// fell off the page and how many after them are unchanged. `None` when the pages don't overlap.
pub(super) fn transcript_overlap(
    shown: &[Message],
    refreshed: &[Message],
) -> Option<(usize, usize)> {
    let first = refreshed.first()?;
    let dropped = shown.iter().position(|m| m.id == first.id)?;
    let unchanged = shown[dropped..]
        .iter()
        .zip(refreshed)
        .take_while(|(old, new)| old == new)
        .count();
    Some((dropped, unchanged))
}

/// Agent reports arrive as `[kind] sender name\nbody`; returns the kind and body.
fn report(body: &str) -> Option<(&str, &str)> {
    let (kind, rest) = body.strip_prefix('[')?.split_once("] ")?;
    REPORT_KINDS
        .contains(&kind)
        .then(|| (kind, rest.split_once('\n').map_or("", |(_, text)| text)))
}

fn first_line(text: &str, limit: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > limit {
        format!("{}…", line.chars().take(limit).collect::<String>())
    } else {
        line.to_owned()
    }
}

pub(super) fn markdown(id: &str, body: &str) -> TextView {
    TextView::markdown(
        SharedString::from(format!("message-markdown-{id}")),
        body.to_owned(),
    )
    .selectable(true)
    .code_block_actions(|block, _, _| {
        let code = block.code();
        button("copy-code")
            .ghost()
            .icon(icon("copy"))
            .label("Copy")
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()))
            })
    })
    .text_size(px(14.))
    .line_height(relative(1.5))
}

fn avatar(icon_name: &str, color: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(26.))
        .rounded_lg()
        .bg(color.opacity(0.12))
        .text_color(color)
        .flex()
        .items_center()
        .justify_center()
        .child(icon(icon_name).size(px(15.)))
}

fn meta(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .text_size(px(10.5))
        .text_color(p.subtle)
        .child(text.into())
}

fn receipt_color(receipt: Receipt, p: Palette) -> Hsla {
    match receipt {
        Receipt::Held => p.yellow,
        Receipt::Completed | Receipt::Acknowledged => p.green,
        _ => p.subtle,
    }
}

/// The human's own message: a right-aligned bubble with its delivery state.
fn human_message(message: &Message, p: Palette) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_end()
        .gap_1()
        .child(
            div()
                .max_w(px(620.))
                .px_4()
                .py_2()
                .rounded_xl()
                .bg(p.overlay)
                .child(markdown(&message.id, &message.body)),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    meta(format!("{:?}", message.receipt), p)
                        .text_color(receipt_color(message.receipt, p)),
                )
                .child(meta(age(message.created_at), p)),
        )
}

/// A message written by an agent: this session's own reply, or another agent's report.
fn agent_message(message: &Message, sender: Option<&Session>, own: bool, p: Palette) -> Div {
    let (kind, body) = match report(&message.body) {
        Some((kind, body)) if !own => (Some(kind), body),
        _ => (None, message.body.as_str()),
    };
    let role = sender.map(|s| s.role);
    div()
        .w_full()
        .flex()
        .gap_3()
        .child(avatar(
            role.map_or("agent", session_icon),
            if own { p.accent } else { p.focus },
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .min_h(px(26.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .text_ellipsis()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(sender.map_or("Agent".to_owned(), |s| s.name.clone())),
                        )
                        .when_some(role.filter(|_| !own), |d, role| {
                            d.child(meta(role.label(), p))
                        })
                        .when_some(kind, |d, kind| d.child(pill(humanize(kind), p.focus)))
                        .child(div().flex_1())
                        .when(!own, |d| {
                            d.child(
                                meta(format!("{:?}", message.receipt), p)
                                    .text_color(receipt_color(message.receipt, p)),
                            )
                        })
                        .child(meta(age(message.created_at), p)),
                )
                .when(!body.trim().is_empty(), |d| {
                    d.child(markdown(&message.id, body))
                }),
        )
}

impl Workspace {
    fn waiting_messages(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.sender.is_none() && m.receipt == Receipt::Queued)
            .count()
    }

    /// Stop is offered only while some agent in the project is mid-turn.
    fn project_working(&self, session: &Session) -> bool {
        self.snapshot
            .iter()
            .flat_map(|s| &s.sessions)
            .any(|s| s.project_id == session.project_id && s.status == Status::Working)
    }

    fn breadcrumb(&self, session: &Session) -> String {
        let mut parts = vec![self.project_name()];
        if session.role.is_worker() {
            if let Some(team) = session.parent_id.as_deref().and_then(|id| self.session(id)) {
                parts.push(team.name.clone());
            }
            if let Some(ticket) = self.ticket_of(&session.id) {
                parts.push(ticket.title.clone());
            }
        }
        parts.push(session.role.label().into());
        parts.join("  ›  ")
    }

    fn conversation_header(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let live = self.live_enabled();
        let paused = session.status == Status::Paused;
        let title = if session.role.is_worker() {
            self.ticket_of(&session.id)
                .map_or(session.name.clone(), |t| t.title.clone())
        } else {
            session.name.clone()
        };
        div()
            .flex_none()
            .h(px(64.))
            .px_6()
            .flex()
            .items_center()
            .gap_3()
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
                            .child(self.breadcrumb(session)),
                    )
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_ellipsis()
                            .child(title),
                    ),
            )
            .children(self.context_chip(session, p, cx))
            .child(status_badge(session.status, p))
            .when(!session.archived, |d| {
                d.child(
                    button("pause-session")
                        .ghost()
                        .icon(icon(if paused { "play" } else { "pause" }))
                        .tooltip(if paused {
                            "Resume this session"
                        } else {
                            "Pause this session · interrupts an active turn"
                        })
                        .on_click(cx.listener(|v, _, w, c| v.toggle_pause(w, c))),
                )
            })
            .when(session.role == Role::TaskOrchestrator, |d| {
                d.child(
                    button("new-ticket")
                        .ghost()
                        .icon(icon("plus"))
                        .label("New ticket")
                        .on_click(
                            cx.listener(|v, _, w, c| v.open_creation(Creation::Ticket, None, w, c)),
                        ),
                )
            })
            // A project runs once you message it, so the only control it needs is Stop.
            .children(if session.archived {
                Some(
                    button("restore-session")
                        .outline()
                        .icon(icon("archive"))
                        .label("Restore")
                        .on_click(cx.listener(|v, _, w, c| {
                            if let Some(id) = v.selected.clone() {
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
                        .on_click(cx.listener(|v, _, w, c| v.set_live(false, w, c))),
                )
            } else {
                None
            })
    }

    /// Conditions that explain why nothing is happening, most urgent first.
    fn notices(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Vec<Div> {
        if session.archived {
            return vec![
                banner(p.subtle)
                    .child(icon("archive").text_color(p.subtle))
                    .child(div().flex_1().child(
                        "Archived. Its conversation and work are kept, and it receives nothing until restored.",
                    )),
            ];
        }
        let mut notices = vec![];
        if let Some(error) = self
            .runtime(&session.id)
            .and_then(|r| r.last_error.as_deref())
        {
            // Pausing is the user's own action, so it does not read as a failure.
            let tone = if session.status == Status::Paused {
                p.yellow
            } else {
                p.red
            };
            notices.push(
                banner(tone)
                    .items_start()
                    .child(icon("triangle-alert").text_color(tone).mt(px(2.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(first_line(error, 180))
                            .child(hint(
                                "The last input is held. Inspect the worktree before retrying: interrupted tools may already have run.",
                                p,
                            )),
                    )
                    .child(
                        button("skip-held")
                            .ghost()
                            .label("Skip")
                            .tooltip("Skip the held input")
                            .on_click(cx.listener(|v, _, w, c| v.reconcile(false, w, c))),
                    )
                    .child(
                        button("retry-held")
                            .label("Retry")
                            .tooltip("Retry the held input")
                            .on_click(cx.listener(|v, _, w, c| v.reconcile(true, w, c))),
                    ),
            );
        }
        let waiting = self.waiting_messages();
        if !self.live_enabled() && waiting > 0 {
            notices.push(
                banner(p.focus)
                    .child(icon("pause").text_color(p.focus))
                    .child(div().flex_1().child(format!(
                        "This project is stopped. {waiting} {} waiting.",
                        if waiting == 1 {
                            "message is"
                        } else {
                            "messages are"
                        }
                    )))
                    .child(
                        button("notice-resume")
                            .primary()
                            .label("Resume")
                            .on_click(cx.listener(|v, _, w, c| v.set_live(true, w, c))),
                    ),
            );
        }
        notices
    }

    /// The project's open decisions, one at a time, so the human can settle them in place.
    /// Hidden while the inbox is open, which already lists them all.
    fn decisions(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        if session.role != Role::ProjectOrchestrator || self.panel == Some(Panel::Attention) {
            return None;
        }
        let items = self.project_attention();
        let position = self
            .answering
            .as_ref()
            .and_then(|id| items.iter().position(|item| &item.id == id))
            .unwrap_or(0);
        let item = items.get(position)?;
        let step = |offset: usize| {
            let id = items[(position + offset) % items.len()].id.clone();
            cx.listener(move |v, _, w, c| v.start_answer(&id, w, c))
        };
        let navigation = div()
            .flex()
            .items_center()
            .gap_1()
            .child(hint(
                if items.len() == 1 {
                    "Your coordinator is waiting on a decision".to_owned()
                } else {
                    format!(
                        "Your coordinator is waiting on {} decisions · {} of {}",
                        items.len(),
                        position + 1,
                        items.len()
                    )
                },
                p,
            ))
            .child(div().flex_1())
            .when(items.len() > 1, |d| {
                d.child(
                    button("previous-decision")
                        .ghost()
                        .small()
                        .icon(icon("chevron-left"))
                        .tooltip("Previous decision")
                        .on_click(step(items.len() - 1)),
                )
                .child(
                    button("next-decision")
                        .ghost()
                        .small()
                        .icon(icon("chevron-right"))
                        .tooltip("Next decision")
                        .on_click(step(1)),
                )
            })
            .child(
                button("open-inbox")
                    .ghost()
                    .small()
                    .label("Open inbox")
                    .on_click(cx.listener(|v, _, w, c| v.show_panel(Panel::Attention, w, c))),
            );
        Some(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .flex()
                .flex_col()
                .gap_1()
                .child(navigation)
                .child(
                    self.attention_card(item, true, p, cx)
                        .border_color(p.yellow.opacity(0.45)),
                ),
        )
    }

    fn empty_conversation(&self, role: Role, p: Palette, cx: &mut Context<Self>) -> Div {
        let (title, description, prompts): (_, _, &[(&str, &str)]) = match role {
            Role::ProjectOrchestrator => (
                "What are we building?",
                "Give your coordinator a goal. Shape the work together, then let the team take it forward.",
                &[
                    (
                        "Shape a plan",
                        "Help me turn this goal into a clear implementation plan: ",
                    ),
                    (
                        "Break down the work",
                        "Break this work into tickets with clear ownership and acceptance criteria: ",
                    ),
                    (
                        "Review the approach",
                        "Review this approach and identify the most important risks: ",
                    ),
                ],
            ),
            Role::TaskOrchestrator => (
                "One repository. A focused team.",
                "This coordinator plans the tickets for its repository and hands them to implementers and testers.",
                &[
                    (
                        "Propose tickets",
                        "Propose tickets with acceptance criteria for: ",
                    ),
                    (
                        "Check status",
                        "Summarize every ticket’s state and anything blocking it.",
                    ),
                ],
            ),
            _ => (
                "A clear brief starts here.",
                "This agent works on one ticket. Message it directly to steer or question its work.",
                &[
                    (
                        "Check status",
                        "Summarize what you have done and what remains.",
                    ),
                    ("Redirect", "Change of direction: "),
                ],
            ),
        };
        let mut suggestions = div().flex().flex_wrap().gap_2().mt_3();
        for (name, text) in prompts {
            suggestions = suggestions.child(button(*name).outline().label(*name).on_click(
                cx.listener(move |v, _, w, c| {
                    v.input.update(c, |input, c| {
                        input.set_value(*text, w, c);
                        input.focus(w, c);
                    })
                }),
            ));
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .px_8()
            .child(
                div()
                    .w_full()
                    .max_w(px(480.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .child(if role == Role::ProjectOrchestrator {
                        brand_image(
                            if self.is_dark(cx) {
                                "brand/symbol-dark.svg"
                            } else {
                                "brand/symbol-light.svg"
                            },
                            72.,
                            36.,
                        )
                        .into_any_element()
                    } else {
                        div()
                            .size(px(44.))
                            .flex_none()
                            .rounded_xl()
                            .bg(p.accent.opacity(0.10))
                            .text_color(p.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(session_icon(role)).size(px(22.)))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .mt_3()
                            .text_size(px(26.))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(relative(1.2))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(13.5))
                            .line_height(relative(1.6))
                            .text_color(p.subtle)
                            .child(description),
                    )
                    .child(suggestions),
            )
    }

    fn transcript(&self, session: &Session, p: Palette) -> List {
        let messages = self.messages.clone();
        let sessions = self
            .snapshot
            .as_ref()
            .map(|s| s.sessions.clone())
            .unwrap_or_default();
        let current = session.clone();
        list(self.list.clone(), move |ix, _, _| {
            let content = match messages.get(ix) {
                Some(message) if message.sender.is_none() => human_message(message, p),
                Some(message) => {
                    let sender = sessions
                        .iter()
                        .find(|s| Some(&s.id) == message.sender.as_ref());
                    let own = message.sender.as_ref() == Some(&current.id);
                    agent_message(message, sender, own, p)
                }
                // The row after the last message appears only while the agent works.
                None => div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(avatar(session_icon(current.role), p.accent))
                    .child(Spinner::new().small().color(p.subtle))
                    .child(hint(format!("{} is working…", current.name), p)),
            };
            div()
                .w_full()
                .px_6()
                .py_3()
                .flex()
                .justify_center()
                .child(div().w_full().max_w(px(COLUMN)).child(content))
                .into_any_element()
        })
        .flex_1()
        .min_h_0()
    }

    fn pagination(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let older = self.messages.len() == MAX_PAGE;
        let viewing_history = self.page.is_some();
        (older || viewing_history).then(|| {
            div()
                .flex_none()
                .py_1()
                .flex()
                .justify_center()
                .gap_2()
                .border_b_1()
                .border_color(p.edge.opacity(0.3))
                .when(older, |d| {
                    d.child(
                        button("older")
                            .ghost()
                            .icon(icon("chevron-up"))
                            .label("Earlier messages")
                            .on_click(cx.listener(|v, _, w, c| {
                                v.page = v.messages.first().map(|m| m.sequence);
                                v.load_messages(w, c);
                            })),
                    )
                })
                .when(viewing_history, |d| {
                    d.child(
                        button("latest")
                            .ghost()
                            .icon(icon("chevron-down"))
                            .label("Back to latest")
                            .on_click(cx.listener(|v, _, w, c| {
                                v.page = None;
                                v.load_messages(w, c);
                            })),
                    )
                })
        })
    }

    fn model_controls(&self, session: &Session, cx: &mut Context<Self>) -> Div {
        let Some(profile) = self.selected_profile() else {
            return div();
        };
        let started = self
            .runtime(&session.id)
            .is_some_and(|runtime| runtime.provider_session_id.is_some());
        let busy = session.status == Status::Working;
        let mut models: BTreeMap<(Provider, String), String> = self
            .model_catalog
            .iter()
            .map(|model| ((model.provider, model.model.clone()), model.label.clone()))
            .collect();
        for approved in self
            .snapshot
            .iter()
            .flat_map(|s| &s.policies)
            .flat_map(|policy| &policy.allowed)
        {
            models
                .entry((approved.provider, approved.model.clone()))
                .or_insert_with(|| approved.model.clone());
        }
        let label = models
            .entry((profile.provider, profile.model.clone()))
            .or_insert_with(|| profile.model.clone())
            .clone();
        let session_id = session.id.clone();
        let current = profile.clone();
        let weak = cx.weak_entity();
        let model = button("chat-model")
            .ghost()
            .disabled(busy)
            .label(format!("{:?} · {label}", profile.provider))
            .dropdown_caret(true)
            .tooltip(if busy {
                "Pause the active turn to change the model"
            } else {
                "Model for this agent only"
            })
            .dropdown_menu(move |mut menu, _, _| {
                for ((provider, model), label) in &models {
                    let next = ModelProfile {
                        provider: *provider,
                        model: model.clone(),
                        effort: if *provider == current.provider {
                            current.effort.clone()
                        } else {
                            "high".into()
                        },
                    };
                    let session_id = session_id.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("{provider:?} · {label}"))
                            .checked(*provider == current.provider && *model == current.model)
                            .disabled(started && *provider != current.provider)
                            .on_click(move |_, w, c| {
                                let _ = weak.update(c, |v, c| {
                                    v.request(
                                        Command::ConfigureSession {
                                            session_id: session_id.clone(),
                                            profile: next.clone(),
                                        },
                                        w,
                                        c,
                                    )
                                });
                            }),
                    );
                }
                menu.scrollable(true).max_h(px(320.))
            });
        let efforts = models::effort_choices(
            &self.model_catalog,
            profile.provider,
            &profile.model,
            &profile.effort,
        );
        let session_id = session.id.clone();
        let weak = cx.weak_entity();
        let effort = button("chat-effort")
            .ghost()
            .disabled(busy)
            .label(effort_label(&profile.effort))
            .dropdown_caret(true)
            .tooltip("Reasoning effort for this agent only")
            .dropdown_menu(move |mut menu, _, _| {
                for effort in &efforts {
                    let next = ModelProfile {
                        effort: effort.clone(),
                        ..profile.clone()
                    };
                    let session_id = session_id.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(effort_label(effort))
                            .checked(*effort == profile.effort)
                            .on_click(move |_, w, c| {
                                let _ = weak.update(c, |v, c| {
                                    v.request(
                                        Command::ConfigureSession {
                                            session_id: session_id.clone(),
                                            profile: next.clone(),
                                        },
                                        w,
                                        c,
                                    )
                                });
                            }),
                    );
                }
                menu
            });
        div().flex().items_center().child(model).child(effort)
    }

    fn composer(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let sending = self.sending.contains(&session.id);
        let empty = self.input.read(cx).value().trim().is_empty();
        div()
            .key_context("ChatComposer")
            .on_action(cx.listener(|v, _: &SendMessage, w, c| {
                if w.has_active_dialog(c)
                    || v.input
                        .update(c, |input, c| input.marked_text_range(w, c).is_some())
                {
                    c.propagate();
                    return;
                }
                v.send(w, c);
            }))
            .flex_none()
            .px_6()
            .pb_4()
            .pt_1()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .children(self.decisions(session, p, cx))
            .children(
                self.notices(session, p, cx)
                    .into_iter()
                    .map(|notice| notice.max_w(px(COLUMN))),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .flex()
                    .flex_col()
                    .rounded_xl()
                    .border_1()
                    .border_color(p.edge.opacity(0.7))
                    .bg(p.surface)
                    .px_3()
                    .pt_2()
                    .pb_2()
                    .child(
                        Textarea::new(&self.input)
                            .w_full()
                            .appearance(false)
                            .bordered(false),
                    )
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(self.model_controls(session, cx))
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(p.subtle)
                                            .child("⏎ send · ⇧⏎ newline"),
                                    )
                                    .child(
                                        button("send")
                                            .primary()
                                            .icon(icon("arrow-up"))
                                            .loading(sending)
                                            .disabled(empty || sending || session.archived)
                                            .tooltip("Send")
                                            .on_click(cx.listener(|v, _, w, c| v.send(w, c))),
                                    ),
                            ),
                    ),
            )
    }

    pub(super) fn conversation(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(session) = self.selected_session() else {
            return div();
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.base)
            .child(self.conversation_header(session, p, cx))
            .children(self.pagination(p, cx))
            .child(if self.list.item_count() == 0 {
                self.empty_conversation(session.role, p, cx)
                    .into_any_element()
            } else {
                self.transcript(session, p).into_any_element()
            })
            .child(self.composer(session, p, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{first_line, report, transcript_overlap};
    use workspace_core::{Message, Receipt};

    #[test]
    fn reports_split_into_kind_and_body() {
        assert_eq!(
            report("[ready_for_testing] Implementer · Toolbar\nresult.txt written"),
            Some(("ready_for_testing", "result.txt written"))
        );
        assert_eq!(report("[completed] celadon-web"), Some(("completed", "")));
    }

    #[test]
    fn ordinary_messages_are_not_reports() {
        assert_eq!(report("[x] A checklist item"), None);
        assert_eq!(report("[See the docs] for details\nmore"), None);
        assert_eq!(report("Plain text"), None);
    }

    #[test]
    fn long_errors_show_only_a_bounded_first_line() {
        assert_eq!(
            first_line("Provider exited\nTraceback…", 80),
            "Provider exited"
        );
        assert_eq!(first_line("abcdef", 3), "abc…");
    }

    fn message(id: &str, body: &str) -> Message {
        Message {
            sequence: 0,
            id: id.into(),
            project_id: "p".into(),
            sender: None,
            recipient: "s".into(),
            body: body.into(),
            receipt: Receipt::Completed,
            created_at: 0,
        }
    }

    #[test]
    fn identical_refreshes_change_nothing() {
        let page = [message("a", "hi"), message("b", "there")];
        assert_eq!(transcript_overlap(&page, &page), Some((0, 2)));
    }

    #[test]
    fn refreshes_keep_the_unchanged_prefix() {
        let shown = [message("a", "hi"), message("b", "there")];
        let appended = [
            message("a", "hi"),
            message("b", "there"),
            message("c", "new"),
        ];
        assert_eq!(transcript_overlap(&shown, &appended), Some((0, 2)));
        let edited = [message("a", "hi"), message("b", "edited")];
        assert_eq!(transcript_overlap(&shown, &edited), Some((0, 1)));
    }

    #[test]
    fn a_full_page_drops_its_oldest_message() {
        let shown = [message("a", "1"), message("b", "2")];
        let refreshed = [message("b", "2"), message("c", "3")];
        assert_eq!(transcript_overlap(&shown, &refreshed), Some((1, 1)));
    }

    #[test]
    fn unrelated_pages_do_not_overlap() {
        let shown = [message("c", "3")];
        assert_eq!(transcript_overlap(&shown, &[message("a", "1")]), None);
        assert_eq!(transcript_overlap(&shown, &[]), None);
    }
}
