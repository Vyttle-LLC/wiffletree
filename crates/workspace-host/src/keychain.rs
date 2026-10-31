//! The client credential of a signed launchd host, the host's remote-access key and, while an
//! import from Linear runs, the human's Linear key, each kept in a login Keychain item whose
//! access list names only the programs given: the signed app and the host's own copy. Keychain matches them by their signing requirement, and asks any other
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
    use workspace_core::pairing::{KeyPair, hex, unhex};
    let secret = secret(HOST_KEY_SERVICE, "host key", data, trusted, || {
        Ok(hex(&KeyPair::generate()?.secret()))
    })?;
    KeyPair::from_secret(&unhex(&secret)?)
}

/// The Linear personal API key's login Keychain item, kept only while an import from Linear
/// runs; its account is the data folder like the others'.
pub const LINEAR_KEY_SERVICE: &str = "com.vyttle.wiffletree.linear-key";

/// Keeps `key` as the Linear key, replacing any kept before, readable by the host only.
#[cfg(target_os = "macos")]
pub fn keep_linear_key(data: &Path, key: &str) -> Result<()> {
    use anyhow::Context as _;
    remove_linear_key(data)?;
    let host = std::env::current_exe()?;
    let list = access::list(&[host.as_path()]).context("Limit the Linear key to Wiffletree")?;
    access::add(
        LINEAR_KEY_SERVICE,
        &data.to_string_lossy(),
        key.as_bytes(),
        &list,
    )
}

/// The Linear key `keep_linear_key` kept.
#[cfg(target_os = "macos")]
pub fn linear_key(data: &Path) -> Result<String> {
    use anyhow::Context as _;
    use security_framework::os::macos::{keychain::SecKeychain, passwords::find_generic_password};
    let _quiet = SecKeychain::disable_user_interaction()?;
    let (password, _) = find_generic_password(None, LINEAR_KEY_SERVICE, &data.to_string_lossy())
        .context("The Linear key is no longer in the Keychain; paste it again")?;
    String::from_utf8(password.to_owned()).context("The Keychain item is not text")
}

/// Removes the Linear key, if one is kept.
#[cfg(target_os = "macos")]
pub fn remove_linear_key(data: &Path) -> Result<()> {
    use security_framework::os::macos::{keychain::SecKeychain, passwords::find_generic_password};
    let _quiet = SecKeychain::disable_user_interaction()?;
    if let Ok((_, item)) = find_generic_password(None, LINEAR_KEY_SERVICE, &data.to_string_lossy())
    {
        item.delete();
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn keep_linear_key(_data: &Path, _key: &str) -> Result<()> {
    anyhow::bail!("Importing from Linear needs the macOS Keychain")
}

#[cfg(not(target_os = "macos"))]
pub fn linear_key(_data: &Path) -> Result<String> {
    anyhow::bail!("Importing from Linear needs the macOS Keychain")
}

#[cfg(not(target_os = "macos"))]
pub fn remove_linear_key(_data: &Path) -> Result<()> {
    Ok(())
}

/// The generic-password item `service`, the `label`, for the data folder, created with
/// `create`'s value and `trusted` as its access list when there is none. The access list is set
/// as the item is added, so changing it never needs the login password.
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
    // An item any program could read is worse than none, so none is added without the list.
    let list = access::list(trusted).with_context(|| format!("Limit the {label} to Wiffletree"))?;
    access::add(service, &account, value.as_bytes(), &list)?;
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

/// Access lists and adding an item with one, which the crate does not wrap.
#[cfg(target_os = "macos")]
mod access {
    use anyhow::{Result, ensure};
    use core_foundation::{
        array::CFArray,
        base::{CFType, CFTypeRef, OSStatus, TCFType},
        data::CFData,
        dictionary::CFDictionary,
        string::{CFString, CFStringRef},
    };
    use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecClass: CFStringRef;
        static kSecClassGenericPassword: CFStringRef;
        static kSecAttrService: CFStringRef;
        static kSecAttrAccount: CFStringRef;
        static kSecAttrAccess: CFStringRef;
        static kSecValueData: CFStringRef;
        fn SecTrustedApplicationCreateFromPath(
            path: *const std::ffi::c_char,
            app: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecAccessCreate(
            descriptor: CFStringRef,
            trusted: core_foundation::array::CFArrayRef,
            access: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecItemAdd(attributes: CFTypeRef, result: *mut CFTypeRef) -> OSStatus;
    }

    /// An access list of exactly the `trusted` programs.
    pub(super) fn list(trusted: &[&Path]) -> Result<CFType> {
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
        let descriptor = CFString::new("Wiffletree");
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
        Ok(unsafe { CFType::wrap_under_create_rule(access) })
    }

    /// Adds a generic-password item to the login keychain with `access` as its access list.
    pub(super) fn add(service: &str, account: &str, value: &[u8], access: &CFType) -> Result<()> {
        // SAFETY: the constants are static CF strings the framework owns.
        let key = |constant: CFStringRef| unsafe { CFString::wrap_under_get_rule(constant) };
        let attributes = CFDictionary::from_CFType_pairs(&[
            (
                key(unsafe { kSecClass }),
                key(unsafe { kSecClassGenericPassword }).as_CFType(),
            ),
            (
                key(unsafe { kSecAttrService }),
                CFString::new(service).as_CFType(),
            ),
            (
                key(unsafe { kSecAttrAccount }),
                CFString::new(account).as_CFType(),
            ),
            (
                key(unsafe { kSecValueData }),
                CFData::from_buffer(value).as_CFType(),
            ),
            (key(unsafe { kSecAttrAccess }), access.clone()),
        ]);
        // SAFETY: a live dictionary; no result is asked for.
        let status = unsafe { SecItemAdd(attributes.as_CFTypeRef(), std::ptr::null_mut()) };
        ensure!(
            status == 0,
            "Add the Keychain item: {}",
            security_framework::base::Error::from_code(status)
        );
        Ok(())
    }
}
