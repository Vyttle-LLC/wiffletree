//! "Start Wiffletree at login": the app as a login item through `SMAppService`, which macOS
//! keeps in System Settings › General › Login Items. Opening the app starts the host, so jobs
//! run from login. It is off until the user turns it on, and only a signed app can register.

// objc 0.2's macros test a `cargo-clippy` feature this crate does not declare.
#![allow(unexpected_cfgs)]
use anyhow::{Result, bail};
use objc::{class, msg_send, runtime::Object, sel, sel_impl};

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

/// `SMAppServiceStatus`.
const ENABLED: isize = 1;
const REQUIRES_APPROVAL: isize = 2;

fn service() -> *mut Object {
    // SAFETY: `mainAppService` is a class property that returns a shared object.
    unsafe { msg_send![class!(SMAppService), mainAppService] }
}

/// Whether the app opens at login, as macOS records it.
pub(super) fn starts_at_login() -> bool {
    // SAFETY: `status` takes no argument and returns an `NSInteger`.
    let status: isize = unsafe { msg_send![service(), status] };
    status == ENABLED || status == REQUIRES_APPROVAL
}

pub(super) fn set_starts_at_login(on: bool) -> Result<()> {
    let mut error: *mut Object = std::ptr::null_mut();
    // SAFETY: both selectors take an `NSError **` and return a `BOOL`.
    let done: bool = unsafe {
        match on {
            true => msg_send![service(), registerAndReturnError: &mut error],
            false => msg_send![service(), unregisterAndReturnError: &mut error],
        }
    };
    if done {
        return Ok(());
    }
    let reason = match error.is_null() {
        true => String::new(),
        // SAFETY: a failed call left an `NSError`, whose description is an `NSString`.
        false => unsafe {
            let text: *mut Object = msg_send![error, localizedDescription];
            let bytes: *const std::os::raw::c_char = msg_send![text, UTF8String];
            std::ffi::CStr::from_ptr(bytes)
                .to_string_lossy()
                .into_owned()
        },
    };
    bail!("macOS did not change Login Items: {reason} Only a signed Wiffletree can open at login.")
}
