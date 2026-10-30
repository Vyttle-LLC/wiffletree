//! The board web view prototype for #163 task 4.1: a WKWebView placed over a GPUI placeholder
//! element. A development example only; nothing in the app uses it.
//!
//! ```sh
//! cargo run -p workspace-desktop --example board_webview              # try it by hand
//! cargo run -p workspace-desktop --example board_webview -- --probe   # scripted checks
//! ```

mod board;
mod probe;
mod scheme;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, Context, Div, FocusHandle, Image, KeyBinding, Menu, MenuItem, OsAction, Pixels,
    SharedString, Stateful, Window, WindowBounds, WindowOptions, actions, anchored, canvas,
    deferred, div, img, point, prelude::*, px, rgb, size,
};

use board::Board;
use scheme::BoardFolder;

actions!(
    board_webview,
    [CloseBoard, OpenSettings, Quit, Copy, SelectAll]
);

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/board_webview/fixture"
);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Board,
    Other,
}

#[derive(Clone, Copy, PartialEq)]
enum Overlay {
    Closed,
    /// Waiting for the board's snapshot before the menu draws over it.
    Opening(Instant),
    Open,
}

/// What the board reports from AppKit callbacks, which have no GPUI context.
enum BoardEvent {
    Focus(bool),
    Snapshot(Option<Arc<Image>>),
}

pub struct Prototype {
    board: Rc<RefCell<Option<Board>>>,
    events: async_channel::Sender<BoardEvent>,
    page: Page,
    wide_sidebar: bool,
    menu: Overlay,
    snapshot: Option<Arc<Image>>,
    snapshot_took: Option<Duration>,
    /// The placeholder's bounds when it painted in the current frame.
    painted: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Where the last frame put the web view; `None` when it hid it.
    placed: Rc<Cell<Option<Bounds<Pixels>>>>,
    settings_presses: usize,
    focus: FocusHandle,
    /// GPUI's focus while the web view has the keyboard: under the window's root, so window
    /// bindings still dispatch, and with no text bindings of its own.
    board_focus: FocusHandle,
}

impl Prototype {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, received) = async_channel::unbounded();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = received.recv().await {
                let _ = this.update_in(cx, |this, window, cx| this.board_event(event, window, cx));
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            board: Rc::default(),
            events,
            page: Page::Board,
            wide_sidebar: false,
            menu: Overlay::Closed,
            snapshot: None,
            snapshot_took: None,
            painted: Rc::default(),
            placed: Rc::default(),
            settings_presses: 0,
            focus,
            board_focus: cx.focus_handle(),
        }
    }

    fn open_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.borrow().is_some() {
            return;
        }
        let folder = BoardFolder::new("fixture", Path::new(FIXTURE)).expect("fixture folder");
        let events = self.events.clone();
        let on_focus = move |inside| {
            let _ = events.try_send(BoardEvent::Focus(inside));
        };
        let board = Board::open(window, folder, on_focus).expect("child web view");
        *self.board.borrow_mut() = Some(board);
        self.page = Page::Board;
        cx.notify();
    }

    fn close_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let closed = self.board.borrow_mut().take();
        drop(closed);
        self.close_menu();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu != Overlay::Closed {
            self.close_menu();
        } else if let Some(board) = self.board.borrow().as_ref() {
            self.menu = Overlay::Opening(Instant::now());
            let events = self.events.clone();
            board.snapshot(move |image| {
                let _ = events.try_send(BoardEvent::Snapshot(image));
            });
        } else {
            self.menu = Overlay::Open;
        }
        cx.notify();
    }

    fn close_menu(&mut self) {
        self.menu = Overlay::Closed;
        self.snapshot = None;
    }

    fn board_event(&mut self, event: BoardEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            BoardEvent::Focus(true) => window.focus(&self.board_focus, cx),
            BoardEvent::Focus(false) => window.focus(&self.focus, cx),
            BoardEvent::Snapshot(image) => {
                if let Overlay::Opening(started) = self.menu {
                    self.snapshot_took = Some(started.elapsed());
                    self.snapshot = image;
                    self.menu = Overlay::Open;
                }
            }
        }
        cx.notify();
    }

    fn board_has_focus(&self) -> bool {
        self.board.borrow().as_ref().is_some_and(Board::has_focus)
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.board.borrow().is_some();
        div()
            .flex()
            .gap_2()
            .p_2()
            .bg(rgb(0x2b2d31))
            .child(if open {
                button("close", "Close board (⌘W)", cx, |this, window, cx| {
                    this.close_board(window, cx)
                })
            } else {
                button("open", "Open board", cx, |this, window, cx| {
                    this.open_board(window, cx)
                })
            })
            .child(button("page", "Switch page", cx, |this, _, cx| {
                this.page = match this.page {
                    Page::Board => Page::Other,
                    Page::Other => Page::Board,
                };
                cx.notify();
            }))
            .child(button("panes", "Move panes", cx, |this, _, cx| {
                this.wide_sidebar = !this.wide_sidebar;
                cx.notify();
            }))
            .child(button("menu", "Menu", cx, |this, _, cx| {
                this.toggle_menu(cx)
            }))
            .child(div().px_2().py_1().child(format!(
                    "Board focus: {} · ⌘, presses: {} · last snapshot: {}",
                    if self.board_has_focus() { "in" } else { "out" },
                    self.settings_presses,
                    self.snapshot_took
                        .map_or("none".into(), |took| format!("{} ms", took.as_millis())),
                )))
    }

    fn render_content(&self, frozen: bool) -> impl IntoElement {
        let content = div().flex_1().h_full().p_4();
        match (self.page, self.board.borrow().is_some()) {
            (Page::Board, true) => {
                let painted = self.painted.clone();
                let placeholder = canvas(
                    |_, _, _| {},
                    move |bounds, _, _, _| painted.set(Some(bounds)),
                )
                .size_full();
                let still = frozen.then(|| self.snapshot.clone()).flatten();
                content.child(
                    div()
                        .track_focus(&self.board_focus)
                        .relative()
                        .size_full()
                        .bg(rgb(0xffffff))
                        .child(placeholder)
                        .when_some(still, |pane, image| {
                            pane.child(img(image).absolute().top_0().left_0().size_full())
                        }),
                )
            }
            (Page::Board, false) => content.child("No board is open."),
            (Page::Other, _) => content.child("Another page. The board is not painted here."),
        }
    }
}

impl Render for Prototype {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frozen = self.menu != Overlay::Closed;
        let (board, painted, placed) = (
            self.board.clone(),
            self.painted.clone(),
            self.placed.clone(),
        );
        // Painted after everything else in the window, so it knows whether the placeholder
        // painted in this frame and can place or hide the web view to match.
        let place_board = canvas(
            |_, _, _| {},
            move |_, _, _, _| {
                let bounds = painted.take().filter(|_| !frozen);
                placed.set(bounds);
                if let Some(board) = board.borrow_mut().as_mut() {
                    board.place(bounds);
                }
            },
        )
        .absolute()
        .size_0();
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xe6e6e6))
            .text_sm()
            .on_action(cx.listener(|this, _: &CloseBoard, window, cx| this.close_board(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.settings_presses += 1;
                cx.notify();
            }))
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(if self.wide_sidebar { 320. } else { 160. }))
                            .h_full()
                            .p_3()
                            .bg(rgb(0x25272b))
                            .child("Sidebar"),
                    )
                    .child(self.render_content(frozen)),
            )
            .when(self.menu == Overlay::Open, |root| {
                root.child(deferred(
                    anchored().position(point(px(380.), px(48.))).child(
                        div()
                            .w(px(240.))
                            .p_3()
                            .rounded_md()
                            .bg(rgb(0x3a3d44))
                            .shadow_lg()
                            .child("A GPUI menu drawn over the board's still image."),
                    ),
                ))
            })
            .child(place_board)
    }
}

fn button(
    id: &'static str,
    label: &'static str,
    cx: &mut Context<Prototype>,
    on_click: impl Fn(&mut Prototype, &mut Window, &mut Context<Prototype>) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .bg(rgb(0x3a3d44))
        .cursor_pointer()
        .child(SharedString::from(label))
        .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
}

fn main() {
    let probe = std::env::args().any(|argument| argument == "--probe");
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("cmd-w", CloseBoard, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-c", Copy, None),
            KeyBinding::new("cmd-a", SelectAll, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // wry's child web view declines every key equivalent, so editing shortcuts reach the page
        // only as these menu items, which AppKit sends to the first responder.
        cx.set_menus(vec![
            Menu::new("Board prototype").items([MenuItem::action("Quit", Quit)]),
            Menu::new("Edit").items([
                MenuItem::os_action("Copy", Copy, OsAction::Copy),
                MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            ]),
        ]);
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let window = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| Prototype::new(window, cx))
            })
            .expect("prototype window");
        cx.activate(true);
        if probe {
            probe::run(window, cx);
        }
    });
}
