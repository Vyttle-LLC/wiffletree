//! The helper's reach into macOS: the on-screen window list, the frontmost app, input posted
//! as the human's would be, ScreenCaptureKit limited to the run's apps, the Escape listener and
//! the menu bar item. Each runs in this process, so macOS charges it to the helper's own
//! grants.
use crate::policy::{self, Allowed, Keystroke, Point, Rect, Window};
use crate::protocol::{Button, Capture, Control, Desktop, Grants};
use anyhow::{Context, Result, bail, ensure};
use block2::RcBlock;
use objc2::{
    AnyThread as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::AnyObject, sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBitmapImageFileType, NSBitmapImageRep, NSMenu,
    NSMenuItem, NSRunningApplication, NSStatusBar, NSStatusItem, NSWorkspace,
};
use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, CGPoint, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGImage, CGMainDisplayID, CGMouseButton,
    CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess, CGScrollEventUnit,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCRunningApplication, SCScreenshotManager, SCShareableContent,
    SCStreamConfiguration,
};
use std::{
    ffi::c_void,
    ptr::NonNull,
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

/// Asks macOS for both grants, which shows its prompts for this helper by name.
pub fn request_grants() -> Grants {
    CGRequestScreenCaptureAccess();
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let prompt = NSNumber::new_bool(true);
    let options = NSDictionary::from_slices(&[&*key], &[&*prompt]);
    // SAFETY: the options are an NSDictionary, toll-free bridged to the CFDictionary it takes.
    unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&options).cast()) };
    grants(false)
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
        // SAFETY: plain reads of the content macOS returned.
        unsafe {
            let main = CGMainDisplayID();
            let display = (content.displays().iter())
                .find(|display| display.displayID() == main)
                .context("The main display is not available to capture")?;
            let included: Vec<Retained<SCRunningApplication>> = (content.applications().iter())
                .filter(|app| apps.contains(&app.bundleIdentifier().to_string()))
                .collect();
            ensure!(
                !included.is_empty(),
                "None of this run's apps is open. Open one with open_app."
            );
            let included = NSArray::from_retained_slice(&included);
            let filter = SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
                SCContentFilter::alloc(),
                &display,
                &included,
                &NSArray::new(),
            );
            let frame = display.frame();
            let scale = match native {
                true => f64::from(filter.pointPixelScale()),
                false => 1.,
            };
            let (width, height) = (
                (frame.size.width * scale).round() as usize,
                (frame.size.height * scale).round() as usize,
            );
            let config = SCStreamConfiguration::new();
            config.setWidth(width);
            config.setHeight(height);
            config.setShowsCursor(true);
            let image = screenshot(&filter, &config)?;
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
                display: Rect {
                    x: frame.origin.x,
                    y: frame.origin.y,
                    width: frame.size.width,
                    height: frame.size.height,
                },
            })
        }
    }

    fn open(&self, app: &str) -> Result<()> {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-b", app])
            .status()
            .context("Could not run open")?;
        ensure!(status.success(), "macOS could not open {app}");
        Ok(())
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
        if policy::stops_run(key, mark)
            && let Some(control) = CONTROL.get()
        {
            control.stop("escape");
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
            if let Some(control) = CONTROL.get() {
                control.stop("stop");
            }
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
pub fn run_app(run: &str, control: Arc<Control>, escape_ready: impl FnOnce(bool)) -> Result<()> {
    let Some(mtm) = MainThreadMarker::new() else {
        bail!("The helper's app must run on the main thread");
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    escape_ready(listen_for_escape(control));
    let _item = status_item(run, mtm);
    app.run();
    Ok(())
}
