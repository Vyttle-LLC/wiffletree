//! An HTML board: a WKWebView that wry places as a subview of GPUI's window view, over the area
//! a GPUI placeholder element reserves (design, decision 6). Board content is untrusted, so the
//! web view loads only its output's folder through `wiffletree-board://`, a content rule list
//! blocks every other scheme, it never navigates or opens a window, it is refused the camera,
//! microphone, downloads and file pickers, and its data store is not persistent. It has no IPC
//! bridge to the app. The board web view example includes this file, so it needs nothing else
//! of the app.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use block2::{DynBlock, RcBlock};
use gpui::{Bounds, Image, ImageFormat, Pixels, Window};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSImage, NSView};
use objc2_foundation::{NSError, NSPoint, NSString};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKFrameInfo, WKMediaCaptureType, WKNavigationAction,
    WKPermissionDecision, WKSecurityOrigin, WKUIDelegate, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore, WKWindowFeatures,
};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{Response, StatusCode, Uri, header};
use wry::{Rect, WebView, WebViewBuilder, WebViewBuilderExtMacos, WebViewExtMacOS, WryWebView};

pub const SCHEME: &str = "wiffletree-board";

/// The policy every board response carries.
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'self'; \
    style-src 'unsafe-inline' 'self'; img-src 'self' data: blob:; font-src 'self' data:; \
    connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

/// Blocks every load whose scheme is not the board's, so a request, fetch or socket fails even
/// if the policy were bypassed. `data:` and `blob:` stay, as the policy allows them for images
/// and fonts; neither can reach anything outside the page.
const RULES: &str = r#"[
  {"trigger": {"url-filter": ".*"}, "action": {"type": "block"}},
  {"trigger": {"url-filter": "^wiffletree-board:"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^blob:"}, "action": {"type": "ignore-previous-rules"}}
]"#;
const RULES_ID: &str = "wiffletree-board-v1";

const ESCAPE_KEY_CODE: u16 = 53;

/// One output's folder, served as `wiffletree-board://<id>/<path>`.
pub struct BoardFolder {
    id: String,
    root: PathBuf,
}

impl BoardFolder {
    pub fn new(id: &str, root: &Path) -> io::Result<Self> {
        Ok(Self {
            id: id.to_owned(),
            root: root.canonicalize()?,
        })
    }

    /// The URL of `path` in the folder, with every byte but a path's unreserved ones escaped,
    /// so a `#`, `?` or `%` in a file's name stays part of it.
    pub fn url(&self, path: &str) -> String {
        let escaped: String = (path.bytes())
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                    char::from(byte).to_string()
                }
                byte => format!("%{byte:02X}"),
            })
            .collect();
        format!("{SCHEME}://{}/{escaped}", self.id)
    }

    pub fn respond(&self, uri: &Uri) -> Response<Cow<'static, [u8]>> {
        let response = Response::builder().header("Content-Security-Policy", CSP);
        let response = match self.read(uri) {
            Some((body, content_type)) => response
                .header(header::CONTENT_TYPE, content_type)
                .body(Cow::Owned(body)),
            None => response
                .status(StatusCode::NOT_FOUND)
                .body(Cow::Borrowed(&[][..])),
        };
        response.expect("static headers are valid")
    }

    /// Reads a file only when the URL names this board and the path, decoded and with its
    /// symbolic links resolved, is still inside its folder.
    fn read(&self, uri: &Uri) -> Option<(Vec<u8>, &'static str)> {
        if uri.scheme_str() != Some(SCHEME) || uri.host() != Some(self.id.as_str()) {
            return None;
        }
        let path = percent_decoded(uri.path())?;
        let file = self
            .root
            .join(path.trim_start_matches('/'))
            .canonicalize()
            .ok()?;
        if !file.starts_with(&self.root) || !file.is_file() {
            return None;
        }
        Some((std::fs::read(&file).ok()?, content_type(&file)))
    }
}

/// A URL path with its `%XX` escapes decoded, or `None` when one is malformed or the result is
/// not UTF-8 or holds a NUL.
fn percent_decoded(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.contains('\0')).then_some(decoded)
}

fn content_type(file: &Path) -> &'static str {
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "txt" | "csv" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// What a board reports from AppKit and WebKit callbacks, which have no GPUI context.
pub enum BoardEvent {
    /// Keyboard focus moved into the board (`true`) or back to GPUI (`false`).
    Focus(bool),
    /// The page asked to go to a web address; the board stayed, and offers to open it.
    Link(String),
}

/// The board's compiled content rule list, or why WebKit could not compile it.
pub type Rules = Result<Retained<WKContentRuleList>, String>;

thread_local! {
    static COMPILED: RefCell<Compiled> = const { RefCell::new(Compiled::NotYet) };
}
enum Compiled {
    NotYet,
    Compiling(Vec<Box<dyn FnOnce(Rules)>>),
    Done(Rules),
}

/// Hands `done` the board's content rule list, compiled once per launch. WebKit compiles it
/// asynchronously; `done` runs on the main thread.
pub fn with_rules(done: impl FnOnce(Rules) + 'static) {
    let compiled = COMPILED.with_borrow(|compiled| match compiled {
        Compiled::Done(rules) => Some(rules.clone()),
        _ => None,
    });
    if let Some(rules) = compiled {
        return done(rules);
    }
    let compile = COMPILED.with_borrow_mut(|compiled| {
        let compile = matches!(compiled, Compiled::NotYet);
        if compile {
            *compiled = Compiled::Compiling(vec![]);
        }
        if let Compiled::Compiling(waiting) = compiled {
            waiting.push(Box::new(done));
        }
        compile
    });
    if compile {
        compile_rules();
    }
}

fn compile_rules() {
    let finished = |rules: Rules| {
        let waiting = COMPILED.with_borrow_mut(|compiled| {
            match std::mem::replace(compiled, Compiled::Done(rules.clone())) {
                Compiled::Compiling(waiting) => waiting,
                _ => vec![],
            }
        });
        for done in waiting {
            done(rules.clone());
        }
    };
    let mtm = MainThreadMarker::new().expect("boards open on the main thread");
    // SAFETY: called on the main thread; WebKit calls the block once, on the main thread.
    unsafe {
        let Some(store) = WKContentRuleListStore::defaultStore(mtm) else {
            return finished(Err("WebKit has no content rule list store".into()));
        };
        let completion = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
            let list = Retained::retain(list);
            finished(list.ok_or_else(|| match error.as_ref() {
                Some(error) => error.localizedDescription().to_string(),
                None => "WebKit did not compile the board's rules".into(),
            }));
        });
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(RULES_ID)),
            Some(&NSString::from_str(RULES)),
            Some(&completion),
        );
    }
}

struct RefusingIvars {
    on_link: Box<dyn Fn(String)>,
}

define_class!(
    /// Refuses what wry's own UI delegate would allow: the camera, the microphone and motion,
    /// and a new window, offering a web address it named instead. It leaves out the file
    /// panel, which WebKit on macOS then treats as cancelled.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WiffletreeBoardUIDelegate"]
    #[ivars = RefusingIvars]
    struct RefusingUiDelegate;

    unsafe impl NSObjectProtocol for RefusingUiDelegate {}

    unsafe impl WKUIDelegate for RefusingUiDelegate {
        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn media_capture(
            &self,
            _: &WKWebView,
            _: &WKSecurityOrigin,
            _: &WKFrameInfo,
            _: WKMediaCaptureType,
            decision: &DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            decision.call((WKPermissionDecision::Deny,));
        }

        #[unsafe(method(webView:requestDeviceOrientationAndMotionPermissionForOrigin:initiatedByFrame:decisionHandler:))]
        fn motion(
            &self,
            _: &WKWebView,
            _: &WKSecurityOrigin,
            _: &WKFrameInfo,
            decision: &DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            decision.call((WKPermissionDecision::Deny,));
        }

        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn new_window(
            &self,
            _: &WKWebView,
            _: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            // SAFETY: read on the main thread from the action WebKit passed.
            let url = unsafe { action.request().URL().and_then(|url| url.absoluteString()) };
            if let Some(url) = url
                .map(|url| url.to_string())
                .filter(|url| web_address(url))
            {
                (self.ivars().on_link)(url);
            }
            None
        }
    }
);

impl RefusingUiDelegate {
    fn new(mtm: MainThreadMarker, on_link: impl Fn(String) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefusingIvars {
            on_link: Box::new(on_link),
        });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

fn web_address(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// The document a board opened, and its reloads, with any fragment ignored.
fn same_document(url: &str, page: &str) -> bool {
    url.split('#').next() == page.split('#').next()
}

pub struct Board {
    webview: WebView,
    focused: Rc<Cell<bool>>,
    on_event: Rc<dyn Fn(BoardEvent)>,
    monitor: Retained<AnyObject>,
    _ui_delegate: Retained<RefusingUiDelegate>,
    frame: Option<Bounds<Pixels>>,
}

impl Board {
    /// Creates the web view hidden over `page` of `folder`; it shows once a frame places it.
    /// `rules` is the list `with_rules` compiled.
    pub fn open(
        window: &Window,
        folder: BoardFolder,
        page: &str,
        rules: &WKContentRuleList,
        on_event: impl Fn(BoardEvent) + 'static,
    ) -> wry::Result<Self> {
        let mtm = MainThreadMarker::new().expect("boards open on the main thread");
        let url = folder.url(page);
        let on_event: Rc<dyn Fn(BoardEvent)> = Rc::new(on_event);
        // SAFETY: a fresh configuration on the main thread, before any web view uses it.
        let configuration = unsafe {
            let configuration = WKWebViewConfiguration::new(mtm);
            configuration.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
            configuration
                .userContentController()
                .addContentRuleList(rules);
            configuration
        };
        let (start, links) = (url.clone(), on_event.clone());
        let webview = WebViewBuilder::new()
            .with_webview_configuration(configuration)
            .with_custom_protocol(SCHEME.into(), move |_, request| {
                folder.respond(request.uri())
            })
            .with_url(url)
            .with_visible(false)
            .with_navigation_handler(move |to| {
                if same_document(&to, &start) {
                    return true;
                }
                if web_address(&to) {
                    links(BoardEvent::Link(to));
                }
                false
            })
            .build_as_child(window)?;
        let offers = on_event.clone();
        let ui_delegate = RefusingUiDelegate::new(mtm, move |url| offers(BoardEvent::Link(url)));
        // SAFETY: on the main thread; the board keeps the delegate, which WebKit holds weakly.
        unsafe {
            webview
                .webview()
                .setUIDelegate(Some(ProtocolObject::from_ref(&*ui_delegate)));
        }
        let focused = Rc::new(Cell::new(false));
        let monitor = watch_focus(webview.webview(), focused.clone(), on_event.clone());
        Ok(Self {
            webview,
            focused,
            on_event,
            monitor,
            _ui_delegate: ui_delegate,
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
                    (self.on_event)(BoardEvent::Focus(false));
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

    pub fn is_shown(&self) -> bool {
        !self.webview.webview().isHidden()
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
}

/// What the board web view probe reads back; the app itself does not need them.
#[allow(dead_code)]
impl Board {
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
    on_event: Rc<dyn Fn(BoardEvent)>,
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
                    on_event(BoardEvent::Focus(inside));
                }
            }
            NSEventType::KeyDown
                if in_window && focused.get() && event_ref.keyCode() == ESCAPE_KEY_CODE =>
            {
                give_back_focus(&webview);
                focused.set(false);
                on_event(BoardEvent::Focus(false));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> (tempfile::TempDir, BoardFolder) {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("board");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), "<p>board</p>").unwrap();
        std::fs::write(root.join("my chart.svg"), "<svg/>").unwrap();
        for name in ["report#1.html", "report?draft.html", "literal%20name.html"] {
            std::fs::write(root.join(name), name).unwrap();
        }
        std::fs::write(parent.path().join("secret.txt"), "secret").unwrap();
        let folder = BoardFolder::new("fixture", &root).unwrap();
        (parent, folder)
    }

    fn get(folder: &BoardFolder, url: &str) -> Response<Cow<'static, [u8]>> {
        folder.respond(&url.parse().unwrap())
    }

    #[test]
    fn serves_the_board_folder_with_the_policy() {
        let (_parent, folder) = board();
        let response = get(&folder, &folder.url("index.html"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"<p>board</p>");
        assert_eq!(response.headers()["Content-Security-Policy"], CSP);
        let named = get(&folder, "wiffletree-board://fixture/my%20chart.svg");
        assert_eq!(named.body().as_ref(), b"<svg/>");
        for name in [
            "report#1.html",
            "report?draft.html",
            "literal%20name.html",
            "my chart.svg",
        ] {
            let response = get(&folder, &folder.url(name));
            assert_eq!(response.status(), StatusCode::OK, "{name}");
        }
    }

    #[test]
    fn refuses_anything_outside_the_board_folder() {
        let (parent, folder) = board();
        std::os::unix::fs::symlink(
            parent.path().join("secret.txt"),
            parent.path().join("board/link.txt"),
        )
        .unwrap();
        for url in [
            "wiffletree-board://fixture/../secret.txt",
            "wiffletree-board://fixture/%2e%2e/secret.txt",
            "wiffletree-board://fixture/%2E%2E%2Fsecret.txt",
            "wiffletree-board://fixture/index.html%00.png",
            "wiffletree-board://fixture/%zz",
            "wiffletree-board://fixture/link.txt",
            "wiffletree-board://other/index.html",
            "https://fixture/index.html",
        ] {
            let response = get(&folder, url);
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{url}");
            assert_eq!(response.headers()["Content-Security-Policy"], CSP, "{url}");
        }
    }

    #[test]
    fn only_the_opened_document_loads_and_a_web_address_is_offered() {
        let page = "wiffletree-board://b1/index.html";
        assert!(same_document(page, page));
        assert!(same_document("wiffletree-board://b1/index.html#week", page));
        assert!(!same_document("wiffletree-board://b1/other.html", page));
        assert!(!same_document("https://sentry.io/issues", page));
        assert!(web_address("https://sentry.io/issues") && !web_address("file:///etc/hosts"));
    }
}
