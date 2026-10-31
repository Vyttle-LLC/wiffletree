//! Boards (OpenSpec change `assistant-spaces-jobs-boards`, screens 4 and 5 of its mockup): every
//! output a thread or job saved, filtered by Space and job and searched by title, beside the
//! jobs; and one output open by its type, with its versions. Markdown renders natively, HTML in
//! the board web view, images and PDFs in Quick Look, and anything else in its default app.
use super::*;
use board_web_view::{Board, BoardEvent, BoardFolder, with_rules};
use gpui::{Bounds, Image, Pixels, canvas, img};
use gpui_component::{
    Sizable,
    button::ButtonVariants,
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    text::TextView,
};
use spaces_view::{space_color, space_mark};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use ui::{card, hint, icon, pill};
use workspace_core::board::{
    BoardOutput, BoardVersion, FileKind, Keeping, LATEST, plain_file_name,
};
use workspace_core::job::{JobKey, JobState, RunOutcome, Runner, Sharing};

/// What the HTML viewer says about its sandbox, as the mockup words it.
const SANDBOX: &str = "🔒 Scripts run · no network · can't navigate away";
/// How long an overlay waits for the board's still image before it opens without one.
const SNAPSHOT_LIMIT: Duration = Duration::from_millis(500);
/// What the pane says in the board's place when an overlay opens before any still image.
const HIDDEN: &str = "Hidden while this is open";
/// How often a shown board's still image is taken again, so an overlay that opens with no
/// warning, such as a shortcut's dialog or another view's menu, has a recent one to show.
const STILL_EVERY: Duration = Duration::from_secs(2);
const MAX_MARKDOWN_BYTES: u64 = 2 * 1024 * 1024;
/// A shared job's output that waits for its runner, before its first push.
pub(super) const WAITING: &str = "Waiting for your approval";
pub(super) const TAKE_OVER: &str = "Take over running";
/// A runner's save that a later save replaced before any push.
const LOCAL_ONLY: &str = "Only on this Mac · not pushed";

/// A text action in a line of labels, as the mockup's links are.
fn link(id: String, label: &'static str, color: Hsla) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .cursor_pointer()
        .text_color(color)
        .child(label)
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Outputs,
    Jobs,
}

#[derive(Clone, PartialEq)]
enum JobFilter {
    All,
    Threads,
    Job(JobKey),
}

/// What reaches the page from AppKit and WebKit callbacks, which have no GPUI context.
enum PaneEvent {
    Board(BoardEvent),
    Snapshot(Option<Arc<Image>>),
    Rules(board_web_view::Rules),
}

/// What opens an overlay once the board's still image is ready.
type OpenOverlay = Box<dyn FnOnce(&mut BoardsPage, &mut Window, &mut Context<BoardsPage>)>;

/// An HTML output's web view and the GPUI side of placing it.
struct HtmlPane {
    board: Rc<RefCell<Option<Board>>>,
    /// The placeholder's bounds when it painted in the current frame.
    painted: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// A still image of the board, shown while an overlay is over the pane.
    still: Option<Arc<Image>>,
    /// What opens once the still image an overlay asked for arrives.
    snapping: Option<OpenOverlay>,
    /// A web address the board asked to go to, offered instead.
    offered: Option<String>,
    failed: Option<String>,
}

enum Body {
    Loading,
    Markdown(SharedString),
    Html(HtmlPane),
    /// Opened in Quick Look, or offered to the default app.
    Elsewhere,
    Failed(String),
}

/// An output open in the viewer, at one of its versions.
struct Viewer {
    output: BoardOutput,
    versions: Vec<BoardVersion>,
    /// The commit shown; `None` until the versions arrive, which shows the latest.
    version: Option<String>,
    folder: PathBuf,
    body: Body,
}

pub(super) struct BoardsPage {
    bridge: Bridge,
    shared: Shared,
    outputs: Option<Result<Vec<BoardOutput>, String>>,
    /// What the outputs were last read for, so a snapshot that changed none reads none.
    read_for: Option<u64>,
    tab: Tab,
    space: Option<SpaceId>,
    job: JobFilter,
    search: Entity<InputState>,
    viewer: Option<Viewer>,
    events: async_channel::Sender<PaneEvent>,
    focus: FocusHandle,
    /// GPUI's focus while the board has the keyboard: under the page, with no text bindings.
    board_focus: FocusHandle,
}
impl EventEmitter<Intent> for BoardsPage {}

/// A figure that changes when a thread or job saves an output.
fn outputs_signature(snapshot: &Snapshot) -> u64 {
    let runs: u64 = (snapshot.jobs.iter())
        .flat_map(|job| &job.runs)
        .flat_map(|run| &run.outputs)
        .map(|output| u64::from(output.version))
        .sum();
    let saved = (snapshot.threads.iter())
        .flat_map(|tab| &tab.thread.cards)
        .filter(|card| {
            matches!(
                card.kind,
                workspace_core::assistant::CardKind::SavedOutput { .. }
            )
        })
        .count() as u64;
    runs.wrapping_mul(1_000_003).wrapping_add(saved)
}

/// Where an output is kept, as the library and viewer say it.
fn keeping_label(output: &BoardOutput, space: &str) -> String {
    match output.keeping {
        Keeping::SharedStorage => format!("Shared in {space}"),
        Keeping::Private | Keeping::LocalStorage => "Only on this Mac".into(),
    }
}

/// "Release dashboard job", or "From a thread".
fn source_label(output: &BoardOutput) -> String {
    match (&output.job, &output.job_name) {
        (None, _) => "From a thread".into(),
        (Some(_), Some(name)) => format!("{name} job"),
        (Some(_), None) => "From a deleted job".into(),
    }
}

/// A version's line in the picker: when, and what wrote it.
fn version_label(version: &BoardVersion, latest: bool) -> (String, String) {
    let by = match (&version.run_id, version.shared) {
        (_, true) => "shared from this Mac".to_owned(),
        (Some(_), _) if version.catch_up > 0 => "job run · catch-up".into(),
        (Some(_), _) => "job run".into(),
        (None, _) => "from a thread".into(),
    };
    let by = match (latest, version.local_only) {
        (_, true) => format!("{by} · {LOCAL_ONLY}"),
        (true, false) => format!("Latest · {by}"),
        (false, false) => by,
    };
    (wording::day_and_time(version.saved_at), by)
}

/// The outputs the filters and search let through, in their order.
fn filtered<'a>(
    outputs: &'a [BoardOutput],
    space: Option<&SpaceId>,
    job: &JobFilter,
    query: &str,
) -> Vec<&'a BoardOutput> {
    let query = query.trim().to_lowercase();
    (outputs.iter())
        .filter(|o| space.is_none_or(|space| &o.space_id == space))
        .filter(|o| match job {
            JobFilter::All => true,
            JobFilter::Threads => o.job.is_none(),
            JobFilter::Job(key) => o.job.as_ref() == Some(key),
        })
        .filter(|o| query.is_empty() || o.title.to_lowercase().contains(&query))
        .collect()
}

impl BoardsPage {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search titles"));
        cx.subscribe(&search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let (events, received) = async_channel::unbounded();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = received.recv().await {
                let _ = this.update_in(cx, |this, window, cx| this.pane_event(event, window, cx));
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(STILL_EVERY).await;
                if this.update(cx, |this, _| this.take_still()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            outputs: None,
            read_for: None,
            tab: Tab::Outputs,
            space: None,
            job: JobFilter::All,
            search,
            viewer: None,
            events,
            focus: cx.focus_handle(),
            board_focus: cx.focus_handle(),
        }
    }
    fn spaces(&self) -> &[Space] {
        self.shared.snapshot().map_or(&[], |s| &s.spaces)
    }
    fn jobs(&self) -> &[JobState] {
        self.shared.snapshot().map_or(&[], |s| &s.jobs)
    }
    fn space_name(&self, id: &SpaceId) -> String {
        (self.spaces().iter())
            .find(|s| &s.id == id)
            .map_or(String::new(), |s| s.name.clone())
    }

    /// Takes the shared state; a board's web view goes when Boards is no longer shown.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let shown = shared.page == Page::Boards;
        self.shared = shared;
        if !shown {
            self.close_web_view(window, cx);
            return;
        }
        let signature = self.shared.snapshot().map(outputs_signature);
        if signature != self.read_for {
            self.read_for = signature;
            self.read_outputs(window, cx);
        }
        cx.notify();
    }
    /// Reads the outputs again as Boards opens, and puts back an open HTML board.
    pub(super) fn opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A board shows only while GPUI's focus is in this page.
        window.focus(&self.focus, cx);
        self.read_outputs(window, cx);
        if let Some(viewer) = &self.viewer
            && viewer.output.kind() == FileKind::Html
            && matches!(&viewer.body, Body::Html(pane) if pane.board.borrow().is_none())
        {
            let (output, folder) = (viewer.output.clone(), viewer.folder.clone());
            self.show_version(output, folder, window, cx);
        }
    }
    fn read_outputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(ask::boards(), window, cx, |view, read, _, cx| {
            view.outputs = Some(read);
            cx.notify();
        });
    }

    fn open_output(&mut self, output: BoardOutput, window: &mut Window, cx: &mut Context<Self>) {
        self.close_web_view(window, cx);
        window.focus(&self.focus, cx);
        let folder = PathBuf::from(&output.folder);
        self.viewer = Some(Viewer {
            output: output.clone(),
            versions: vec![],
            version: None,
            folder: folder.clone(),
            body: Body::Loading,
        });
        let id = output.id.clone();
        self.ask(
            ask::board_versions(id.clone()),
            window,
            cx,
            move |view, versions, window, cx| {
                // A reply for an output the viewer has since left is dropped.
                let Some(viewer) = view.viewer.as_mut().filter(|v| v.output.id == id) else {
                    return;
                };
                match versions {
                    Ok(versions) => {
                        viewer.version = Some(LATEST.to_owned());
                        viewer.versions = versions;
                    }
                    Err(error) => toast_error(error, window, cx),
                }
                cx.notify();
            },
        );
        if matches!(output.kind(), FileKind::Image | FileKind::Pdf)
            && let Some(path) = contained(&folder, &output.file)
        {
            quick_look(&path);
        }
        self.show_version(output, folder, window, cx);
    }
    fn pick_version(&mut self, commit: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        // An earlier save of the title may have named another file, of another type.
        if let Some(version) = viewer.versions.iter().find(|v| v.commit == commit) {
            viewer.output.file = version.file.clone();
        }
        viewer.version = Some(commit.clone());
        let id = viewer.output.id.clone();
        let ask = ask::board_version_folder(id.clone(), commit.clone());
        self.ask(
            ask,
            window,
            cx,
            move |view, folder, window, cx| match folder {
                Ok(folder) => {
                    // A reply for another output, or a version picked before the latest pick, is
                    // dropped.
                    let Some(output) = (view.viewer.as_ref())
                        .filter(|v| v.output.id == id && v.version.as_ref() == Some(&commit))
                        .map(|v| v.output.clone())
                    else {
                        return;
                    };
                    view.show_version(output, PathBuf::from(folder), window, cx);
                }
                Err(error) => toast_error(error, window, cx),
            },
        );
    }
    /// Shows the output's file from `folder` by its type.
    fn show_version(
        &mut self,
        output: BoardOutput,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_web_view(window, cx);
        let body = match output.kind() {
            _ if !plain_file_name(&output.file) => Body::Failed(
                "This version names a file outside its folder, so it is not opened.".into(),
            ),
            FileKind::Markdown => match contained(&folder, &output.file) {
                Some(path) => match read_markdown(&path) {
                    Ok(text) => Body::Markdown(text.into()),
                    Err(error) => Body::Failed(error),
                },
                None => Body::Failed("This version's file is not in its folder.".into()),
            },
            FileKind::Html => {
                let events = self.events.clone();
                with_rules(move |rules| {
                    let _ = events.try_send(PaneEvent::Rules(rules));
                });
                Body::Html(HtmlPane {
                    board: Rc::default(),
                    painted: Rc::default(),
                    still: None,
                    snapping: None,
                    offered: None,
                    failed: None,
                })
            }
            FileKind::Image | FileKind::Pdf | FileKind::Other => Body::Elsewhere,
        };
        if let Some(viewer) = &mut self.viewer {
            viewer.folder = folder;
            viewer.body = body;
        }
        cx.notify();
    }
    /// Drops the open board's web view, so a closed or hidden board holds none.
    fn close_web_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Viewer {
            body: Body::Html(pane),
            ..
        }) = &mut self.viewer
        else {
            return;
        };
        let closed = pane.board.borrow_mut().take();
        if closed.is_some() && self.board_focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        drop(closed);
        pane.still = None;
        pane.snapping = None;
    }
    fn close_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_web_view(window, cx);
        self.viewer = None;
        cx.notify();
    }

    fn pane_event(&mut self, event: PaneEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The first still image, so an overlay that opens at once has one to show.
        if matches!(event, PaneEvent::Board(BoardEvent::Loaded)) {
            return self.take_still();
        }
        let Some(Viewer {
            body: Body::Html(pane),
            output,
            folder,
            ..
        }) = &mut self.viewer
        else {
            return;
        };
        match event {
            PaneEvent::Rules(Err(error)) => pane.failed = Some(error),
            PaneEvent::Rules(Ok(rules)) => {
                if pane.board.borrow().is_some() || self.shared.page != Page::Boards {
                    return;
                }
                let events = self.events.clone();
                let on_event = move |event| {
                    let _ = events.try_send(PaneEvent::Board(event));
                };
                let opened = BoardFolder::new(&output.id, folder)
                    .map_err(|e| e.to_string())
                    .and_then(|board| {
                        Board::open(window, board, &output.file, &rules, on_event)
                            .map_err(|e| e.to_string())
                    });
                match opened {
                    Ok(board) => *pane.board.borrow_mut() = Some(board),
                    Err(error) => pane.failed = Some(error),
                }
            }
            PaneEvent::Board(BoardEvent::Focus(true)) => window.focus(&self.board_focus, cx),
            // GPUI's focus may have moved on since, such as to a field the click landed in.
            PaneEvent::Board(BoardEvent::Focus(false)) => {
                if self.board_focus.is_focused(window) {
                    window.focus(&self.focus, cx);
                }
            }
            PaneEvent::Board(BoardEvent::Link(url)) => pane.offered = Some(url),
            PaneEvent::Board(BoardEvent::Loaded) => {}
            PaneEvent::Snapshot(image) => {
                pane.still = image;
                if let Some(then) = pane.snapping.take() {
                    then(self, window, cx);
                }
            }
        }
        cx.notify();
    }
    /// Takes a new still image of a board while it shows.
    fn take_still(&self) {
        let Some(Viewer {
            body: Body::Html(pane),
            ..
        }) = &self.viewer
        else {
            return;
        };
        if let Some(board) = pane
            .board
            .borrow()
            .as_ref()
            .filter(|board| board.is_shown())
        {
            let events = self.events.clone();
            board.snapshot(move |image| {
                let _ = events.try_send(PaneEvent::Snapshot(image));
            });
        }
    }
    /// Runs `then`, which opens an overlay, once the board's still image is ready to show
    /// under it, or after `SNAPSHOT_LIMIT`; until then the live board shows. With no HTML
    /// board open it runs now.
    fn before_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let events = self.events.clone();
        let snapping = match &self.viewer {
            Some(Viewer {
                body: Body::Html(pane),
                ..
            }) => pane.board.borrow().as_ref().map(|board| {
                board.snapshot(move |image| {
                    let _ = events.try_send(PaneEvent::Snapshot(image));
                })
            }),
            _ => None,
        };
        let (
            Some(()),
            Some(Viewer {
                body: Body::Html(pane),
                ..
            }),
        ) = (snapping, &mut self.viewer)
        else {
            return then(self, window, cx);
        };
        pane.snapping = Some(Box::new(then));
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SNAPSHOT_LIMIT).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(Viewer {
                    body: Body::Html(pane),
                    ..
                }) = &mut this.viewer
                else {
                    return;
                };
                if let Some(then) = pane.snapping.take() {
                    then(this, window, cx);
                }
            });
        })
        .detach();
    }

    /// Share or Keep private, for an output waiting for its runner's approval.
    fn answer_output(
        &mut self,
        run: String,
        title: String,
        share: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::approve_job_output(run, title, share);
        self.ask(ask, window, cx, |view, done, window, cx| match done {
            Ok(()) => view.read_outputs(window, cx),
            Err(error) => toast_error(error, window, cx),
        });
    }
    /// Whether this Mac may take over running the job.
    fn takes_over(&self, key: &JobKey) -> bool {
        (self.jobs().iter()).any(|job| &job.recipe.key() == key && job.can_take_over())
    }
    fn take_over(&mut self, key: JobKey, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self.jobs().iter().find(|job| job.recipe.key() == key) else {
            return;
        };
        let ask = ask::take_over_job(job.recipe.space_id.clone(), job.recipe.id.clone());
        self.ask(ask, window, cx, |view, done, window, cx| match done {
            Ok(_) => view.read_outputs(window, cx),
            Err(error) => toast_error(error, window, cx),
        });
    }
    /// Share to Space…: asks which Space, naming who can read its storage, then copies it.
    fn share_output(&mut self, output: BoardOutput, window: &mut Window, cx: &mut Context<Self>) {
        let spaces: Vec<Space> = (self.spaces().iter())
            .filter(|s| {
                let locals = self.shared.snapshot().map(|s| &s.space_locals);
                locals.and_then(|l| l.get(&s.id)).is_none_or(|l| l.enabled)
            })
            .cloned()
            .collect();
        let Some(first) = spaces
            .iter()
            .find(|s| s.id == output.space_id)
            .or(spaces.first())
            .cloned()
        else {
            return;
        };
        self.ask_readers(output, first, spaces, window, cx);
    }
    fn ask_readers(
        &mut self,
        output: BoardOutput,
        space: Space,
        spaces: Vec<Space>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::space_storage(space.id.clone());
        self.ask(ask, window, cx, move |view, status, window, cx| {
            let readers = status.ok().and_then(|status| status.readers);
            let who = spaces_view::readers_line(&space, readers);
            view.before_overlay(window, cx, move |view, window, cx| {
                view.confirm_share(output, space, spaces, who, window, cx)
            });
        });
    }
    fn confirm_share(
        &mut self,
        output: BoardOutput,
        space: Space,
        spaces: Vec<Space>,
        who: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        let others: Vec<Space> = spaces
            .iter()
            .filter(|s| s.id != space.id)
            .cloned()
            .collect();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, id, target) = (weak.clone(), output.id.clone(), space.id.clone());
            let mut body = div().flex().flex_col().gap_2().child(format!(
                "“{}” is copied into {}’s storage. {who} can read it there.",
                output.title, space.name
            ));
            for other in &others {
                let (weak, output, other, spaces) =
                    (weak.clone(), output.clone(), other.clone(), spaces.clone());
                body = body.child(
                    button(SharedString::from(format!("share-to-{}", other.id)))
                        .ghost()
                        .xsmall()
                        .label(format!("Share to {} instead", other.name))
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let (output, other, spaces) =
                                (output.clone(), other.clone(), spaces.clone());
                            let _ = weak.update(cx, |view, cx| {
                                view.ask_readers(output, other, spaces, window, cx)
                            });
                        }),
                );
            }
            dialog
                .title(format!("Share to {}?", space.name))
                .confirm()
                .ok_text("Share")
                .child(body)
                .on_ok(move |_, window, cx| {
                    let ask = ask::share_board(id.clone(), target.clone());
                    let _ = weak.update(cx, |view, cx| {
                        view.ask(ask, window, cx, |view, shared, window, cx| match shared {
                            Ok(shared) => {
                                view.read_outputs(window, cx);
                                view.open_output(shared, window, cx);
                            }
                            Err(error) => toast_error(error, window, cx),
                        });
                    });
                    true
                })
        });
    }

    fn header(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let tab = self.tab;
        div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .mr_2()
                    .text_size(px(font::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Boards"),
            )
            .child(
                div()
                    .flex()
                    .child(
                        ui::segment("boards-outputs", "Outputs", tab == Tab::Outputs).on_click(
                            cx.listener(|v, _, _, c| {
                                v.tab = Tab::Outputs;
                                c.notify();
                            }),
                        ),
                    )
                    .child(
                        ui::segment("boards-jobs", "Jobs", tab == Tab::Jobs).on_click(cx.listener(
                            |v, _, _, c| {
                                v.tab = Tab::Jobs;
                                c.notify();
                            },
                        )),
                    ),
            )
            .child(self.space_menu(cx))
            .when(tab == Tab::Outputs, |d| {
                d.child(self.job_menu(cx)).child(
                    div()
                        .ml_auto()
                        .w(px(220.))
                        .child(Input::new(&self.search).xsmall().cleanable(true)),
                )
            })
            .child(div().when(tab == Tab::Jobs, |d| d.ml_auto()).child(hint(
                match tab {
                    Tab::Outputs => "",
                    Tab::Jobs => "Jobs run only while Wiffletree runs on this Mac.",
                },
                p,
            )))
    }
    fn space_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let spaces = self.spaces().to_vec();
        let current = self.space.clone();
        let label = (spaces.iter())
            .find(|s| Some(&s.id) == current.as_ref())
            .map_or("All Spaces".into(), |s| s.name.clone());
        let weak = cx.weak_entity();
        button("boards-space")
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
                        move |_, _, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.space = space.clone();
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            })
    }
    fn job_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let jobs: Vec<(JobKey, String)> = (self.jobs().iter())
            .filter(|job| {
                self.space
                    .as_ref()
                    .is_none_or(|s| &job.recipe.space_id == s)
            })
            .map(|job| (job.recipe.key(), job.recipe.name.clone()))
            .collect();
        let current = self.job.clone();
        let label = match &current {
            JobFilter::All => "All jobs".to_owned(),
            JobFilter::Threads => "From threads".into(),
            JobFilter::Job(key) => (jobs.iter())
                .find(|(k, _)| k == key)
                .map_or("A job".into(), |(_, name)| name.clone()),
        };
        let weak = cx.weak_entity();
        button("boards-job")
            .ghost()
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let choices = [
                    (JobFilter::All, "All jobs".to_owned()),
                    (JobFilter::Threads, "From threads".into()),
                ]
                .into_iter()
                .chain(
                    jobs.iter()
                        .map(|(k, n)| (JobFilter::Job(k.clone()), n.clone())),
                );
                for (filter, name) in choices {
                    let (weak, checked) = (weak.clone(), filter == current);
                    menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(
                        move |_, _, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.job = filter.clone();
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            })
    }

    fn library(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let outputs = match &self.outputs {
            None => return div().child(hint("Reading Boards…", p)),
            Some(Err(error)) => return div().child(hint(error.clone(), p)),
            Some(Ok(outputs)) => outputs,
        };
        let query = self.search.read(cx).value().to_string();
        let shown = filtered(outputs, self.space.as_ref(), &self.job, &query);
        if shown.is_empty() {
            return div().child(ui::empty_state(
                "book",
                "Nothing here yet",
                match outputs.is_empty() {
                    true => "What threads and jobs save with save_output is listed here.",
                    false => "No output matches these filters.",
                },
                p,
            ));
        }
        let spaces = self.spaces().to_vec();
        let mut grid = div().grid().grid_cols(3).gap_3();
        for output in shown {
            grid = grid.child(self.tile(output, &spaces, p, cx));
        }
        grid
    }
    fn tile(
        &self,
        output: &BoardOutput,
        spaces: &[Space],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let space = self.space_name(&output.space_id);
        let (opened, shared) = (output.clone(), output.clone());
        let when = wording::day_and_time(output.updated_at);
        let kind = output.kind();
        let answer = |share: bool| {
            let (run, title) = (output.run_id.clone(), output.title.clone());
            cx.listener(move |v: &mut Self, _: &ClickEvent, w, c| {
                c.stop_propagation();
                if let Some(run) = &run {
                    v.answer_output(run.clone(), title.clone(), share, w, c)
                }
            })
        };
        let take_over = (output.job.clone())
            .filter(|key| output.stale && self.takes_over(key))
            .map(|key| {
                cx.listener(move |v: &mut Self, _: &ClickEvent, w, c| {
                    c.stop_propagation();
                    v.take_over(key.clone(), w, c)
                })
            });
        div()
            .id(SharedString::from(format!("board-{}", output.id)))
            .flex()
            .flex_col()
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(p.edge.opacity(edge::CARD))
            .bg(p.surface)
            .cursor_pointer()
            .hover(|d| d.border_color(p.edge))
            .on_click(cx.listener(move |v, _, w, c| v.open_output(opened.clone(), w, c)))
            .child(preview(output, p))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(font::SMALL))
                            .child(output.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(font::META))
                            .text_color(p.subtle)
                            .child(space_mark(space_color(spaces, &output.space_id, p)))
                            .child(format!("{space} · {} · {when}", source_label(output))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(font::META))
                            .text_color(p.subtle)
                            .when(!output.waiting, |d| {
                                d.child(pill(keeping_label(output, &space), p.subtle))
                            })
                            .when(output.waiting, |d| {
                                d.child(pill(WAITING, p.accent))
                                    .child(
                                        link(format!("approve-{}", output.id), "Share", p.accent)
                                            .on_click(answer(true)),
                                    )
                                    .child("·")
                                    .child(
                                        link(
                                            format!("keep-{}", output.id),
                                            "Keep private",
                                            p.subtle,
                                        )
                                        .on_click(answer(false)),
                                    )
                            })
                            .when(output.stale, |d| d.child(pill("Stale", p.yellow)))
                            .children((output.runner.as_ref()).map(Runner::label))
                            .children(take_over.map(|take_over| {
                                link(format!("take-over-{}", output.id), TAKE_OVER, p.accent)
                                    .on_click(take_over)
                            }))
                            .when(matches!(kind, FileKind::Image | FileKind::Pdf), |d| {
                                d.child(format!("{} · Quick Look", type_label(kind)))
                            })
                            .when(output.keeping == Keeping::Private && !output.waiting, |d| {
                                d.child(
                                    link(
                                        format!("share-{}", output.id),
                                        "Share to Space…",
                                        p.accent,
                                    )
                                    .on_click(cx.listener(
                                        move |v, _, w, c| {
                                            c.stop_propagation();
                                            v.share_output(shared.clone(), w, c)
                                        },
                                    )),
                                )
                            }),
                    ),
            )
    }

    fn jobs_list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let spaces = self.spaces().to_vec();
        let jobs: Vec<&JobState> = (self.jobs().iter())
            .filter(|job| {
                self.space
                    .as_ref()
                    .is_none_or(|s| &job.recipe.space_id == s)
            })
            .collect();
        if jobs.is_empty() {
            return div().child(ui::empty_state(
                "activity",
                "No jobs yet",
                "Ask the assistant for recurring work, such as a brief every weekday at 7, and save the recipe it shows.",
                p,
            ));
        }
        let mut list = div().flex().flex_col().gap_2();
        for job in jobs {
            let key = job.recipe.key();
            let next = match (&job.not_here, job.recipe.enabled, job.next_runs.first()) {
                (Some(why), _, _) => why.clone(),
                (None, false, _) => "Paused".into(),
                (None, true, Some(next)) => format!("Next {next}"),
                (None, true, None) => "No next run".into(),
            };
            let last = job.runs.first().map(|run| {
                let color = match run.outcome {
                    RunOutcome::Finished => p.green,
                    RunOutcome::Running => p.accent,
                    RunOutcome::Stopped => p.subtle,
                    RunOutcome::Failed | RunOutcome::NotStarted => p.red,
                };
                pill(run.outcome.label(), color)
            });
            let runner = (job.recipe.sharing == Sharing::OneRunner)
                .then_some(job.recipe.runner.as_ref())
                .flatten()
                .map(Runner::label);
            list = list.child(
                card(p)
                    .id(SharedString::from(format!("boards-job-{}", key.as_str())))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.overlay))
                    .on_click(
                        cx.listener(move |_, _, _, c| c.emit(Intent::OpenJob(Some(key.clone())))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(space_mark(space_color(&spaces, &job.recipe.space_id, p)))
                            .child(
                                div()
                                    .flex_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(job.recipe.name.clone()),
                            )
                            .children(last),
                    )
                    .child(hint(format!("{} · {next}", job.schedule), p))
                    .children(runner.map(|runner| hint(runner, p))),
            );
        }
        list
    }

    fn viewer(
        &self,
        viewer: &Viewer,
        window: &mut Window,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let output = &viewer.output;
        let space = self.space_name(&output.space_id);
        let spaces = self.spaces().to_vec();
        let when = wording::day_and_time(output.updated_at);
        let mut note = vec![space.clone()];
        note.extend(output.runner.as_ref().map(Runner::label));
        note.push(match output.job {
            Some(_) => format!("last run {when}"),
            None => format!("{} · saved {when}", source_label(output)),
        });
        // Each opens the file only once it is confirmed to be in the version's folder.
        let file = (viewer.folder.clone(), output.file.clone());
        let (reveal, open, shared) = (file.clone(), file.clone(), output.clone());
        let waiting = (output.run_id.clone())
            .filter(|_| output.waiting)
            .map(|run| (run, output.title.clone()));
        let run_job = (output.job.clone()).filter(|key| {
            self.jobs()
                .iter()
                .any(|j| &j.recipe.key() == key && j.not_here.is_none())
        });
        let head = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .child(
                button("boards-back")
                    .ghost()
                    .xsmall()
                    .icon(icon("arrow-left"))
                    .label("Boards")
                    .on_click(cx.listener(|v, _, w, c| v.close_viewer(w, c))),
            )
            .child(space_mark(space_color(&spaces, &output.space_id, p)))
            .child(
                div()
                    .text_size(px(font::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(output.title.clone()),
            )
            .child(hint(note.join(" · "), p))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(output.keeping == Keeping::Private && !output.waiting, |d| {
                        d.child(
                            button("boards-share")
                                .ghost()
                                .xsmall()
                                .label("Share to Space…")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    v.share_output(shared.clone(), w, c)
                                })),
                        )
                    })
                    .when_some(waiting, |d, (run, title)| {
                        let (kept_run, kept_title) = (run.clone(), title.clone());
                        d.child(pill(WAITING, p.accent))
                            .child(button("boards-approve").xsmall().label("Share").on_click(
                                cx.listener(move |v, _, w, c| {
                                    v.answer_output(run.clone(), title.clone(), true, w, c);
                                    v.close_viewer(w, c);
                                }),
                            ))
                            .child(
                                button("boards-keep-private")
                                    .ghost()
                                    .xsmall()
                                    .label("Keep private")
                                    .on_click(cx.listener(move |v, _, w, c| {
                                        v.answer_output(
                                            kept_run.clone(),
                                            kept_title.clone(),
                                            false,
                                            w,
                                            c,
                                        );
                                        v.close_viewer(w, c);
                                    })),
                            )
                    })
                    .when(
                        matches!(output.kind(), FileKind::Image | FileKind::Pdf),
                        |d| {
                            let (folder, file) = file.clone();
                            d.child(
                                button("boards-quick-look")
                                    .ghost()
                                    .xsmall()
                                    .label("Quick Look")
                                    .on_click(move |_, _, _| {
                                        if let Some(path) = contained(&folder, &file) {
                                            quick_look(&path)
                                        }
                                    }),
                            )
                        },
                    )
                    .child(
                        button("boards-reveal")
                            .ghost()
                            .xsmall()
                            .label("Reveal in Finder")
                            .on_click(move |_, _, cx| {
                                if let Some(path) = contained(&reveal.0, &reveal.1) {
                                    cx.reveal_path(&path)
                                }
                            }),
                    )
                    .child(
                        button("boards-default-app")
                            .ghost()
                            .xsmall()
                            .label("Open in default app")
                            .on_click(move |_, _, cx| {
                                if let Some(path) = contained(&open.0, &open.1) {
                                    open_in_default_app(&path, cx)
                                }
                            }),
                    )
                    .children(run_job.map(|key| {
                        button("boards-run-now")
                            .xsmall()
                            .label("Run now")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.ask(ask::run_job_now(key.clone()), w, c, |_, done, w, c| {
                                    if let Err(error) = done {
                                        toast_error(error, w, c);
                                    }
                                })
                            }))
                    })),
            );
        div().size_full().flex().flex_col().child(head).child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(self.body(viewer, window, p, cx)),
                )
                .child(self.versions(viewer, p, cx)),
        )
    }
    fn body(
        &self,
        viewer: &Viewer,
        window: &mut Window,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let output = &viewer.output;
        match &viewer.body {
            Body::Loading => hint("Opening…", p).p_4().into_any_element(),
            Body::Failed(error) => hint(error.clone(), p).p_4().into_any_element(),
            Body::Markdown(text) => div()
                .id("board-markdown")
                .size_full()
                .overflow_y_scroll()
                .px_6()
                .py_4()
                .child(TextView::markdown(
                    SharedString::from(format!("board-markdown-{}", output.id)),
                    text.clone(),
                ))
                .into_any_element(),
            Body::Elsewhere => {
                let (folder, file) = (viewer.folder.clone(), output.file.clone());
                let kind = output.kind();
                let (lead, act) = match kind {
                    FileKind::Image | FileKind::Pdf => (
                        format!("{} opens in Quick Look.", type_label(kind)),
                        "Quick Look",
                    ),
                    _ => (
                        format!("Wiffletree cannot show {} itself.", output.file),
                        "Open in default app",
                    ),
                };
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .child(hint(lead, p))
                    .child(
                        button("boards-open-elsewhere")
                            .xsmall()
                            .label(act)
                            .on_click(move |_, _, cx| {
                                let Some(path) = contained(&folder, &file) else {
                                    return;
                                };
                                match kind {
                                    FileKind::Image | FileKind::Pdf => quick_look(&path),
                                    _ => open_in_default_app(&path, cx),
                                }
                            }),
                    )
                    .into_any_element()
            }
            Body::Html(pane) => self
                .html_pane(pane, output, window, p, cx)
                .into_any_element(),
        }
    }
    fn html_pane(
        &self,
        pane: &HtmlPane,
        output: &BoardOutput,
        window: &mut Window,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        // A board never draws over an overlay. Every menu and popover, this page's or another
        // view's, takes GPUI's focus as it opens; tooltips take none, so they are watched, as
        // are dialogs, sheets and notices.
        let (tooltip, again) = tooltip_frame(cx);
        if again {
            window.request_animation_frame();
        }
        let covered = window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || !window.notifications(cx).is_empty()
            || tooltip
            || !self.focus.contains_focused(window, cx);
        let painted = pane.painted.clone();
        let placeholder = canvas(
            |_, _, _| {},
            move |bounds, _, _, _| {
                // Whole points, as AppKit places the web view.
                let snapped = Bounds::new(
                    gpui::point(bounds.origin.x.round(), bounds.origin.y.round()),
                    gpui::size(bounds.size.width.round(), bounds.size.height.round()),
                );
                if !covered {
                    painted.set(Some(snapped));
                }
            },
        )
        .size_full();
        let shows = under_overlay(covered, pane.still.clone());
        let offered = pane.offered.clone();
        let url = format!("{}://{}/{}", board_web_view::SCHEME, output.id, output.file);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py(px(5.))
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::SEPARATOR))
                    .bg(p.base)
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(SANDBOX)
                    .child(div().ml_auto().child(url)),
            )
            .children(offered.map(|link| {
                let opened = link.clone();
                ui::banner(p.blue)
                    .flex_none()
                    .rounded_none()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(format!("The board links to {link}")),
                    )
                    .child(
                        button("board-open-browser")
                            .xsmall()
                            .label("Open in browser")
                            .on_click(cx.listener(move |v, _, _, c| {
                                c.open_url(&opened);
                                v.dismiss_offer(c);
                            })),
                    )
                    .child(
                        button("board-dismiss-link")
                            .ghost()
                            .xsmall()
                            .label("Dismiss")
                            .on_click(cx.listener(|v, _, _, c| v.dismiss_offer(c))),
                    )
            }))
            .children(pane.failed.clone().map(|error| hint(error, p).p_4()))
            .child(
                div()
                    .track_focus(&self.board_focus)
                    // A click on the still image brings the live board back.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|v, _, w, c| w.focus(&v.focus, c)),
                    )
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .bg(p.surface)
                    .child(placeholder)
                    .map(|d| match shows {
                        Shows::Live => d,
                        Shows::Still(image) => {
                            d.child(img(image).absolute().top_0().left_0().size_full())
                        }
                        Shows::Hidden => d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(output.title.clone()),
                                )
                                .child(hint(HIDDEN, p)),
                        ),
                    }),
            )
    }
    fn dismiss_offer(&mut self, cx: &mut Context<Self>) {
        if let Some(Viewer {
            body: Body::Html(pane),
            ..
        }) = &mut self.viewer
        {
            pane.offered = None;
        }
        cx.notify();
    }
    fn versions(&self, viewer: &Viewer, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut list = div()
            .id("board-versions")
            .flex_none()
            .w(px(200.))
            .h_full()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .border_l_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .text_size(px(font::SMALL))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Versions"));
        for (at, version) in viewer.versions.iter().enumerate() {
            let (when, by) = version_label(version, at == 0);
            let on = viewer.version.as_ref() == Some(&version.commit);
            let commit = version.commit.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!(
                        "board-version-{}",
                        version.commit
                    )))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(on, |d| d.bg(p.overlay))
                    .hover(|d| d.bg(p.overlay))
                    .on_click(cx.listener(move |v, _, w, c| v.pick_version(commit.clone(), w, c)))
                    .child(when)
                    .child(hint(by, p)),
            );
        }
        list
    }
}

/// What the board pane shows: the live board, or under an overlay its still image or, before
/// it has one, a plain stand-in.
enum Shows {
    Live,
    Still(Arc<Image>),
    Hidden,
}
fn under_overlay(covered: bool, still: Option<Arc<Image>>) -> Shows {
    match (covered, still) {
        (false, _) => Shows::Live,
        (true, Some(image)) => Shows::Still(image),
        (true, None) => Shows::Hidden,
    }
}

/// Whether a tooltip is open, which GPUI draws above every view, a board's included, and which
/// takes no focus. GPUI's own tooltips live from their creation to their release. A managed
/// tooltip, such as `Button::tooltip`'s, is built afresh in every frame by the window's tooltip
/// overlay, which draws after the page and notifies as one shows, switches or hides; so the
/// page's next frame knows whether the last one built one.
#[derive(Default)]
struct TooltipWatch {
    /// Tooltips alive now.
    open: usize,
    /// Tooltips built so far.
    built: u64,
    /// `built` at the page's last frame.
    seen: u64,
    /// The last frame built one.
    shown: bool,
    /// An overlay showed, switched or hid a tooltip since the page's last frame.
    changed: bool,
}
impl TooltipWatch {
    fn tooltip_built(&mut self) {
        self.open += 1;
        self.built += 1;
    }
    fn tooltip_released(&mut self) {
        self.open = self.open.saturating_sub(1);
    }
    fn overlay_changed(&mut self) {
        self.changed = true;
    }
    /// At the page's frame: whether a tooltip may be open, and whether to draw another frame,
    /// as one is needed to see what an overlay change, or a tooltip that came or went, now
    /// draws. A frame right after an overlay's notice counts as covered, since the overlay may
    /// be about to draw a tooltip over the board after the page.
    fn frame(&mut self) -> (bool, bool) {
        let shown = self.built != self.seen;
        self.seen = self.built;
        let changed = std::mem::take(&mut self.changed);
        let again = changed || shown != self.shown;
        self.shown = shown;
        (self.open > 0 || shown || changed, again)
    }
}
struct Tooltips(Rc<RefCell<TooltipWatch>>);
impl Global for Tooltips {}

/// Watches tooltips and the tooltip overlays; call once, before any window opens.
pub(super) fn watch_tooltips(cx: &mut App) {
    let watch = Rc::new(RefCell::new(TooltipWatch::default()));
    cx.set_global(Tooltips(watch.clone()));
    let tooltips = watch.clone();
    cx.observe_new::<gpui_component::tooltip::Tooltip>(move |_, _, cx| {
        tooltips.borrow_mut().tooltip_built();
        let tooltips = tooltips.clone();
        cx.on_release(move |_, _| tooltips.borrow_mut().tooltip_released())
            .detach();
    })
    .detach();
    cx.observe_new::<gpui_base::TooltipOverlay>(move |_, _, cx| {
        let watch = watch.clone();
        cx.observe_self(move |_, _| watch.borrow_mut().overlay_changed())
            .detach();
    })
    .detach();
}
fn tooltip_frame(cx: &App) -> (bool, bool) {
    cx.try_global::<Tooltips>()
        .map_or((false, false), |watch| watch.0.borrow_mut().frame())
}

/// `file` in `folder`, when it is one plain name of a file there, links resolved, so a native
/// read never leaves the output's folder.
fn contained(folder: &Path, file: &str) -> Option<PathBuf> {
    if !plain_file_name(file) {
        return None;
    }
    let root = folder.canonicalize().ok()?;
    let path = root.join(file).canonicalize().ok()?;
    (path.starts_with(&root) && path.is_file()).then_some(path)
}

/// Opens a file in its default app, or shows it in Finder when opening it would run it.
fn open_in_default_app(path: &Path, cx: &App) {
    match workspace_core::runs_when_opened(path) {
        true => cx.reveal_path(path),
        false => cx.open_with_system(path),
    }
}

/// Shows an image or PDF in Quick Look's own window.
fn quick_look(path: &Path) {
    let spawned = std::process::Command::new("/usr/bin/qlmanage")
        .arg("-p")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}

fn read_markdown(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_MARKDOWN_BYTES {
        return Err(
            "This Markdown file is too large to show here. Open it in its default app.".into(),
        );
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

fn type_label(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Markdown => "Markdown",
        FileKind::Html => "HTML",
        FileKind::Image => "Image",
        FileKind::Pdf => "PDF",
        FileKind::Other => "File",
    }
}

/// A tile's preview: an image itself, a Markdown file's first lines, an HTML file's title.
fn preview(output: &BoardOutput, p: Palette) -> Div {
    let thumb = div()
        .h(px(92.))
        .p_3()
        .flex()
        .flex_col()
        .gap_1()
        .overflow_hidden()
        .border_b_1()
        .border_color(p.edge.opacity(edge::CARD))
        .bg(p.base)
        .text_size(px(font::META))
        .text_color(p.subtle);
    match (output.kind(), &output.preview) {
        (FileKind::Image, _) => thumb.p_0().child(
            img(output.path())
                .size_full()
                .object_fit(gpui::ObjectFit::Cover),
        ),
        (kind, Some(text)) => thumb
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(type_label(kind)),
            )
            .child(text.clone()),
        (kind, None) => thumb
            .items_center()
            .justify_center()
            .child(icon("book").size(px(18.)))
            .child(type_label(kind)),
    }
}

impl Requests for BoardsPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}
impl Render for BoardsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        if self.bridge.remote().is_some() {
            return div().size_full().bg(p.base).child(ui::empty_state(
                "book",
                "Boards are on the host’s Mac",
                "Open Boards in Wiffletree on that Mac.",
                p,
            ));
        }
        let content = match &self.viewer {
            Some(viewer) => self.viewer(viewer, window, p, cx).into_any_element(),
            None => div()
                .id("boards-library")
                .size_full()
                .overflow_y_scroll()
                .px_5()
                .py_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(self.header(p, cx))
                        .child(match self.tab {
                            Tab::Outputs => self.library(p, cx),
                            Tab::Jobs => self.jobs_list(p, cx),
                        }),
                )
                .into_any_element(),
        };
        let (board, painted) = match &self.viewer {
            Some(Viewer {
                body: Body::Html(pane),
                ..
            }) => (Some(pane.board.clone()), Some(pane.painted.clone())),
            _ => (None, None),
        };
        // Painted after the rest of the page, so it knows whether the placeholder painted in
        // this frame and can place or hide the web view to match.
        let place_board = canvas(
            |_, _, _| {},
            move |_, _, _, _| {
                let (Some(board), Some(painted)) = (&board, &painted) else {
                    return;
                };
                if let Some(board) = board.borrow_mut().as_mut() {
                    board.place(painted.take());
                }
            },
        )
        .absolute()
        .size_0();
        div()
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(p.surface)
            .child(content)
            .child(place_board)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JobFilter, Shows, TooltipWatch, contained, filtered, keeping_label, source_label,
        under_overlay, version_label,
    };
    use gpui::{Image, ImageFormat};
    use std::sync::Arc;
    use workspace_core::SpaceId;
    use workspace_core::board::{BoardOutput, BoardVersion, Keeping};
    use workspace_core::job::JobKey;

    fn output(title: &str, space: &str, job: Option<&str>) -> BoardOutput {
        BoardOutput {
            id: title.into(),
            title: title.into(),
            space_id: SpaceId(space.into()),
            job: job.map(|j| JobKey::recipe(&SpaceId(space.into()), j)),
            job_name: job.map(str::to_owned),
            file: "out.md".into(),
            folder: "/tmp".into(),
            updated_at: 0,
            keeping: Keeping::Private,
            runner: None,
            stale: false,
            run_id: None,
            waiting: false,
            preview: None,
        }
    }

    #[test]
    fn an_overlay_before_the_first_still_hides_the_board_behind_a_stand_in() {
        assert!(matches!(under_overlay(false, None), Shows::Live));
        assert!(matches!(under_overlay(true, None), Shows::Hidden));
        let still = Arc::new(Image::from_bytes(ImageFormat::Png, vec![]));
        assert!(matches!(under_overlay(true, Some(still)), Shows::Still(_)));
    }

    #[test]
    fn a_managed_tooltip_built_afresh_each_frame_covers_the_board_until_it_hides() {
        let mut watch = TooltipWatch::default();
        // A button's tooltip shows: the page draws before the overlay builds it, so the board is
        // covered from that first frame, and it looks again.
        watch.overlay_changed();
        assert_eq!(watch.frame(), (true, true));
        let draw_overlay = |watch: &mut TooltipWatch| {
            watch.tooltip_built();
            watch.tooltip_released();
        };
        draw_overlay(&mut watch);
        assert_eq!(watch.frame(), (true, true));
        draw_overlay(&mut watch);
        assert_eq!(watch.frame(), (true, false));
        draw_overlay(&mut watch);
        // It hides: the next frame still shows the one built before, then the board returns.
        watch.overlay_changed();
        assert_eq!(watch.frame(), (true, true));
        assert_eq!(watch.frame(), (false, true));
        assert_eq!(watch.frame(), (false, false));
    }

    #[test]
    fn a_gpui_tooltip_covers_the_board_from_its_creation_to_its_release() {
        let mut watch = TooltipWatch::default();
        watch.tooltip_built();
        assert_eq!(watch.frame(), (true, true));
        assert_eq!(watch.frame(), (true, true));
        assert_eq!(watch.frame(), (true, false));
        watch.tooltip_released();
        assert_eq!(watch.frame(), (false, false));
    }

    #[test]
    fn a_native_read_stays_in_the_versions_folder() {
        let parent = tempfile::tempdir().unwrap();
        let folder = parent.path().join("brief");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("brief.md"), "# Brief").unwrap();
        std::fs::write(parent.path().join("secret.md"), "secret").unwrap();
        std::os::unix::fs::symlink(parent.path().join("secret.md"), folder.join("link.md"))
            .unwrap();
        assert!(contained(&folder, "brief.md").is_some());
        for file in ["../secret.md", "/etc/passwd", "link.md", "missing.md"] {
            assert!(contained(&folder, file).is_none(), "{file}");
        }
    }

    #[test]
    fn filters_and_search_narrow_the_library() {
        let outputs = [
            output("Release dashboard", "sage", Some("release-dashboard")),
            output("Morning brief", "sage", Some("morning-brief")),
            output("Competitor pricing", "vyttle", None),
        ];
        let titles = |shown: Vec<&BoardOutput>| -> Vec<String> {
            shown.into_iter().map(|o| o.title.clone()).collect()
        };
        let sage = SpaceId("sage".into());
        assert_eq!(
            titles(filtered(&outputs, Some(&sage), &JobFilter::All, "")).len(),
            2
        );
        assert_eq!(
            titles(filtered(&outputs, None, &JobFilter::Threads, "")),
            ["Competitor pricing"]
        );
        let brief = JobFilter::Job(JobKey::recipe(&sage, "morning-brief"));
        assert_eq!(
            titles(filtered(&outputs, None, &brief, "")),
            ["Morning brief"]
        );
        assert_eq!(
            titles(filtered(&outputs, None, &JobFilter::All, " DASH")),
            ["Release dashboard"]
        );
    }

    #[test]
    fn an_output_says_where_it_is_kept_and_what_wrote_it() {
        let mut shared = output("Release dashboard", "sage", Some("Release dashboard"));
        shared.keeping = Keeping::SharedStorage;
        assert_eq!(keeping_label(&shared, "Sage"), "Shared in Sage");
        assert_eq!(source_label(&shared), "Release dashboard job");
        let chart = output("Chart", "sage", None);
        assert_eq!(keeping_label(&chart, "Sage"), "Only on this Mac");
        assert_eq!(source_label(&chart), "From a thread");
        let mut deleted = shared.clone();
        deleted.job_name = None;
        assert_eq!(source_label(&deleted), "From a deleted job");
        let version = BoardVersion {
            commit: "c".into(),
            saved_at: 0,
            run_id: Some("r".into()),
            catch_up: 2,
            shared: false,
            local_only: false,
            file: "brief.md".into(),
        };
        assert_eq!(
            version_label(&version, true).1,
            "Latest · job run · catch-up"
        );
        let replaced = BoardVersion {
            local_only: true,
            catch_up: 0,
            ..version
        };
        assert_eq!(
            version_label(&replaced, true).1,
            "job run · Only on this Mac · not pushed"
        );
    }
}
