//! Development-only input injection, enabled by `--automation-socket`.
//!
//! Each JSON line received on the Unix socket becomes real input: mouse events are posted to the
//! application's AppKit queue, so hit-testing and focus behave as they do for a person, and keys
//! go through GPUI's keystroke dispatch. The pointer and other applications are not involved.
//! Nothing listens unless the flag is passed.

// objc 0.2's macros test a `cargo-clippy` feature this crate does not declare.
#![allow(unexpected_cfgs)]
use anyhow::Context as _;
use async_channel::{Receiver, Sender};
use gpui::{App, Keystroke, Modifiers, Window};
use objc::{class, msg_send, runtime::Object, sel, sel_impl};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::Path,
};

#[derive(Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Step {
    Click {
        x: f64,
        y: f64,
        #[serde(default = "single")]
        count: isize,
    },
    Move {
        x: f64,
        y: f64,
    },
    Scroll {
        x: f64,
        y: f64,
        dy: i32,
    },
    /// A keystroke in GPUI binding syntax, such as `enter` or `cmd-n`.
    Key {
        keystroke: String,
    },
    Type {
        text: String,
    },
    /// Reports the window's number and size so a caller can capture it.
    Window,
}

fn single() -> isize {
    1
}

pub type Request = (Step, Sender<Result<Value, String>>);

#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateScrollWheelEvent2(
        source: *const std::ffi::c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> *mut std::ffi::c_void;
    fn CGEventSetLocation(event: *mut std::ffi::c_void, location: NsPoint);
    fn CFRelease(object: *const std::ffi::c_void);
}

unsafe extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

extern "C" fn draw(context: *mut std::ffi::c_void) {
    // SAFETY: `context` is the window retained by `NativeWindow::draw_later`; this runs once on
    // the main thread, outside any GPUI update.
    unsafe {
        let window = context.cast::<Object>();
        let content: *mut Object = msg_send![window, contentView];
        let subviews: *mut Object = msg_send![content, subviews];
        let count: usize = msg_send![subviews, count];
        // GPUI's view draws a frame in its layer-delegate callback. AppKit does not call it for
        // a covered window, so find that view and call it directly.
        for index in 0..=count {
            let view: *mut Object = if index == count {
                content
            } else {
                msg_send![subviews, objectAtIndex: index]
            };
            let draws: bool = msg_send![view, respondsToSelector: sel!(displayLayer:)];
            if draws {
                let layer: *mut Object = msg_send![view, layer];
                let _: () = msg_send![view, displayLayer: layer];
                break;
            }
        }
        let _: () = msg_send![window, release];
    }
}

/// A retained window and event, delivered once the main queue is outside any GPUI update.
struct Delivery {
    window: *mut Object,
    event: *mut Object,
}

extern "C" fn deliver(context: *mut std::ffi::c_void) {
    // SAFETY: `context` is the box leaked by `NativeWindow::send_later`, and both objects were
    // retained there; this runs once on the main thread.
    unsafe {
        let delivery = Box::from_raw(context.cast::<Delivery>());
        let _: () = msg_send![delivery.window, sendEvent: delivery.event];
        let _: () = msg_send![delivery.event, release];
        let _: () = msg_send![delivery.window, release];
    }
}

const LEFT_MOUSE_DOWN: usize = 1;
const LEFT_MOUSE_UP: usize = 2;
const MOUSE_MOVED: usize = 5;
const SCROLL_UNIT_PIXEL: u32 = 0;

/// The application's one visible, main-capable window.
pub struct NativeWindow {
    window: *mut Object,
    number: isize,
}

impl NativeWindow {
    fn find() -> Result<Self, String> {
        // SAFETY: runs on the main thread, where AppKit objects may be messaged; the window list
        // is owned by NSApp and outlives this call.
        unsafe {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let windows: *mut Object = msg_send![app, windows];
            let count: usize = msg_send![windows, count];
            for index in 0..count {
                let window: *mut Object = msg_send![windows, objectAtIndex: index];
                let visible: bool = msg_send![window, isVisible];
                let main: bool = msg_send![window, canBecomeMainWindow];
                if visible && main {
                    let number: isize = msg_send![window, windowNumber];
                    return Ok(Self { window, number });
                }
            }
        }
        Err("No visible window".into())
    }

    /// Keeps the window composited at full size when another application is in front, so it can
    /// be captured without taking focus. Stage Manager otherwise shrinks it to a thumbnail.
    pub fn stay_on_stage() -> Result<(), String> {
        const STATIONARY: usize = 1 << 4;
        const IGNORES_CYCLE: usize = 1 << 6;
        let native = Self::find()?;
        // SAFETY: main thread; `window` is a live NSWindow owned by NSApp.
        unsafe {
            let _: () = msg_send![native.window, setCollectionBehavior: STATIONARY | IGNORES_CYCLE];
        }
        Ok(())
    }

    /// Draws a frame once the current GPUI update has finished.
    fn draw_later(&self) {
        // SAFETY: the window is retained until `draw` releases it on the main queue.
        unsafe {
            let _: *mut Object = msg_send![self.window, retain];
            dispatch_async_f(&raw const _dispatch_main_q, self.window.cast(), draw);
        }
    }

    /// Sends an event straight to the window after the current GPUI update has finished.
    fn send_later(&self, event: *mut Object) {
        // SAFETY: both objects are retained until `deliver` releases them on the main queue.
        unsafe {
            let _: *mut Object = msg_send![self.window, retain];
            let _: *mut Object = msg_send![event, retain];
            let delivery = Box::new(Delivery {
                window: self.window,
                event,
            });
            dispatch_async_f(
                &raw const _dispatch_main_q,
                Box::into_raw(delivery).cast(),
                deliver,
            );
        }
    }

    fn post(&self, event: *mut Object) {
        // SAFETY: `event` is a valid autoreleased NSEvent; posting only enqueues it.
        unsafe {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, postEvent: event atStart: false];
        }
    }

    /// Posts a mouse event at a point given in window coordinates with a top-left origin.
    fn mouse(&self, kind: usize, x: f64, y: f64, height: f64, click_count: isize) {
        let location = NsPoint { x, y: height - y };
        // SAFETY: the selector and argument types match NSEvent's documented class method.
        unsafe {
            let process: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
            let timestamp: f64 = msg_send![process, systemUptime];
            let event: *mut Object = msg_send![class!(NSEvent),
                mouseEventWithType: kind
                location: location
                modifierFlags: 0usize
                timestamp: timestamp
                windowNumber: self.number
                context: std::ptr::null_mut::<Object>()
                eventNumber: 0isize
                clickCount: click_count
                pressure: 1.0f32];
            self.post(event);
        }
    }

    /// Sends a scroll at a point given in window coordinates with a top-left origin.
    fn scroll(&self, x: f64, y: f64, height: f64, dy: i32) {
        #[repr(C)]
        struct NsRect {
            origin: NsPoint,
            size: NsPoint,
        }
        // SAFETY: the CGEvent is created, converted to an NSEvent that retains what it needs,
        // and released here; all calls happen on the main thread.
        unsafe {
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let primary: *mut Object = msg_send![screens, objectAtIndex: 0usize];
            let screen: NsRect = msg_send![primary, frame];
            let event =
                CGEventCreateScrollWheelEvent2(std::ptr::null(), SCROLL_UNIT_PIXEL, 1, dy, 0, 0);
            // An NSEvent made from a CGEvent has no window, so GPUI reads its screen location as
            // a window location. Place it where that reading gives the requested point.
            CGEventSetLocation(
                event,
                NsPoint {
                    x,
                    y: screen.size.y - (height - y),
                },
            );
            let ns_event: *mut Object = msg_send![class!(NSEvent), eventWithCGEvent: event];
            // AppKit routes queued scroll events by the real pointer, so hand this one to the
            // window directly.
            self.send_later(ns_event);
            CFRelease(event);
        }
    }
}

impl Step {
    pub fn perform(self, window: &mut Window, cx: &mut App) -> Result<Value, String> {
        // GPUI stops drawing a window that is covered by others; draw this one on request.
        window.refresh();
        NativeWindow::find()?.draw_later();
        let size = window.viewport_size();
        let height = f64::from(f32::from(size.height));
        match self {
            Self::Move { x, y } => NativeWindow::find()?.mouse(MOUSE_MOVED, x, y, height, 0),
            Self::Click { x, y, count } => {
                let native = NativeWindow::find()?;
                native.mouse(MOUSE_MOVED, x, y, height, 0);
                for click in 1..=count {
                    native.mouse(LEFT_MOUSE_DOWN, x, y, height, click);
                    native.mouse(LEFT_MOUSE_UP, x, y, height, click);
                }
            }
            Self::Scroll { x, y, dy } => {
                let native = NativeWindow::find()?;
                native.mouse(MOUSE_MOVED, x, y, height, 0);
                native.scroll(x, y, height, dy);
            }
            Self::Key { keystroke } => {
                let keystroke = Keystroke::parse(&keystroke).map_err(|e| e.to_string())?;
                window.dispatch_keystroke(keystroke, cx);
            }
            Self::Type { text } => {
                for character in text.chars() {
                    window.dispatch_keystroke(
                        Keystroke {
                            modifiers: Modifiers::none(),
                            key: character.to_string(),
                            key_char: Some(character.to_string()),
                        },
                        cx,
                    );
                }
            }
            Self::Window => {
                return Ok(json!({
                    "number": NativeWindow::find()?.number,
                    "width": f32::from(size.width),
                    "height": f32::from(size.height),
                }));
            }
        }
        Ok(Value::Null)
    }
}

/// Accepts connections on `path` and yields each step with a channel for its outcome.
pub fn listen(path: &Path) -> anyhow::Result<Receiver<Request>> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).context("Bind automation socket")?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let (requests, receiver) = async_channel::bounded(16);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Ok(mut writer) = stream.try_clone() else {
                continue;
            };
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let outcome = serde_json::from_str::<Step>(&line)
                    .map_err(|e| format!("Invalid step: {e}"))
                    .and_then(|step| {
                        let (reply, done) = async_channel::bounded(1);
                        requests
                            .send_blocking((step, reply))
                            .map_err(|_| "Window closed".to_owned())?;
                        done.recv_blocking()
                            .unwrap_or_else(|_| Err("Window closed".into()))
                    });
                let response = match outcome {
                    Ok(result) => json!({"ok": true, "result": result}),
                    Err(error) => json!({"ok": false, "error": error}),
                };
                if writeln!(writer, "{response}").is_err() {
                    break;
                }
            }
        }
    });
    Ok(receiver)
}
