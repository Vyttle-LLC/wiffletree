//! The selected session's conversation: its header, the transcript, and the composer with its
//! notices and decision card.
use super::*;
use conversation_header::ConversationHeader;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
};
use transcript::Transcript;
use ui::{banner, hint, icon};
use wording::{effort_label, plural};

/// The files dropped on each session's draft that the host is still checking, and the send
/// pressed on each meanwhile, with the text it was pressed on.
#[derive(Default)]
struct DropChecks {
    checking: BTreeMap<String, usize>,
    waiting: BTreeMap<String, String>,
}
impl DropChecks {
    fn started(&mut self, session: &str) {
        *self.checking.entry(session.to_owned()).or_default() += 1;
    }
    /// Whether a send of `body` must wait for `session`'s dropped files; it waits with that text.
    fn hold(&mut self, session: &str, body: &str) -> bool {
        let holds = self.checking.contains_key(session);
        if holds {
            self.waiting.insert(session.to_owned(), body.to_owned());
        }
        holds
    }
    /// One of `session`'s checks came back. Returns the text of the send to make now: one that
    /// waited for this, the last check. A failed check drops the waiting send for good.
    fn finished(&mut self, session: &str, succeeded: bool) -> Option<String> {
        if let Some(count) = self.checking.get_mut(session) {
            *count -= 1;
            if *count == 0 {
                self.checking.remove(session);
            }
        }
        if !succeeded {
            self.waiting.remove(session);
            return None;
        }
        (!self.checking.contains_key(session))
            .then(|| self.waiting.remove(session))
            .flatten()
    }
}

/// Wide enough for the composer, narrow enough that message lines stay readable.
pub(super) const COLUMN: f32 = 720.;

/// Forgets the minimized card of every project that has no open decision, so the next one it
/// brings starts expanded. A decision that merely arrives or changes leaves the card as it is.
pub(super) fn forget_settled(minimized: &mut BTreeSet<String>, open: &[Attention]) {
    minimized.retain(|project| open.iter().any(|item| &item.project_id == project));
}

/// The selected session's conversation. The transcript is its own view, cached, so a keystroke in
/// the composer draws only this one.
pub(super) struct Conversation {
    bridge: Bridge,
    shared: Shared,
    inbox: Entity<inbox::Inbox>,
    header: Entity<ConversationHeader>,
    pub(super) transcript: Entity<Transcript>,
    input: Entity<TextareaState>,
    drafts: BTreeMap<String, String>,
    /// Files dropped on each session's conversation, waiting to go with its draft.
    attached: BTreeMap<String, Vec<Attachment>>,
    /// Why files in the selected conversation's last drop were not attached.
    refused_files: Vec<String>,
    sending: BTreeSet<String>,
    drop_checks: DropChecks,
    /// Projects whose decision card is folded to one row. Kept in memory only.
    minimized_decisions: BTreeSet<String>,
    /// The chat's model menu, rebuilt when the catalog or the snapshot changes.
    chat_models: Rc<BTreeMap<(Provider, String), String>>,
}
impl EventEmitter<Intent> for Conversation {}
impl Conversation {
    pub(super) fn new(
        bridge: Bridge,
        inbox: Entity<inbox::Inbox>,
        contexts: Entity<context_view::Contexts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 9));
        // The send button follows the draft, so redraw as it changes.
        cx.subscribe(&input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        // The decision card draws the inbox's answer box.
        cx.observe(&inbox, |_, _, cx| cx.notify()).detach();
        let transcript = cx.new(|cx| Transcript::new(bridge.clone(), input.clone(), window, cx));
        let header = cx.new(|cx| ConversationHeader::new(bridge.clone(), contexts, cx));
        cx.subscribe(&header, |_, _, intent: &Intent, cx| cx.emit(intent.clone()))
            .detach();
        cx.subscribe(&transcript, |_, _, intent: &Intent, cx| {
            cx.emit(intent.clone())
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            inbox,
            header,
            transcript,
            input,
            drafts: BTreeMap::new(),
            attached: BTreeMap::new(),
            refused_files: vec![],
            sending: BTreeSet::new(),
            drop_checks: DropChecks::default(),
            minimized_decisions: BTreeSet::new(),
            chat_models: Rc::default(),
        }
    }
    /// A new selection brings its draft back; each snapshot can change the model menu, the
    /// placeholder and which decision cards are left.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.shared, shared);
        let (selection, moved) = self.shared.moved_from(&old);
        let catalog = !Rc::ptr_eq(&old.catalog, &self.shared.catalog);
        if selection {
            if let Some(old) = &old.selected {
                (self.drafts).insert(old.clone(), self.input.read(cx).value().to_string());
            }
            let draft = (self.shared.selected.as_ref())
                .and_then(|id| self.drafts.get(id).cloned())
                .unwrap_or_default();
            self.refused_files.clear();
            self.input
                .update(cx, |input, cx| input.set_value(draft, window, cx));
        }
        if moved && let Some(snapshot) = self.shared.snapshot() {
            forget_settled(&mut self.minimized_decisions, &snapshot.attention);
        }
        if moved || catalog {
            self.list_chat_models();
        }
        if selection || moved {
            self.sync_composer(window, cx);
        }
        let shared = self.shared.clone();
        self.header
            .update(cx, |header, cx| header.share(shared.clone(), cx));
        self.transcript
            .update(cx, |transcript, cx| transcript.share(shared, window, cx));
        if selection || moved || catalog || old.panel != self.shared.panel {
            cx.notify();
        }
    }
    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match self.shared.selected_session().map(|s| s.role) {
            Some(Role::ProjectOrchestrator) => "Describe the goal, or reply to your coordinator…",
            Some(Role::TaskOrchestrator) => "This retired repository coordinator is read-only",
            Some(_) => "Give this agent a direct instruction…",
            None => "Create a project to start…",
        };
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
    }
    /// Adds dropped files to the selected session's draft. Each refused file is named with
    /// its reason; the rest are still attached.
    fn attach_files(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.shared.selected.clone() else {
            return;
        };
        self.drop_checks.started(&session);
        let ask = ask::inspect_attachments(paths.clone());
        self.ask(ask, window, cx, move |view, inspected, window, cx| {
            let inspected = match inspected {
                Ok(inspected) => inspected,
                Err(error) => {
                    // A send pressed meanwhile is not made; its draft stays to send again.
                    view.drop_checks.finished(&session, false);
                    return toast_error(error, window, cx);
                }
            };
            let attached = view.attached.entry(session.clone()).or_default();
            let refused = attachment_chips::add_dropped(attached, &paths, inspected);
            // Why files were refused shows only with the conversation they were dropped on.
            if view.shared.selected.as_ref() == Some(&session) {
                view.refused_files = refused;
            }
            if let Some(body) = view.drop_checks.finished(&session, true) {
                view.send_draft(session, body, window, cx);
            }
        });
    }
    fn remove_attachment(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if let Some(files) = self
            .shared
            .selected
            .as_ref()
            .and_then(|s| self.attached.get_mut(s))
        {
            files.retain(|a| a.path != path);
        }
        cx.notify();
    }
    /// Sends the selected session's draft as it reads now, or once its dropped files are checked.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().to_string();
        let Some(recipient) = self.shared.selected.clone() else {
            return;
        };
        if !self.drop_checks.hold(&recipient, &body) {
            self.send_draft(recipient, body, window, cx);
        }
    }
    /// Sends `body` with the files attached to `recipient`'s draft, whichever session is
    /// selected by then.
    fn send_draft(
        &mut self,
        recipient: String,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let attachments = self
            .attached
            .get(&recipient)
            .map(|files| files.iter().map(|f| f.path.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        if (body.trim().is_empty() && attachments.is_empty()) || self.sending.contains(&recipient) {
            return;
        }
        // Another session's saved draft is what was typed after; the reply leaves it.
        if self.shared.selected.as_ref() == Some(&recipient) {
            self.drafts.insert(recipient.clone(), body.clone());
            self.refused_files.clear();
        }
        self.sending.insert(recipient.clone());
        self.request(
            Command::Send {
                id: new_id(),
                sender: None,
                recipient,
                body,
                attachments,
            },
            window,
            cx,
        );
    }
    fn reconcile(&mut self, retry: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session_id) = self.shared.selected.clone() {
            self.request(Command::ReconcileSession { session_id, retry }, window, cx);
        }
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
        if !self.refused_files.is_empty() {
            notices.push(
                banner(p.yellow)
                    .items_start()
                    .child(icon("triangle-alert").text_color(p.yellow).mt(px(2.)))
                    .child(
                        div().flex_1().min_w_0().flex().flex_col().children(
                            self.refused_files
                                .iter()
                                .map(|reason| div().child(reason.clone())),
                        ),
                    )
                    .child(
                        button("dismiss-refused")
                            .ghost()
                            .icon(icon("close"))
                            .tooltip("Dismiss")
                            .on_click(cx.listener(|v, _, _, c| {
                                v.refused_files.clear();
                                c.notify();
                            })),
                    ),
            );
        }
        if let Some(error) = self
            .shared
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
        let waiting = self.transcript.read(cx).waiting_messages();
        if !self.shared.live_enabled() && waiting > 0 {
            notices.push(
                banner(p.focus)
                    .child(icon("pause").text_color(p.focus))
                    .child(div().flex_1().child(format!(
                        "This project is stopped. {waiting} {} waiting.",
                        plural(waiting, "message is", "messages are")
                    )))
                    .child(button("notice-resume").primary().label("Resume").on_click(
                        cx.listener(|v, _, w, c| {
                            if let Some(project_id) = v.shared.project_id() {
                                v.set_live(project_id, true, w, c)
                            }
                        }),
                    )),
            );
        }
        notices
    }
    /// The project's open decisions, one at a time, so the human can settle them in place.
    /// Hidden while the inbox is open, which already lists them all. The card takes at most
    /// half the window's height, so a long one keeps its buttons and the composer in view.
    fn decisions(
        &self,
        session: &Session,
        window: &Window,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if session.role != Role::ProjectOrchestrator || self.shared.panel == Some(Panel::Attention)
        {
            return None;
        }
        let items = self.shared.project_attention();
        let answering = self.inbox.read(cx).answering.clone();
        let position = answering
            .as_ref()
            .and_then(|id| items.iter().position(|item| &item.id == id))
            .unwrap_or(0);
        let item = items.get(position)?;
        let project = item.project_id.clone();
        if self.minimized_decisions.contains(&project) {
            return Some(self.minimized_decision(item, position, items.len(), p, cx));
        }
        let step = |offset: usize| {
            let id = items[(position + offset) % items.len()].id.clone();
            cx.listener(move |v, _, w, c| {
                v.inbox.update(c, |inbox, c| inbox.start_answer(&id, w, c))
            })
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
                    .on_click(
                        cx.listener(|_, _, _, c| c.emit(Intent::ShowPanel(Panel::Attention))),
                    ),
            )
            .child(
                button("minimize-decision")
                    .ghost()
                    .small()
                    .icon(icon("chevron-down"))
                    .tooltip("Minimize")
                    .on_click(cx.listener(move |v, _, _, c| {
                        v.minimized_decisions.insert(project.clone());
                        c.notify();
                    })),
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
                    (self.inbox)
                        .update(cx, |inbox, cx| {
                            let max_height = window.viewport_size().height / 2.;
                            inbox.attention_card(item, true, Some(max_height), p, cx)
                        })
                        .border_color(p.yellow.opacity(0.45)),
                ),
        )
    }
    fn minimized_decision(
        &self,
        item: &Attention,
        position: usize,
        total: usize,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let project = item.project_id.clone();
        let expand = move |v: &mut Self, _: &ClickEvent, _: &mut Window, c: &mut Context<Self>| {
            v.minimized_decisions.remove(&project);
            c.notify();
        };
        div().w_full().max_w(px(COLUMN)).child(
            div()
                .id("minimized-decision")
                .flex()
                .items_center()
                .gap_2()
                .pl_3()
                .pr_2()
                .py_1p5()
                .rounded_lg()
                .border_1()
                .border_color(p.yellow.opacity(0.45))
                .bg(p.surface)
                .cursor_pointer()
                .child(div().size(px(7.)).flex_none().rounded_full().bg(p.yellow))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(font::MESSAGE))
                        .child(first_line(&item.prompt, 200)),
                )
                .when(total > 1, |d| {
                    d.child(hint(format!("{} of {total}", position + 1), p).flex_none())
                })
                .child(
                    button("expand-decision")
                        .ghost()
                        .small()
                        .icon(icon("chevron-up"))
                        .tooltip("Expand"),
                )
                .on_click(cx.listener(expand)),
        )
    }
    /// The chat's model menu: every catalog model and every one the table names, by provider
    /// and model.
    pub(super) fn list_chat_models(&mut self) {
        let mut models: BTreeMap<(Provider, String), String> = self
            .shared
            .catalog
            .iter()
            .map(|model| ((model.provider, model.model.clone()), model.label.clone()))
            .collect();
        if let Some(snapshot) = self.shared.snapshot() {
            for profile in models::table_profiles(&snapshot.model_selection) {
                models
                    .entry((profile.provider, profile.model.clone()))
                    .or_insert_with(|| profile.model.clone());
            }
        }
        self.chat_models = Rc::new(models);
    }
    fn model_controls(&self, session: &Session, cx: &mut Context<Self>) -> Div {
        // An agent without a model is held; its menu asks for one instead of naming a model.
        let pinned = self.shared.selected_profile();
        let unchosen = pinned.is_none();
        let Some(profile) = pinned.or_else(|| {
            session.parent_id.is_some().then(|| ModelProfile {
                provider: session.provider,
                model: String::new(),
                effort: "high".into(),
            })
        }) else {
            return div();
        };
        let started = self
            .shared
            .runtime(&session.id)
            .is_some_and(|runtime| runtime.provider_session_id.is_some());
        let busy = session.status == Status::Working;
        // A PR keeper agent keeps the model chosen in Models when it was created.
        let fixed = busy || session.role == Role::Keeper;
        let models = self.chat_models.clone();
        // A model pinned before it left the catalog and the allowed list is still listed.
        let pinned = (!unchosen)
            .then(|| (profile.provider, profile.model.clone()))
            .filter(|key| !models.contains_key(key));
        let label = if unchosen {
            "Choose a model".to_owned()
        } else {
            let model = models
                .get(&(profile.provider, profile.model.clone()))
                .unwrap_or(&profile.model);
            format!("{} · {model}", profile.provider.label())
        };
        let session_id = session.id.clone();
        let current = profile.clone();
        let weak = cx.weak_entity();
        let model = button("chat-model")
            .ghost()
            .disabled(fixed)
            .label(label)
            .dropdown_caret(true)
            .tooltip(if session.role == Role::Keeper {
                "The PR keeper's model comes from its row in Models → Who runs what"
            } else if busy {
                "Pause the active turn to change the model"
            } else {
                "Model for this agent only"
            })
            .dropdown_menu(move |mut menu, _, _| {
                let mut entries: Vec<_> = (models.iter())
                    .chain(pinned.iter().map(|key| (key, &key.1)))
                    .collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                for ((provider, model), label) in entries {
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
                        PopupMenuItem::new(format!("{} · {label}", provider.label()))
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
            &self.shared.catalog,
            profile.provider,
            &profile.model,
            &[&profile.effort],
        );
        let session_id = session.id.clone();
        let weak = cx.weak_entity();
        let effort = button("chat-effort")
            .ghost()
            .disabled(fixed)
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
        div()
            .flex()
            .items_center()
            .child(model)
            .when(!unchosen, |d| d.child(effort))
    }
    /// Who chose this agent's model and why, above the composer's model pills.
    fn chosen_by_line(&self, session: &Session, p: Palette) -> Option<Div> {
        let selection = self.shared.runtime(&session.id)?.selection.as_ref()?;
        let index = self.shared.index.as_ref()?;
        Some(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .px_1()
                .text_size(px(font::SMALL))
                .text_color(p.subtle)
                .text_ellipsis()
                .child(models::chosen_by(selection, index)),
        )
    }
    fn composer(
        &self,
        session: &Session,
        window: &Window,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let sending = self.sending.contains(&session.id);
        let attached = self.attached.get(&session.id).cloned().unwrap_or_default();
        let empty = self.input.read(cx).value().trim().is_empty() && attached.is_empty();
        let unsendable = empty || sending || session.archived;
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
            .children(self.decisions(session, window, p, cx))
            .children(
                self.notices(session, p, cx)
                    .into_iter()
                    .map(|notice| notice.max_w(px(COLUMN))),
            )
            .children(self.chosen_by_line(session, p))
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .flex()
                    .flex_col()
                    .rounded_xl()
                    .border_1()
                    .border_color(p.edge.opacity(edge::FIRM))
                    .bg(p.surface)
                    .px_3()
                    .pt_2()
                    .pb_2()
                    .when(!attached.is_empty(), |composer| {
                        composer.child(self.pending_attachments(&attached, p, cx))
                    })
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
                                            .text_size(px(font::META))
                                            .text_color(p.subtle)
                                            .child("⏎ send · ⇧⏎ newline"),
                                    )
                                    .child(
                                        ui::icon_or_spinner(
                                            button("send").primary(),
                                            icon("arrow-up"),
                                            None,
                                            sending,
                                            gpui_component::Size::Small,
                                        )
                                        .disabled(unsendable)
                                        // The stock disabled arrow is too faint on its tint.
                                        .when(unsendable, |b| b.text_color(p.subtle))
                                        .tooltip("Send")
                                        .on_click(cx.listener(|v, _, w, c| v.send(w, c))),
                                    ),
                            ),
                    ),
            )
    }
    /// Dropped files waiting in the composer, each with a button to take it back out.
    fn pending_attachments(
        &self,
        attached: &[Attachment],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .pt_1()
            .pb_2()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(attached.iter().enumerate().map(|(index, attachment)| {
                let path = attachment.path.clone();
                attachment_chips::chip(
                    ("pending-attachment", index),
                    attachment,
                    attachment_chips::Stored::Draft,
                    p,
                )
                // Keeps a file's name clear of the remove button.
                .when(attachment.image.is_none(), |chip| chip.pr_7())
                .child(
                    div().absolute().top(px(2.)).right(px(2.)).child(
                        button(("remove-attachment", index))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip(format!("Remove {}", attachment.name()))
                            .on_click(cx.listener(move |v, _, _, c| v.remove_attachment(&path, c))),
                    ),
                )
            }))
    }
}
impl Requests for Conversation {
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
        if let Command::Send { recipient, .. } = &command {
            self.sending.remove(recipient);
        }
        if let Err(error) = reply {
            return toast_error(error, window, cx);
        }
        let Command::Send {
            recipient,
            body,
            attachments,
            ..
        } = command
        else {
            return;
        };
        if self
            .attached
            .get(&recipient)
            .is_some_and(|files| files.iter().map(|f| &f.path).eq(attachments.iter()))
        {
            self.attached.remove(&recipient);
        }
        let selected = self.shared.selected.as_deref() == Some(&recipient);
        if selected && self.input.read(cx).value().as_str() == body {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if self.drafts.get(&recipient) == Some(&body) {
            self.drafts.remove(&recipient);
        }
        if selected {
            self.transcript
                .update(cx, |transcript, cx| transcript.show_latest(window, cx));
        }
    }
}
impl Render for Conversation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Conversation");
        let p = palette(is_dark(cx));
        let Some(session) = self.shared.selected_session() else {
            return div();
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.base)
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(p.overlay))
            .on_drop(cx.listener(|v, paths: &ExternalPaths, w, c| {
                v.attach_files(paths.paths().to_vec(), w, c)
            }))
            .child(self.header.clone().cached(conversation_header::style()))
            .child(
                (self.transcript.clone())
                    .cached(StyleRefinement::default().w_full().flex_1().min_h_0()),
            )
            .child(self.composer(session, window, p, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{DropChecks, forget_settled};
    use std::collections::BTreeSet;
    use workspace_core::Attention;

    fn decision(id: &str, project: &str) -> Attention {
        Attention {
            id: id.to_owned(),
            project_id: project.to_owned(),
            session_id: "s".to_owned(),
            host: String::new(),
            operation_id: String::new(),
            prompt: "Which way?".to_owned(),
            options: Vec::new(),
            answer: None,
        }
    }

    #[test]
    fn a_send_pressed_while_files_are_checked_goes_with_its_own_text_after_the_last_check() {
        let mut checks = DropChecks::default();
        assert!(!checks.hold("s", "Now"), "nothing is being checked");
        checks.started("s");
        checks.started("s");
        assert!(checks.hold("s", "Look at these"));
        assert!(
            !checks.hold("t", "Elsewhere"),
            "another session's send goes at once"
        );
        assert_eq!(
            checks.finished("s", true),
            None,
            "a check is still on its way"
        );
        assert_eq!(checks.finished("s", true).as_deref(), Some("Look at these"));
        assert_eq!(checks.finished("s", true), None, "it went once");
    }

    #[test]
    fn a_failed_check_drops_the_waiting_send_and_no_later_drop_makes_it() {
        let mut checks = DropChecks::default();
        checks.started("s");
        checks.started("s");
        assert!(checks.hold("s", "Look at these"));
        assert_eq!(checks.finished("s", false), None);
        assert_eq!(
            checks.finished("s", true),
            None,
            "the other check sends nothing"
        );
        checks.started("s");
        assert_eq!(checks.finished("s", true), None, "nor does a later drop");
    }

    #[test]
    fn a_minimized_card_survives_another_decision_arriving() {
        let mut minimized = BTreeSet::from(["p1".to_owned()]);
        forget_settled(&mut minimized, &[decision("a", "p1")]);
        forget_settled(&mut minimized, &[decision("a", "p1"), decision("b", "p1")]);
        assert!(minimized.contains("p1"));
    }

    #[test]
    fn the_last_decision_settling_expands_the_next() {
        let mut minimized = BTreeSet::from(["p1".to_owned(), "p2".to_owned()]);
        forget_settled(&mut minimized, &[decision("b", "p2")]);
        assert_eq!(minimized, BTreeSet::from(["p2".to_owned()]));
    }
}
