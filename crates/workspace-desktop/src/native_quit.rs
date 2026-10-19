//! Quits that come from macOS rather than the app's own menu, such as the Dock's Quit or a quit
//! Apple event. GPUI hears of them only once termination is under way, too late to ask first or
//! go to the background, so this adds `applicationShouldTerminate:` to GPUI's application
//! delegate. It lets through the app's own quits and those of a logout, restart or shutdown,
//! and turns every other one into ⌘Q.
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

/// Sends quits from macOS, other than a logout's, to `requests` instead of terminating. Call
/// once GPUI's application delegate exists.
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
    if ALLOWED.load(Ordering::Relaxed) || ending_session() {
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

/// Whether the quit being handled comes from a logout, restart or shutdown, which must not be
/// held up. AppKit gives the reason as a parameter of the quit event; an attribute is read too.
fn ending_session() -> bool {
    // SAFETY: called on the main thread from the delegate; every object is only read.
    unsafe {
        let manager: *mut Object = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let event: *mut Object = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return false;
        }
        let parameter: *mut Object = msg_send![event, paramDescriptorForKeyword: QUIT_REASON];
        let attribute: *mut Object = msg_send![event, attributeDescriptorForKeyword: QUIT_REASON];
        [parameter, attribute].into_iter().any(|reason| {
            !reason.is_null() && {
                let code: u32 = msg_send![reason, enumCodeValue];
                ends_session(code)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ends_session;

    #[test]
    fn a_logout_restart_or_shutdown_ends_the_session_and_nothing_else_does() {
        for code in [b"logo", b"rlgo", b"rrst", b"rest", b"rsdn", b"shut"] {
            assert!(ends_session(u32::from_be_bytes(*code)), "{code:?}");
        }
        assert!(!ends_session(0));
        assert!(!ends_session(u32::from_be_bytes(*b"quit")));
    }
}
