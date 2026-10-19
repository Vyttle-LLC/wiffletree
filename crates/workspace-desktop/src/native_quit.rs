//! Quits that come from macOS rather than the app's own menu, such as the Dock's Quit. GPUI hears
//! of them only once termination is under way, too late to ask first or go to the background, so
//! this adds `applicationShouldTerminate:` to GPUI's application delegate. The human's quit from
//! the Dock decides as ⌘Q does. Every other quit ends the app: its own, a logout's, restart's or
//! shutdown's, and one a script or installer sends, as `scripts/install.sh` does before it
//! replaces the app.
// objc 0.2's macros test a `cargo-clippy` feature this crate does not declare.
#![allow(unexpected_cfgs)]
use objc::{
    class, msg_send,
    runtime::{Class, Object, Sel, class_addMethod},
    sel, sel_impl,
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

/// `NSTerminateCancel` and `NSTerminateNow`.
const CANCEL: usize = 0;
const NOW: usize = 1;
/// `keyAEQuitReason`, the parameter a logout, restart or shutdown puts on its quit event.
const QUIT_REASON: u32 = u32::from_be_bytes(*b"why?");
/// `keySenderPIDAttr`, the process that sent an Apple event.
const SENDER: u32 = u32::from_be_bytes(*b"spid");
const DOCK: &str = "com.apple.dock";
/// The reasons that end the session: `kAELogOut`, `kAEReallyLogOut`, `kAEShowRestartDialog`,
/// `kAERestart`, `kAEShowShutdownDialog` and `kAEShutDown`.
const SESSION_ENDS: [&[u8; 4]; 6] = [b"logo", b"rlgo", b"rrst", b"rest", b"rsdn", b"shut"];

/// The app has decided to end itself; set by `allow`.
static ALLOWED: AtomicBool = AtomicBool::new(false);
/// Where a quit from macOS is sent, to be handled as ⌘Q.
static REQUESTS: Mutex<Option<async_channel::Sender<()>>> = Mutex::new(None);

/// Lets the next termination through: the app is quitting itself.
pub(crate) fn allow() {
    ALLOWED.store(true, Ordering::Relaxed);
}

/// Sends the human's quits from the Dock to `requests` instead of terminating. Call once GPUI's
/// application delegate exists.
pub(crate) fn intercept(requests: async_channel::Sender<()>) {
    *REQUESTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(requests);
    let Some(delegate) = Class::get("GPUIApplicationDelegate") else {
        eprintln!("No GPUI application delegate; quits from the Dock end the app at once");
        return;
    };
    type ShouldTerminate = extern "C" fn(&Object, Sel, *mut Object) -> usize;
    // SAFETY: the delegate class is registered; the method's encoding, `Q@:@`, matches its
    // signature: an NSUInteger reply from self, the selector and the application.
    unsafe {
        class_addMethod(
            delegate as *const Class as *mut Class,
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<ShouldTerminate, objc::runtime::Imp>(should_terminate),
            c"Q@:@".as_ptr(),
        );
    }
}

extern "C" fn should_terminate(_: &Object, _: Sel, _: *mut Object) -> usize {
    if ALLOWED.load(Ordering::Relaxed) || !is_users_quit(current_quit().as_ref()) {
        return NOW;
    }
    match REQUESTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(requests) => {
            let _ = requests.try_send(());
            CANCEL
        }
        None => NOW,
    }
}

/// Whether a quit event's `keyAEQuitReason` says a logout, restart or shutdown.
fn ends_session(reason: u32) -> bool {
    SESSION_ENDS
        .iter()
        .any(|code| u32::from_be_bytes(**code) == reason)
}

/// The Apple event behind a quit: why it was sent, and by which app.
struct QuitEvent {
    reasons: Vec<u32>,
    sender: Option<String>,
}

/// Whether a quit is the human's, which decides as ⌘Q does: one the Dock sent, or a terminate
/// with no Apple event behind it. A logout, restart or shutdown, and a quit sent by a script, an
/// installer or another app, end the app.
fn is_users_quit(event: Option<&QuitEvent>) -> bool {
    let Some(event) = event else {
        return true;
    };
    !event.reasons.iter().any(|reason| ends_session(*reason))
        && event.sender.as_deref() == Some(DOCK)
}

/// The Apple event being handled, if any. AppKit gives a quit's reason as a parameter; an
/// attribute is read too.
fn current_quit() -> Option<QuitEvent> {
    // SAFETY: called on the main thread from the delegate; every object is only read.
    unsafe {
        let manager: *mut Object = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let event: *mut Object = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return None;
        }
        let parameter: *mut Object = msg_send![event, paramDescriptorForKeyword: QUIT_REASON];
        let attribute: *mut Object = msg_send![event, attributeDescriptorForKeyword: QUIT_REASON];
        let reasons = [parameter, attribute]
            .into_iter()
            .filter(|reason| !reason.is_null())
            .map(|reason| msg_send![reason, enumCodeValue])
            .collect();
        let pid: *mut Object = msg_send![event, attributeDescriptorForKeyword: SENDER];
        let sender = (!pid.is_null()).then(|| {
            let pid: i32 = msg_send![pid, int32Value];
            let app: *mut Object = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
            let bundle: *mut Object = if app.is_null() {
                std::ptr::null_mut()
            } else {
                msg_send![app, bundleIdentifier]
            };
            if bundle.is_null() {
                return String::new();
            }
            let text: *const std::ffi::c_char = msg_send![bundle, UTF8String];
            std::ffi::CStr::from_ptr(text).to_string_lossy().into_owned()
        });
        Some(QuitEvent { reasons, sender })
    }
}

#[cfg(test)]
mod tests {
    use super::{QuitEvent, ends_session, is_users_quit};

    #[test]
    fn a_logout_restart_or_shutdown_ends_the_session_and_nothing_else_does() {
        for code in [b"logo", b"rlgo", b"rrst", b"rest", b"rsdn", b"shut"] {
            assert!(ends_session(u32::from_be_bytes(*code)), "{code:?}");
        }
        assert!(!ends_session(0));
        assert!(!ends_session(u32::from_be_bytes(*b"quit")));
    }

    #[test]
    fn only_the_docks_quit_decides_as_command_q_and_any_other_ends_the_app() {
        let event = |sender: Option<&str>, reasons: &[&[u8; 4]]| QuitEvent {
            sender: sender.map(String::from),
            reasons: reasons.iter().map(|r| u32::from_be_bytes(**r)).collect(),
        };
        assert!(is_users_quit(Some(&event(Some("com.apple.dock"), &[]))));
        assert!(is_users_quit(None), "a terminate with no Apple event");
        // The installer's `osascript … to quit`, and other apps' quits.
        assert!(!is_users_quit(Some(&event(
            Some("com.apple.osascript"),
            &[]
        ))));
        assert!(!is_users_quit(Some(&event(Some(""), &[]))));
        assert!(!is_users_quit(Some(&event(None, &[]))));
        // A logout ends the app whoever sent it.
        assert!(!is_users_quit(Some(&event(
            Some("com.apple.dock"),
            &[b"logo"]
        ))));
    }
}
