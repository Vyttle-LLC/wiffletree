//! `--probe`: drives the prototype through each requirement with real AppKit events posted to
//! the application's queue, prints what passed and the memory it measured, then exits.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::{Duration, Instant};

use gpui::{App, AsyncApp, Bounds, Pixels, Point, WindowHandle, point, px, size};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};
use wry::WryWebView;

use crate::board::parent;
use crate::{Overlay, Page, Prototype};

const KEY_A: u16 = 0;
const KEY_W: u16 = 13;
const KEY_COMMA: u16 = 43;
const KEY_ESCAPE: u16 = 53;

pub fn run(window: WindowHandle<Prototype>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut report = Report::default();
        if let Err(error) = steps(window, cx, &mut report).await {
            report.check("the probe ran to the end", false, error);
        }
        std::process::exit(report.print());
    })
    .detach();
}

async fn steps(
    window: WindowHandle<Prototype>,
    cx: &mut AsyncApp,
    report: &mut Report,
) -> Result<(), String> {
    let update = |cx: &mut AsyncApp,
                  f: &mut dyn FnMut(
        &mut Prototype,
        &mut gpui::Window,
        &mut gpui::Context<Prototype>,
    )| {
        window
            .update(cx, |this, window, cx| f(this, window, cx))
            .map_err(|error| error.to_string())
    };
    let read = |cx: &mut AsyncApp| -> Result<State, String> {
        window
            .update(cx, |this, window, _| State::of(this, window))
            .map_err(|error| error.to_string())
    };

    wait(cx, 1000).await;
    let app = std::process::id() as i32;
    let webkit_before = webkit_processes();
    let app_before = footprint_mib(app);

    let opened = Instant::now();
    update(cx, &mut |this, window, cx| this.open_board(window, cx))?;
    let mut page = String::new();
    for _ in 0..50 {
        wait(cx, 100).await;
        page = eval(
            window,
            cx,
            "[probe.inline, probe.served, probe.fetch, probe.remote, probe.local].join(',')",
        )
        .await;
        if !page.is_empty() && !page.contains("pending") {
            break;
        }
    }
    report.measure(
        "board loaded and settled",
        format!("{} ms", opened.elapsed().as_millis()),
    );
    let flags: Vec<&str> = page.split(',').collect();
    report.check(
        "an inline script ran",
        flags.first() == Some(&"true"),
        &page,
    );
    report.check(
        "the scheme served a script file",
        flags.get(1) == Some(&"true"),
        &page,
    );
    report.check(
        "a fetch over the network failed",
        flags.get(2) == Some(&"blocked"),
        &page,
    );
    report.check(
        "an image over the network failed",
        flags.get(3) == Some(&"blocked"),
        &page,
    );
    report.check(
        "a file:// image failed",
        flags.get(4) == Some(&"blocked"),
        &page,
    );

    let state = read(cx)?;
    report.check(
        "the web view covers the placeholder",
        state.covers(),
        state.describe(),
    );
    let first = state.placed;

    update(cx, &mut |_, window, _| {
        window.resize(size(px(1240.), px(820.)))
    })?;
    wait(cx, 400).await;
    let state = read(cx)?;
    report.check(
        "it follows the placeholder when the window resizes",
        state.covers() && state.placed != first,
        state.describe(),
    );

    let before = state.placed;
    update(cx, &mut |this, _, cx| {
        this.wide_sidebar = true;
        cx.notify();
    })?;
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "it follows the placeholder when the panes move",
        state.covers() && state.placed.map(|b| b.origin.x) != before.map(|b| b.origin.x),
        state.describe(),
    );

    update(cx, &mut |this, _, cx| {
        this.page = Page::Other;
        cx.notify();
    })?;
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "it hides when the placeholder is not painted",
        state.hidden,
        state.describe(),
    );
    update(cx, &mut |this, _, cx| {
        this.page = Page::Board;
        cx.notify();
    })?;
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "it returns when the placeholder paints again",
        state.covers(),
        state.describe(),
    );

    update(cx, &mut |this, _, cx| this.toggle_menu(cx))?;
    for _ in 0..40 {
        wait(cx, 25).await;
        if read(cx)?.menu_open {
            break;
        }
    }
    wait(cx, 200).await;
    let state = read(cx)?;
    report.check(
        "a GPUI overlay draws over a still snapshot, with the web view hidden",
        state.menu_open && state.hidden && state.has_snapshot,
        state.describe(),
    );
    if let Some(took) = state.snapshot_took {
        report.measure(
            "snapshot before the overlay can draw",
            format!("{} ms", took.as_millis()),
        );
    }
    update(cx, &mut |this, _, cx| this.toggle_menu(cx))?;
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "the live board returns when the overlay closes",
        state.covers(),
        state.describe(),
    );

    let centre = state.placed.ok_or("the board is not placed")?.center();
    let sidebar = point(px(40.), px(300.));
    let native = read_native(window, cx)?;

    click(&native, centre);
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "a click moves keyboard focus into the web view",
        responder_is_webview(&native) && state.board_focus && !state.gpui_focus,
        state.describe(),
    );
    eval(window, cx, "document.getElementById('field').focus(); 'ok'").await;
    key(&native, "a", KEY_A, false);
    wait(cx, 300).await;
    let typed = eval(window, cx, "document.getElementById('field').value").await;
    report.check("typed keys reach the page", typed == "a", &typed);
    key(&native, "a", KEY_A, true);
    wait(cx, 300).await;
    let selected = eval(window, cx, "(() => { const f = document.getElementById('field'); return `${f.selectionStart}-${f.selectionEnd}`; })()").await;
    report.check(
        "⌘A reaches the page through the Edit menu",
        selected == "0-1",
        &selected,
    );

    key(&native, "\u{1b}", KEY_ESCAPE, false);
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "Escape gives focus back to GPUI",
        responder_is_parent(&native) && !state.board_focus && state.gpui_focus,
        state.describe(),
    );

    click(&native, centre);
    wait(cx, 300).await;
    click(&native, sidebar);
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "a click outside gives focus back to GPUI",
        responder_is_parent(&native) && !state.board_focus && state.gpui_focus,
        state.describe(),
    );

    click(&native, centre);
    wait(cx, 300).await;
    key(&native, ",", KEY_COMMA, true);
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "⌘, reaches GPUI while the board has focus",
        state.settings_presses == 1 && state.board_focus,
        state.describe(),
    );

    let content_process: i32 =
        // SAFETY: a private, read-only WKWebView property, used by this probe only.
        unsafe { objc2::msg_send![&*native, _webProcessIdentifier] };
    let webkit_open = webkit_processes();
    let started: BTreeMap<i32, String> = webkit_open
        .iter()
        .filter(|(pid, _)| !webkit_before.contains_key(pid))
        .map(|(pid, name)| (*pid, name.clone()))
        .collect();
    let app_open = footprint_mib(app);
    let webkit_open_mib: f64 = started
        .keys()
        .filter_map(|pid| footprint_mib(*pid))
        .fold(0.0, |total, each| total + each);
    report.measure("app footprint before the board", mib(app_before));
    report.measure("app footprint with the board open", mib(app_open));
    report.measure(
        "WebKit processes the board started",
        describe_processes(&started),
    );
    report.measure(
        "their footprint with the board open",
        mib(Some(webkit_open_mib)),
    );
    report.measure(
        "the board's WebContent process",
        content_process.to_string(),
    );

    key(&native, "w", KEY_W, true);
    drop(native);
    let pressed = Instant::now();
    wait(cx, 300).await;
    let state = read(cx)?;
    report.check(
        "⌘W reaches GPUI while the board has focus",
        !state.board_open,
        state.describe(),
    );
    while alive(content_process) && pressed.elapsed() < Duration::from_secs(10) {
        wait(cx, 20).await;
    }
    let released = pressed.elapsed();
    report.check(
        "closing the board ends its WebContent process",
        !alive(content_process),
        format!("pid {content_process} still running after 10 s"),
    );
    report.measure(
        "WebContent process gone after ⌘W (checked from 300 ms)",
        format!("{} ms", released.as_millis()),
    );
    wait(cx, 3000).await;
    let remaining: BTreeMap<i32, String> = webkit_processes()
        .into_iter()
        .filter(|(pid, _)| !webkit_before.contains_key(pid))
        .collect();
    let remaining_mib: f64 = remaining
        .keys()
        .filter_map(|pid| footprint_mib(*pid))
        .fold(0.0, |total, each| total + each);
    report.measure("app footprint 3 s after closing", mib(footprint_mib(app)));
    report.measure(
        "WebKit processes still running",
        describe_processes(&remaining),
    );
    report.measure("their footprint after closing", mib(Some(remaining_mib)));
    Ok(())
}

/// What the probe reads back from the prototype after each step.
struct State {
    placed: Option<Bounds<Pixels>>,
    frame: Option<Bounds<Pixels>>,
    hidden: bool,
    board_open: bool,
    board_focus: bool,
    gpui_focus: bool,
    menu_open: bool,
    has_snapshot: bool,
    snapshot_took: Option<Duration>,
    settings_presses: usize,
}

impl State {
    fn of(prototype: &Prototype, window: &gpui::Window) -> Self {
        let board = prototype.board.borrow();
        let native = board.as_ref().map(|board| board.native());
        Self {
            placed: prototype.placed.get(),
            frame: native.as_deref().map(frame_in_window),
            hidden: native.as_ref().is_none_or(|native| native.isHidden()),
            board_open: board.is_some(),
            board_focus: prototype.board_has_focus(),
            gpui_focus: prototype.focus.is_focused(window),
            menu_open: prototype.menu == Overlay::Open,
            has_snapshot: prototype.snapshot.is_some(),
            snapshot_took: prototype.snapshot_took,
            settings_presses: prototype.settings_presses,
        }
    }

    /// The web view is shown exactly over the placeholder, to within AppKit's whole points.
    fn covers(&self) -> bool {
        let (Some(placed), Some(frame)) = (self.placed, self.frame) else {
            return false;
        };
        let close = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() <= 1.0;
        !self.hidden
            && close(placed.origin.x, frame.origin.x)
            && close(placed.origin.y, frame.origin.y)
            && close(placed.size.width, frame.size.width)
            && close(placed.size.height, frame.size.height)
    }

    fn describe(&self) -> String {
        format!(
            "placeholder {:?}, web view {:?}, hidden {}, board focus {}, GPUI focus {}",
            self.placed.map(short),
            self.frame.map(short),
            self.hidden,
            self.board_focus,
            self.gpui_focus,
        )
    }
}

fn short(bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
    (
        bounds.origin.x.into(),
        bounds.origin.y.into(),
        bounds.size.width.into(),
        bounds.size.height.into(),
    )
}

/// The web view's frame in GPUI's window coordinates: points from the top-left corner.
fn frame_in_window(native: &WryWebView) -> Bounds<Pixels> {
    let frame = native.frame();
    let parent = parent(native).expect("web view parent");
    let top = if parent.isFlipped() {
        frame.origin.y
    } else {
        parent.bounds().size.height - frame.origin.y - frame.size.height
    };
    Bounds::new(
        point(px(frame.origin.x as f32), px(top as f32)),
        size(px(frame.size.width as f32), px(frame.size.height as f32)),
    )
}

fn read_native(
    window: WindowHandle<Prototype>,
    cx: &mut AsyncApp,
) -> Result<Retained<WryWebView>, String> {
    window
        .read_with(cx, |this, _| {
            this.board.borrow().as_ref().map(|board| board.native())
        })
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the board is closed".to_owned())
}

async fn eval(window: WindowHandle<Prototype>, cx: &mut AsyncApp, script: &str) -> String {
    let (sender, receiver) = async_channel::bounded(1);
    let sent = window.read_with(cx, |this, _| {
        this.board.borrow().as_ref().map(|board| {
            board
                .webview()
                .evaluate_script_with_callback(script, move |result| {
                    let _ = sender.try_send(result);
                })
        })
    });
    if !matches!(sent, Ok(Some(Ok(())))) {
        return String::new();
    }
    let timeout = cx.background_executor().timer(Duration::from_secs(2));
    let result = futures_lite::future::or(async { receiver.recv().await.ok() }, async {
        timeout.await;
        None
    })
    .await;
    result.unwrap_or_default().trim_matches('"').to_owned()
}

async fn wait(cx: &mut AsyncApp, milliseconds: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(milliseconds))
        .await;
}

fn responder_is_webview(native: &WryWebView) -> bool {
    let responder = native.window().and_then(|window| window.firstResponder());
    responder.is_some_and(|responder| std::ptr::addr_eq(Retained::as_ptr(&responder), native))
}

fn responder_is_parent(native: &WryWebView) -> bool {
    let responder = native.window().and_then(|window| window.firstResponder());
    let parent = parent(native);
    match (responder, parent) {
        (Some(responder), Some(parent)) => {
            std::ptr::addr_eq(Retained::as_ptr(&responder), Retained::as_ptr(&parent))
        }
        _ => false,
    }
}

/// Posts a left click at a GPUI window point to the application's event queue, so hit-testing,
/// the first responder and event monitors behave as they do for a person.
fn click(native: &WryWebView, at: Point<Pixels>) {
    let parent = parent(native).expect("web view parent");
    let window = native.window().expect("web view window");
    let y = if parent.isFlipped() {
        f64::from(at.y)
    } else {
        parent.bounds().size.height - f64::from(at.y)
    };
    let location = parent.convertPoint_toView(NSPoint::new(f64::from(at.x), y), None);
    for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            location,
            NSEventModifierFlags::empty(),
            0.0,
            window.windowNumber(),
            None,
            0,
            1,
            1.0,
        )
        .expect("mouse event");
        post(&event);
    }
}

fn key(native: &WryWebView, characters: &str, key_code: u16, command: bool) {
    let window = native.window().expect("web view window");
    let flags = if command {
        NSEventModifierFlags::Command
    } else {
        NSEventModifierFlags::empty()
    };
    let characters = NSString::from_str(characters);
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            window.windowNumber(),
            None,
            &characters,
            &characters,
            false,
            key_code,
        )
        .expect("key event");
        post(&event);
    }
}

fn post(event: &NSEvent) {
    let main_thread = MainThreadMarker::new().expect("the probe runs on the main thread");
    NSApplication::sharedApplication(main_thread).postEvent_atStart(event, false);
}

/// Running WebKit helper processes by pid. They are XPC services started by launchd, so the
/// probe tells the board's from others by comparing with the list taken before it opened.
fn webkit_processes() -> BTreeMap<i32, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,comm="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(' ')?;
            let name = command
                .rsplit('/')
                .next()?
                .strip_prefix("com.apple.WebKit.")?;
            Some((pid.parse().ok()?, name.to_owned()))
        })
        .collect()
}

fn describe_processes(processes: &BTreeMap<i32, String>) -> String {
    if processes.is_empty() {
        return "none".into();
    }
    let described: Vec<String> = processes
        .iter()
        .map(|(pid, name)| format!("{name} ({pid}, {})", mib(footprint_mib(*pid))))
        .collect();
    described.join(", ")
}

/// The process's physical footprint, the figure Activity Monitor shows as Memory.
fn footprint_mib(pid: i32) -> Option<f64> {
    // SAFETY: `info` is a correctly sized rusage_info_v2 for the flavour requested.
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let buffer = (&raw mut info).cast::<libc::rusage_info_t>();
        (libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, buffer) == 0)
            .then(|| info.ri_phys_footprint as f64 / 1_048_576.0)
    }
}

fn mib(value: Option<f64>) -> String {
    value.map_or("unreadable".into(), |value| format!("{value:.1} MiB"))
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[derive(Default)]
struct Report {
    checks: Vec<(String, bool, String)>,
    measurements: Vec<(String, String)>,
}

impl Report {
    fn check(&mut self, name: &str, passed: bool, detail: impl Into<String>) {
        self.checks.push((name.to_owned(), passed, detail.into()));
    }

    fn measure(&mut self, name: &str, value: String) {
        self.measurements.push((name.to_owned(), value));
    }

    /// Prints the report and returns the exit code: 1 when any check failed.
    fn print(&self) -> i32 {
        println!("Checks");
        for (name, passed, detail) in &self.checks {
            let mark = if *passed { "pass" } else { "FAIL" };
            println!("  {mark}  {name}");
            if !passed {
                println!("        {detail}");
            }
        }
        println!("Measurements");
        for (name, value) in &self.measurements {
            println!("  {name}: {value}");
        }
        i32::from(self.checks.iter().any(|(_, passed, _)| !passed))
    }
}
