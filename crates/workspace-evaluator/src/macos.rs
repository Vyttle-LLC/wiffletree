//! The helper's reach into macOS: the on-screen window list, the frontmost app, input posted
//! as the human's would be, ScreenCaptureKit limited to the run's apps, the Escape listener and
//! the menu bar item. Each runs in this process, so macOS charges it to the helper's own
//! grants.
use crate::panel::{self, Corner, StopSource};
use crate::policy::{self, Allowed, Keystroke, Point, Rect, Window};
use crate::protocol::{Button, Capture, Control, Desktop, Grants};
use anyhow::{Context, Result, bail, ensure};
use block2::RcBlock;
use objc2::{
    AnyThread as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::AnyObject, sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBitmapImageFileType,
    NSBitmapImageRep, NSBox, NSBoxType, NSButton, NSColor, NSEvent, NSFont, NSMenu, NSMenuItem,
    NSPanel, NSRunningApplication, NSScreen, NSStatusBar, NSStatusItem, NSStatusWindowLevel,
    NSTextField, NSTitlePosition, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
    NSWorkspace,
};
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CGPoint, CGRect, CGSize, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGColorSpace, CGContext, CGDisplayBounds, CGEvent, CGEventField, CGEventFlags,
    CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType,
    CGImage, CGMainDisplayID, CGMouseButton, CGPreflightScreenCaptureAccess,
    CGRequestScreenCaptureAccess, CGScrollEventUnit, CGWindowListCopyWindowInfo,
    CGWindowListOption,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSPoint, NSRect, NSSize, NSString, NSTimer,
    NSUserDefaults,
};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration, SCWindow,
};
use std::{
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, OnceLock, mpsc},
    time::Duration,
};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

/// How long ScreenCaptureKit may take to answer.
const CAPTURE_WAIT: Duration = Duration::from_secs(15);
/// How long open_app waits for the app to come to the front.
const OPEN_WAIT: Duration = Duration::from_secs(5);
/// A pause between the parts of one gesture, so the app sees each.
const BEAT: Duration = Duration::from_millis(40);

/// What this process may do now. macOS reads them for the helper itself, the responsible
/// process when LaunchServices started it.
pub fn grants(escape: bool) -> Grants {
    Grants {
        screen_recording: CGPreflightScreenCaptureAccess(),
        // SAFETY: a plain query of this process's trust.
        accessibility: unsafe { AXIsProcessTrusted() },
        escape,
    }
}

/// How long `request` stays for the human to answer macOS's prompts.
const REQUEST_WAIT: Duration = Duration::from_secs(120);

/// Asks macOS for both grants, which shows its prompts for this helper by name, and stays
/// until both are given or `REQUEST_WAIT` passes: macOS shows a prompt on behalf of a live
/// process, so one that quits at once leaves the human nothing to answer.
pub fn request_grants() -> Grants {
    CGRequestScreenCaptureAccess();
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let prompt = NSNumber::new_bool(true);
    let options = NSDictionary::from_slices(&[&*key], &[&*prompt]);
    // SAFETY: the options are an NSDictionary, toll-free bridged to the CFDictionary it takes.
    unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&options).cast()) };
    let deadline = std::time::Instant::now() + REQUEST_WAIT;
    loop {
        let now = grants(false);
        if (now.screen_recording && now.accessibility) || std::time::Instant::now() >= deadline {
            return now;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

pub struct Mac;

/// A ScreenCaptureKit object handed back from its completion queue.
struct Sent<T>(T);
// SAFETY: each is retained and only read on the thread that receives it.
unsafe impl<T> Send for Sent<T> {}

fn post(event: &CGEvent) {
    CGEvent::set_integer_value_field(
        Some(event),
        CGEventField::EventSourceUserData,
        policy::POSTED_MARK,
    );
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(event));
}
fn mouse(kind: CGEventType, at: Point, button: CGMouseButton) -> Result<CFRetained<CGEvent>> {
    CGEvent::new_mouse_event(None, kind, CGPoint::new(at.x, at.y), button)
        .context("macOS could not make the mouse event")
}
fn bundle_id(pid: i32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    Some(app.bundleIdentifier()?.to_string())
}
fn number(dictionary: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<f64> {
    let value = dictionary.objectForKey(&NSString::from_str(key))?;
    Some(value.downcast::<NSNumber>().ok()?.doubleValue())
}
fn text(dictionary: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<String> {
    let value = dictionary.objectForKey(&NSString::from_str(key))?;
    Some(value.downcast::<NSString>().ok()?.to_string())
}

impl Desktop for Mac {
    fn windows(&self) -> Result<Vec<Window>> {
        let options =
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
        let list = CGWindowListCopyWindowInfo(options, 0).context("macOS listed no windows")?;
        // SAFETY: the window list is a CFArray of CFDictionary, toll-free bridged to these.
        let list: &NSArray<NSDictionary<NSString, AnyObject>> =
            unsafe { &*(CFRetained::as_ptr(&list).as_ptr().cast()) };
        let mut windows = vec![];
        for info in list.iter() {
            let bounds = info
                .objectForKey(&NSString::from_str("kCGWindowBounds"))
                .and_then(|bounds| bounds.downcast::<NSDictionary>().ok());
            let Some(bounds) = bounds else { continue };
            // SAFETY: the bounds dictionary's keys are strings.
            let bounds: &NSDictionary<NSString, AnyObject> =
                unsafe { &*(Retained::as_ptr(&bounds).cast()) };
            let pid = number(&info, "kCGWindowOwnerPID").unwrap_or_default() as i32;
            windows.push(Window {
                id: number(&info, "kCGWindowNumber").unwrap_or_default() as u32,
                app: bundle_id(pid),
                title: text(&info, "kCGWindowName").filter(|title| !title.is_empty()),
                layer: number(&info, "kCGWindowLayer").unwrap_or_default() as i64,
                bounds: Rect {
                    x: number(bounds, "X").unwrap_or_default(),
                    y: number(bounds, "Y").unwrap_or_default(),
                    width: number(bounds, "Width").unwrap_or_default(),
                    height: number(bounds, "Height").unwrap_or_default(),
                },
            });
        }
        Ok(windows)
    }

    fn frontmost(&self) -> Option<(String, i32)> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some((app.bundleIdentifier()?.to_string(), app.processIdentifier()))
    }

    fn click(&self, at: Point, button: Button, count: u32) -> Result<()> {
        let (down, up, button) = match button {
            Button::Left => (
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGMouseButton::Left,
            ),
            Button::Right => (
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGMouseButton::Right,
            ),
        };
        self.move_to(at)?;
        for click in 1..=count {
            for kind in [down, up] {
                let event = mouse(kind, at, button)?;
                CGEvent::set_integer_value_field(
                    Some(&event),
                    CGEventField::MouseEventClickState,
                    i64::from(click),
                );
                post(&event);
            }
        }
        std::thread::sleep(BEAT);
        Ok(())
    }

    fn move_to(&self, at: Point) -> Result<()> {
        post(&*mouse(CGEventType::MouseMoved, at, CGMouseButton::Left)?);
        std::thread::sleep(BEAT);
        Ok(())
    }

    fn scroll(&self, at: Point, dy: i32) -> Result<()> {
        self.move_to(at)?;
        let event = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, dy, 0, 0)
            .context("macOS could not make the scroll event")?;
        CGEvent::set_location(Some(&event), CGPoint::new(at.x, at.y));
        post(&event);
        std::thread::sleep(BEAT);
        Ok(())
    }

    fn drag(&self, from: Point, to: Point) -> Result<()> {
        self.move_to(from)?;
        post(&*mouse(
            CGEventType::LeftMouseDown,
            from,
            CGMouseButton::Left,
        )?);
        for part in [0.1, 0.5, 0.9, 1.0] {
            std::thread::sleep(BEAT);
            let at = Point {
                x: from.x + (to.x - from.x) * part,
                y: from.y + (to.y - from.y) * part,
            };
            post(&*mouse(
                CGEventType::LeftMouseDragged,
                at,
                CGMouseButton::Left,
            )?);
        }
        post(&*mouse(CGEventType::LeftMouseUp, to, CGMouseButton::Left)?);
        std::thread::sleep(BEAT);
        Ok(())
    }

    fn type_text(&self, pid: i32, text: &str) -> Result<()> {
        for c in text.chars() {
            let mut units = [0u16; 2];
            let units = c.encode_utf16(&mut units);
            for down in [true, false] {
                let event = CGEvent::new_keyboard_event(None, 0, down)
                    .context("macOS could not make the key event")?;
                // SAFETY: `units` holds `units.len()` UTF-16 units.
                unsafe {
                    CGEvent::keyboard_set_unicode_string(
                        Some(&event),
                        units.len() as _,
                        units.as_ptr(),
                    )
                };
                post_to(pid, &event);
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        Ok(())
    }

    fn key(&self, pid: i32, keystroke: Keystroke) -> Result<()> {
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(None, keystroke.key_code, down)
                .context("macOS could not make the key event")?;
            CGEvent::set_flags(Some(&event), CGEventFlags(keystroke.flags));
            post_to(pid, &event);
        }
        std::thread::sleep(BEAT);
        Ok(())
    }

    fn capture(&self, apps: &Allowed, native: bool) -> Result<Capture> {
        let content = shareable_content()?;
        // Front to back, so they are drawn back to front.
        let windows = self.windows()?;
        let allowed = policy::allowed_windows(&windows, apps);
        // SAFETY: plain reads of the content macOS returned, and drawing into a context this
        // function owns.
        unsafe {
            let main = CGMainDisplayID();
            let display = (content.displays().iter())
                .find(|display| display.displayID() == main)
                .context("The main display is not available to capture")?;
            let frame = display.frame();
            let screen = Rect {
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
            };
            let probe = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            );
            let scale = match native {
                true => f64::from(probe.pointPixelScale()),
                false => 1.,
            };
            let (width, height) = (
                (screen.width * scale).round() as usize,
                (screen.height * scale).round() as usize,
            );
            let shareable: Vec<Retained<SCWindow>> = content.windows().iter().collect();
            let canvas = Canvas::new(width, height)?;
            let mut drawn = 0;
            for window in allowed.iter().rev() {
                let Some(found) = shareable.iter().find(|w| w.windowID() == window.id) else {
                    continue;
                };
                let at = policy::placement(window.bounds, screen, scale);
                if at.width < 1. || at.height < 1. {
                    continue;
                }
                // Each window alone, so nothing of another app can be in its image.
                let filter = SCContentFilter::initWithDesktopIndependentWindow(
                    SCContentFilter::alloc(),
                    found,
                );
                let config = SCStreamConfiguration::new();
                config.setWidth(at.width.round() as usize);
                config.setHeight(at.height.round() as usize);
                config.setIgnoreShadowsSingleWindow(true);
                config.setShowsCursor(false);
                let image = screenshot(&filter, &config)?;
                canvas.draw(&image, at);
                drawn += 1;
            }
            ensure!(
                drawn > 0,
                "None of this run's apps has a window on screen. Open one with open_app."
            );
            let image = canvas.image()?;
            let bitmap = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
            let png = bitmap
                .representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
                .context("macOS could not encode the screenshot")?;
            Ok(Capture {
                png: png.to_vec(),
                width,
                height,
                scale,
                display: screen,
            })
        }
    }

    fn open(&self, app: &str) -> Result<()> {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-b", app])
            .status()
            .context("Could not run open")?;
        ensure!(status.success(), "macOS could not open {app}");
        // Stage Manager may show it only once it is frontmost, so the next step sees its window.
        let deadline = std::time::Instant::now() + OPEN_WAIT;
        while std::time::Instant::now() < deadline {
            if self
                .frontmost()
                .is_some_and(|(front, _)| front.eq_ignore_ascii_case(app))
            {
                std::thread::sleep(Duration::from_millis(300));
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!(
            "{app} opened but did not come to the front within {} s",
            OPEN_WAIT.as_secs()
        )
    }
}

/// Keys go to the frontmost allowed app's process alone, never through the system, so no
/// global shortcut such as Spotlight's can take them.
fn post_to(pid: i32, event: &CGEvent) {
    CGEvent::set_integer_value_field(
        Some(event),
        CGEventField::EventSourceUserData,
        policy::POSTED_MARK,
    );
    CGEvent::post_to_pid(pid, Some(event));
}

unsafe extern "C-unwind" {
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Option<&CGColorSpace>,
        bitmap_info: u32,
    ) -> Option<NonNull<CGContext>>;
}
/// kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little.
const BGRA: u32 = 2 | (2 << 12);

/// A white image of the display, that windows are drawn into.
struct Canvas(CFRetained<CGContext>);
impl Canvas {
    fn new(width: usize, height: usize) -> Result<Self> {
        let space = CGColorSpace::new_device_rgb().context("No RGB color space")?;
        // SAFETY: Core Graphics allocates the pixels itself when given none.
        let context = unsafe {
            CGBitmapContextCreate(
                std::ptr::null_mut(),
                width,
                height,
                8,
                0,
                Some(&space),
                BGRA,
            )
        }
        .context("macOS could not make the screenshot's canvas")?;
        // SAFETY: a context made by a Create function is ours to release.
        let context = unsafe { CFRetained::from_raw(context) };
        CGContext::set_rgb_fill_color(Some(&context), 1., 1., 1., 1.);
        CGContext::fill_rect(
            Some(&context),
            CGRect::new(CGPoint::ZERO, CGSize::new(width as f64, height as f64)),
        );
        Ok(Self(context))
    }
    fn draw(&self, image: &CGImage, at: Rect) {
        let rect = CGRect::new(CGPoint::new(at.x, at.y), CGSize::new(at.width, at.height));
        CGContext::draw_image(Some(&self.0), rect, Some(image));
    }
    fn image(&self) -> Result<CFRetained<CGImage>> {
        objc2_core_graphics::CGBitmapContextCreateImage(Some(&self.0))
            .context("macOS could not finish the screenshot")
    }
}

fn shareable_content() -> Result<Retained<SCShareableContent>> {
    let (sender, receiver) = mpsc::channel();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = match NonNull::new(content) {
                // SAFETY: macOS hands the content to this block; retaining keeps it.
                Some(content) => Ok(unsafe { Retained::retain(content.as_ptr()) }.unwrap()),
                None => Err(error_text(error)),
            };
            let _ = sender.send(Sent(result));
        },
    );
    // SAFETY: the handler matches the signature ScreenCaptureKit calls.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &handler,
        )
    };
    let Sent(result) = receiver
        .recv_timeout(CAPTURE_WAIT)
        .context("ScreenCaptureKit did not answer")?;
    result.map_err(|error| {
        anyhow::anyhow!(
            "macOS refused the screenshot: {error}. Wiffletree Evaluator needs Screen Recording."
        )
    })
}

fn screenshot(
    filter: &SCContentFilter,
    config: &SCStreamConfiguration,
) -> Result<CFRetained<CGImage>> {
    let (sender, receiver) = mpsc::channel();
    let handler = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let result = match NonNull::new(image) {
            // SAFETY: macOS hands the image to this block; retaining keeps it.
            Some(image) => Ok(unsafe { CFRetained::retain(image) }),
            None => Err(error_text(error)),
        };
        let _ = sender.send(Sent(result));
    });
    // SAFETY: the handler matches the signature ScreenCaptureKit calls.
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            filter,
            config,
            Some(&handler),
        )
    };
    let Sent(result) = receiver
        .recv_timeout(CAPTURE_WAIT)
        .context("ScreenCaptureKit did not answer")?;
    result.map_err(|error| anyhow::anyhow!("macOS refused the screenshot: {error}"))
}

fn error_text(error: *mut NSError) -> String {
    // SAFETY: a non-null error is a live NSError for the handler's duration.
    match unsafe { error.as_ref() } {
        Some(error) => error.localizedDescription().to_string(),
        None => "no reason given".into(),
    }
}

/// Writes `message` to the unified log, as `wiffletree-evaluator`, so a run's Esc and Stop can be
/// traced afterwards with `/usr/bin/log show --predicate 'process == "wiffletree-evaluator"'`.
/// It never names a key but Esc.
pub fn note(message: &str) {
    let Ok(message) = std::ffi::CString::new(message) else {
        return;
    };
    // SAFETY: a constant format string with one C string argument.
    unsafe { libc::syslog(libc::LOG_NOTICE, c"%s".as_ptr(), message.as_ptr()) };
}

/// Ends the run, the one way for Esc, the menu bar's Stop and the panel's: logged, told to the
/// host, and every later request refused.
fn stop_run(source: StopSource) {
    if let Some(control) = CONTROL.get()
        && !control.stopped()
    {
        note(source.note());
        control.stop(source);
    }
}

/// Where Escape and the menu's Stop report.
static CONTROL: OnceLock<Arc<Control>> = OnceLock::new();

/// Listens to every key press in the session, without changing any, for the human's Escape.
/// Returns whether the listener is in place; macOS allows one only with Accessibility.
pub fn listen_for_escape(control: Arc<Control>) -> bool {
    let _ = CONTROL.set(control);
    let mask = 1u64 << CGEventType::KeyDown.0;
    // SAFETY: the callback is a plain function that reads the event it is given.
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(heard),
            std::ptr::null_mut(),
        )
    };
    let Some(tap) = tap else { return false };
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&tap), 0) else {
        return false;
    };
    let Some(main) = CFRunLoop::main() else {
        return false;
    };
    // SAFETY: the common modes constant is set by Core Foundation.
    main.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&tap, true);
    note("Listening for Esc");
    // The tap lives as long as the helper.
    std::mem::forget(tap);
    true
}

unsafe extern "C-unwind" fn heard(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _user: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: macOS passes a live event for the callback's duration.
    let event_ref = unsafe { event.as_ref() };
    if kind == CGEventType::KeyDown {
        let key = CGEvent::integer_value_field(Some(event_ref), CGEventField::KeyboardEventKeycode);
        let mark = CGEvent::integer_value_field(Some(event_ref), CGEventField::EventSourceUserData);
        if policy::stops_run(key, mark) {
            stop_run(StopSource::Escape);
        }
    }
    event.as_ptr()
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class has no Drop.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WiffletreeEvaluatorStop"]
    struct StopTarget;

    impl StopTarget {
        #[unsafe(method(stop:))]
        fn stop(&self, _sender: Option<&AnyObject>) {
            stop_run(StopSource::MenuBar);
        }

        #[unsafe(method(panelStop:))]
        fn panel_stop(&self, _sender: Option<&AnyObject>) {
            stop_run(StopSource::Panel);
        }
    }
);

/// The menu bar item that shows for the whole run, with Stop.
fn status_item(run: &str, mtm: MainThreadMarker) -> (Retained<NSStatusItem>, Retained<StopTarget>) {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.);
    if let Some(button) = item.button(mtm) {
        button.setTitle(&NSString::from_str("Evaluator using this Mac"));
    }
    let menu = NSMenu::new(mtm);
    let about = NSMenuItem::new(mtm);
    about.setTitle(&NSString::from_str(&format!(
        "UI evaluator run {run} · press Esc anywhere to stop"
    )));
    about.setEnabled(false);
    menu.addItem(&about);
    let target: Retained<StopTarget> = {
        let this = StopTarget::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    };
    // SAFETY: `stop:` is the target's action, and the target outlives the menu.
    let stop = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Stop"),
            Some(sel!(stop:)),
            &NSString::from_str(""),
        )
    };
    // SAFETY: the target is an NSObject kept alive with the item.
    unsafe { stop.setTarget(Some(&target)) };
    menu.addItem(&stop);
    item.setMenu(Some(&menu));
    (item, target)
}

/// Runs the helper's app: no Dock icon, the menu bar item, and the Escape listener. Never
/// returns; the helper quits when the host closes its control connection.
pub fn run_app(
    run: &str,
    control: Arc<Control>,
    model: String,
    apps: &[String],
    queued: usize,
    escape_ready: impl FnOnce(bool),
) -> Result<()> {
    let Some(mtm) = MainThreadMarker::new() else {
        bail!("The helper's app must run on the main thread");
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    escape_ready(listen_for_escape(control));
    let (_item, target) = status_item(run, mtm);
    let names = apps.iter().map(|app| app_name(app)).collect();
    let run = panel::Run {
        model,
        apps: names,
        queued,
    };
    let _panel = Panel::show(run, &target, mtm);
    app.run();
    Ok(())
}

/// An app's name as Finder shows it, or its bundle id when it is not installed.
fn app_name(bundle_id: &str) -> String {
    let url = NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id));
    url.and_then(|url| url.URLByDeletingPathExtension())
        .and_then(|url| url.lastPathComponent())
        .map_or_else(|| bundle_id.to_owned(), |name| name.to_string())
}

/// Where the panel's corner is kept between runs, in the helper's own defaults.
const CORNER_KEY: &str = "panelCorner";
/// The panel's size in points, normally and over a full-screen app.
const FULL: (f64, f64) = (400., 52.);
const COMPACT: (f64, f64) = (210., 36.);

/// The floating control panel: above every window, on every Space and over full-screen apps,
/// for the whole run. It never activates the helper, so a click on it leaves the app under test
/// frontmost; the helper's own windows are no allowed app's, so the evaluator can neither see
/// it in a screenshot nor click it.
struct Panel {
    window: Retained<NSPanel>,
    background: Retained<NSBox>,
    dot: Retained<NSBox>,
    title: Retained<NSTextField>,
    detail: Retained<NSTextField>,
    esc: Retained<NSTextField>,
    stop: Retained<NSButton>,
    run: panel::Run,
    started: std::time::Instant,
    corner: std::cell::Cell<Corner>,
    compact: std::cell::Cell<Option<bool>>,
}
impl Panel {
    fn show(run: panel::Run, target: &StopTarget, mtm: MainThreadMarker) -> Rc<Self> {
        let label = |size: f64, bold: bool, white: f64| {
            let field = NSTextField::labelWithString(&NSString::from_str(""), mtm);
            let font = match bold {
                true => NSFont::boldSystemFontOfSize(size),
                false => NSFont::systemFontOfSize(size),
            };
            field.setFont(Some(&font));
            field.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
                white, white, white, 1.,
            )));
            field
        };
        let rounded = |fill: &NSColor, border: Option<&NSColor>, radius: f64| {
            let view = NSBox::new(mtm);
            view.setBoxType(NSBoxType::Custom);
            view.setTitlePosition(NSTitlePosition::NoTitle);
            view.setFillColor(fill);
            view.setCornerRadius(radius);
            match border {
                Some(border) => {
                    view.setBorderColor(border);
                    view.setBorderWidth(1.5);
                }
                None => view.setBorderWidth(0.),
            }
            view
        };
        let red = NSColor::colorWithSRGBRed_green_blue_alpha(0.898, 0.282, 0.302, 1.);
        let dark = NSColor::colorWithSRGBRed_green_blue_alpha(0.118, 0.118, 0.118, 0.92);
        let background = rounded(&dark, Some(&red), 12.);
        let dot = rounded(&red, None, 4.5);
        let esc = label(10., false, 0.8);
        esc.setStringValue(&NSString::from_str("Esc"));
        // SAFETY: `panelStop:` is the target's action, and the target lives as long as the app.
        let stop = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Stop"),
                Some(target),
                Some(sel!(panelStop:)),
                mtm,
            )
        };
        stop.setBezelColor(Some(&red));
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(FULL.0, FULL.1)),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: the panel is kept for the app's life, so it must not release itself.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setFloatingPanel(true);
        window.setBecomesKeyOnlyIfNeeded(true);
        // The helper is never the active app, so the panel must not hide when it is not.
        window.setHidesOnDeactivate(false);
        window.setLevel(NSStatusWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(true);
        window.setMovableByWindowBackground(true);
        let content = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(FULL.0, FULL.1)),
        );
        let (title, detail) = (label(12., true, 1.), label(11., false, 0.8));
        let views: [&NSView; 6] = [&background, &dot, &title, &detail, &esc, &stop];
        for view in views {
            content.addSubview(view);
        }
        window.setContentView(Some(&content));
        let corner = NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str(CORNER_KEY))
            .map_or_else(Corner::default, |key| Corner::from_key(&key.to_string()));
        let panel = Rc::new(Self {
            window,
            background,
            dot,
            title,
            detail,
            esc,
            stop,
            run,
            started: std::time::Instant::now(),
            corner: std::cell::Cell::new(corner),
            compact: std::cell::Cell::new(None),
        });
        panel.update();
        panel.window.orderFrontRegardless();
        let ticking = panel.clone();
        let tick = RcBlock::new(move |_timer: NonNull<NSTimer>| ticking.update());
        // SAFETY: the timer runs on the main run loop, where the panel lives.
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.5, true, &tick) };
        panel
    }

    /// The visible frame of the main screen, less its menu bar and Dock.
    fn visible(&self) -> Rect {
        let screen = MainThreadMarker::new()
            .and_then(NSScreen::mainScreen)
            .map(|screen| screen.visibleFrame())
            .unwrap_or_default();
        Rect {
            x: screen.origin.x,
            y: screen.origin.y,
            width: screen.size.width,
            height: screen.size.height,
        }
    }

    /// Shows the run's state, shrinks over a full-screen app, and keeps the panel in its corner,
    /// or in the corner nearest where the human dragged it.
    fn update(&self) {
        let stopping = CONTROL.get().is_some_and(|control| control.stopped());
        let elapsed = self.started.elapsed();
        let display = CGDisplayBounds(CGMainDisplayID());
        let display = Rect {
            x: display.origin.x,
            y: display.origin.y,
            width: display.size.width,
            height: display.size.height,
        };
        let compact = Mac
            .windows()
            .is_ok_and(|windows| panel::full_screen(&windows, display));
        if self.compact.replace(Some(compact)) != Some(compact) {
            self.lay_out(compact);
        }
        let (title, detail) = match (stopping, compact) {
            (true, _) => (panel::STOPPING.0.to_owned(), panel::STOPPING.1.to_owned()),
            (false, false) => (panel::TITLE.to_owned(), panel::detail(&self.run, elapsed)),
            (false, true) => ("Evaluator".to_owned(), panel::elapsed(elapsed)),
        };
        self.title.setStringValue(&NSString::from_str(&title));
        self.detail.setStringValue(&NSString::from_str(&detail));
        self.stop.setEnabled(!stopping);
        let size = if compact { COMPACT } else { FULL };
        let visible = self.visible();
        let frame = self.window.frame();
        let at = Rect {
            x: frame.origin.x,
            y: frame.origin.y,
            width: frame.size.width,
            height: frame.size.height,
        };
        let placed = panel::frame(self.corner.get(), visible, size.0, size.1);
        // Once the human lets go of a drag, the panel settles in the nearest corner.
        if at != placed && NSEvent::pressedMouseButtons() == 0 {
            if at.width == size.0 && at.height == size.1 && frame.origin != NSPoint::ZERO {
                let corner = panel::nearest(at, visible);
                if corner != self.corner.get() {
                    self.corner.set(corner);
                    let key = NSString::from_str(corner.key());
                    // SAFETY: a string value under the panel's own key.
                    unsafe {
                        NSUserDefaults::standardUserDefaults()
                            .setObject_forKey(Some(&key), &NSString::from_str(CORNER_KEY))
                    };
                }
            }
            let placed = panel::frame(self.corner.get(), visible, size.0, size.1);
            self.window.setFrame_display(
                NSRect::new(
                    NSPoint::new(placed.x, placed.y),
                    NSSize::new(placed.width, placed.height),
                ),
                true,
            );
        }
    }

    /// Positions the views for the full panel or the compact one.
    fn lay_out(&self, compact: bool) {
        let rect =
            |x: f64, y: f64, w: f64, h: f64| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
        let (width, height) = if compact { COMPACT } else { FULL };
        self.background.setFrame(rect(0., 0., width, height));
        self.dot.setFrame(rect(12., height / 2. - 4.5, 9., 9.));
        if compact {
            self.title.setFrame(rect(28., 10., 72., 16.));
            self.detail.setFrame(rect(100., 10., 46., 16.));
            self.esc.setHidden(true);
            self.stop.setFrame(rect(width - 64., 4., 56., 28.));
        } else {
            self.title.setFrame(rect(30., 26., 270., 17.));
            self.detail.setFrame(rect(30., 8., 270., 16.));
            self.esc.setHidden(false);
            self.esc.setFrame(rect(width - 96., 18., 28., 14.));
            self.stop.setFrame(rect(width - 64., 12., 56., 28.));
        }
    }
}
