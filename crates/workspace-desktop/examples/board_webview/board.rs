//! One open board: a WKWebView that wry places as a subview of GPUI's window view, over the
//! area a GPUI placeholder element reserves.

use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use block2::RcBlock;
use gpui::{Bounds, Image, ImageFormat, Pixels, Window};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSImage, NSView};
use objc2_foundation::{NSError, NSPoint};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowResponse, Rect, WebView, WebViewBuilder, WebViewExtMacOS, WryWebView};

use crate::scheme::{BoardFolder, SCHEME};

const ESCAPE_KEY_CODE: u16 = 53;

pub struct Board {
    webview: WebView,
    focused: Rc<Cell<bool>>,
    on_focus: Rc<dyn Fn(bool)>,
    monitor: Retained<AnyObject>,
    frame: Option<Bounds<Pixels>>,
}

impl Board {
    /// Creates the web view hidden; it shows once a frame places it. `on_focus` hears when
    /// keyboard focus moves into the board (`true`) or back to GPUI (`false`).
    pub fn open(
        window: &Window,
        folder: BoardFolder,
        on_focus: impl Fn(bool) + 'static,
    ) -> wry::Result<Self> {
        let url = folder.url("index.html");
        let board_origin = format!("{SCHEME}://");
        let webview = WebViewBuilder::new()
            .with_custom_protocol(SCHEME.into(), move |_, request| {
                folder.respond(request.uri())
            })
            .with_url(url)
            .with_incognito(true)
            .with_visible(false)
            .with_navigation_handler(move |url| url.starts_with(&board_origin))
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .build_as_child(window)?;
        let focused = Rc::new(Cell::new(false));
        let on_focus: Rc<dyn Fn(bool)> = Rc::new(on_focus);
        let monitor = watch_focus(webview.webview(), focused.clone(), on_focus.clone());
        Ok(Self {
            webview,
            focused,
            on_focus,
            monitor,
            frame: None,
        })
    }

    /// Shows the web view over `bounds` (window points, top-left origin), or hides it.
    pub fn place(&mut self, bounds: Option<Bounds<Pixels>>) {
        let native = self.webview.webview();
        let Some(bounds) = bounds else {
            if !native.isHidden() {
                native.setHidden(true);
                if self.focused.get() {
                    give_back_focus(&native);
                    self.focused.set(false);
                    (self.on_focus)(false);
                }
            }
            return;
        };
        if self.frame != Some(bounds) {
            let rect = Rect {
                position: LogicalPosition::new(
                    f64::from(bounds.origin.x),
                    f64::from(bounds.origin.y),
                )
                .into(),
                size: LogicalSize::new(f64::from(bounds.size.width), f64::from(bounds.size.height))
                    .into(),
            };
            self.webview
                .set_bounds(rect)
                .expect("child web view bounds");
            self.frame = Some(bounds);
        }
        if native.isHidden() {
            native.setHidden(false);
        }
    }

    /// Takes a still image of the page as it is drawn now; `done` runs on the main thread.
    pub fn snapshot(&self, done: impl Fn(Option<Arc<Image>>) + 'static) {
        let completion = RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
            // SAFETY: WebKit passes a valid image or null.
            let image = unsafe { image.as_ref() };
            let tiff = image.and_then(|image| image.TIFFRepresentation());
            done(tiff.map(|data| Arc::new(Image::from_bytes(ImageFormat::Tiff, data.to_vec()))));
        });
        // SAFETY: a nil configuration snapshots the visible page; the block matches the
        // documented completion handler.
        unsafe {
            self.webview
                .webview()
                .takeSnapshotWithConfiguration_completionHandler(None, &completion);
        }
    }

    pub fn has_focus(&self) -> bool {
        self.focused.get()
    }

    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    pub fn native(&self) -> Retained<WryWebView> {
        self.webview.webview()
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        // The monitor's block holds the web view; it must go before the web view can.
        // SAFETY: `monitor` is the object `addLocalMonitorForEventsMatchingMask` returned.
        unsafe { NSEvent::removeMonitor(&self.monitor) };
        if self.focused.get() {
            give_back_focus(&self.webview.webview());
        }
    }
}

/// Watches the application's events before AppKit delivers them: a click inside the web view
/// moves focus in, a click elsewhere in the window or Escape gives it back to GPUI's view.
fn watch_focus(
    webview: Retained<WryWebView>,
    focused: Rc<Cell<bool>>,
    on_focus: Rc<dyn Fn(bool)>,
) -> Retained<AnyObject> {
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid event for the duration of the call.
        let event_ref = unsafe { event.as_ref() };
        let in_window = webview
            .window()
            .is_some_and(|window| window.windowNumber() == event_ref.windowNumber());
        match event_ref.r#type() {
            NSEventType::LeftMouseDown | NSEventType::RightMouseDown if in_window => {
                let inside =
                    !webview.isHidden() && contains(&webview, event_ref.locationInWindow());
                if inside != focused.get() {
                    if !inside {
                        give_back_focus(&webview);
                    }
                    focused.set(inside);
                    on_focus(inside);
                }
            }
            NSEventType::KeyDown
                if in_window && focused.get() && event_ref.keyCode() == ESCAPE_KEY_CODE =>
            {
                give_back_focus(&webview);
                focused.set(false);
                on_focus(false);
                return std::ptr::null_mut();
            }
            _ => {}
        }
        event.as_ptr()
    });
    let mask = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::KeyDown;
    // SAFETY: the handler returns the event it was given or null to swallow it.
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) }
        .expect("local event monitor")
}

fn contains(view: &NSView, location_in_window: NSPoint) -> bool {
    let point = view.convertPoint_fromView(location_in_window, None);
    let bounds = view.bounds();
    point.x >= bounds.origin.x
        && point.y >= bounds.origin.y
        && point.x < bounds.origin.x + bounds.size.width
        && point.y < bounds.origin.y + bounds.size.height
}

/// GPUI's window view, which wry made the web view's parent.
pub fn parent(webview: &WryWebView) -> Option<Retained<NSView>> {
    // SAFETY: read on the main thread, where AppKit owns the view hierarchy.
    unsafe { webview.superview() }
}

/// Makes GPUI's window view the first responder again.
fn give_back_focus(webview: &WryWebView) {
    if let (Some(window), Some(parent)) = (webview.window(), parent(webview)) {
        window.makeFirstResponder(Some(&parent));
    }
}
