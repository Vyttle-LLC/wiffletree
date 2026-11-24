//! An app's name as Finder shows it, from its bundle id, for the apps a full-desktop UI
//! evaluator run may use. Each id is looked up once; an app not installed on this Mac has none.
#![allow(unexpected_cfgs)]
use objc::{class, msg_send, runtime::Object, sel, sel_impl};
use std::{cell::RefCell, collections::HashMap, ffi::CString};

thread_local! {
    static NAMES: RefCell<HashMap<String, Option<String>>> = RefCell::default();
}

/// "Calculator" for `com.apple.calculator`, where it is installed.
pub(crate) fn installed_app_name(bundle_id: &str) -> Option<String> {
    NAMES.with(|names| {
        (names.borrow_mut())
            .entry(bundle_id.to_owned())
            .or_insert_with(|| look_up(bundle_id))
            .clone()
    })
}

fn look_up(bundle_id: &str) -> Option<String> {
    let id = CString::new(bundle_id).ok()?;
    // SAFETY: AppKit's shared workspace and file manager are thread-safe for these reads; each
    // object is checked for nil before it is messaged, and the strings outlive the calls.
    unsafe {
        let id: *mut Object = msg_send![class!(NSString), stringWithUTF8String: id.as_ptr()];
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let url: *mut Object = msg_send![workspace, URLForApplicationWithBundleIdentifier: id];
        if url.is_null() {
            return None;
        }
        let path: *mut Object = msg_send![url, path];
        let files: *mut Object = msg_send![class!(NSFileManager), defaultManager];
        let name: *mut Object = msg_send![files, displayNameAtPath: path];
        if name.is_null() {
            return None;
        }
        let utf8: *const std::ffi::c_char = msg_send![name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let name = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
        let name = name.strip_suffix(".app").unwrap_or(&name).trim().to_owned();
        (!name.is_empty()).then_some(name)
    }
}
