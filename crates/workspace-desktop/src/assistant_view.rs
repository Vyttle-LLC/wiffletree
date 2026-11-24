//! The Assistant (OpenSpec change `assistant-does-the-work`, tasks 3.1–3.5 and screens 1–3 of
//! its mockup): one chat across every Space with no tabs, the cards its tools put in it, a
//! composer whose Space is the Assistant's to infer until the human picks one, and the rail of
//! what needs the human, what runs, the loops and what finished. History keeps the threads from
//! before, read-only; the Loops page is in `jobs`. An issue thread still docks beside the
//! Issues page.
mod cards;
mod issue_threads;
pub(super) mod jobs;
mod rail;
pub(crate) mod work;

use super::*;
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::ButtonVariants,
    input::{Input, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    text::TextView,
};
use spaces_view::{space_color, space_mark};
use ui::{card, hint, icon};
use workspace_core::assistant::conversation::{ConversationCard, ConversationState, Route};
use workspace_core::assistant::{
    CardKind, DEFAULT_NAME, Thread, ThreadCard, ThreadHit, ThreadTab, is_notice,
};

/// What the composer says the Assistant may do, as the mockup words it.
const AUTHORITY: &str = "Can act on this Mac · repositories change through work";
const GOES_TO: &str = "The Assistant passes a follow-up to the work it is about and tags your message; Change on the tag moves it.";

/// The assistant's name as the snapshot carries it; "Assistant" until the user names it.
pub(super) fn assistant_name(snapshot: Option<&Snapshot>) -> String {
    match snapshot.map(|s| s.assistant_name.trim()) {
        Some(name) if !name.is_empty() => name.into(),
        _ => DEFAULT_NAME.into(),
    }
}
fn composer_placeholder(name: &str) -> String {
    format!("Ask {name}, or tell it what to do…")
}
/// Whether any issue thread is in a turn, for the sidebar row's working mark.
pub(super) fn any_working(tabs: &[ThreadTab]) -> bool {
    tabs.iter().any(|tab| tab.status == Status::Working)
}
/// The composer's Space once a message sent with `sent` arrived: none for the next message,
/// unless the human picked another while it was sending.
fn space_after_send(chosen: Option<SpaceId>, sent: &Option<SpaceId>) -> Option<SpaceId> {
    chosen.filter(|chosen| Some(chosen) != sent.as_ref())
}
/// A saved output's line on its card.
fn saved_line(title: &str, version: u32) -> String {
    match version {
        1 => format!("Saved “{title}” to Boards · Only on this Mac"),
        n => format!("Saved “{title}” to Boards · Only on this Mac · version {n}"),
    }
}
/// "Working 30 min", as the chat and the sidebar row note a long turn.
pub(crate) fn long_turn_text(minutes: i64) -> String {
    format!("Working {}", jobs::elapsed(minutes))
}

/// One entry of the chat, in the order it happened.
#[derive(Clone, Copy, Debug)]
enum Entry<'a> {
    Message(&'a Message),
    Card(&'a ConversationCard),
}
/// The conversation's messages and cards in the order they were made, a message first when
/// both were made at once.
fn timeline<'a>(messages: &'a [Message], cards: &'a [ConversationCard]) -> Vec<Entry<'a>> {
    let mut entries: Vec<(i64, usize, Entry)> = (messages.iter())
        .map(|m| (m.created_at, 0, Entry::Message(m)))
        .chain(cards.iter().map(|c| (c.created_at, 1, Entry::Card(c))))
        .collect();
    entries.sort_by_key(|(at, kind, _)| (*at, *kind));
    entries.into_iter().map(|(_, _, entry)| entry).collect()
}

/// A search History asked for. Its reply shows only while History still asks for it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchAsked {
    query: String,
    space: Option<SpaceId>,
}

/// What the Assistant shows beside its rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Chat,
    History,
}

pub(super) struct AssistantPage {
    bridge: Bridge,
    shared: Shared,
    pane: Pane,
    /// The Space picked with the composer's chip for the next message; `None` lets the
    /// Assistant infer it.
    chosen_space: Option<SpaceId>,
    composer: Entity<TextareaState>,
    /// The human's unsent draft was read into the composer.
    draft_read: bool,
    sending: bool,
    /// The assistant's name the composer's placeholder uses.
    named: String,
    /// The conversation's messages, and the session and revision they were read at.
    chat: Vec<Message>,
    chat_read: Option<(String, u64)>,
    chat_scroll: ScrollHandle,
    /// How many entries the chat showed, so a new one scrolls it to the end.
    chat_shown: usize,
    /// The card the rail asked the chat to show.
    scroll_to_card: Option<String>,
    /// The decision the human answers in their own words, and the words.
    answering: Option<String>,
    answer: Entity<InputState>,
    /// The issue thread docked beside the Issues page, which Ask the assistant opens.
    docked: Option<String>,
    open: Option<String>,
    /// The docked thread's messages, and the thread and revision they were read at.
    messages: Vec<Message>,
    read: Option<(String, u64)>,
    /// History: its search, what it found, and the thread opened from it with its messages.
    search: Entity<InputState>,
    hits: Option<Vec<ThreadHit>>,
    search_space: Option<SpaceId>,
    history_open: Option<(Thread, Vec<Message>)>,
    /// The Loops page, a loop's page or its form, shown in place of the chat.
    jobs_view: Option<jobs::JobsView>,
    /// Whether the app opens at login, read when the Loops page first opens.
    login: Option<bool>,
    /// The docked issue thread's pending proposals as the host read them, and the thread and
    /// proposals they were read for.
    proposal_views: Vec<workspace_core::assistant::proposal::ProposalView>,
    proposals_read: Option<(String, usize)>,
    /// Who wrote the docked issue thread's issue text, by thread, for its notice.
    authors: Option<(String, Vec<String>)>,
    /// A proposal the human is editing before applying it.
    editing: Option<issue_threads::ProposalEdit>,
}
impl EventEmitter<Intent> for AssistantPage {}
impl AssistantPage {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 9)
                .placeholder(composer_placeholder(DEFAULT_NAME))
        });
        cx.subscribe(&composer, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search history"));
        cx.subscribe_in(&search, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.run_search(window, cx);
            }
        })
        .detach();
        let answer = cx.new(|cx| InputState::new(window, cx).placeholder("Your answer"));
        cx.subscribe_in(&answer, window, |view, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                view.send_answer(window, cx);
            }
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            pane: Pane::Chat,
            chosen_space: None,
            composer,
            draft_read: false,
            sending: false,
            named: DEFAULT_NAME.into(),
            chat: vec![],
            chat_read: None,
            chat_scroll: ScrollHandle::new(),
            chat_shown: 0,
            scroll_to_card: None,
            answering: None,
            answer,
            docked: None,
            open: None,
            messages: vec![],
            read: None,
            search,
            hits: None,
            search_space: None,
            history_open: None,
            jobs_view: None,
            login: None,
            proposal_views: vec![],
            proposals_read: None,
            authors: None,
            editing: None,
        }
    }
    fn tabs(&self) -> &[ThreadTab] {
        self.shared.snapshot().map_or(&[], |s| &s.threads)
    }
    fn spaces(&self) -> &[Space] {
        self.shared.snapshot().map_or(&[], |s| &s.spaces)
    }
    fn conversation_state(&self) -> Option<&ConversationState> {
        self.shared.snapshot()?.conversation.as_ref()
    }
    fn open_tab(&self) -> Option<&ThreadTab> {
        let open = self.open.as_ref()?;
        self.tabs().iter().find(|tab| &tab.thread.id == open)
    }
    fn space_name(&self, id: &SpaceId) -> String {
        (self.spaces().iter())
            .find(|s| &s.id == id)
            .map_or_else(String::new, |s| s.name.clone())
    }
    /// A work item's project, by id.
    fn work_project(&self, id: &str) -> Option<&Project> {
        self.shared.snapshot()?.projects.iter().find(|p| p.id == id)
    }

    /// Takes the shared state and reads the conversation's or the docked thread's messages
    /// again when they changed.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        self.shared = shared;
        let shown = self.shared.page == Page::Assistant || self.is_docked_now();
        if shown {
            // An issue a proposal changes may have changed with this snapshot, by sync or here.
            self.proposals_read = None;
        }
        let name = assistant_name(self.shared.snapshot());
        if name != self.named {
            let placeholder = composer_placeholder(&name);
            self.composer.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
            self.named = name;
        }
        if shown {
            self.read_messages(window, cx);
            self.read_chat(window, cx);
            cx.notify();
        }
    }
    /// Whether the docked issue thread shows now: the Issues page is open beside it.
    fn is_docked_now(&self) -> bool {
        self.shared.page == Page::Issues && self.docked().is_some()
    }
    /// Shows the chat at its end, as opening the Assistant does.
    pub(super) fn opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_shown = 0;
        self.read_chat(window, cx);
        self.read_draft(window, cx);
        cx.notify();
    }
    /// Puts the human's unsent draft, such as what the router held before the upgrade, in an
    /// empty composer, once, so their next message does not drop it unseen.
    fn read_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft_read {
            return;
        }
        self.draft_read = true;
        self.ask(
            ask::conversation_cards(),
            window,
            cx,
            |view, read, window, cx| {
                let draft = match read {
                    Ok(read) => read.draft.filter(|d| !d.trim().is_empty()),
                    Err(_) => {
                        view.draft_read = false;
                        return;
                    }
                };
                if let Some(draft) = draft
                    && view.composer.read(cx).value().trim().is_empty()
                {
                    view.composer
                        .update(cx, |input, cx| input.set_value(draft, window, cx));
                    cx.notify();
                }
            },
        );
    }
    fn read_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.conversation_state() else {
            self.chat.clear();
            self.chat_read = None;
            return;
        };
        let at = (state.session_id.clone(), state.message_revision);
        if self.chat_read.as_ref() == Some(&at) {
            return;
        }
        self.chat_read = Some(at.clone());
        let ask = ask::messages(at.0.clone(), None, MAX_PAGE);
        self.ask(ask, window, cx, move |view, messages, _, cx| {
            if view.chat_read.as_ref() == Some(&at) {
                match messages {
                    Ok(messages) => view.chat = messages,
                    Err(_) => view.chat_read = None,
                }
                cx.notify();
            }
        });
    }
    /// Reads the docked issue thread's messages and proposals when they changed.
    fn read_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_issue_thread(window, cx);
        let Some(tab) = self.open_tab() else {
            self.messages.clear();
            self.read = None;
            return;
        };
        let at = (tab.thread.id.clone(), tab.message_revision);
        if self.read.as_ref() == Some(&at) {
            return;
        }
        self.read = Some(at.clone());
        let ask = ask::messages(at.0.clone(), None, MAX_PAGE);
        self.ask(ask, window, cx, move |view, messages, _, cx| {
            if view.read.as_ref().is_some_and(|(id, _)| *id == at.0) {
                match messages {
                    Ok(messages) => view.messages = messages,
                    Err(_) => view.read = None,
                }
                cx.notify();
            }
        });
    }
    fn select(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.open = id;
        self.read_messages(window, cx);
        cx.notify();
    }
    /// Shows the chat in place of History or the Loops page, at `card` when given.
    fn show_chat(&mut self, card: Option<String>, cx: &mut Context<Self>) {
        self.pane = Pane::Chat;
        self.jobs_view = None;
        self.history_open = None;
        self.scroll_to_card = card;
        cx.notify();
    }
    fn show_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::History;
        self.jobs_view = None;
        self.history_open = None;
        self.run_search(window, cx);
        cx.notify();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).value().to_string();
        if body.trim().is_empty() || self.sending {
            return;
        }
        self.sending = true;
        // A turn the human stopped waits for a resume, which their next message gives.
        let stopped = self
            .conversation_state()
            .filter(|c| c.status == Status::Paused);
        if let Some(state) = stopped {
            let resume = Command::SetStatus {
                session_id: state.session_id.clone(),
                status: Status::Ready,
            };
            self.request(resume, window, cx);
        }
        let picked = self.chosen_space.clone();
        let ask = ask::send_to_assistant(new_id(), body, picked.clone());
        self.ask(ask, window, cx, move |view, sent, window, cx| {
            view.sending = false;
            match sent {
                Ok(_) => {
                    view.chosen_space = space_after_send(view.chosen_space.take(), &picked);
                    view.clear_composer(window, cx);
                }
                // The message is still in the composer, so its Space stays with it.
                Err(error) => toast_error(error, window, cx),
            }
            cx.notify();
        });
    }
    /// Stop on the Assistant's running turn, as an agent's Stop does.
    pub(crate) fn stop_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.conversation_state() else {
            return;
        };
        let stop = Command::SetStatus {
            session_id: state.session_id.clone(),
            status: Status::Paused,
        };
        self.request(stop, window, cx);
    }
    fn clear_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn search_asked(&self, cx: &App) -> SearchAsked {
        SearchAsked {
            query: self.search.read(cx).value().trim().to_string(),
            space: self.search_space.clone(),
        }
    }
    /// Lists History: every earlier thread that is not an issue thread whose title or text
    /// holds the search, in the Space chosen, newest first.
    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = self.search_asked(cx);
        let ask = ask::search_threads(asked.query.clone(), asked.space.clone());
        self.ask(ask, window, cx, move |view, hits, window, cx| {
            // A reply for a query or Space no longer asked for is dropped; the later one wins.
            if view.search_asked(cx) != asked {
                return;
            }
            match hits {
                Ok(hits) => {
                    let history = hits.into_iter().filter(|h| h.thread.is_history());
                    view.hits = Some(history.collect());
                }
                Err(error) => toast_error(error, window, cx),
            }
            cx.notify();
        });
    }
    /// Opens a history thread to read.
    fn open_history(&mut self, thread: Thread, window: &mut Window, cx: &mut Context<Self>) {
        let id = thread.id.clone();
        self.history_open = Some((thread, vec![]));
        self.ask(
            ask::messages(id.clone(), None, MAX_PAGE),
            window,
            cx,
            move |view, messages, window, cx| {
                match (messages, &mut view.history_open) {
                    (Ok(messages), Some((thread, shown))) if thread.id == id => *shown = messages,
                    (Err(error), _) => toast_error(error, window, cx),
                    _ => {}
                }
                cx.notify();
            },
        );
        cx.notify();
    }
    fn confirm_delete(&mut self, thread: &Thread, window: &mut Window, cx: &mut Context<Self>) {
        let (id, title) = (thread.id.clone(), thread.title.clone());
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, id) = (weak.clone(), id.clone());
            dialog
                .title(format!("Delete “{title}”?"))
                .confirm()
                .ok_text("Delete")
                .child("Its conversation and scratch folder are removed from this Mac. What it logged to a brain stays.")
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    let _ = weak.update(cx, |view, cx| {
                        view.ask(ask::delete_thread(id), window, cx, |view, done, window, cx| {
                            match done {
                                Ok(()) => view.show_history(window, cx),
                                Err(error) => toast_error(error, window, cx),
                            }
                        });
                    });
                    true
                })
        });
    }
    fn dismiss(&mut self, card: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.open.clone() else {
            return;
        };
        self.ask(
            ask::dismiss_thread_card(thread, card),
            window,
            cx,
            |_, done, window, cx| {
                if let Err(error) = done {
                    toast_error(error, window, cx);
                }
            },
        );
    }

    /// The bar above the chat: the Assistant's name, History and Loops.
    fn top_bar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let on_chat = self.pane == Pane::Chat && self.jobs_view.is_none();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_5()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(assistant_name(self.shared.snapshot())),
            )
            .when(!on_chat, |d| {
                d.child(
                    button("back-to-chat")
                        .ghost()
                        .xsmall()
                        .label("Back to chat")
                        .on_click(cx.listener(|v, _, _, c| v.show_chat(None, c))),
                )
            })
            .child(
                button("open-history")
                    .ghost()
                    .xsmall()
                    .selected(self.pane == Pane::History && self.jobs_view.is_none())
                    .label("History")
                    .on_click(cx.listener(|v, _, w, c| v.show_history(w, c))),
            )
            .child(
                button("open-loops")
                    .ghost()
                    .xsmall()
                    .selected(self.jobs_view.is_some())
                    .label("Loops")
                    .on_click(cx.listener(|v, _, w, c| v.open_job(None, w, c))),
            )
    }

    fn chat_body(&mut self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.conversation_state().cloned() else {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    div().flex_1().flex().items_center().justify_center().child(hint(
                        format!(
                            "Ask {} anything. It can act on this Mac, and changes to your repositories go through work it runs for you.",
                            assistant_name(self.shared.snapshot())
                        ),
                        p,
                    )),
                )
                .child(self.composer_box(p, cx))
                .into_any_element();
        };
        let entries = timeline(&self.chat, &state.cards);
        let mut column = div()
            .id("assistant-chat")
            .track_scroll(&self.chat_scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .px_5()
            .py_4();
        let mut shown = 0;
        let mut wanted = None;
        for entry in &entries {
            let element = match entry {
                Entry::Message(message) => self.chat_message(message, &state, p, cx),
                Entry::Card(card) => {
                    if self.scroll_to_card.as_deref() == Some(card.id.as_str()) {
                        wanted = Some(shown);
                    }
                    self.chat_card(card, p, cx).into_any_element()
                }
            };
            column = column.child(element);
            shown += 1;
        }
        if let Some(line) = self.turn_line(&state, p, cx) {
            column = column.child(line);
            shown += 1;
        }
        // A card the rail asked for, else the end when something new arrived.
        if let Some(at) = wanted {
            self.chat_scroll.scroll_to_item(at);
            self.scroll_to_card = None;
        } else if shown != self.chat_shown {
            self.chat_scroll.scroll_to_bottom();
        }
        self.chat_shown = shown;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(column)
            .child(self.composer_box(p, cx))
            .into_any_element()
    }

    /// While the Assistant is in a turn: that it is working, with Stop and, past 30 minutes,
    /// how long; once the human stopped it, that their next message starts it again.
    fn turn_line(
        &self,
        state: &ConversationState,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let name = assistant_name(self.shared.snapshot());
        let line = div().flex().items_center().gap_2();
        match state.status {
            Status::Working => {
                let text = match work::long_turn_minutes(state, ui::now()) {
                    Some(minutes) => long_turn_text(minutes),
                    None => format!("{name} is working…"),
                };
                Some(
                    line.child(ui::Spinner::new().xsmall().color(p.accent))
                        .child(hint(text, p))
                        .child(
                            button("stop-assistant-turn")
                                .ghost()
                                .xsmall()
                                .label("Stop")
                                .on_click(cx.listener(|v, _, w, c| v.stop_turn(w, c))),
                        ),
                )
            }
            Status::Paused => {
                Some(line.child(hint("Stopped. Your next message starts it again.", p)))
            }
            _ => None,
        }
    }

    /// One message of the chat: the human's on the right with its routing tag, the
    /// Assistant's reply, or a line for what work or Wiffletree told it.
    fn chat_message(
        &self,
        message: &Message,
        state: &ConversationState,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body: SharedString = message.body.clone().into();
        if message.from_human() {
            let route = state.routes.iter().find(|r| r.message_id == message.id);
            return div()
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .child(Self::bubble(&message.body, p))
                .children(route.map(|route| self.route_tag(route, p, cx)))
                .into_any_element();
        }
        if message.sender.as_deref() == Some(state.session_id.as_str()) {
            if is_notice(&message.id) {
                return hint(body, p).text_color(p.red).into_any_element();
            }
            return div()
                .max_w(relative(0.88))
                .flex()
                .flex_col()
                .gap_1()
                .child(hint(assistant_name(self.shared.snapshot()), p))
                .child(
                    TextView::markdown(SharedString::from(format!("reply-{}", message.id)), body)
                        .selectable(true)
                        .text_size(px(font::BODY)),
                )
                .into_any_element();
        }
        // A report from work or a notice from Wiffletree: context the Assistant read.
        let from = (message.sender.as_deref())
            .and_then(|sender| self.shared.session(sender))
            .and_then(|session| self.work_project(&session.project_id))
            .map_or("Wiffletree".to_owned(), |project| project.name.clone());
        let first = message.body.lines().next().unwrap_or_default().to_owned();
        div()
            .text_size(px(font::META))
            .text_color(p.subtle)
            .px_2()
            .whitespace_nowrap()
            .overflow_hidden()
            .text_ellipsis()
            .child(format!("From {from}: {first}"))
            .into_any_element()
    }
    fn bubble(body: &str, p: Palette) -> Div {
        div()
            .max_w(relative(0.85))
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(p.overlay)
            .child(body.to_owned())
    }
    /// "→ ‹work title› · Change" under a message the Assistant passed to its work.
    fn route_tag(&self, route: &Route, p: Palette, cx: &mut Context<Self>) -> Div {
        let to = match &route.project_id {
            Some(id) => self
                .work_project(id)
                .map_or("its work".to_owned(), |project| project.name.clone()),
            None => "Just the Assistant".into(),
        };
        let running: Vec<(String, String)> = (self.shared.index.as_deref())
            .map(|index| {
                (work::work_items(index).into_iter())
                    .filter(|item| item.running())
                    .map(|item| (item.project.id.clone(), item.project.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let (message, current) = (route.message_id.clone(), route.project_id.clone());
        let weak = cx.weak_entity();
        let change = button(SharedString::from(format!("change-route-{message}")))
            .ghost()
            .xsmall()
            .label("Change")
            .dropdown_menu(move |mut menu, _, _| {
                let choices = (running.iter())
                    .map(|(id, name)| (Some(id.clone()), name.clone()))
                    .chain([(None, "Just the Assistant".to_owned())]);
                for (to, name) in choices {
                    let (weak, message) = (weak.clone(), message.clone());
                    let checked = to == current;
                    menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(
                        move |_, window, cx| {
                            let ask = ask::move_routed_message(message.clone(), to.clone());
                            let _ = weak.update(cx, |view, cx| {
                                view.ask(ask, window, cx, |_, moved, window, cx| {
                                    if let Err(error) = moved {
                                        toast_error(error, window, cx);
                                    }
                                });
                            });
                        },
                    ));
                }
                menu
            });
        div()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(font::META))
            .text_color(p.subtle)
            .child(format!("→ {to}"))
            .child(change)
    }

    /// The conversation's model: once it started, a menu of its provider's models, whose choice
    /// applies from the next turn; before, this Mac's Assistant row, which its first turn uses.
    fn model_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.conversation_state();
        let current = (state.and_then(|c| c.profile.clone())).or_else(|| {
            let selection = &self.shared.snapshot()?.model_selection;
            models::thread_model(selection, None, &self.shared.detection)
        });
        let label = current
            .as_ref()
            .map_or("Who runs what".into(), |m| m.short());
        let (Some(current), Some(state)) = (current, state) else {
            return button("assistant-model")
                .xsmall()
                .label(label)
                .tooltip("Your first message starts on this Mac's Assistant row")
                .into_any_element();
        };
        let selection = self.shared.snapshot().map(|s| s.model_selection.clone());
        let choices = selection.map_or(vec![], |selection| {
            models::profile_choices(&self.shared.catalog, &selection, current.provider)
        });
        let (weak, session_id) = (cx.weak_entity(), state.session_id.clone());
        // The host takes the choice at once, even during a turn, and the next turn starts on it.
        button("assistant-model")
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .tooltip("Applies from the Assistant's next turn")
            .dropdown_menu(move |mut menu, _, _| {
                for profile in &choices {
                    let (weak, session_id) = (weak.clone(), session_id.clone());
                    let configure = Command::ConfigureSession {
                        session_id,
                        profile: profile.clone(),
                    };
                    menu = menu.item(
                        PopupMenuItem::new(profile.short())
                            .checked(*profile == current)
                            .on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |view, cx| {
                                    view.request(configure.clone(), window, cx)
                                });
                            }),
                    );
                }
                menu.scrollable(true).max_h(px(320.))
            })
            .into_any_element()
    }
    fn composer_box(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let spaces = self.spaces().to_vec();
        let chosen = self.chosen_space.clone();
        let chip_label = match &chosen {
            Some(id) => self.space_name(id),
            None => "Space · auto".into(),
        };
        let weak = cx.weak_entity();
        // Auto lets the Assistant infer the Space; one picked by hand binds the next message.
        let space_chip = button("composer-space")
            .xsmall()
            .when_some(chosen.clone(), |b, id| {
                b.child(space_mark(space_color(&spaces, &id, p)))
            })
            .label(chip_label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let choices = std::iter::once((None, "Auto".to_owned()))
                    .chain(spaces.iter().map(|s| (Some(s.id.clone()), s.name.clone())));
                for (space, name) in choices {
                    let (weak, checked) = (weak.clone(), space == chosen);
                    menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(
                        move |_, _, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.chosen_space = space.clone();
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            });
        let goes_to = div()
            .id("composer-goes-to")
            .px_2()
            .rounded(px(7.))
            .border_1()
            .border_color(p.edge)
            .child("Goes to · auto")
            .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new(GOES_TO).build(window, cx));
        let model = self.model_chip(cx);
        let empty = self.composer.read(cx).value().trim().is_empty();
        div()
            .flex_none()
            .m_4()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(p.edge)
            .bg(p.surface)
            .key_context("ChatComposer")
            .on_action(cx.listener(|v, _: &SendMessage, w, c| v.send(w, c)))
            .child(
                Textarea::new(&self.composer)
                    .w_full()
                    .appearance(false)
                    .bordered(false),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(font::CAPTION))
                    .text_color(p.subtle)
                    .child(space_chip)
                    .child(goes_to)
                    .child(model)
                    .child(AUTHORITY)
                    .child(div().flex_1())
                    .child(
                        button("send-to-assistant")
                            .primary()
                            .xsmall()
                            .icon(icon("arrow-up"))
                            .disabled(empty || self.sending)
                            .on_click(cx.listener(|v, _, w, c| v.send(w, c))),
                    ),
            )
    }

    /// History: the threads from before one conversation, read-only, searchable by title and
    /// text and filterable by Space.
    fn history_body(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if let Some((thread, messages)) = &self.history_open {
            return self
                .history_thread(thread, messages, p, cx)
                .into_any_element();
        }
        let spaces = self.spaces().to_vec();
        let mut list = div().flex().flex_col().gap_1();
        match &self.hits {
            None => {}
            Some(hits) if hits.is_empty() => {
                let empty = match self.search.read(cx).value().trim().is_empty() {
                    true => "No earlier threads on this Mac.",
                    false => "No earlier thread has that in its title or messages.",
                };
                list = list.child(hint(empty, p));
            }
            Some(hits) => {
                for hit in hits {
                    let thread = hit.thread.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("history-{}", thread.id)))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|d| d.bg(p.overlay))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(space_mark(space_color(&spaces, &thread.space_id, p)))
                                    .child(thread.title.clone())
                                    .child(hint(self.space_name(&thread.space_id), p))
                                    .child(div().flex_1())
                                    .child(hint(wording::day_and_time(thread.last_message_at), p)),
                            )
                            .children(hit.excerpt.clone().map(|line| hint(line, p)))
                            .on_click(
                                cx.listener(move |v, _, w, c| v.open_history(thread.clone(), w, c)),
                            ),
                    );
                }
            }
        }
        div()
            .id("history")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px_5()
                    .py_4()
                    .child(
                        div()
                            .text_size(px(font::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("History"),
                    )
                    .child(hint(
                        "Threads from before the one conversation. They stay on this Mac, read-only.",
                        p,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().w(px(260.)).child(Input::new(&self.search).small().cleanable(true)))
                            .child(self.search_space_menu(cx)),
                    )
                    .child(list),
            )
            .into_any_element()
    }
    /// Limits History to one Space, or shows them all.
    fn search_space_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let spaces = self.spaces().to_vec();
        let current = self.search_space.clone();
        let label = (spaces.iter())
            .find(|s| Some(&s.id) == current.as_ref())
            .map_or("All Spaces".into(), |s| s.name.clone());
        let weak = cx.weak_entity();
        button("search-space")
            .ghost()
            .small()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let choices = std::iter::once((None, "All Spaces".to_owned()))
                    .chain(spaces.iter().map(|s| (Some(s.id.clone()), s.name.clone())));
                for (space, name) in choices {
                    let (weak, checked) = (weak.clone(), space == current);
                    menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(
                        move |_, window, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.search_space = space.clone();
                                view.run_search(window, cx);
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            })
    }
    /// A history thread, read-only, with Delete.
    fn history_thread(
        &self,
        thread: &Thread,
        messages: &[Message],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let deleted = thread.clone();
        let mut column = div().flex().flex_col().gap_3().px_5().py_4();
        column = column.children(
            messages
                .iter()
                .map(|message| self.thread_message(message, &thread.space_id, p)),
        );
        for card in &thread.cards {
            column = column.child(self.thread_card(card, true, p, cx));
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_5()
                    .py_2()
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::LIGHT))
                    .child(
                        button("history-back")
                            .ghost()
                            .xsmall()
                            .label("‹ History")
                            .on_click(cx.listener(|v, _, w, c| v.show_history(w, c))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(thread.title.clone()),
                            )
                            .child(hint(
                                format!("{} · Read-only", self.space_name(&thread.space_id)),
                                p,
                            )),
                    )
                    .child(
                        button("delete-history-thread")
                            .ghost()
                            .xsmall()
                            .icon(icon("archive"))
                            .label("Delete")
                            .on_click(
                                cx.listener(move |v, _, w, c| v.confirm_delete(&deleted, w, c)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("history-thread")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(column),
            )
    }

    /// A message of a thread: the human's in a bubble, else the thread's reply.
    fn thread_message(&self, message: &Message, space: &SpaceId, p: Palette) -> Div {
        let body: SharedString = message.body.clone().into();
        match (message.sender.is_none(), is_notice(&message.id)) {
            (true, _) => div()
                .flex()
                .justify_end()
                .child(Self::bubble(&message.body, p)),
            (false, true) => hint(body, p).text_color(p.red),
            (false, false) => div()
                .max_w(relative(0.9))
                .flex()
                .flex_col()
                .gap_1()
                .child(hint(
                    format!(
                        "{} · {}",
                        assistant_name(self.shared.snapshot()),
                        self.space_name(space)
                    ),
                    p,
                ))
                .child(
                    TextView::markdown(SharedString::from(format!("reply-{}", message.id)), body)
                        .selectable(true)
                        .text_size(px(font::BODY)),
                ),
        }
    }
    /// The docked issue thread's conversation: its notice, messages, cards and proposals.
    fn thread_conversation(&self, tab: &ThreadTab, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut column = div().flex().flex_col().gap_3().px_5().py_4();
        column = column.children(self.authors_notice(tab, p));
        column = column.children(
            self.messages
                .iter()
                .map(|message| self.thread_message(message, &tab.thread.space_id, p)),
        );
        if tab.status == Status::Working {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::Spinner::new().xsmall().color(p.accent))
                    .child(hint(
                        format!("{} is working…", assistant_name(self.shared.snapshot())),
                        p,
                    )),
            );
        }
        for card in &tab.thread.cards {
            column = column.child(self.thread_card(card, false, p, cx));
        }
        for proposal in &tab.thread.proposals {
            column = column.child(self.proposal_card(proposal, p, cx));
        }
        column
    }
    /// A card a thread's tool showed; in History, only what it said.
    fn thread_card(
        &self,
        thread_card: &ThreadCard,
        read_only: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = thread_card.id.clone();
        let hide = (!read_only).then(|| {
            let id = id.clone();
            button(SharedString::from(format!("dismiss-{id}")))
                .ghost()
                .xsmall()
                .label("Hide")
                .on_click(cx.listener(move |v, _, w, c| v.dismiss(id.clone(), w, c)))
        });
        match &thread_card.kind {
            CardKind::SuggestSpace { name, description } => {
                let (name, description) = (name.clone(), description.clone());
                card(p)
                    .gap_2()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("A new Space: {name}")),
                    )
                    .child(hint(description.clone(), p))
                    .child(
                        div().flex().gap_2().justify_end().children(hide).child(
                            button(SharedString::from(format!("suggested-{id}")))
                                .xsmall()
                                .label("Open Spaces")
                                .on_click(cx.listener(move |_, _, _, c| {
                                    c.emit(Intent::SuggestSpace {
                                        name: name.clone(),
                                        description: description.clone(),
                                    })
                                })),
                        ),
                    )
            }
            CardKind::ProjectBrief { name, brief } => card(p)
                .gap_2()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("A brief for a project: {name}")),
                )
                .child(hint(brief.clone(), p)),
            CardKind::SavedOutput { title, version, .. } => card(p)
                .flex_row()
                .items_center()
                .gap_2()
                .child(icon("file-text").size(px(14.)))
                .child(div().flex_1().child(saved_line(title, *version)))
                .children(hide),
            CardKind::Recipe { recipe, preview } => card(p)
                .gap_1()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("A loop it proposed: {}", recipe.name)),
                )
                .child(hint(format!("{} · never saved", preview.schedule), p)),
        }
    }
    /// The docked issue thread's composer, which sends to it alone.
    fn thread_composer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let empty = self.composer.read(cx).value().trim().is_empty();
        div()
            .flex_none()
            .m_4()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(p.edge)
            .bg(p.surface)
            .key_context("ChatComposer")
            .on_action(cx.listener(|v, _: &SendMessage, w, c| v.send_to_thread(w, c)))
            .child(
                Textarea::new(&self.composer)
                    .w_full()
                    .appearance(false)
                    .bordered(false),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(font::CAPTION))
                    .text_color(p.subtle)
                    .child("Goes to this thread only")
                    .child(div().flex_1())
                    .child(
                        button("send-to-thread")
                            .primary()
                            .xsmall()
                            .icon(icon("arrow-up"))
                            .disabled(empty || self.sending)
                            .on_click(cx.listener(|v, _, w, c| v.send_to_thread(w, c))),
                    ),
            )
    }
    fn send_to_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).value().to_string();
        let Some(thread) = self.open.clone() else {
            return;
        };
        if body.trim().is_empty() || self.sending {
            return;
        }
        self.sending = true;
        let send = Command::Send {
            id: new_id(),
            sender: None,
            recipient: thread,
            body,
            attachments: vec![],
            uploads: vec![],
        };
        self.request(send, window, cx);
    }
}
impl Requests for AssistantPage {
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
        match (command, reply) {
            (Command::Send { .. }, reply) => {
                self.sending = false;
                match reply {
                    Ok(_) => self.clear_composer(window, cx),
                    Err(error) => toast_error(error, window, cx),
                }
            }
            (_, Err(error)) => toast_error(error, window, cx),
            _ => {}
        }
    }
}
impl Render for AssistantPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        if self.is_docked_now()
            && let Some(tab) = self.open_tab().cloned()
        {
            return self.docked_body(&tab, p, cx);
        }
        let body = match (&self.jobs_view, self.pane) {
            (Some(view), _) => self.jobs_body(view, p, cx).into_any_element(),
            (None, Pane::History) => self.history_body(p, cx),
            (None, Pane::Chat) => self.chat_body(p, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(self.top_bar(p, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(div().flex_1().min_w_0().flex().flex_col().child(body))
                    .child(self.rail(p, cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Entry, assistant_name, composer_placeholder, long_turn_text, saved_line, space_after_send,
        timeline,
    };
    use workspace_core::assistant::conversation::{CardKind, ConversationCard};
    use workspace_core::{Message, Snapshot, SpaceId};

    #[test]
    fn the_sidebar_row_names_the_assistant() {
        assert_eq!(assistant_name(None), "Assistant");
        let named = Snapshot {
            assistant_name: "Wren".into(),
            ..Default::default()
        };
        assert_eq!(assistant_name(Some(&named)), "Wren");
        assert_eq!(assistant_name(Some(&Snapshot::default())), "Assistant");
        assert_eq!(
            composer_placeholder("Wren"),
            "Ask Wren, or tell it what to do…"
        );
    }

    fn message(id: &str, at: i64) -> Message {
        serde_json::from_value(serde_json::json!({
            "sequence": 0, "id": id, "project_id": "", "sender": null, "recipient": "a",
            "body": "", "receipt": "delivered", "created_at": at,
        }))
        .unwrap()
    }

    #[test]
    fn the_chat_shows_messages_and_cards_in_the_order_they_were_made() {
        let card = |id: &str, at| ConversationCard {
            id: id.into(),
            created_at: at,
            read: vec![],
            kind: CardKind::SuggestSpace {
                name: id.into(),
                description: String::new(),
            },
        };
        let messages = [message("ask", 1), message("reply", 5)];
        let cards = [card("work", 3), card("same", 5)];
        let order: Vec<&str> = timeline(&messages, &cards)
            .iter()
            .map(|entry| match entry {
                Entry::Message(m) => m.id.as_str(),
                Entry::Card(c) => c.id.as_str(),
            })
            .collect();
        assert_eq!(order, ["ask", "work", "reply", "same"]);
    }

    #[test]
    fn a_picked_space_is_kept_until_its_message_is_sent() {
        let (home, work) = (Some(SpaceId("home".into())), Some(SpaceId("work".into())));
        assert_eq!(space_after_send(home.clone(), &home), None);
        assert_eq!(space_after_send(work.clone(), &home), work);
        assert_eq!(space_after_send(None, &home), None);
    }

    #[test]
    fn a_long_turn_and_a_saved_output_are_worded_for_the_human() {
        assert_eq!(long_turn_text(30), "Working 30 min");
        assert_eq!(long_turn_text(90), "Working 1 h 30 min");
        assert_eq!(
            saved_line("AWS costs", 1),
            "Saved “AWS costs” to Boards · Only on this Mac"
        );
        assert!(saved_line("AWS costs", 2).ends_with("· version 2"));
    }
}
