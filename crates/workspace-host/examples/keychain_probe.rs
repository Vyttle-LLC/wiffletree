//! Keychain probe for the client credential (task 0.1 of
//! `openspec/changes/archive/2026-10-19-standalone-host`).
//!
//! Creates, reads and deletes a generic-password item the way design §2 describes, and
//! reports the Keychain's answer without printing the secret. Sign the built example (and
//! copies of it) to see which programs the item admits; `docs/DEVELOPMENT.md` lists the runs.
//!
//!   keychain_probe create <service> [--data-protection] [--access-group <group>] [--trust <path>]...
//!   keychain_probe read   <service> [--data-protection] [--access-group <group>]
//!   keychain_probe delete <service> [--data-protection] [--access-group <group>]
//!
//! Without `--data-protection` the item goes to the file-based login keychain, and its access
//! list holds this binary plus every `--trust` path. Use a test service such as
//! `com.vyttle.wiffletree.spike`, never a real Wiffletree item.

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match probe::run(&args) {
        Ok(report) => println!("{report}"),
        Err(report) => {
            eprintln!("{report}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("keychain_probe runs only on macOS");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
mod probe {
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, CFTypeRef, OSStatus, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::{CString, c_void};
    use std::io::Read;
    use std::ptr;

    const ACCOUNT: &str = "client-credential";

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecClass: CFStringRef;
        static kSecClassGenericPassword: CFStringRef;
        static kSecAttrService: CFStringRef;
        static kSecAttrAccount: CFStringRef;
        static kSecAttrAccess: CFStringRef;
        static kSecAttrAccessGroup: CFStringRef;
        static kSecUseDataProtectionKeychain: CFStringRef;
        static kSecValueData: CFStringRef;
        static kSecReturnData: CFStringRef;

        fn SecItemAdd(attributes: CFTypeRef, result: *mut CFTypeRef) -> OSStatus;
        fn SecItemCopyMatching(query: CFTypeRef, result: *mut CFTypeRef) -> OSStatus;
        fn SecItemDelete(query: CFTypeRef) -> OSStatus;
        fn SecCopyErrorMessageString(status: OSStatus, reserved: *mut c_void) -> CFStringRef;
        // The file-based keychain's access list. Deprecated since 10.10 but still the only way
        // to name more than the creating program.
        fn SecTrustedApplicationCreateFromPath(path: *const i8, app: *mut CFTypeRef) -> OSStatus;
        fn SecAccessCreate(
            descriptor: CFStringRef,
            trusted: CFTypeRef,
            access: *mut CFTypeRef,
        ) -> OSStatus;
    }

    struct Options {
        service: String,
        data_protection: bool,
        access_group: Option<String>,
        trusted: Vec<String>,
    }

    pub fn run(args: &[String]) -> Result<String, String> {
        let (command, rest) = args.split_first().ok_or_else(usage)?;
        let options = parse(rest)?;
        match command.as_str() {
            "create" => create(&options),
            "read" => read(&options),
            "delete" => delete(&options),
            _ => Err(usage()),
        }
    }

    fn usage() -> String {
        "usage: keychain_probe <create|read|delete> <service> [--data-protection] \
         [--access-group <group>] [--trust <path>]..."
            .to_owned()
    }

    fn parse(args: &[String]) -> Result<Options, String> {
        let (service, flags) = args.split_first().ok_or_else(usage)?;
        let mut options = Options {
            service: service.clone(),
            data_protection: false,
            access_group: None,
            trusted: Vec::new(),
        };
        let mut flags = flags.iter();
        while let Some(flag) = flags.next() {
            match flag.as_str() {
                "--data-protection" => options.data_protection = true,
                "--access-group" => options.access_group = flags.next().cloned(),
                "--trust" => options.trusted.extend(flags.next().cloned()),
                _ => return Err(usage()),
            }
        }
        Ok(options)
    }

    fn create(options: &Options) -> Result<String, String> {
        let mut secret = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut random| random.read_exact(&mut secret))
            .map_err(|error| format!("random: {error}"))?;
        let mut attributes = query(options);
        attributes.push((
            key(unsafe { kSecValueData }),
            CFData::from_buffer(&secret).as_CFType(),
        ));
        if !options.data_protection {
            attributes.push((
                key(unsafe { kSecAttrAccess }),
                access_list(&options.trusted)?,
            ));
        }
        let status = unsafe { SecItemAdd(dictionary(&attributes).as_CFTypeRef(), ptr::null_mut()) };
        outcome("create", status, || "created a 32-byte item".to_owned())
    }

    fn read(options: &Options) -> Result<String, String> {
        let mut attributes = query(options);
        attributes.push((
            key(unsafe { kSecReturnData }),
            CFBoolean::true_value().as_CFType(),
        ));
        let mut result: CFTypeRef = ptr::null();
        let status =
            unsafe { SecItemCopyMatching(dictionary(&attributes).as_CFTypeRef(), &mut result) };
        outcome("read", status, || {
            let data = unsafe { CFData::wrap_under_create_rule(result.cast()) };
            format!("read {} bytes", data.len())
        })
    }

    fn delete(options: &Options) -> Result<String, String> {
        let status = unsafe { SecItemDelete(dictionary(&query(options)).as_CFTypeRef()) };
        outcome("delete", status, || "deleted".to_owned())
    }

    fn query(options: &Options) -> Vec<(CFString, CFType)> {
        let mut attributes = vec![
            (
                key(unsafe { kSecClass }),
                key(unsafe { kSecClassGenericPassword }).as_CFType(),
            ),
            (
                key(unsafe { kSecAttrService }),
                CFString::new(&options.service).as_CFType(),
            ),
            (
                key(unsafe { kSecAttrAccount }),
                CFString::new(ACCOUNT).as_CFType(),
            ),
        ];
        if options.data_protection {
            attributes.push((
                key(unsafe { kSecUseDataProtectionKeychain }),
                CFBoolean::true_value().as_CFType(),
            ));
        }
        if let Some(group) = &options.access_group {
            attributes.push((
                key(unsafe { kSecAttrAccessGroup }),
                CFString::new(group).as_CFType(),
            ));
        }
        attributes
    }

    /// This binary (a null path) plus every `--trust` path.
    fn access_list(trusted: &[String]) -> Result<CFType, String> {
        let mut applications = vec![trusted_application(None)?];
        for path in trusted {
            applications.push(trusted_application(Some(path))?);
        }
        let applications = CFArray::from_CFTypes(&applications);
        let mut access: CFTypeRef = ptr::null();
        let descriptor = CFString::new("Wiffletree client credential (spike)");
        let status = unsafe {
            SecAccessCreate(
                descriptor.as_concrete_TypeRef(),
                applications.as_CFTypeRef(),
                &mut access,
            )
        };
        if status != 0 {
            return Err(format!("access list: {}", message(status)));
        }
        Ok(unsafe { CFType::wrap_under_create_rule(access) })
    }

    fn trusted_application(path: Option<&str>) -> Result<CFType, String> {
        let path = path
            .map(|path| CString::new(path).map_err(|_| format!("bad path {path:?}")))
            .transpose()?;
        let mut application: CFTypeRef = ptr::null();
        let status = unsafe {
            SecTrustedApplicationCreateFromPath(
                path.as_ref().map_or(ptr::null(), |path| path.as_ptr()),
                &mut application,
            )
        };
        if status != 0 {
            return Err(format!("trusted application {path:?}: {}", message(status)));
        }
        Ok(unsafe { CFType::wrap_under_create_rule(application) })
    }

    fn outcome(
        action: &str,
        status: OSStatus,
        success: impl FnOnce() -> String,
    ) -> Result<String, String> {
        if status == 0 {
            Ok(success())
        } else {
            Err(format!("{action}: {}", message(status)))
        }
    }

    fn message(status: OSStatus) -> String {
        let text = unsafe { SecCopyErrorMessageString(status, ptr::null_mut()) };
        if text.is_null() {
            return format!("OSStatus {status}");
        }
        let text = unsafe { CFString::wrap_under_create_rule(text) };
        format!("OSStatus {status} ({text})")
    }

    fn key(constant: CFStringRef) -> CFString {
        unsafe { CFString::wrap_under_get_rule(constant) }
    }

    fn dictionary(pairs: &[(CFString, CFType)]) -> CFDictionary<CFString, CFType> {
        CFDictionary::from_CFType_pairs(pairs)
    }
}
