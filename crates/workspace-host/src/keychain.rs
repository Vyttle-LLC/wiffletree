//! The client credential of a signed launchd host, kept in a login Keychain item whose access
//! list names only the programs given: the signed app and the host's own copy. Keychain matches
//! them by their signing requirement, and asks any other program, an agent's shell among them,
//! for the login password in a dialog.
//!
//! Design §2 of `openspec/changes/standalone-host` settles which keychain, access list and
//! signature this needs; everything that depends on it is in this module.
use anyhow::Result;
use std::path::Path;

#[cfg(target_os = "macos")]
use workspace_core::CREDENTIAL_SERVICE as SERVICE;

/// The data folder's credential, created at the first start with `trusted` as its access list.
#[cfg(target_os = "macos")]
pub fn credential(data: &Path, trusted: &[&Path]) -> Result<String> {
    use anyhow::Context as _;
    use security_framework::os::macos::{keychain::SecKeychain, passwords::find_generic_password};
    let account = data.to_string_lossy();
    // The host never shows a Keychain dialog; a refusal is an error.
    let _quiet = SecKeychain::disable_user_interaction()?;
    if let Ok((password, _)) = find_generic_password(None, SERVICE, &account) {
        return String::from_utf8(password.to_owned()).context("The client credential is not text");
    }
    let credential = workspace_core::new_credential()?;
    let keychain = SecKeychain::default()?;
    keychain.add_generic_password(SERVICE, &account, credential.as_bytes())?;
    let (_, item) = keychain.find_generic_password(SERVICE, &account)?;
    if let Err(error) = access::limit(&item, trusted) {
        // An item any program could read is worse than none.
        item.delete();
        return Err(error.context("Limit the client credential to Wiffletree"));
    }
    Ok(credential)
}

#[cfg(not(target_os = "macos"))]
pub fn credential(_data: &Path, _trusted: &[&Path]) -> Result<String> {
    anyhow::bail!("The client credential needs the macOS Keychain; start the host with --child")
}

/// Access lists, which the crate does not wrap.
#[cfg(target_os = "macos")]
mod access {
    use anyhow::{Result, ensure};
    use core_foundation::{
        array::CFArray,
        base::{CFType, CFTypeRef, OSStatus, TCFType},
        string::{CFString, CFStringRef},
    };
    use security_framework::os::macos::keychain_item::SecKeychainItem;
    use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecTrustedApplicationCreateFromPath(
            path: *const std::ffi::c_char,
            app: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecAccessCreate(
            descriptor: CFStringRef,
            trusted: core_foundation::array::CFArrayRef,
            access: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecKeychainItemSetAccess(item: CFTypeRef, access: CFTypeRef) -> OSStatus;
    }

    /// Replaces the item's access list with exactly the `trusted` programs.
    pub(super) fn limit(item: &SecKeychainItem, trusted: &[&Path]) -> Result<()> {
        let mut apps = vec![];
        for path in trusted {
            let path = CString::new(path.as_os_str().as_bytes())?;
            let mut app: CFTypeRef = std::ptr::null();
            // SAFETY: `path` is a valid C string and `app` receives an owned reference.
            let status = unsafe { SecTrustedApplicationCreateFromPath(path.as_ptr(), &mut app) };
            ensure!(
                status == 0 && !app.is_null(),
                "Name {path:?} in the access list: {status}"
            );
            // SAFETY: created above, so this takes ownership of the one reference.
            apps.push(unsafe { CFType::wrap_under_create_rule(app) });
        }
        let apps = CFArray::from_CFTypes(&apps);
        let descriptor = CFString::new("Wiffletree client credential");
        let mut access: CFTypeRef = std::ptr::null();
        // SAFETY: both arguments are live CF objects; `access` receives an owned reference.
        let status = unsafe {
            SecAccessCreate(
                descriptor.as_concrete_TypeRef(),
                apps.as_concrete_TypeRef(),
                &mut access,
            )
        };
        ensure!(
            status == 0 && !access.is_null(),
            "Create the access list: {status}"
        );
        // SAFETY: created above, so this takes ownership of the one reference.
        let access = unsafe { CFType::wrap_under_create_rule(access) };
        // SAFETY: the item and the access list are live for the call.
        let status =
            unsafe { SecKeychainItemSetAccess(item.as_CFTypeRef(), access.as_CFTypeRef()) };
        ensure!(status == 0, "Set the access list: {status}");
        Ok(())
    }
}
