//! Keeps the Mac out of idle sleep while agents work: one IOKit assertion, held while any turn
//! or compaction runs. The display may still sleep, and a closed lid without an external
//! display still sleeps the Mac. Elsewhere it holds nothing.

/// The name `pmset -g assertions` shows.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const ASSERTION_NAME: &str = "Wiffletree agents are working";

/// The assertion, held between `hold(true)` and `hold(false)` or the drop.
#[derive(Default)]
pub(crate) struct Awake {
    assertion: Option<u32>,
}
impl Awake {
    pub(crate) fn hold(&mut self, working: bool) {
        match (working, self.assertion) {
            (true, None) => self.assertion = iokit::create(),
            (false, Some(id)) => {
                iokit::release(id);
                self.assertion = None;
            }
            _ => {}
        }
    }
}
impl Drop for Awake {
    fn drop(&mut self) {
        self.hold(false);
    }
}

#[cfg(target_os = "macos")]
mod iokit {
    use core_foundation::{base::TCFType, string::CFString};

    /// `kIOPMAssertionLevelOn`.
    const LEVEL_ON: u32 = 255;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            kind: core_foundation::string::CFStringRef,
            level: u32,
            name: core_foundation::string::CFStringRef,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    pub(super) fn create() -> Option<u32> {
        let kind = CFString::new("PreventUserIdleSystemSleep");
        let name = CFString::new(super::ASSERTION_NAME);
        let mut id = 0;
        // SAFETY: both strings live for the call, and `id` is valid for a write.
        let status = unsafe {
            IOPMAssertionCreateWithName(
                kind.as_concrete_TypeRef(),
                LEVEL_ON,
                name.as_concrete_TypeRef(),
                &mut id,
            )
        };
        if status != 0 {
            eprintln!("Keep the Mac awake: IOKit refused the power assertion ({status})");
            return None;
        }
        Some(id)
    }

    pub(super) fn release(id: u32) {
        // SAFETY: `id` came from `create` and is released once.
        unsafe { IOPMAssertionRelease(id) };
    }
}

#[cfg(not(target_os = "macos"))]
mod iokit {
    pub(super) fn create() -> Option<u32> {
        None
    }
    pub(super) fn release(_: u32) {}
}
