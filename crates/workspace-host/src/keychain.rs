//! The client credential of a signed launchd host and the host's remote-access key, each kept in a
//! login Keychain item whose access list names only the programs given: the signed app and the
//! host's own copy. Keychain matches them by their signing requirement, and asks any other
//! program, an agent's shell among them, for the login password in a dialog.
//!
//! Design §2 of `openspec/changes/archive/2026-10-19-standalone-host` settles which keychain,
//! access list and signature this needs; everything that depends on it is in this module.
use anyhow::Result;
use std::path::Path;

/// The host key's login Keychain item, whose account is the data folder like the credential's.
pub const HOST_KEY_SERVICE: &str = "com.vyttle.wiffletree.host-key";

/// The data folder's credential, created at the first start with `trusted` as its access list.
pub fn credential(data: &Path, trusted: &[&Path]) -> Result<String> {
    secret(
        workspace_core::CREDENTIAL_SERVICE,
        "client credential",
        data,
        trusted,
        || Ok(workspace_core::new_credential()?),
    )
}

/// The host's long-term P-256 key, created the first time remote access needs it and kept as
/// its secret scalar in hex, with `trusted` as its access list.
pub fn host_key(data: &Path, trusted: &[&Path]) -> Result<workspace_core::pairing::KeyPair> {
    use workspace_core::pairing::{KeyPair, hex};
    let secret = secret(HOST_KEY_SERVICE, "host key", data, trusted, || {
        Ok(hex(&KeyPair::generate()?.secret()))
    })?;
    let bytes: Vec<u8> = (0..secret.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(secret.get(i..i + 2)?, 16).ok())
        .collect();
    KeyPair::from_secret(&bytes)
}

/// The generic-password item `service`, the `label`, for the data folder, created with `create`'s value and
/// `trusted` as its access list when there is none.
#[cfg(target_os = "macos")]
fn secret(
    service: &str,
    label: &str,
    data: &Path,
    trusted: &[&Path],
    create: impl FnOnce() -> Result<String>,
) -> Result<String> {
    use anyhow::Context as _;
    use security_framework::os::macos::{keychain::SecKeychain, passwords::find_generic_password};
    let account = data.to_string_lossy();
    // The host never shows a Keychain dialog; a refusal is an error.
    let _quiet = SecKeychain::disable_user_interaction()?;
    if let Ok((password, _)) = find_generic_password(None, service, &account) {
        return String::from_utf8(password.to_owned()).context("The Keychain item is not text");
    }
    let value = create()?;
    let keychain = SecKeychain::default()?;
    keychain.add_generic_password(service, &account, value.as_bytes())?;
    let (_, item) = keychain.find_generic_password(service, &account)?;
    if let Err(error) = access::limit(&item, trusted) {
        // An item any program could read is worse than none.
        item.delete();
        return Err(error.context(format!("Limit the {label} to Wiffletree")));
    }
    Ok(value)
}

#[cfg(not(target_os = "macos"))]
fn secret(
    _service: &str,
    _label: &str,
    _data: &Path,
    _trusted: &[&Path],
    _create: impl FnOnce() -> Result<String>,
) -> Result<String> {
    anyhow::bail!("Keychain items need macOS; start the host with --child")
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
