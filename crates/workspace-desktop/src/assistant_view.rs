//! The assistant (OpenSpec change `assistant-spaces-jobs-boards`, screen 1 of its mockup): one
//! view of thread tabs, each a small read-only provider session in a Space, with search, the
//! cards a thread's tools show, and a composer whose Space chip is the router's until the user
//! picks one by hand. Its jobs are in the `jobs` view.
mod issue_threads;
pub(super) mod jobs;
mod routing;

use super::*;
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    text::TextView,
};
use spaces_view::{space_color, space_mark};
use ui::{card, hint, icon};
use workspace_core::assistant::{
    CardKind, DEFAULT_NAME, ThreadCard, ThreadHit, ThreadTab, is_notice,
};

/// What the composer says about a thread's authority, as the mockup words it.
const AUTHORITY: &str = "Read-only · scratch folder";

/// The assistant's name as the snapshot carries it; "Assistant" until the user names it.
pub(super) fn assistant_name(snapshot: Option<&Snapshot>) -> String {
    match snapshot.map(|s| s.assistant_name.trim()) {
        Some(name) if !name.is_empty() => name.into(),
        _ => DEFAULT_NAME.into(),
    }
}
fn composer_placeholder(name: &str) -> String {
    format!("Ask {name}…")
}
/// Whether any thread is in a turn, for the sidebar row's working mark.
pub(super) fn any_working(tabs: &[ThreadTab]) -> bool {
    tabs.iter().any(|tab| tab.status == Status::Working)
}
/// How a thread's model was chosen, as its header says it.
fn chosen_line(selection: Option<&Selection>) -> Option<String> {
    let selection = selection?;
    let who = match selection.chosen_by {
        Chooser::Human => return Some(selection.reason.clone()),
        Chooser::Wiffletree | Chooser::Default | Chooser::Coordinator { .. } => "Wiffletree",
    };
    Some(format!("Chosen by {who}: {}", selection.reason))
}
/// The strip's order after `dragged` is dropped onto `onto`, whose place it takes.
fn reordered(order: &[String], dragged: &str, onto: &str) -> Vec<String> {
    let place = order.iter().position(|id| id == onto);
    let mut order: Vec<String> = order.iter().filter(|id| *id != dragged).cloned().collect();
    let at = place.unwrap_or(order.len()).min(order.len());
    order.insert(at, dragged.into());
    order
}
/// The tab to show once `closed` leaves the strip: the one after it, else the one before.
fn after_closing(tabs: &[ThreadTab], closed: &str) -> Option<String> {
    let at = tabs.iter().position(|tab| tab.thread.id == closed)?;
    let next = tabs
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|i| tabs.get(i)));
    next.map(|tab| tab.thread.id.clone())
}
/// A saved output's line on its card.
fn saved_line(title: &str, version: u32) -> String {
    match version {
        1 => format!("Saved “{title}” to Boards · Only on this Mac"),
        n => format!("Saved “{title}” to Boards · Only on this Mac · version {n}"),
    }
}

/// A search the box asked for. Its reply shows only while the box still asks for it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchAsked {
    query: String,
    space: Option<SpaceId>,
}

/// A tab being dragged to a new place in the strip.
#[derive(Clone)]
struct TabDrag {
    id: String,
    title: SharedString,
}
impl Render for TabDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(p.surface)
            .border_1()
            .border_color(p.edge)
            .text_size(px(font::CAPTION))
            .child(self.title.clone())
    }
}

/// Make this a project's brief while the user edits it.
struct BriefForm {
    card: String,
    name: Entity<InputState>,
    brief: Entity<TextareaState>,
    /// The scratch files, each ticked to be copied into the project.
    files: Vec<(String, bool)>,
    busy: bool,
}

pub(super) struct AssistantPage {
    bridge: Bridge,
    shared: Shared,
    /// The tab shown: a thread, or with `None` a new tab.
    open: Option<String>,
    /// The Space picked with the composer's chip for the next message; `None` lets the router
    /// choose.
    chosen_space: Option<SpaceId>,
    /// A message sent from a new tab, whose thread the view opens once it has one.
    following: Option<String>,
    composer: Entity<TextareaState>,
    /// The open thread's messages, and the thread and revision they were read at.
    messages: Vec<Message>,
    read: Option<(String, u64)>,
    search: Entity<InputState>,
    /// What the search box found, while it holds a query.
    hits: Option<Vec<ThreadHit>>,
    /// The Space search is limited to; `None` searches every Space.
    search_space: Option<SpaceId>,
    title: Entity<InputState>,
    renaming: bool,
    brief: Option<BriefForm>,
    sending: bool,
    /// The assistant's name the composer's placeholder uses.
    named: String,
    /// The Jobs list, a job's page or its form, shown in place of a thread.
    jobs_view: Option<jobs::JobsView>,
    /// Whether the app opens at login, read when the Jobs list first opens.
    login: Option<bool>,
    /// The issue thread docked beside the Issues page, which Ask the assistant opens.
    docked: Option<String>,
    /// The open issue thread's pending proposals as the host read them, and the thread and
    /// proposals they were read for.
    proposal_views: Vec<workspace_core::assistant::proposal::ProposalView>,
    proposals_read: Option<(String, usize)>,
    /// Who wrote the open issue thread's issue text, by thread, for its notice.
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search threads"));
        cx.subscribe_in(&search, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.run_search(window, cx);
            }
        })
        .detach();
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Thread title"));
        cx.subscribe_in(&title, window, |view, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => view.save_title(window, cx),
            InputEvent::Blur => {
                view.renaming = false;
                cx.notify();
            }
            _ => {}
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            open: None,
            chosen_space: None,
            following: None,
            composer,
            messages: vec![],
            read: None,
            search,
            hits: None,
            search_space: None,
            title,
            renaming: false,
            brief: None,
            sending: false,
            named: DEFAULT_NAME.into(),
            jobs_view: None,
            login: None,
            docked: None,
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
    fn open_tab(&self) -> Option<&ThreadTab> {
        let open = self.open.as_ref()?;
        self.tabs().iter().find(|tab| &tab.thread.id == open)
    }
    /// The Space the composer's chip shows: the one picked by hand, else the open thread's,
    /// else the first one on.
    fn composer_space(&self) -> Option<SpaceId> {
        let on = |id: &SpaceId| {
            let locals = self.shared.snapshot().map(|s| &s.space_locals);
            locals.and_then(|l| l.get(id)).is_none_or(|l| l.enabled)
        };
        let open = self.open_tab().map(|tab| tab.thread.space_id.clone());
        // `on` borrows only `self`, so it is `Copy` and passes by value twice.
        (self.chosen_space.clone().filter(on))
            .or(open)
            .or_else(|| self.spaces().iter().map(|s| s.id.clone()).find(on))
    }

    /// Takes the shared state, follows the open tab out of the strip, and reads the open
    /// thread's messages again when they changed.
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
        if self.open.is_some() && self.open_tab().is_none() {
            self.open = self.tabs().first().map(|tab| tab.thread.id.clone());
        }
        if let Some(sent) = &self.following
            && let Some(found) = routing::thread_of(self.tabs(), sent)
        {
            self.following = None;
            self.open = Some(found);
            self.read = None;
        }
        if shown {
            self.read_messages(window, cx);
            cx.notify();
        }
    }
    /// Whether the docked issue thread shows now: the Issues page is open beside it.
    fn is_docked_now(&self) -> bool {
        self.shared.page == Page::Issues && self.docked().is_some()
    }
    /// Shows the first tab, or a new tab when there is none, as opening the assistant does.
    pub(super) fn opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_tab().is_none() {
            self.open = self.tabs().first().map(|tab| tab.thread.id.clone());
        }
        self.read_messages(window, cx);
        cx.notify();
    }
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
        self.renaming = false;
        self.brief = None;
        self.jobs_view = None;
        self.read_messages(window, cx);
        cx.notify();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).value().to_string();
        if body.trim().is_empty() || self.sending {
            return;
        }
        let read_only = self
            .open_tab()
            .is_some_and(|t| t.thread.project_id.is_some());
        if read_only {
            return;
        }
        self.sending = true;
        // An issue thread takes its messages without the router.
        let issue_thread = (self.open_tab())
            .filter(|tab| tab.thread.attached.is_some())
            .map(|tab| tab.thread.id.clone());
        if let Some(thread) = issue_thread {
            self.chosen_space = None;
            let send = Command::Send {
                id: new_id(),
                sender: None,
                recipient: thread,
                body,
                attachments: vec![],
                uploads: vec![],
            };
            self.request(send, window, cx);
            return;
        }
        let open_space = self.open_tab().map(|tab| tab.thread.space_id.clone());
        // A Space picked by hand skips the router: its thread, or a new one there.
        match (self.chosen_space.take(), self.open.clone()) {
            (Some(space), Some(thread)) if Some(&space) == open_space.as_ref() => {
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
            (Some(space), _) => {
                let ask = ask::start_thread(space, body, None);
                self.ask(ask, window, cx, |view, thread, window, cx| {
                    view.sending = false;
                    match thread {
                        Ok(thread) => {
                            view.clear_composer(window, cx);
                            view.select(Some(thread.id), window, cx);
                        }
                        Err(error) => toast_error(error, window, cx),
                    }
                });
            }
            (None, open) => {
                let id = new_id();
                if open.is_none() {
                    self.following = Some(id.clone());
                }
                let ask = ask::route_message(id, open, body);
                self.ask(ask, window, cx, |view, routed, window, cx| {
                    view.sending = false;
                    match routed {
                        Ok(()) => view.clear_composer(window, cx),
                        Err(error) => {
                            view.following = None;
                            toast_error(error, window, cx)
                        }
                    }
                });
            }
        }
    }
    fn clear_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |input, cx| input.set_value("", window, cx));
    }
    fn close_tab(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.as_ref() == Some(&id) {
            let next = after_closing(self.tabs(), &id);
            self.select(next, window, cx);
        }
        self.ask(
            ask::set_thread_open(id, false),
            window,
            cx,
            |_, done, window, cx| {
                if let Err(error) = done {
                    toast_error(error, window, cx);
                }
            },
        );
    }
    fn drop_tab(
        &mut self,
        dragged: &TabDrag,
        onto: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if dragged.id == onto {
            return;
        }
        let order: Vec<String> = self.tabs().iter().map(|t| t.thread.id.clone()).collect();
        let ask = ask::order_threads(reordered(&order, &dragged.id, onto));
        self.ask(ask, window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    fn search_asked(&self, cx: &App) -> SearchAsked {
        SearchAsked {
            query: self.search.read(cx).value().trim().to_string(),
            space: self.search_space.clone(),
        }
    }
    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = self.search_asked(cx);
        if asked.query.is_empty() {
            self.hits = None;
            return cx.notify();
        }
        let ask = ask::search_threads(asked.query.clone(), asked.space.clone());
        self.ask(ask, window, cx, move |view, hits, window, cx| {
            // A reply for a query or Space no longer asked for is dropped; the later one wins.
            if view.search_asked(cx) != asked {
                return;
            }
            match hits {
                Ok(hits) => view.hits = Some(hits),
                Err(error) => toast_error(error, window, cx),
            }
            cx.notify();
        });
    }
    /// Brings a search result's tab back and shows it.
    fn open_hit(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.hits = None;
        self.ask(
            ask::set_thread_open(id, true),
            window,
            cx,
            |view, thread, window, cx| match thread {
                Ok(thread) => view.select(Some(thread.id), window, cx),
                Err(error) => toast_error(error, window, cx),
            },
        );
    }
    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.open_tab().map(|t| t.thread.title.clone()) else {
            return;
        };
        self.renaming = true;
        self.title.update(cx, |input, cx| {
            input.set_value(title, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }
    fn save_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), title) = (self.open.clone(), self.title.read(cx).value().to_string()) else {
            return;
        };
        self.renaming = false;
        self.ask(
            ask::rename_thread(id, title),
            window,
            cx,
            |_, done, window, cx| {
                if let Err(error) = done {
                    toast_error(error, window, cx);
                }
            },
        );
        cx.notify();
    }
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.open_tab() else {
            return;
        };
        let (id, title) = (tab.thread.id.clone(), tab.thread.title.clone());
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
                        let next = after_closing(view.tabs(), &id);
                        view.ask(ask::delete_thread(id), window, cx, move |view, done, window, cx| {
                            match done {
                                Ok(()) => view.select(next, window, cx),
                                Err(error) => toast_error(error, window, cx),
                            }
                        });
                    });
                    true
                })
        });
    }
    fn draft_brief(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.open.clone() else {
            return;
        };
        self.ask(
            ask::draft_project_brief(id),
            window,
            cx,
            |_, sent, window, cx| {
                if let Err(error) = sent {
                    toast_error(error, window, cx);
                }
            },
        );
    }
    /// Opens a brief card for editing, with the scratch files to tick.
    fn edit_brief(&mut self, card: &ThreadCard, window: &mut Window, cx: &mut Context<Self>) {
        let (CardKind::ProjectBrief { name, brief }, Some(thread)) =
            (&card.kind, self.open.clone())
        else {
            return;
        };
        let name_input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
        let brief_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 14)
                .default_value(brief.clone())
        });
        self.brief = Some(BriefForm {
            card: card.id.clone(),
            name: name_input,
            brief: brief_input,
            files: vec![],
            busy: false,
        });
        self.ask(
            ask::scratch_files(thread),
            window,
            cx,
            |view, files, _, cx| {
                if let (Some(form), Ok(files)) = (&mut view.brief, files) {
                    form.files = files.into_iter().map(|f| (f, false)).collect();
                    cx.notify();
                }
            },
        );
        cx.notify();
    }
    fn make_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(thread), Some(form)) = (self.open.clone(), &mut self.brief) else {
            return;
        };
        form.busy = true;
        let name = form.name.read(cx).value().to_string();
        let brief = form.brief.read(cx).value().to_string();
        let files = (form.files.iter())
            .filter(|(_, ticked)| *ticked)
            .map(|(f, _)| f.clone())
            .collect();
        let card = form.card.clone();
        let ask = ask::make_project(thread.clone(), name, brief, files);
        self.ask(ask, window, cx, move |view, project, window, cx| {
            match project {
                Ok(project) => {
                    view.brief = None;
                    toast(format!("Started the project {}", project.name), window, cx);
                    let dismiss = ask::dismiss_thread_card(thread, card);
                    view.ask(dismiss, window, cx, |_, _, _, _| {});
                }
                Err(error) => {
                    if let Some(form) = &mut view.brief {
                        form.busy = false;
                    }
                    toast_error(error, window, cx);
                }
            }
            cx.notify();
        });
        cx.notify();
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
    /// The coordinator of the project a thread became, to open from the thread.
    fn project_coordinator(&self, project: &str) -> Option<(String, String)> {
        let snapshot = self.shared.snapshot()?;
        let name = snapshot
            .projects
            .iter()
            .find(|p| p.id == project)?
            .name
            .clone();
        let coordinator = (snapshot.sessions.iter())
            .find(|s| s.project_id == project && s.role == Role::ProjectOrchestrator)?;
        Some((coordinator.id.clone(), name))
    }

    fn tab_strip(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let spaces = self.spaces().to_vec();
        let mut strip = div()
            .flex_none()
            .flex()
            .items_end()
            .gap(px(2.))
            .px_3()
            .pt_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .bg(p.base);
        for tab in self.tabs() {
            let thread = &tab.thread;
            let on = self.open.as_ref() == Some(&thread.id);
            let (id, close) = (thread.id.clone(), thread.id.clone());
            let drag = TabDrag {
                id: thread.id.clone(),
                title: thread.title.clone().into(),
            };
            let onto = thread.id.clone();
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("thread-tab-{}", thread.id)))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .max_w(px(190.))
                    .px_3()
                    .py(px(6.))
                    .rounded_t_md()
                    .border_1()
                    .border_b_0()
                    .text_size(px(font::CAPTION))
                    .cursor_pointer()
                    .map(|d| match on {
                        true => d
                            .bg(p.surface)
                            .border_color(p.edge)
                            .text_color(p.text)
                            .mb(px(-1.)),
                        false => d
                            .border_color(gpui::transparent_black())
                            .text_color(p.subtle),
                    })
                    .child(space_mark(space_color(&spaces, &thread.space_id, p)))
                    .when(tab.status == Status::Working, |d| {
                        d.child(ui::Spinner::new().xsmall().color(p.accent))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(thread.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("close-thread-{}", thread.id)))
                            .text_color(p.subtle)
                            .hover(|d| d.text_color(p.text))
                            .child(icon("close").size(px(11.)))
                            .on_click(cx.listener(move |v, _, w, c| {
                                c.stop_propagation();
                                v.close_tab(close.clone(), w, c)
                            })),
                    )
                    .on_click(cx.listener(move |v, _, w, c| v.select(Some(id.clone()), w, c)))
                    .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .drag_over::<TabDrag>(move |d, _, _, _| d.bg(p.overlay))
                    .on_drop(cx.listener(move |v, dragged: &TabDrag, w, c| {
                        v.drop_tab(dragged, &onto, w, c)
                    })),
            );
        }
        strip
            .child(
                div()
                    .id("new-thread-tab")
                    .px_3()
                    .py(px(6.))
                    .cursor_pointer()
                    .text_color(if self.open.is_none() {
                        p.text
                    } else {
                        p.subtle
                    })
                    .hover(|d| d.text_color(p.text))
                    .child(icon("plus").size(px(13.)))
                    .on_click(cx.listener(|v, _, w, c| v.select(None, w, c))),
            )
            .child(
                div().ml_auto().mb(px(6.)).child(
                    button("open-jobs")
                        .ghost()
                        .xsmall()
                        .selected(self.jobs_view.is_some())
                        .label("Jobs")
                        .on_click(cx.listener(|v, _, w, c| v.open_job(None, w, c))),
                ),
            )
            .child(div().mb(px(6.)).child(self.search_space_menu(cx)))
            .child(
                div()
                    .mb(px(6.))
                    .w(px(200.))
                    .child(Input::new(&self.search).xsmall().cleanable(true)),
            )
    }

    /// Limits search to one Space, or searches all of them.
    fn search_space_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let spaces = self.spaces().to_vec();
        let current = self.search_space.clone();
        let label = (spaces.iter())
            .find(|s| Some(&s.id) == current.as_ref())
            .map_or("All Spaces".into(), |s| s.name.clone());
        let weak = cx.weak_entity();
        button("search-space")
            .ghost()
            .xsmall()
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

    fn search_results(&self, hits: &[ThreadHit], p: Palette, cx: &mut Context<Self>) -> Div {
        let spaces = self.spaces().to_vec();
        let mut list = div().flex().flex_col().gap_1().p_4();
        if hits.is_empty() {
            return list.child(hint("No thread has that in its title or messages.", p));
        }
        for hit in hits {
            let id = hit.thread.id.clone();
            let space = spaces.iter().find(|s| s.id == hit.thread.space_id);
            list = list.child(
                div()
                    .id(SharedString::from(format!("hit-{id}")))
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
                            .child(space_mark(space_color(&spaces, &hit.thread.space_id, p)))
                            .child(hit.thread.title.clone())
                            .child(hint(space.map_or(String::new(), |s| s.name.clone()), p)),
                    )
                    .children(hit.excerpt.clone().map(|line| hint(line, p)))
                    .on_click(cx.listener(move |v, _, w, c| v.open_hit(id.clone(), w, c))),
            );
        }
        list
    }

    fn header(&self, tab: &ThreadTab, p: Palette, cx: &mut Context<Self>) -> Div {
        let thread = &tab.thread;
        let space = self.spaces().iter().find(|s| s.id == thread.space_id);
        let project = (thread.project_id.as_deref()).and_then(|id| self.project_coordinator(id));
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
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(match self.renaming {
                        true => Input::new(&self.title).small().into_any_element(),
                        false => div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(thread.title.clone())
                            .into_any_element(),
                    })
                    .child(hint(
                        [
                            space.map(|s| s.name.clone()),
                            tab.profile.as_ref().map(ModelProfile::short),
                            chosen_line(tab.selection.as_ref()),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · "),
                        p,
                    )),
            )
            .children(project.map(|(coordinator, name)| {
                button("open-thread-project")
                    .xsmall()
                    .icon(icon("project"))
                    .label(format!("Project: {name}"))
                    .on_click(
                        cx.listener(move |_, _, _, c| c.emit(Intent::Select(coordinator.clone()))),
                    )
            }))
            .when(thread.project_id.is_none(), |d| {
                d.child(
                    button("make-project")
                        .xsmall()
                        .label("Make this a project")
                        .on_click(cx.listener(|v, _, w, c| v.draft_brief(w, c))),
                )
            })
            .child(
                button("rename-thread")
                    .ghost()
                    .xsmall()
                    .icon(icon("rename"))
                    .tooltip("Rename")
                    .on_click(cx.listener(|v, _, w, c| v.start_rename(w, c))),
            )
            .child(
                button("delete-thread")
                    .ghost()
                    .xsmall()
                    .icon(icon("archive"))
                    .tooltip("Delete thread")
                    .on_click(cx.listener(|v, _, w, c| v.confirm_delete(w, c))),
            )
    }

    fn conversation(&self, tab: &ThreadTab, p: Palette, cx: &mut Context<Self>) -> Div {
        let name = assistant_name(self.shared.snapshot());
        let space = (self.spaces().iter())
            .find(|s| s.id == tab.thread.space_id)
            .map_or(String::new(), |s| s.name.clone());
        let thread = &tab.thread;
        let mut column = div().flex().flex_col().gap_3().px_5().py_4();
        column = column.children(self.authors_notice(tab, p));
        // A message Change moved out, and the replies after it, show dimmed.
        let mut moved: Option<&str> = None;
        for entry in routing::timeline(&self.messages, thread) {
            let message = match entry {
                routing::Entry::Away(line) => {
                    column = column
                        .child(Self::bubble(&line.body, p))
                        .child(self.route_line(thread, line, p, cx));
                    continue;
                }
                routing::Entry::Message(message) => message,
            };
            if message.sender.is_none() {
                moved = (thread.moved.iter())
                    .find(|m| m.message_id == message.id)
                    .map(|m| m.to.as_str());
            }
            let body: SharedString = message.body.clone().into();
            let shown = match (message.sender.is_none(), is_notice(&message.id)) {
                (true, _) => Self::bubble(&message.body, p),
                (false, true) => hint(body, p).text_color(p.red),
                (false, false) => div()
                    .max_w(relative(0.9))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(hint(format!("{name} · {space}"), p))
                    .child(
                        TextView::markdown(
                            SharedString::from(format!("reply-{}", message.id)),
                            body,
                        )
                        .selectable(true)
                        .text_size(px(font::BODY)),
                    ),
            };
            column = column.child(shown.when(moved.is_some(), |d| d.opacity(0.5)));
            if let Some(to) = moved.filter(|_| message.sender.is_none()) {
                column = column.child(self.moved_line(to, p));
            }
            // A message from a new tab keeps its line, with Change, in the thread it went to.
            let here = (thread.routed.iter()).find(|l| l.message_id == message.id);
            if let Some(line) = here {
                column = column.child(self.route_line(thread, line, p, cx));
            }
        }
        if tab.status == Status::Working {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::Spinner::new().xsmall().color(p.accent))
                    .child(hint(format!("{name} is working…"), p)),
            );
        }
        for card in &tab.thread.cards {
            column = column.child(self.card(card, p, cx));
        }
        for proposal in &tab.thread.proposals {
            column = column.child(self.proposal_card(proposal, p, cx));
        }
        for held in self.held_for(Some(&thread.id)) {
            column = column.child(self.held_message(&held, p, cx));
        }
        column
    }

    fn card(&self, thread_card: &ThreadCard, p: Palette, cx: &mut Context<Self>) -> Div {
        let id = thread_card.id.clone();
        let dismiss = {
            let id = id.clone();
            button(SharedString::from(format!("dismiss-{id}")))
                .ghost()
                .xsmall()
                .label("Not now")
                .on_click(cx.listener(move |v, _, w, c| v.dismiss(id.clone(), w, c)))
        };
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
                    .child(hint("Nothing is created until you save it in Spaces.", p))
                    .child(
                        div().flex().gap_2().justify_end().child(dismiss).child(
                            button(SharedString::from(format!("suggested-{id}")))
                                .primary()
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
            CardKind::ProjectBrief { name, brief } => {
                if let Some(form) = self.brief.as_ref().filter(|f| f.card == id) {
                    return self.brief_form(form, p, cx);
                }
                let edited = thread_card.clone();
                card(p)
                    .gap_2()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Make this a project: {name}")),
                    )
                    .child(hint(brief.clone(), p))
                    .child(
                        div().flex().gap_2().justify_end().child(dismiss).child(
                            button(SharedString::from(format!("brief-{id}")))
                                .primary()
                                .xsmall()
                                .label("Review the brief")
                                .on_click(
                                    cx.listener(move |v, _, w, c| v.edit_brief(&edited, w, c)),
                                ),
                        ),
                    )
            }
            CardKind::SavedOutput { title, version, .. } => card(p)
                .flex_row()
                .items_center()
                .gap_2()
                .child(icon("file-text").size(px(14.)))
                .child(div().flex_1().child(saved_line(title, *version)))
                .child(dismiss.label("Hide")),
            CardKind::Recipe { recipe, preview } => {
                self.recipe_card(&thread_card.id, recipe, preview, p, cx)
            }
        }
    }

    fn brief_form(&self, form: &BriefForm, p: Palette, cx: &mut Context<Self>) -> Div {
        let space = (self.open_tab())
            .and_then(|tab| self.spaces().iter().find(|s| s.id == tab.thread.space_id))
            .map_or(String::new(), |s| s.name.clone());
        let mut files = div().flex().flex_col().gap_1();
        for (index, (file, ticked)) in form.files.iter().enumerate() {
            files = files.child(
                Checkbox::new(SharedString::from(format!("scratch-{index}")))
                    .label(file.clone())
                    .checked(*ticked)
                    .on_click(cx.listener(move |v, _, _, c| {
                        if let Some(form) = &mut v.brief {
                            form.files[index].1 ^= true;
                        }
                        c.notify();
                    })),
            );
        }
        card(p)
            .gap_2()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("A new project in {space}")))
            .child(Input::new(&form.name).small())
            .child(Textarea::new(&form.brief).w_full())
            .child(hint(
                "The brief is the project's first message, with a link back to this thread. The thread stays, read-only.",
                p,
            ))
            .when(!form.files.is_empty(), |d| {
                d.child(hint("Copy these scratch files into the project:", p)).child(files)
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        button("cancel-brief")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(cx.listener(|v, _, _, c| {
                                v.brief = None;
                                c.notify();
                            })),
                    )
                    .child(
                        button("confirm-brief")
                            .primary()
                            .xsmall()
                            .label("Make this a project")
                            .disabled(form.busy)
                            .on_click(cx.listener(|v, _, w, c| v.make_project(w, c))),
                    ),
            )
    }

    /// The model a new thread in `space` starts on, as the host picks it with nothing chosen.
    fn new_thread_model(&self, space: &SpaceId) -> Option<ModelProfile> {
        let snapshot = self.shared.snapshot()?;
        let space = (snapshot.spaces.iter()).find(|s| s.id == *space);
        models::thread_model(&snapshot.model_selection, space, &self.shared.detection)
    }
    fn composer_box(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let tab = self.open_tab();
        if let Some(project) = tab.and_then(|t| t.thread.project_id.as_deref()) {
            let named = self
                .project_coordinator(project)
                .map_or("a project".into(), |(_, n)| n);
            return div().m_4().child(hint(
                format!("This thread became {named} and is read-only."),
                p,
            ));
        }
        let issue_thread = tab.is_some_and(|t| t.thread.attached.is_some());
        let space = self.composer_space();
        let spaces = self.spaces().to_vec();
        let chip_name = (spaces.iter())
            .find(|s| Some(&s.id) == space.as_ref())
            .map_or("No Space".into(), |s| s.name.clone());
        let chip_name = match self.chosen_space {
            Some(_) => chip_name,
            None => format!("{chip_name} · auto"),
        };
        // A thread shows its own model; a new tab what its thread would start on in the Space
        // picked by hand. On auto the router may pick a Space that overrides this Mac's row.
        let model = match tab {
            Some(tab) => tab.profile.clone(),
            None => (self.chosen_space.as_ref())
                .and(space.as_ref())
                .and_then(|space| self.new_thread_model(space)),
        };
        let model = div()
            .px_2()
            .rounded(px(7.))
            .border_1()
            .border_color(p.edge)
            .child(model.map_or("Who runs what".into(), |m| m.short()));
        let weak = cx.weak_entity();
        let chosen = self.chosen_space.clone();
        // Auto lets the router choose; a Space picked by hand sends the next message there.
        let chip = button("composer-space")
            .xsmall()
            .when_some(space.clone(), |b, id| {
                b.child(space_mark(space_color(&spaces, &id, p)))
            })
            .label(chip_name)
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
                    .map(|bar| match issue_thread {
                        true => bar.child("Goes to this thread, never the router"),
                        false => bar.child(chip).child(model).child(AUTHORITY),
                    })
                    .child(div().flex_1())
                    .child(
                        button("send-to-thread")
                            .primary()
                            .xsmall()
                            .icon(icon("arrow-up"))
                            .disabled(empty || self.sending)
                            .on_click(cx.listener(|v, _, w, c| v.send(w, c))),
                    ),
            )
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
        if let Command::Send { .. } = command {
            self.sending = false;
            match reply {
                Ok(_) => self.clear_composer(window, cx),
                Err(error) => toast_error(error, window, cx),
            }
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
        let body = match (&self.jobs_view, &self.hits, self.open_tab().cloned()) {
            (Some(view), _, _) => self.jobs_body(view, p, cx).into_any_element(),
            (None, Some(hits), _) => self.search_results(hits, p, cx).into_any_element(),
            (None, None, Some(tab)) => div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.header(&tab, p, cx))
                .child(
                    div()
                        .id("thread-conversation")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(self.conversation(&tab, p, cx)),
                )
                .into_any_element(),
            (None, None, None) => {
                let held = self.held_for(None);
                match held.is_empty() {
                    true => div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(hint(
                            format!(
                                "Ask {} anything. Each thread is read-only plus its own scratch folder, and stays on this Mac.",
                                assistant_name(self.shared.snapshot())
                            ),
                            p,
                        ))
                        .into_any_element(),
                    false => div()
                        .id("held-messages")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(div().flex().flex_col().gap_3().px_5().py_4().children(
                            held.iter().map(|held| self.held_message(held, p, cx)),
                        ))
                        .into_any_element(),
                }
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(self.tab_strip(p, cx))
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .when(self.hits.is_none() && self.jobs_view.is_none(), |d| {
                d.child(self.composer_box(p, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SearchAsked, after_closing, any_working, assistant_name, chosen_line, composer_placeholder,
        reordered, saved_line,
    };
    use workspace_core::assistant::ThreadTab;
    use workspace_core::{Chooser, Selection, Snapshot, SpaceId, Status};

    fn tab(id: &str, status: Status) -> ThreadTab {
        ThreadTab {
            thread: workspace_core::assistant::Thread {
                id: id.into(),
                space_id: SpaceId("sage".into()),
                title: id.into(),
                created_at: 0,
                last_message_at: 0,
                opened_at: 0,
                closed: false,
                position: 0,
                project_id: None,
                cards: vec![],
                routed: vec![],
                moved: vec![],
                job: None,
                attached: None,
                proposals: vec![],
            },
            status,
            profile: None,
            selection: None,
            message_revision: 0,
        }
    }

    #[test]
    fn the_sidebar_row_names_the_assistant_and_shows_any_thread_working() {
        assert_eq!(assistant_name(None), "Assistant");
        let named = Snapshot {
            assistant_name: "Wren".into(),
            ..Default::default()
        };
        assert_eq!(assistant_name(Some(&named)), "Wren");
        assert_eq!(assistant_name(Some(&Snapshot::default())), "Assistant");
        assert!(!any_working(&[tab("a", Status::Ready)]));
        assert!(any_working(&[
            tab("a", Status::Ready),
            tab("b", Status::Working)
        ]));
    }

    #[test]
    fn a_threads_header_says_who_chose_its_model() {
        let selection = |chosen_by, reason: &str| Selection {
            chosen_by,
            reason: reason.into(),
            revision: 1,
            at: 0,
        };
        assert_eq!(
            chosen_line(Some(&selection(Chooser::Wiffletree, "First choice"))).unwrap(),
            "Chosen by Wiffletree: First choice"
        );
        assert_eq!(
            chosen_line(Some(&selection(Chooser::Human, "Chosen by you"))).unwrap(),
            "Chosen by you"
        );
        assert_eq!(chosen_line(None), None);
    }

    #[test]
    fn a_dropped_tab_takes_the_place_of_the_one_it_lands_on() {
        let order: Vec<String> = ["a", "b", "c"].map(String::from).into();
        assert_eq!(reordered(&order, "c", "a"), ["c", "a", "b"]);
        assert_eq!(reordered(&order, "a", "c"), ["b", "c", "a"]);
        assert_eq!(reordered(&order, "a", "b"), ["b", "a", "c"]);
        assert_eq!(reordered(&order, "b", "missing"), ["a", "c", "b"]);
    }

    #[test]
    fn the_composer_asks_by_the_assistants_name() {
        assert_eq!(composer_placeholder("Wren"), "Ask Wren…");
        assert_eq!(composer_placeholder("Assistant"), "Ask Assistant…");
    }

    #[test]
    fn a_late_search_reply_for_another_space_or_query_is_dropped() {
        let asked = |query: &str, space: Option<&str>| SearchAsked {
            query: query.into(),
            space: space.map(|s| SpaceId(s.into())),
        };
        // Sage was asked for, then the box moved to Personal before Sage's reply came.
        let now = asked("dentist", Some("personal"));
        assert_ne!(asked("dentist", Some("sage")), now);
        assert_ne!(asked("dentist", None), now);
        assert_ne!(asked("dent", Some("personal")), now);
        assert_eq!(asked("dentist", Some("personal")), now);
    }

    #[test]
    fn closing_a_tab_shows_its_neighbour() {
        let tabs = [
            tab("a", Status::Ready),
            tab("b", Status::Ready),
            tab("c", Status::Ready),
        ];
        assert_eq!(after_closing(&tabs, "b").as_deref(), Some("c"));
        assert_eq!(after_closing(&tabs, "c").as_deref(), Some("b"));
        assert_eq!(after_closing(&tabs[..1], "a"), None);
    }

    #[test]
    fn a_saved_output_says_it_stays_on_this_mac() {
        assert_eq!(
            saved_line("AWS costs", 1),
            "Saved “AWS costs” to Boards · Only on this Mac"
        );
        assert!(saved_line("AWS costs", 2).ends_with("· version 2"));
    }
}
