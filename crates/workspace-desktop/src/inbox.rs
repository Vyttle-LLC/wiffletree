//! The project's open decisions and the workspace's notes: the answer box the conversation's
//! decision card and the Inbox panel share, and the requests that settle them.
use super::*;
use file_links::Linked;
use gpui_component::button::ButtonVariants;
use gpui_component::input::InputState;
use repository_tree::shown_path;
use std::{collections::HashMap, sync::Arc};
use ui::{card, hint, mono, pill};

/// Opens the issue an Inbox note names.
pub(super) type OpenIssue = Rc<dyn Fn(&mut Window, &mut App)>;
/// Opens the sub-project that asked a question its parent sent on.
pub(super) type OpenChild = Rc<dyn Fn(&mut Window, &mut App)>;

pub(super) struct Inbox {
    bridge: Bridge,
    shared: Shared,
    answer: Entity<TextareaState>,
    /// The question whose answer box is open.
    pub(super) answering: Option<String>,
    /// The workspace note open in Compare.
    compared: workspace_notes::Compared,
    /// Each open item's prompt, or a repository request's reason, with its file references
    /// linked, by item id.
    prompts: HashMap<String, Linked>,
    /// Each publish or merge approval's open-finding summaries, linked, by item id.
    findings: HashMap<String, Vec<Linked>>,
    /// The brain conflict whose Keep both line is open, by its card's key, and that line.
    keeping_both: Option<String>,
    scope: Entity<InputState>,
    /// The form of the held entry being edited or marked.
    held: held_entry::HeldForm,
    /// The reason line of the alert being marked.
    alert: scan_alert::AlertForm,
    /// The choice selected on each open "Allow UI evaluators" card, by item id; App only until
    /// the human picks another.
    allow_choices: HashMap<String, EvaluatorPermission>,
    /// The items answered or dismissed here, from the click until a newer snapshot.
    settlings: Settlings,
}

/// Where the human's answer or dismissal of an Inbox item stands.
#[derive(Debug, PartialEq)]
enum Settling {
    /// Sent; the host has not replied yet. `shown_open` once a snapshot meanwhile still
    /// listed the item, which may then be the one the answer leaves open.
    Sending { shown_open: bool },
    /// The host took it; the next snapshot drops the item, or shows it still open.
    Sent,
    /// The host refused it, in its words; the item's buttons work again.
    Failed(String),
}

/// How the host begins its refusal of an answer to an item already answered.
const ALREADY_RESOLVED: &str = "Request already resolved";

/// Each item's `Settling`, by item id. One click sends one answer: further clicks on the item
/// wait until the host's reply.
#[derive(Default)]
struct Settlings(HashMap<String, Settling>);
impl Settlings {
    /// Marks `id` as sending; `false` when an answer to it is already on its way or taken.
    fn start(&mut self, id: &str) -> bool {
        if matches!(
            self.0.get(id),
            Some(Settling::Sending { .. } | Settling::Sent)
        ) {
            return false;
        }
        self.0
            .insert(id.to_owned(), Settling::Sending { shown_open: false });
        true
    }
    fn replied(&mut self, id: &str, reply: &Result<Value, String>) {
        let shown_open = matches!(self.0.get(id), Some(Settling::Sending { shown_open: true }));
        let settling = match reply {
            // A snapshot may have shown the item open after the answer was taken, as Later
            // leaves a Start card: no newer one need come, so its buttons work again now.
            Ok(_) if shown_open => {
                self.0.remove(id);
                return;
            }
            // Answered already, from another window or device: settled all the same.
            Err(error) if error.starts_with(ALREADY_RESOLVED) => Settling::Sent,
            Ok(_) => Settling::Sent,
            Err(error) => Settling::Failed(error.clone()),
        };
        self.0.insert(id.to_owned(), settling);
    }
    /// After a newer snapshot: a sent item is settled either way, and only a refusal of an
    /// item still open stays, or a reply still awaited.
    fn refresh(&mut self, open: &[Attention]) {
        self.0.retain(|id, settling| {
            *settling != Settling::Sent && open.iter().any(|item| &item.id == id)
        });
        for settling in self.0.values_mut() {
            if let Settling::Sending { shown_open } = settling {
                *shown_open = true;
            }
        }
    }
    fn get(&self, id: &str) -> Option<&Settling> {
        self.0.get(id)
    }
}

/// The item a command answers or dismisses.
fn settled_item(command: &Command) -> Option<&str> {
    match command {
        Command::ResolveAttention { id, .. } | Command::DismissAttention { id } => Some(id),
        _ => None,
    }
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
            findings: HashMap::new(),
            keeping_both: None,
            scope: cx
                .new(|cx| InputState::new(window, cx).placeholder("One line that scopes both")),
            held: held_entry::HeldForm::new(window, cx),
            alert: scan_alert::AlertForm::new(window, cx),
            allow_choices: HashMap::new(),
            settlings: Settlings::default(),
        }
    }
    /// A held log entry's card.
    pub(super) fn held_entry_card(
        &mut self,
        entry: &HeldEntry,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (view, kept) = (cx.entity().downgrade(), entry.clone());
        let on: held_entry::OnHeld = Rc::new(move |action, window, cx| {
            let entry = kept.clone();
            let _ = view.update(cx, |view: &mut Self, cx| {
                held_entry::act(view, |v| &mut v.held, entry, action, window, cx)
            });
        });
        held_entry::held_entry_card(entry, &self.held, on, p)
    }
    /// An alert's card.
    pub(super) fn alert_card(
        &mut self,
        alert: &ScanAlert,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (view, kept) = (cx.entity().downgrade(), alert.clone());
        let on: scan_alert::OnAlert = Rc::new(move |action, window, cx| {
            let alert = kept.clone();
            let _ = view.update(cx, |view: &mut Self, cx| {
                scan_alert::act(view, |v| &mut v.alert, alert, action, window, cx)
            });
        });
        scan_alert::alert_card(alert, &self.alert, on, p)
    }
    /// A brain conflict's card; `evidence` opens the brain browser's Conflicts view.
    pub(super) fn conflict_card(
        &self,
        card: &ConflictCard,
        evidence: OpenIssue,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let view = cx.entity().downgrade();
        let (repository, conflict) = (card.repository.clone(), card.conflict.id.clone());
        let key = conflict_card::key(card);
        let open = self.keeping_both.as_deref() == Some(key.as_str());
        let on: conflict_card::OnConflict = Rc::new(move |action, window, cx| match action {
            conflict_card::ConflictAction::Evidence => evidence(window, cx),
            conflict_card::ConflictAction::KeepBoth(open) => {
                let key = key.clone();
                let _ = view.update(cx, |view, cx| {
                    view.keeping_both = open.then_some(key);
                    if open {
                        view.scope.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                            input.focus(window, cx);
                        });
                    }
                    cx.notify();
                });
            }
            conflict_card::ConflictAction::Rule(ruling) => {
                let command = Command::RuleConflict {
                    repository: repository.clone(),
                    conflict_id: conflict.clone(),
                    ruling,
                };
                let _ = view.update(cx, |view, cx| view.request(command, window, cx));
            }
        });
        conflict_card::conflict_card(card, open, &self.scope, on, p)
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
            self.settlings.refresh(&snapshot.attention);
            // Linked here, as the items arrive, so drawing them asks the host nothing.
            let link = |item: &Attention| {
                let bases = self.shared.link_bases(&item.session_id).into();
                let words = match &item.repository_request {
                    Some(request) => &request.reason,
                    None => &item.prompt,
                };
                let linked = Linked::new(
                    &format!("attention-{}", item.id),
                    words,
                    bases,
                    &self.bridge,
                );
                (item.id.clone(), linked)
            };
            self.prompts = snapshot.attention.iter().map(link).collect();
            let findings = |item: &Attention| {
                let card = self.approval_card(item)?;
                let bases: Arc<[PathBuf]> = self.shared.link_bases(&item.session_id).into();
                let summaries = (card.findings.iter().enumerate())
                    .map(|(ix, (_, summary))| {
                        let id = format!("finding-{}-{ix}", item.id);
                        Linked::new(&id, summary, bases.clone(), &self.bridge)
                    })
                    .collect();
                Some((item.id.clone(), summaries))
            };
            let findings = snapshot.attention.iter().filter_map(findings).collect();
            self.findings = findings;
            let linked = self
                .prompts
                .values()
                .chain(self.findings.values().flatten());
            file_links::link_when_resolved(linked, &self.bridge, cx);
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
        let command = Command::ResolveAttention {
            id: id.into(),
            answer: answer.into(),
        };
        self.settle(id, command, window, cx);
    }
    fn dismiss_attention(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let command = Command::DismissAttention { id: id.clone() };
        self.settle(&id, command, window, cx);
    }
    /// Sends `command`, which answers or dismisses the item `id`, unless an answer to it is
    /// already on its way. The card shows it is sending in this frame.
    fn settle(&mut self, id: &str, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.settlings.start(id) {
            cx.notify();
            self.request(command, window, cx);
        }
    }
    fn note_action(
        &mut self,
        note: workspace_notes::NoteRef,
        action: workspace_notes::NoteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match action {
            workspace_notes::NoteAction::Compare => {
                workspace_notes::toggle_compare(&mut self.compared, note)
            }
            workspace_notes::NoteAction::Settle(settle) => Some(note.settle(settle)),
        };
        match command {
            Some(command) => self.request(command, window, cx),
            None => cx.notify(),
        }
    }
    /// One inbox item with everything needed to settle it. `answering` opens the answer box.
    /// With `max_height`, everything between the header and the buttons scrolls as one, so the
    /// buttons stay in view; without it, only a long question's prompt scrolls.
    pub(super) fn attention_card(
        &self,
        item: &Attention,
        answering: bool,
        max_height: Option<Pixels>,
        open_child: Option<OpenChild>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let projects = self.shared.snapshot().map_or(&[][..], |s| &s.projects);
        let escalated = escalated_from(item, projects);
        let asker = match &escalated {
            Some(escalated) => escalated.label.clone(),
            None => (self.shared.session(&item.session_id))
                .map_or("Agent".to_owned(), |s| s.name.clone()),
        };
        let note = escalated.and_then(|e| e.note);
        let evaluator = evaluations::EvaluatorCard::of(item);
        let disk_card = disk_use::DiskCard::of(&item.operation_id);
        // The hard floor's card is red-edged and a failing delete's yellow, as the study draws them.
        let disk_edge = disk_card.as_ref().map(|card| match card {
            disk_use::DiskCard::BuildsHeld(_) => p.red,
            disk_use::DiskCard::DeleteFailed(_) => p.yellow,
        });
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(hint(asker, p).flex_1().min_w_0().text_ellipsis())
            .children(open_child.map(|open| {
                button(SharedString::from(format!("open-child-{}", item.id)))
                    .link()
                    .label("Open ›")
                    .on_click(move |_, window, cx| open(window, cx))
            }))
            .child(match (&evaluator, disk_edge) {
                (Some(_), _) => pill("UI evaluators", p.focus),
                (None, Some(color)) => pill("Disk", color),
                (None, None) if item.is_permission() => pill("Approval", p.yellow),
                (None, None) => pill("Question", p.focus),
            });
        let approval = evaluator
            .is_none()
            .then(|| self.approval_card(item))
            .flatten();
        let (body, actions) = match (approval, &item.repository_request, &evaluator) {
            _ if let Some(disk_card) = &disk_card => self.disk_card(item, disk_card, p, cx),
            (_, _, Some(card)) => self.evaluator_card(item, card, p, cx),
            (Some(approval), _, _) => {
                let findings = self.findings.get(&item.id).map_or(&[][..], Vec::as_slice);
                approval_card(
                    item.id.clone(),
                    approval,
                    findings,
                    &self.shared.host,
                    p,
                    cx,
                )
            }
            (None, Some(request), None) => self.repository_request(item, request, p, cx),
            (None, None, None) if item.is_permission() => self.permission(item, p, cx),
            (None, None, None) => self.question(item, answering, max_height.is_none(), p, cx),
        };
        let settling = self.settlings.get(&item.id);
        let pending = matches!(settling, Some(Settling::Sending { .. } | Settling::Sent));
        // Dimmed while pending; its choices send nothing more until the host replies.
        let body = (body.flex().flex_col().gap_2()).when(pending, |body| body.opacity(0.6));
        let status = |text| div().flex().justify_end().child(hint(text, p));
        let actions = match settling {
            Some(Settling::Sending { .. }) => status("Sending…"),
            Some(Settling::Sent) => status("Sent"),
            Some(Settling::Failed(error)) => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(hint(error.clone(), p).text_color(p.red))
                .child(actions),
            None => actions,
        };
        let edge = |color| {
            let strip = div().absolute().left_0().top_0().bottom_0().w(px(3.));
            strip.rounded_l_lg().bg(color)
        };
        let entry = card(p)
            .relative()
            .children(disk_edge.map(edge))
            .child(header)
            .when_some(note, |entry, note| entry.child(hint(note, p)));
        match max_height {
            // Keyed by item, so stepping to another decision starts at its top.
            Some(height) => entry.max_h(height).child(
                body.overflow_y_scrollbar()
                    .id(SharedString::from(format!("attention-body-{}", item.id))),
            ),
            None => entry.child(body),
        }
        .child(actions)
    }
    /// The host's UI evaluator cards, as screens 1 and 3 of the study draw them: the title on
    /// its own line, what the runs are, and the card's own choices. Only the Allow card offers
    /// a way out, Ask me later; the host refuses to dismiss a Start card, which holds the screen.
    fn evaluator_card(
        &self,
        item: &Attention,
        card: &evaluations::EvaluatorCard,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        use evaluations::*;
        let (catalog, runs) = (&self.shared.catalog, self.shared.evaluations());
        let title = div()
            .text_size(px(font::HEADING_SMALL))
            .font_weight(FontWeight::SEMIBOLD)
            .child(card.title());
        let id = item.id.clone();
        let actions = div().flex().justify_end().gap_2();
        match card {
            EvaluatorCard::Allow { .. } => {
                let chosen =
                    (self.allow_choices.get(&item.id).copied()).unwrap_or(EvaluatorPermission::App);
                let mut choices = div().flex().flex_col().gap_1p5();
                for (label, permission) in EvaluatorPermission::CHOICES {
                    choices =
                        choices.child(self.allow_choice(item, label, permission, chosen, p, cx));
                }
                let body = div()
                    .child(title)
                    .child(hint(
                        allow_summary(catalog, runs, &item.project_id),
                        p,
                    ))
                    .child(choices)
                    .child(hint(
                        "One choice per project, kept until you change it on the project's menu › UI evaluators. Closing the project turns it off. A coordinator can never change it.",
                        p,
                    ));
                let later = id.clone();
                let actions = actions
                    .child(
                        button(SharedString::from(format!("later-{id}")))
                            .ghost()
                            .label("Ask me later")
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.dismiss_attention(later.clone(), w, c)
                            })),
                    )
                    .child(
                        self.answer_button(
                            &id,
                            allow_button(chosen),
                            permission_answer(chosen),
                            cx,
                        )
                        .primary(),
                    );
                (body, actions)
            }
            EvaluatorCard::Start { run_id } => {
                let run = self.shared.evaluation(run_id);
                let summary = match run {
                    Some(run) => start_summary(catalog, run, runs),
                    // Not in the snapshot yet: the host's own words, under the title.
                    None => (item.prompt.split_once('\n'))
                        .map_or(item.prompt.clone(), |(_, rest)| rest.to_owned()),
                };
                let apps = run.map_or(&[][..], |run| &run.apps);
                let body = div()
                    .child(title)
                    .child(hint(summary, p))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(apps.iter().map(|app| app_line(app, p))),
                    )
                    .child(hint(
                        "Don't use this Mac until it finishes. Press Esc anywhere, or Stop on the evaluator's panel or in the menu bar, to stop it. Nothing touches the screen until you press Start now.",
                        p,
                    ));
                let actions = actions
                    .child(
                        self.answer_button(&id, "Skip these runs", "Skip these runs", cx)
                            .ghost(),
                    )
                    .child(self.answer_button(&id, "Later", "Later", cx))
                    .child(
                        self.answer_button(&id, "Start now", "Start now", cx)
                            .primary(),
                    );
                (body, actions)
            }
        }
    }
    /// A button that answers the item `id` with `answer`.
    fn answer_button(
        &self,
        id: &str,
        label: &'static str,
        answer: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        let id = id.to_owned();
        button(SharedString::from(format!("{label}-{id}")))
            .label(label)
            .map(|b| ui::gated(b, &self.shared.host))
            .on_click(cx.listener(move |v, _, w, c| v.resolve_attention(&id, answer, w, c)))
    }
    /// One choice on the Allow card: a radio dot, its name and what it means.
    fn allow_choice(
        &self,
        item: &Attention,
        label: &'static str,
        permission: EvaluatorPermission,
        chosen: EvaluatorPermission,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = permission == chosen;
        let id = item.id.clone();
        let dot = div()
            .flex_none()
            .mt(px(3.))
            .size(px(12.))
            .rounded_full()
            .border_1()
            .border_color(if selected { p.focus } else { p.subtle })
            .flex()
            .items_center()
            .justify_center()
            .when(selected, |d| {
                d.child(div().size(px(6.)).rounded_full().bg(p.focus))
            });
        div()
            .id(SharedString::from(format!("allow-{label}-{id}")))
            .flex()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                p.focus
            } else {
                p.edge.opacity(edge::HALF)
            })
            .bg(p.surface)
            .cursor_pointer()
            .hover(|s| s.border_color(p.focus))
            .child(dot)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(font::TEXT))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label),
                    )
                    .child(hint(evaluations::permission_detail(permission), p)),
            )
            .on_click(cx.listener(move |v, _, _, c| {
                v.allow_choices.insert(id.clone(), permission);
                c.notify();
            }))
    }
    /// A card the disk work raised: its ask and rows, or its notice until the host's reading
    /// has them, then `disk_use::card_actions`' buttons, the last one primary.
    fn disk_card(
        &self,
        item: &Attention,
        card: &disk_use::DiskCard,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        use disk_use::CardAction;
        let disk = disk_use::current(cx);
        let home = spaces_view::home();
        let body = match disk_use::card_text(card, disk, home.as_deref()) {
            Some(text) => disk_card_text(text, p),
            None => div().children(self.prompts.get(&item.id).map(Linked::view)),
        };
        let on_this_mac = self.bridge.remote().is_none();
        let offered = disk_use::card_actions(card, disk, on_this_mac);
        let last = offered.len() - 1;
        // Wrapped, so the hard floor's two long buttons fit the Inbox panel.
        let mut actions = div().flex().flex_wrap().justify_end().gap_2();
        for (at, action) in offered.into_iter().enumerate() {
            let label = match &action {
                CardAction::OpenSettings => "Open Settings › Disk",
                CardAction::ReclaimAll => "Reclaim all but running builds",
                CardAction::Dismiss => "Dismiss",
                CardAction::ShowInFinder(_) => "Show in Finder",
                CardAction::RetryNow(_) => "Retry now",
            };
            let id = item.id.clone();
            let drawn = button(SharedString::from(format!("disk-{at}-{id}")))
                .label(label)
                .map(|b| if at == last { b.primary() } else { b.ghost() });
            let drawn = match action {
                CardAction::OpenSettings => {
                    drawn.on_click(|_, _, cx| settings::Settings::open_at_disk(cx))
                }
                CardAction::ShowInFinder(path) => {
                    drawn.on_click(move |_, _, cx| cx.reveal_path(std::path::Path::new(&path)))
                }
                CardAction::ReclaimAll => {
                    ui::gated(drawn, &self.shared.host).on_click(move |_, window, cx| {
                        let ask = ask::reclaim_build_output(None);
                        disk_use::ask(ask, Some(window.window_handle()), cx)
                    })
                }
                CardAction::RetryNow(path) => {
                    ui::gated(drawn, &self.shared.host).on_click(move |_, window, cx| {
                        let ask = ask::retry_delete(path.clone());
                        disk_use::ask(ask, Some(window.window_handle()), cx)
                    })
                }
                CardAction::Dismiss => ui::gated(drawn, &self.shared.host)
                    .on_click(cx.listener(move |v, _, w, c| v.dismiss_attention(id.clone(), w, c))),
            };
            actions = actions.child(drawn);
        }
        (body, actions)
    }
    /// A permission request's operation, then Deny and Approve once.
    fn permission(&self, item: &Attention, p: Palette, cx: &mut Context<Self>) -> (Div, Div) {
        let (id, approve) = (item.id.clone(), item.id.clone());
        let body = div()
            // Plain, not Linked: the prompt holds the tool's raw JSON input, which Markdown
            // would mangle.
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .line_height(relative(1.5))
                    .child(item.prompt.clone()),
            )
            .child(hint(format!("{} · {}", item.host, item.operation_id), p));
        let actions = div()
            .flex()
            .justify_end()
            .gap_2()
            .child(
                button(SharedString::from(format!("deny-{id}")))
                    .ghost()
                    .label("Deny")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(
                        cx.listener(move |v, _, w, c| v.resolve_attention(&id, "denied", w, c)),
                    ),
            )
            .child(
                button(SharedString::from(format!("approve-{approve}")))
                    .primary()
                    .label("Approve once")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(cx.listener(move |v, _, w, c| {
                        v.resolve_attention(&approve, "approved", w, c)
                    })),
            );
        (body, actions)
    }
    /// A coordinator's repository request: what would be added, why, which Spaces list it and
    /// which this project is in. Approving changes this project only, so no Space is offered.
    fn repository_request(
        &self,
        item: &Attention,
        request: &RepositoryRequest,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        let snapshot = self.shared.snapshot();
        let spaces = snapshot.map_or(&[][..], |s| &s.spaces);
        let space_name = |space: &Space| -> AnyElement {
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(spaces_view::space_mark(spaces_view::space_color(
                    spaces, &space.id, p,
                )))
                .child(space.name.clone())
                .into_any_element()
        };
        let listing = request.identity.as_deref().map_or(vec![], |identity| {
            (spaces.iter())
                .filter(|space| space.lists_named(identity))
                .map(space_name)
                .collect()
        });
        let listed_by = match listing.is_empty() {
            true => div().child("No Space"),
            false => div().flex().flex_wrap().gap_x_3().children(listing),
        };
        let project_space = snapshot.and_then(|snapshot| {
            let project = (snapshot.projects.iter()).find(|p| p.id == item.project_id)?;
            snapshot.space_of(project)
        });
        let folder = shown_path(&request.path, spaces_view::home().as_deref());
        let (label, target) = match (&request.repository_id, &request.identity) {
            (Some(_), Some(identity)) => ("Repository", identity.clone()),
            (Some(_), None) => ("Repository", folder),
            (None, _) => ("Folder", folder),
        };
        let rows = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(approval_row(label, code(target, p).into_any_element(), p))
            .child(approval_row("Listed by", listed_by.into_any_element(), p))
            .child(approval_row(
                "This project",
                div()
                    .children(project_space.map(space_name))
                    .into_any_element(),
                p,
            ));
        let body = div()
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .font_weight(FontWeight::MEDIUM)
                    .child(request.title()),
            )
            .children(self.prompts.get(&item.id).map(Linked::view))
            .child(rows);
        let approve = "Add to this project";
        let actions = approval_actions(item.id.clone(), approve, &self.shared.host, cx);
        (body, actions)
    }
    /// A question's prompt, choices and answer box, then Dismiss and Answer or Send answer.
    /// `cap_prompt` gives a long prompt its own scroll.
    fn question(
        &self,
        item: &Attention,
        answering: bool,
        cap_prompt: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        let id = item.id.clone();
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
                .map(|b| ui::gated(b, &self.shared.host))
                .on_click(
                    cx.listener(move |v, _, w, c| v.dismiss_attention(dismiss.clone(), w, c)),
                ),
        );
        let prompt = div().children(self.prompts.get(&id).map(Linked::view));
        let body = div()
            .child(if cap_prompt {
                prompt
                    .max_h(px(280.))
                    .overflow_y_scrollbar()
                    .id(SharedString::from(format!("question-{id}")))
                    .into_any_element()
            } else {
                prompt.into_any_element()
            })
            .when(!item.options.is_empty(), |d| d.child(choices));
        if answering {
            let body = body.child(Textarea::new(&self.answer).w_full());
            let actions = actions.child(
                button(SharedString::from(format!("answer-{id}")))
                    .primary()
                    .label("Send answer")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(cx.listener(move |v, _, w, c| {
                        let answer = v.answer.read(c).value().trim().to_string();
                        if !answer.is_empty() {
                            v.resolve_attention(&id, &answer, w, c);
                        }
                    })),
            );
            (body, actions)
        } else {
            let actions = actions.child(
                button(SharedString::from(format!("start-answer-{id}")))
                    .label("Answer")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(cx.listener(move |v, _, w, c| v.start_answer(&id, w, c))),
            );
            (body, actions)
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
                    .child(hint(
                        match &note.repository {
                            Some(repository) => format!("Workspace · {repository}"),
                            None => "Workspace · settings sync".into(),
                        },
                        p,
                    ))
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
    /// An issue note: the issue's ID, what happened and a second line; Open for news, which
    /// `open` acts on, or Compare and the choices that settle a paused file or part. Issue text
    /// is shown as plain text.
    pub(super) fn issue_card(
        &self,
        note: &IssueNote,
        open: OpenIssue,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let row = div()
            .flex()
            .items_start()
            .gap_2()
            .child(mono(note.id.to_string(), p).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(font::TEXT))
                    .line_height(relative(1.5))
                    .child(note.text.clone()),
            );
        let detail = (!note.detail.is_empty()).then(|| hint(note.detail.clone(), p));
        let Some(paused) = &note.paused else {
            let id = SharedString::from(format!("issue-note-open-{}-{}", note.id, note.at));
            let open = button(id)
                .link()
                .label("Open")
                .on_click(move |_, window, cx| open(window, cx));
            return card(p).child(row.child(open)).children(detail);
        };
        let view = cx.entity().downgrade();
        let on: workspace_notes::OnNote = Rc::new(move |path, action, window, cx| {
            let _ = view.update(cx, |view, cx| view.note_action(path, action, window, cx));
        });
        let compared = &self.compared;
        card(p)
            .bg(p.surface)
            .border_color(p.yellow.opacity(0.55))
            .child(row)
            .children(detail)
            .children(workspace_notes::comparison(paused, compared, p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .flex_wrap()
                    .gap_2()
                    .child(div().flex_1().flex().child(workspace_notes::compare_link(
                        paused,
                        "Compare",
                        "Hide comparison",
                        compared,
                        &on,
                    )))
                    .children(workspace_notes::settle_buttons(paused, &on)),
            )
            .children(
                workspace_notes::kept_note(paused, compared)
                    .map(|kept| hint(kept, p).text_size(px(font::CAPTION))),
            )
    }
    /// A phone paired by QR code, which anyone who photographed the code could have done. Its
    /// buttons are the host's alone, so a Mac driving the host shows only the note.
    pub(super) fn paired_card(
        &self,
        notice: &PairedNotice,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let device_id = notice.device_id.clone();
        let on_this_mac = self.bridge.remote().is_none();
        card(p)
            .bg(p.surface)
            .border_color(p.yellow.opacity(0.55))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(hint("Workspace · Remote access", p))
                    .child(pill("New device", p.yellow)),
            )
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .line_height(relative(1.5))
                    .child(paired_text(&notice.device_name)),
            )
            .when(on_this_mac, |card| {
                card.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            button(SharedString::from(format!("remote-{}", notice.device_id)))
                                .small()
                                .label("Remote access")
                                .on_click(|_, _, cx| settings::Settings::open_at_remote_access(cx)),
                        )
                        .child(
                            button(SharedString::from(format!("seen-{}", notice.device_id)))
                                .ghost()
                                .small()
                                .label("Dismiss")
                                .on_click(cx.listener(move |inbox, _, window, cx| {
                                    let device_id = device_id.clone();
                                    let command = Command::DismissPairedNotice { device_id };
                                    inbox.request(command, window, cx);
                                })),
                        ),
                )
            })
    }
    /// A storage repository's file a scan held back from a push: which file, the check and its
    /// rule, never what matched.
    pub(super) fn held_card(&self, held: &HeldFile, p: Palette) -> Div {
        card(p)
            .bg(p.surface)
            .border_color(p.yellow.opacity(0.55))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(hint(format!("Workspace · {}", held.repository), p))
                    .child(pill("Held", p.yellow)),
            )
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .line_height(relative(1.5))
                    .child(held.text.clone()),
            )
    }
    /// The card for a publish step or a merge waiting on this item, drawn from the ticket's
    /// stored request, its review and ledger, and the repository's facts. `None` for any other
    /// item.
    fn approval_card(&self, item: &Attention) -> Option<ApprovalCard> {
        let index = self.shared.index.as_ref()?;
        let snapshot = index.snapshot();
        let repository_of =
            |ticket: &Ticket| (snapshot.repositories.iter()).find(|r| r.id == ticket.repository_id);
        index.tickets().find_map(|ticket| {
            if let Some(request) = ticket.publishing.as_ref()
                && request.waiting.as_deref() == Some(item.id.as_str())
            {
                let allowed = &snapshot.allowed_owners;
                return Some(waiting_card(
                    ticket,
                    request,
                    repository_of(ticket)?,
                    allowed,
                ));
            }
            let request = ticket.merging.as_ref()?;
            let waiting_here = MergeStage::WaitingForHuman {
                item: item.id.clone(),
            };
            if request.stage != waiting_here {
                return None;
            }
            Some(merge_card(ticket, request, repository_of(ticket)?))
        })
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
        // An item's refusal shows on its card, beside the buttons that try again.
        if let Some(id) = settled_item(&command) {
            self.settlings.replied(id, &reply);
        }
        let value = match reply {
            Ok(value) => value,
            Err(_) if settled_item(&command).is_some() => return,
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
            Command::ComparePausedFile { path, repository } => {
                match serde_json::from_value(value) {
                    Ok(lines) => {
                        let note = workspace_notes::NoteRef { path, repository };
                        workspace_notes::compared_arrived(&mut self.compared, &note, lines)
                    }
                    Err(error) => eprintln!("Compare paused file: {error}"),
                }
            }
            Command::SettlePausedFile { .. } => self.compared = None,
            Command::RuleConflict { .. } => self.keeping_both = None,
            command if held_entry::settles(&command) => self.held.close(),
            command if scan_alert::settles(&command) => self.alert.close(),
            _ => {}
        }
    }
}

/// A question a parent project sent on to the human: its card's label and the parent's note.
struct Escalated {
    label: String,
    note: Option<String>,
}

/// For an escalated question, "<child> · via <parent>" and the note; none for any other item.
fn escalated_from(item: &Attention, projects: &[Project]) -> Option<Escalated> {
    let QuestionRoute::Escalated { parent_id, note } = &item.route else {
        return None;
    };
    let name = |id: &str| (projects.iter().find(|p| p.id == id)).map(|p| p.name.as_str());
    Some(Escalated {
        label: format!("{} · via {}", name(&item.project_id)?, name(parent_id)?),
        note: note.clone(),
    })
}

/// Whether `item` is a sub-project's question that `project_id`'s coordinator sent on to the
/// human.
pub(super) fn escalated_via(item: &Attention, project_id: &str) -> bool {
    matches!(&item.route, QuestionRoute::Escalated { parent_id, .. } if parent_id == project_id)
}

fn paired_text(device_name: &str) -> String {
    format!("A phone was paired: {device_name}. Not you? Remove it in Remote access.")
}

/// A repository or folder in full, wrapping rather than cut off: this is what approving adds.
fn code(text: String, p: Palette) -> Div {
    div().flex().child(
        div()
            .min_w_0()
            .px_1()
            .rounded(px(4.))
            .bg(p.overlay)
            .font_family(mono_font())
            .text_size(px(font::CAPTION))
            .child(text),
    )
}

/// One row of an approval card: its label, then its value.
fn approval_row(label: &'static str, value: AnyElement, p: Palette) -> Div {
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
}

/// A disk card's bold ask over its rows; the folder reads in monospace, wrapped so its name
/// shows in a narrow Inbox.
fn disk_card_text(text: disk_use::CardText, p: Palette) -> Div {
    let rows = text.rows.into_iter().map(|(label, value)| {
        let value = match label {
            "Folder" => div()
                .font_family(palette::mono_font())
                .text_color(p.subtle)
                .child(value)
                .into_any_element(),
            _ => value.into_any_element(),
        };
        approval_row(label, value, p)
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(font::LEAD))
                .font_weight(FontWeight::MEDIUM)
                .child(text.ask),
        )
        .child(div().flex().flex_col().gap_0p5().children(rows))
}

/// A publish or merge approval: what would leave the machine or merge, then Deny and the
/// request's own approve button. `findings` are the card's open-finding summaries, linked.
fn approval_card(
    id: String,
    card: ApprovalCard,
    findings: &[Linked],
    host: &HostView,
    p: Palette,
    cx: &mut Context<Inbox>,
) -> (Div, Div) {
    let row = |label, value| approval_row(label, value, p);
    let mut rows = div().flex().flex_col().gap_1();
    for (label, value) in card.rows {
        rows = rows.child(row(label, value.into_any_element()));
    }
    if !card.findings.is_empty() {
        let mut shown = div().flex().flex_col().gap_1p5();
        for ((heading, _), summary) in card.findings.into_iter().zip(findings) {
            shown = shown.child(div().child(mono(heading, p)).child(summary.view()));
        }
        rows = rows.child(row("Open findings", shown.into_any_element()));
    }
    if let Some(sent) = card.would_send {
        rows = rows.child(row("Would send", sent.into_any_element()));
    }
    // Plain, not Linked: the description shows exactly the text that will be sent.
    if let Some(description) = card.description {
        let block = div().p_2().rounded_md().bg(p.overlay).child(description);
        rows = rows.child(row("Description", block.into_any_element()));
    }
    let body = div()
        .child(
            div()
                .text_size(px(font::TEXT))
                .font_weight(FontWeight::MEDIUM)
                .child(card.title),
        )
        .child(rows)
        .child(hint(card.note, p));
    (body, approval_actions(id, card.approve, host, cx))
}

/// Deny and the request's approve button, which answer the item `id` and nothing else.
fn approval_actions(
    id: String,
    approve: &'static str,
    host: &HostView,
    cx: &mut Context<Inbox>,
) -> Div {
    let approved = id.clone();
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            button(SharedString::from(format!("deny-{id}")))
                .ghost()
                .label("Deny")
                .map(|b| ui::gated(b, host))
                .on_click(cx.listener(move |v, _, w, c| v.resolve_attention(&id, "denied", w, c))),
        )
        .child(
            button(SharedString::from(format!("approve-{approved}")))
                .primary()
                .label(approve)
                .map(|b| ui::gated(b, host))
                .on_click(
                    cx.listener(move |v, _, w, c| v.resolve_attention(&approved, "approved", w, c)),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::{Settling, Settlings, escalated_from, escalated_via};
    use serde_json::json;
    use workspace_core::{Attention, Project, QuestionRoute};

    #[test]
    fn a_parent_lists_only_the_questions_it_sent_on_to_the_human() {
        let mut item = open("q");
        let parent = "parent".to_owned();
        assert!(!escalated_via(&item, &parent), "the child's own");
        item.route = QuestionRoute::Parent {
            parent_id: parent.clone(),
        };
        assert!(!escalated_via(&item, &parent), "still with the parent");
        item.route = QuestionRoute::Escalated {
            parent_id: parent.clone(),
            note: None,
        };
        assert!(escalated_via(&item, &parent));
        assert!(!escalated_via(&item, "another"));
    }

    fn open(id: &str) -> Attention {
        serde_json::from_value(json!({
            "id": id, "project_id": "p", "session_id": "s", "host": "local",
            "operation_id": "copy", "prompt": "Which wording?", "answer": null,
        }))
        .unwrap()
    }

    #[test]
    fn a_second_click_sends_nothing_until_the_host_replies() {
        let mut settlings = Settlings::default();
        assert!(settlings.start("q"));
        assert!(!settlings.start("q"), "still sending");
        settlings.replied("q", &Ok(json!({})));
        assert!(!settlings.start("q"), "taken, the card not yet gone");
    }

    #[test]
    fn a_refusal_stays_on_its_open_card_and_allows_another_try() {
        let mut settlings = Settlings::default();
        settlings.start("q");
        settlings.replied("q", &Err("The host has stopped".into()));
        settlings.refresh(&[open("q")]);
        assert_eq!(
            settlings.get("q"),
            Some(&Settling::Failed("The host has stopped".into()))
        );
        assert!(settlings.start("q"));
    }

    #[test]
    fn an_answer_already_taken_is_no_error() {
        let mut settlings = Settlings::default();
        settlings.start("q");
        let stale = "Request already resolved; stale answer rejected";
        settlings.replied("q", &Err(stale.into()));
        assert_eq!(settlings.get("q"), Some(&Settling::Sent));
    }

    #[test]
    fn a_newer_snapshot_frees_a_card_the_answer_left_open() {
        // Later on a Start card is taken, but the card stays.
        let mut settlings = Settlings::default();
        settlings.start("start");
        settlings.start("waiting");
        settlings.replied("start", &Ok(json!({})));
        settlings.refresh(&[open("start"), open("waiting")]);
        assert_eq!(settlings.get("start"), None);
        assert!(matches!(
            settlings.get("waiting"),
            Some(Settling::Sending { .. })
        ));
    }

    #[test]
    fn an_answer_whose_snapshot_came_first_frees_a_card_it_left_open() {
        // Later's snapshot, still listing the Start card, arrives before Later's reply.
        let mut settlings = Settlings::default();
        settlings.start("start");
        settlings.refresh(&[open("start")]);
        settlings.replied("start", &Ok(json!({})));
        assert_eq!(settlings.get("start"), None);
        assert!(settlings.start("start"), "Start now works after Later");
    }

    #[test]
    fn an_escalated_question_names_its_child_via_its_parent_with_the_note() {
        let projects: Vec<Project> = ["Contrast warnings", "Accessible contrast"]
            .map(|name| {
                serde_json::from_value(json!({"id": name, "name": name, "space": "s"})).unwrap()
            })
            .into();
        let mut item: Attention = serde_json::from_value(json!({
            "id": "q", "project_id": "Contrast warnings", "session_id": "s", "host": "local",
            "operation_id": "copy", "prompt": "Which wording?", "answer": null,
        }))
        .unwrap();
        assert!(
            escalated_from(&item, &projects).is_none(),
            "the human's own"
        );

        item.route = QuestionRoute::Escalated {
            parent_id: "Accessible contrast".into(),
            note: Some("Product call on user-facing copy".into()),
        };
        let escalated = escalated_from(&item, &projects).unwrap();
        assert_eq!(
            escalated.label,
            "Contrast warnings · via Accessible contrast"
        );
        assert_eq!(
            escalated.note.as_deref(),
            Some("Product call on user-facing copy")
        );
    }

    /// A window showing one Inbox card as the Inbox panel draws it, 480px wide.
    struct Shown(gpui::Entity<super::Inbox>, Attention);
    impl gpui::Render for Shown {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{InteractiveElement as _, ParentElement as _, Styled as _};
            let item = self.1.clone();
            let p = crate::palette::palette(false);
            let card = (self.0).update(cx, |inbox, cx| {
                inbox.attention_card(&item, false, None, None, p, cx)
            });
            gpui::div()
                .w(gpui::px(480.))
                .child(card.debug_selector(|| "card".into()))
        }
    }

    /// A window showing `item`'s card from `snapshot`, drawn once, and the card's bounds.
    fn show_card(
        cx: &mut gpui::TestAppContext,
        bridge: crate::bridge::Bridge,
        snapshot: workspace_core::Snapshot,
        item: Attention,
    ) -> (
        gpui::Entity<Shown>,
        &mut gpui::VisualTestContext,
        gpui::Bounds<gpui::Pixels>,
    ) {
        use gpui::AppContext as _;
        cx.update(gpui_component::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let inbox = cx.new(|cx| {
                let mut inbox = super::Inbox::new(bridge, window, cx);
                let index = crate::WorkspaceIndex::new(snapshot, Default::default());
                inbox.shared = crate::Shared {
                    index: Some(std::rc::Rc::new(index)),
                    ..Default::default()
                };
                inbox
            });
            Shown(inbox, item)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let card = cx.debug_bounds("card").unwrap();
        (view, cx, card)
    }

    /// Clicks the change-of-base card's button `from_right` places from its bottom right, with
    /// the host out of reach, and says whether the click sent the card's answer: the bridge
    /// refuses it at once, and the card shows that refusal.
    fn sends_an_answer(cx: &mut gpui::TestAppContext, from_right: f32) -> bool {
        let mut item = open("r");
        item.operation_id = "retarget:r1".into();
        item.prompt = "Move PR #12 for ticket \"Chat\" (t) in o/r from stacked to main?".into();
        assert!(item.is_retarget() && item.is_permission());
        let snapshot = workspace_core::Snapshot {
            attention: vec![item.clone()],
            ..Default::default()
        };
        let bridge = crate::bridge::Bridge::unavailable("The host is out of reach".into());
        let (view, cx, card) = show_card(cx, bridge, snapshot, item);
        let button = card.bottom_right() - gpui::point(gpui::px(from_right), gpui::px(24.));
        cx.simulate_click(button, gpui::Modifiers::default());
        let refused = Settling::Failed("The host is out of reach".into());
        cx.update(|_, cx| view.read(cx).0.read(cx).settlings.get("r") == Some(&refused))
    }

    /// While a busy host holds every request, a click on Dismiss shows the card sending in the
    /// frame it draws, before any reply; the reply, once the host answers, settles it.
    #[gpui::test]
    fn a_dismissed_card_shows_it_is_sending_at_once_while_a_busy_host_holds_the_reply(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::sync::atomic::Ordering;
        let served = crate::bridge::tests::serve(|host| {
            let project = host.create_project("Busy project").unwrap();
            let coordinator = (host.sessions().unwrap().into_iter())
                .find(|s| s.project_id == project.id)
                .unwrap();
            let ask = json!({"request_id": "ship", "question": "Ship it?"});
            host.agent_tool(&coordinator.id, "ask_user", ask).unwrap();
        });
        let snapshot = (served.bridge)
            .ask_now(workspace_core::ask::snapshot())
            .unwrap();
        let item = snapshot.attention[0].clone();
        let id = item.id.clone();
        let (bridge, held) = served.busy();
        held.store(true, Ordering::Relaxed);
        // The host's replies wake the app from its client's thread.
        cx.executor().allow_parking();
        let (view, cx, card) = show_card(cx, bridge, snapshot, item);
        let settling_is = |expected: Settling, cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| view.read(cx).0.read(cx).settlings.get(&id) == Some(&expected))
        };

        // Dismiss sits left of Answer.
        let dismiss = card.bottom_right() - gpui::point(gpui::px(110.), gpui::px(24.));
        cx.simulate_click(dismiss, gpui::Modifiers::default());
        let sending = || Settling::Sending { shown_open: false };
        assert!(
            settling_is(sending(), cx),
            "sending in the frame of the click"
        );
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(settling_is(sending(), cx), "the host holds the reply");

        held.store(false, Ordering::Relaxed);
        let answered = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !settling_is(Settling::Sent, cx) {
            assert!(
                std::time::Instant::now() < answered,
                "the host never answered"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn approve_on_a_change_of_base_card_sends_its_answer(cx: &mut gpui::TestAppContext) {
        assert!(sends_an_answer(cx, 40.));
    }

    #[gpui::test]
    fn deny_on_a_change_of_base_card_sends_its_answer(cx: &mut gpui::TestAppContext) {
        // Deny sits left of Approve once.
        assert!(sends_an_answer(cx, 150.));
    }
}
