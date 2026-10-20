//! The project's open decisions and the workspace's notes: the answer box the conversation's
//! decision card and the Inbox panel share, and the requests that settle them.
use super::*;
use file_links::Linked;
use gpui_component::button::ButtonVariants;
use std::collections::HashMap;
use ui::{card, hint, mono, pill};

pub(super) struct Inbox {
    bridge: Bridge,
    shared: Shared,
    answer: Entity<TextareaState>,
    /// The question whose answer box is open.
    pub(super) answering: Option<String>,
    /// The workspace note open in Compare.
    compared: workspace_notes::Compared,
    /// Each open item's prompt with its file references linked, by item id.
    prompts: HashMap<String, Linked>,
}
impl Inbox {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            bridge,
            shared: Shared::default(),
            answer: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 6)
                    .placeholder("Your answer…")
            }),
            answering: None,
            compared: None,
            prompts: HashMap::new(),
        }
    }
    /// An open comparison is read again when the synced settings or the notes change.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let synced = |s: &Snapshot| (s.settings_sync.clone(), s.workspace_notes.clone());
        let before = self.shared.snapshot().map(synced);
        let (_, moved) = shared.moved_from(&self.shared);
        self.shared = shared;
        let Some(snapshot) = self.shared.snapshot() else {
            return;
        };
        if moved {
            // Linked here, as the items arrive, so drawing them asks the host nothing.
            let link = |item: &Attention| {
                let bases = self.shared.link_bases(&item.session_id).into();
                let linked = Linked::new(
                    &format!("attention-{}", item.id),
                    &item.prompt,
                    bases,
                    &self.bridge,
                );
                (item.id.clone(), linked)
            };
            self.prompts = snapshot.attention.iter().map(link).collect();
            file_links::link_when_resolved(self.prompts.values(), &self.bridge, cx);
        }
        if before.as_ref() != Some(&synced(snapshot)) {
            let paused = snapshot.paused_settings();
            if let Some(command) = workspace_notes::recompare(&mut self.compared, paused) {
                self.request(command, window, cx);
            }
            cx.notify();
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
    fn note_action(
        &mut self,
        path: String,
        action: workspace_notes::NoteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match action {
            workspace_notes::NoteAction::Compare => {
                workspace_notes::toggle_compare(&mut self.compared, path)
            }
            workspace_notes::NoteAction::Settle(settle) => {
                Some(Command::SettlePausedFile { path, settle })
            }
        };
        match command {
            Some(command) => self.request(command, window, cx),
            None => cx.notify(),
        }
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
            .shared
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
        if let Some(publish) = self.publish_card(item) {
            return publish_approval(entry, id, publish, p, cx);
        }
        if item.is_permission() {
            let approve = id.clone();
            return entry
                .child(
                    div()
                        .text_size(px(font::TEXT))
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
                    .border_color(p.edge.opacity(edge::HALF))
                    .text_size(px(font::TEXT))
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
                    .children(self.prompts.get(&id).map(Linked::view)),
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
    /// A paused setting's note: the setting, what happened, Compare and the choices that settle
    /// it for every project at once.
    pub(super) fn paused_card(
        &self,
        note: &PausedSetting,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let view = cx.entity().downgrade();
        let on: workspace_notes::OnNote = Rc::new(move |path, action, window, cx| {
            let _ = view.update(cx, |view, cx| view.note_action(path, action, window, cx));
        });
        let said = (note.text.strip_prefix(&note.setting)).unwrap_or(&note.text);
        let compared = &self.compared;
        card(p)
            .bg(p.surface)
            .border_color(p.yellow.opacity(0.55))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(hint("Workspace · settings sync", p))
                    .child(pill("Paused", p.yellow)),
            )
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .line_height(relative(1.5))
                    .child(
                        StyledText::new(format!("{}{said}", note.setting)).with_highlights([(
                            0..note.setting.len(),
                            HighlightStyle {
                                font_weight: Some(FontWeight::SEMIBOLD),
                                ..Default::default()
                            },
                        )]),
                    ),
            )
            .children(workspace_notes::comparison(note, compared, p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .flex_wrap()
                    .gap_2()
                    // Narrow, the choices wrap to the right under Compare.
                    .child(div().flex_1().flex().child(workspace_notes::compare_link(
                        note,
                        "Compare",
                        "Hide comparison",
                        compared,
                        &on,
                    )))
                    .children(workspace_notes::settle_buttons(note, &on)),
            )
            .children(
                workspace_notes::kept_note(note, compared)
                    .map(|kept| hint(kept, p).text_size(px(font::CAPTION))),
            )
    }
    /// The card for a publish step waiting on this item, drawn from the ticket's frozen request,
    /// its review and ledger, and the repository's facts. `None` for any other item.
    fn publish_card(&self, item: &Attention) -> Option<PublishCard> {
        let index = self.shared.index.as_ref()?;
        let snapshot = index.snapshot();
        let (ticket, request) = index.tickets().find_map(|ticket| {
            let request = ticket.publishing.as_ref()?;
            (request.waiting.as_deref() == Some(item.id.as_str())).then_some((ticket, request))
        })?;
        let repository = snapshot
            .repositories
            .iter()
            .find(|r| r.id == ticket.repository_id)?;
        Some(waiting_card(
            ticket,
            request,
            repository,
            &snapshot.allowed_owners,
        ))
    }
}
impl Requests for Inbox {
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
        let value = match reply {
            Ok(value) => value,
            Err(error) => {
                workspace_notes::compare_failed(&mut self.compared, &command);
                return toast_error(error, window, cx);
            }
        };
        match command {
            Command::ResolveAttention { .. } => {
                self.answering = None;
                self.answer
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            Command::DismissAttention { id } => {
                if self.answering.as_ref() == Some(&id) {
                    self.answering = None;
                }
            }
            Command::ComparePausedFile { path } => match serde_json::from_value(value) {
                Ok(lines) => workspace_notes::compared_arrived(&mut self.compared, &path, lines),
                Err(error) => eprintln!("Compare paused file: {error}"),
            },
            Command::SettlePausedFile { .. } => self.compared = None,
            _ => {}
        }
    }
}

/// A publish approval: what would leave the machine, then Deny and the step's own approve button.
fn publish_approval(
    entry: Div,
    id: String,
    card: PublishCard,
    p: Palette,
    cx: &mut Context<Inbox>,
) -> Div {
    let row = |label: &'static str, value: AnyElement| {
        div()
            .flex()
            .gap_3()
            .text_size(px(font::SMALL))
            .line_height(relative(1.5))
            .child(
                div()
                    .w(px(104.))
                    .flex_none()
                    .text_color(p.subtle)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(value))
    };
    let mut rows = div().flex().flex_col().gap_1();
    for (label, value) in card.rows {
        rows = rows.child(row(label, value.into_any_element()));
    }
    if !card.findings.is_empty() {
        let mut findings = div().flex().flex_col().gap_1p5();
        for (heading, summary) in card.findings {
            findings = findings.child(div().child(mono(heading, p)).child(summary));
        }
        rows = rows.child(row("Open findings", findings.into_any_element()));
    }
    if let Some(sent) = card.would_send {
        rows = rows.child(row("Would send", sent.into_any_element()));
    }
    if let Some(description) = card.description {
        let block = div().p_2().rounded_md().bg(p.overlay).child(description);
        rows = rows.child(row("Description", block.into_any_element()));
    }
    let approve = id.clone();
    entry
        .child(
            div()
                .text_size(px(font::TEXT))
                .font_weight(FontWeight::MEDIUM)
                .child(card.title),
        )
        .child(rows)
        .child(hint(card.note, p))
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    button(SharedString::from(format!("deny-{id}")))
                        .ghost()
                        .label("Deny")
                        .on_click(
                            cx.listener(move |v, _, w, c| v.resolve_attention(&id, "denied", w, c)),
                        ),
                )
                .child(
                    button(SharedString::from(format!("approve-{approve}")))
                        .primary()
                        .label(card.approve)
                        .on_click(cx.listener(move |v, _, w, c| {
                            v.resolve_attention(&approve, "approved", w, c)
                        })),
                ),
        )
}
