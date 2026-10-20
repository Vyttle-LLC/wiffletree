//! Advertising `_wiffletree._tcp` through the system's DNS-SD service, `dns_sd.h` in libSystem.
use anyhow::{Result, bail, ensure};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const SERVICE_TYPE: &str = "_wiffletree._tcp";

/// A TXT record: each `key=value` behind its length byte.
pub fn txt_record(fields: &[(&str, &str)]) -> Result<Vec<u8>> {
    let mut record = vec![];
    for (key, value) in fields {
        let entry = format!("{key}={value}");
        ensure!(entry.len() <= 255, "The TXT field {key} is too long");
        record.push(entry.len() as u8);
        record.extend(entry.as_bytes());
    }
    Ok(record)
}

type ServiceRef = *mut c_void;
type RegisterReply = extern "C" fn(
    ServiceRef,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);

unsafe extern "C" {
    #[allow(clippy::too_many_arguments)]
    fn DNSServiceRegister(
        service: *mut ServiceRef,
        flags: u32,
        interface: u32,
        name: *const c_char,
        kind: *const c_char,
        domain: *const c_char,
        host: *const c_char,
        port: u16,
        txt_length: u16,
        txt: *const c_void,
        reply: RegisterReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(service: ServiceRef) -> i32;
    fn DNSServiceProcessResult(service: ServiceRef) -> i32;
    fn DNSServiceRefDeallocate(service: ServiceRef);
    fn DNSServiceUpdateRecord(
        service: ServiceRef,
        record: *mut c_void,
        flags: u32,
        length: u16,
        data: *const c_void,
        ttl: u32,
    ) -> i32;
}

/// The local network was refused to this process, as Local Network privacy does.
pub const POLICY_DENIED: i32 = -65570;

/// What the daemon answered a registration: its result and the name it registered.
#[derive(Default)]
struct Reply {
    error: Option<i32>,
    name: String,
}

extern "C" fn on_register(
    _: ServiceRef,
    _: u32,
    error: i32,
    name: *const c_char,
    _: *const c_char,
    _: *const c_char,
    context: *mut c_void,
) {
    // SAFETY: `context` is the `Mutex<Reply>` the advertisement owns for as long as it processes
    // results, and `name` is a C string for the call.
    let reply = unsafe { &*(context as *const Mutex<Reply>) };
    let mut reply = reply.lock().unwrap_or_else(|e| e.into_inner());
    reply.error = Some(error);
    if !name.is_null() {
        reply.name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
    }
}

struct Handle(ServiceRef);
// SAFETY: the reference is used only under the advertisement's mutex.
unsafe impl Send for Handle {}

/// A registered service, withdrawn when dropped.
pub struct Advertisement {
    handle: Arc<Mutex<Handle>>,
    reply: Arc<Mutex<Reply>>,
    stopping: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// The name the daemon registered, which differs from the one asked for on a conflict.
    pub name: String,
}
impl Advertisement {
    /// Registers `name` on every interface at `port` with `txt`, and waits for the daemon's
    /// answer. A refusal, such as Local Network privacy's, is an error.
    pub fn register(name: &str, port: u16, txt: &[u8]) -> Result<Self> {
        let (name_c, kind) = (CString::new(name)?, CString::new(SERVICE_TYPE)?);
        let reply = Arc::new(Mutex::new(Reply::default()));
        let mut service: ServiceRef = std::ptr::null_mut();
        // SAFETY: the strings and TXT bytes are valid for the call, and `reply` outlives every
        // result processed, since the worker stops before the reference is deallocated.
        let status = unsafe {
            DNSServiceRegister(
                &mut service,
                0,
                0,
                name_c.as_ptr(),
                kind.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                port.to_be(),
                txt.len() as u16,
                txt.as_ptr().cast(),
                on_register,
                Arc::as_ptr(&reply) as *mut c_void,
            )
        };
        ensure!(status == 0, "Bonjour refused the registration: {status}");
        let handle = Arc::new(Mutex::new(Handle(service)));
        let mut advertisement = Self {
            handle,
            reply,
            stopping: Arc::default(),
            worker: None,
            name: String::new(),
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let error = loop {
            advertisement.process(Duration::from_millis(100));
            if let Some(error) = advertisement.lock_reply().error {
                break error;
            }
            if std::time::Instant::now() > deadline {
                bail!("Bonjour did not answer the registration");
            }
        };
        ensure!(error == 0, "Bonjour refused the registration: {error}");
        let name = advertisement.lock_reply().name.clone();
        advertisement.name = name;
        advertisement.start_worker();
        Ok(advertisement)
    }

    /// Replaces the TXT record.
    pub fn update(&self, txt: &[u8]) -> Result<()> {
        let handle = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: the reference is live while the advertisement is, and the bytes for the call.
        let status = unsafe {
            DNSServiceUpdateRecord(
                handle.0,
                std::ptr::null_mut(),
                0,
                txt.len() as u16,
                txt.as_ptr().cast(),
                0,
            )
        };
        ensure!(status == 0, "Bonjour refused the TXT update: {status}");
        Ok(())
    }

    fn lock_reply(&self) -> std::sync::MutexGuard<'_, Reply> {
        self.reply.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn process(&self, wait: Duration) {
        process(&self.handle, wait);
    }

    /// Keeps reading the daemon's later answers, such as a rename after a conflict, so its
    /// socket never fills.
    fn start_worker(&mut self) {
        let (handle, stopping) = (self.handle.clone(), self.stopping.clone());
        self.worker = Some(std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                process(&handle, Duration::from_millis(200));
            }
        }));
    }
}
impl Drop for Advertisement {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let handle = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: nothing processes results any more, so this is the last use.
        unsafe { DNSServiceRefDeallocate(handle.0) };
    }
}

/// Processes one answer if the daemon sends one within `wait`.
fn process(handle: &Mutex<Handle>, wait: Duration) {
    let handle = handle.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the reference is live under the lock.
    let fd = unsafe { DNSServiceRefSockFD(handle.0) };
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid `pollfd`.
    let ready = unsafe { libc::poll(&mut poll, 1, wait.as_millis() as i32) };
    if ready > 0 {
        // SAFETY: data is waiting, so this does not block; the reference is live under the lock.
        unsafe { DNSServiceProcessResult(handle.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_fields_are_length_prefixed() {
        assert_eq!(
            txt_record(&[("id", "ab"), ("pairing", "0")]).unwrap(),
            b"\x05id=ab\x09pairing=0"
        );
        assert!(txt_record(&[("name", &"x".repeat(300))]).is_err());
    }
}
