//! Development-only input injection, enabled by `--automation-socket`.
//!
//! Each JSON line received on the Unix socket becomes real input: mouse events are posted to the
//! application's AppKit queue, so hit-testing and focus behave as they do for a person, and keys
//! go through GPUI's keystroke dispatch, and file drops replay the events the macOS platform
//! sends for a Finder drag. The pointer and other applications are not involved.
//! Nothing listens unless the flag is passed.

// objc 0.2's macros test a `cargo-clippy` feature this crate does not declare.
#![allow(unexpected_cfgs)]
use anyhow::Context as _;
use async_channel::{Receiver, Sender};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gpui::{
    App, ExternalPaths, FileDropEvent, Keystroke, Modifiers, PlatformInput, Window, point, px,
};
use objc::{class, msg_send, runtime::Object, sel, sel_impl};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub(crate) enum Step {
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
    /// One part of a drag: `press`, any number of `drag` moves, then `release`. Each is its own
    /// step so the app draws a frame between them, as it does under a person's hand. `count`
    /// makes the press the second click of a double-click.
    Press {
        x: f64,
        y: f64,
        #[serde(default = "single")]
        count: isize,
        #[serde(default)]
        button: Button,
    },
    Drag {
        x: f64,
        y: f64,
    },
    Release {
        x: f64,
        y: f64,
        #[serde(default)]
        button: Button,
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
    /// Files dragged from Finder and dropped at a point.
    Drop {
        x: f64,
        y: f64,
        paths: Vec<PathBuf>,
    },
    /// Reports the window's number, size in points, scale and the largest size its display
    /// allows.
    Window,
    /// The window's own pixels as a PNG, at one pixel per point or at the display's scale. The
    /// app may capture its own windows without Screen Recording permission.
    Capture {
        #[serde(default)]
        native: bool,
    },
    /// Resizes the window's content to a size in points, kept within its minimum and its
    /// display, and moved to stay on that display. The new size shows in `window` once AppKit
    /// has applied it.
    Resize {
        width: f64,
        height: f64,
    },
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Button {
    #[default]
    Left,
    Right,
}

impl Button {
    fn down(self) -> usize {
        match self {
            Self::Left => LEFT_MOUSE_DOWN,
            Self::Right => RIGHT_MOUSE_DOWN,
        }
    }
    fn up(self) -> usize {
        match self {
            Self::Left => LEFT_MOUSE_UP,
            Self::Right => RIGHT_MOUSE_UP,
        }
    }
}

fn single() -> isize {
    1
}

pub(crate) type Request = (Step, Sender<Result<Value, String>>);

#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}

/// An `NSRect` or `CGRect`; `size` holds the width in `x` and the height in `y`.
#[repr(C)]
#[derive(Clone, Copy)]
struct NsRect {
    origin: NsPoint,
    size: NsPoint,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGColorSpaceSRGB: *const std::ffi::c_void;
    fn CGWindowListCreateImage(
        bounds: NsRect,
        list_option: u32,
        window: u32,
        image_option: u32,
    ) -> *mut std::ffi::c_void;
    fn CGImageGetWidth(image: *const std::ffi::c_void) -> usize;
    fn CGImageGetHeight(image: *const std::ffi::c_void) -> usize;
    fn CGColorSpaceCreateWithName(name: *const std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CGBitmapContextCreate(
        data: *mut std::ffi::c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *const std::ffi::c_void,
        bitmap_info: u32,
    ) -> *mut std::ffi::c_void;
    fn CGContextDrawImage(
        context: *mut std::ffi::c_void,
        rect: NsRect,
        image: *const std::ffi::c_void,
    );
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
const RIGHT_MOUSE_DOWN: usize = 3;
const RIGHT_MOUSE_UP: usize = 4;
const MOUSE_MOVED: usize = 5;
const LEFT_MOUSE_DRAGGED: usize = 6;
const SCROLL_UNIT_PIXEL: u32 = 0;
const LIST_INCLUDING_WINDOW: u32 = 1 << 3;
const IMAGE_IGNORE_FRAMING: u32 = 1 << 0;
const IMAGE_OPAQUE: u32 = 1 << 1;
const IMAGE_BEST_RESOLUTION: u32 = 1 << 3;
const IMAGE_NOMINAL_RESOLUTION: u32 = 1 << 4;
/// `kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big`: RGBA bytes in memory order.
const RGBA: u32 = 1 | 4 << 12;

/// Where a window may go: its frame, how much larger the frame is than the content, the
/// visible part of its display and its smallest content size.
struct Geometry {
    frame: NsRect,
    chrome: NsPoint,
    visible: NsRect,
    min: NsPoint,
}
impl Geometry {
    /// The largest content size the window's display allows.
    fn max(&self) -> NsPoint {
        NsPoint {
            x: self.visible.size.x - self.chrome.x,
            y: self.visible.size.y - self.chrome.y,
        }
    }
    /// The frame for content of `width` by `height` points, within the minimum and the display,
    /// keeping the top-left corner where it can and moved onto the display where it cannot.
    fn frame_for(&self, width: f64, height: f64) -> NsRect {
        let max = self.max();
        let size = NsPoint {
            x: width.clamp(self.min.x, max.x.max(self.min.x)) + self.chrome.x,
            y: height.clamp(self.min.y, max.y.max(self.min.y)) + self.chrome.y,
        };
        let top = self.frame.origin.y + self.frame.size.y;
        let (left, bottom) = (self.visible.origin.x, self.visible.origin.y);
        let right = left + self.visible.size.x - size.x;
        let top_limit = bottom + self.visible.size.y - size.y;
        NsRect {
            origin: NsPoint {
                x: self.frame.origin.x.min(right).max(left),
                y: (top - size.y).min(top_limit).max(bottom),
            },
            size,
        }
    }
}

/// The application's one visible, main-capable window.
pub(crate) struct NativeWindow {
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
    pub(crate) fn stay_on_stage() -> Result<(), String> {
        const STATIONARY: usize = 1 << 4;
        const IGNORES_CYCLE: usize = 1 << 6;
        let native = Self::find()?;
        // SAFETY: main thread; `window` is a live NSWindow owned by NSApp.
        unsafe {
            let _: () = msg_send![native.window, setCollectionBehavior: STATIONARY | IGNORES_CYCLE];
        }
        Ok(())
    }

    fn geometry(&self) -> Geometry {
        // SAFETY: main thread; the window and its screen are live AppKit objects, and these
        // getters only read them.
        unsafe {
            let frame: NsRect = msg_send![self.window, frame];
            let content: NsRect = msg_send![self.window, contentRectForFrameRect: frame];
            let mut screen: *mut Object = msg_send![self.window, screen];
            if screen.is_null() {
                screen = msg_send![class!(NSScreen), mainScreen];
            }
            let visible: NsRect = msg_send![screen, visibleFrame];
            let min: NsPoint = msg_send![self.window, contentMinSize];
            Geometry {
                frame,
                chrome: NsPoint {
                    x: frame.size.x - content.size.x,
                    y: frame.size.y - content.size.y,
                },
                visible,
                min,
            }
        }
    }

    /// The window's last composited frame as RGBA rows in sRGB, without its shadow, at one
    /// pixel per point or at the display's scale. Like any app's own windows, it needs no
    /// Screen Recording permission.
    fn capture(&self, native: bool) -> Result<image::RgbaImage, String> {
        let resolution = if native {
            IMAGE_BEST_RESOLUTION
        } else {
            IMAGE_NOMINAL_RESOLUTION
        };
        let null_rect = NsRect {
            origin: NsPoint {
                x: f64::INFINITY,
                y: f64::INFINITY,
            },
            size: NsPoint { x: 0., y: 0. },
        };
        // SAFETY: each Core Graphics object created here is checked for null and released
        // here; the bitmap context draws into `pixels`, which outlives it.
        unsafe {
            let image = CGWindowListCreateImage(
                null_rect,
                LIST_INCLUDING_WINDOW,
                self.number as u32,
                IMAGE_IGNORE_FRAMING | IMAGE_OPAQUE | resolution,
            );
            if image.is_null() {
                return Err("macOS gave no image of the window".into());
            }
            let (width, height) = (CGImageGetWidth(image), CGImageGetHeight(image));
            let mut pixels = vec![0u8; width * height * 4];
            let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            let context = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                space,
                RGBA,
            );
            if !context.is_null() {
                let bounds = NsRect {
                    origin: NsPoint { x: 0., y: 0. },
                    size: NsPoint {
                        x: width as f64,
                        y: height as f64,
                    },
                };
                CGContextDrawImage(context, bounds, image);
                CFRelease(context);
            }
            CFRelease(space);
            CFRelease(image);
            if context.is_null() {
                return Err("Could not read the window's image".into());
            }
            image::RgbaImage::from_raw(width as u32, height as u32, pixels)
                .ok_or_else(|| "The window's image has no pixels".into())
        }
    }

    /// Moves and resizes the window to `frame` once the current GPUI update has finished, as
    /// GPUI applies its own resizes.
    fn set_frame_later(&self, frame: NsRect) {
        struct Move {
            window: *mut Object,
            frame: NsRect,
        }
        extern "C" fn apply(context: *mut std::ffi::c_void) {
            // SAFETY: `context` is the box leaked below and the window was retained there;
            // this runs once on the main thread.
            unsafe {
                let change = Box::from_raw(context.cast::<Move>());
                let _: () =
                    msg_send![change.window, setFrame: change.frame display: objc::runtime::YES];
                let _: () = msg_send![change.window, release];
            }
        }
        // SAFETY: the window is retained until `apply` releases it on the main queue.
        unsafe {
            let _: *mut Object = msg_send![self.window, retain];
            let change = Box::new(Move {
                window: self.window,
                frame,
            });
            dispatch_async_f(
                &raw const _dispatch_main_q,
                Box::into_raw(change).cast(),
                apply,
            );
        }
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
    pub(crate) fn perform(self, window: &mut Window, cx: &mut App) -> Result<Value, String> {
        // GPUI stops drawing a window that is covered by others; draw this one on request.
        // Notifying the root asks for a frame without discarding the cached views, so a step
        // redraws what a person's input would.
        if let Some(Some(root)) = window.root::<gpui_component::Root>() {
            cx.notify(root.entity_id());
        }
        NativeWindow::find()?.draw_later();
        let size = window.viewport_size();
        let height = f64::from(f32::from(size.height));
        match self {
            Self::Move { x, y } => NativeWindow::find()?.mouse(MOUSE_MOVED, x, y, height, 0),
            Self::Press {
                x,
                y,
                count,
                button,
            } => {
                let native = NativeWindow::find()?;
                native.mouse(MOUSE_MOVED, x, y, height, 0);
                native.mouse(button.down(), x, y, height, count);
            }
            Self::Drag { x, y } => NativeWindow::find()?.mouse(LEFT_MOUSE_DRAGGED, x, y, height, 1),
            Self::Release { x, y, button } => {
                NativeWindow::find()?.mouse(button.up(), x, y, height, 1)
            }
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
            Self::Drop { x, y, paths } => {
                let position = point(px(x as f32), px(y as f32));
                for event in [
                    FileDropEvent::Entered {
                        position,
                        paths: ExternalPaths(paths.into()),
                    },
                    FileDropEvent::Pending { position },
                    FileDropEvent::Submit { position },
                    FileDropEvent::Exited,
                ] {
                    window.dispatch_event(PlatformInput::FileDrop(event), cx);
                }
            }
            Self::Window => {
                let native = NativeWindow::find()?;
                let max = native.geometry().max();
                return Ok(json!({
                    "number": native.number,
                    "width": f32::from(size.width),
                    "height": f32::from(size.height),
                    "scale": window.scale_factor(),
                    "max_width": max.x,
                    "max_height": max.y,
                }));
            }
            Self::Capture { native } => {
                let image = NativeWindow::find()?.capture(native)?;
                let mut png = std::io::Cursor::new(Vec::new());
                image
                    .write_to(&mut png, image::ImageFormat::Png)
                    .map_err(|e| e.to_string())?;
                return Ok(json!({
                    "png": BASE64.encode(png.into_inner()),
                    "width": image.width(),
                    "height": image.height(),
                }));
            }
            Self::Resize { width, height } => {
                let native = NativeWindow::find()?;
                let frame = native.geometry().frame_for(width, height);
                native.set_frame_later(frame);
            }
        }
        Ok(Value::Null)
    }
}

/// Accepts connections on `path` and yields each step with a channel for its outcome.
pub(crate) fn listen(path: &Path) -> anyhow::Result<Receiver<Request>> {
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
