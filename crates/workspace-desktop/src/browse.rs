//! Wiffletree hosts on this network (design 6 of `remote-hosts`): browsing `_wiffletree._tcp`
//! through the system's DNS-SD service, `dns_sd.h` in libSystem, and resolving each to its
//! address and TXT fields. Connect a computer lists them, and a paired host that moved is found
//! again by its id.
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_char, c_void},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use workspace_core::SERVICE_TYPE;

/// A host as it advertises itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// Its address, `name:port`.
    pub address: String,
    /// The first 8 bytes of its id, in hex.
    pub short_id: String,
    pub name: String,
    pub version: String,
    /// Its pairing mode is open.
    pub pairing: bool,
}

/// How often known hosts are resolved again while something wants them fresh: a host changes
/// its TXT fields in place, as when it opens pairing, which browsing does not report.
const FRESH_EVERY: Duration = Duration::from_secs(2);

/// The wait before browsing again after the daemon's connection failed, doubling to the last.
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_LAST: Duration = Duration::from_secs(60);

/// What browsing has found, shared with the browsing thread.
#[derive(Default)]
pub(crate) struct Browsed {
    /// Each host by its advertised service name.
    found: Mutex<BTreeMap<String, Found>>,
    /// Known hosts are resolved again until then.
    fresh_until: Mutex<Option<Instant>>,
}
impl Browsed {
    pub(crate) fn found(&self) -> Vec<Found> {
        let found = self.found.lock().unwrap_or_else(|e| e.into_inner());
        found.values().cloned().collect()
    }
    /// Keeps the TXT fields current for the next few seconds, as Connect a computer does while
    /// it is open.
    pub(crate) fn keep_fresh(&self) {
        let until = Instant::now() + FRESH_EVERY * 2;
        *self.fresh_until.lock().unwrap_or_else(|e| e.into_inner()) = Some(until);
    }
    fn wants_fresh(&self) -> bool {
        let until = *self.fresh_until.lock().unwrap_or_else(|e| e.into_inner());
        until.is_some_and(|until| Instant::now() < until)
    }
    fn clear(&self) {
        self.found.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    fn set(&self, name: &str, resolved: Option<Found>) {
        let mut found = self.found.lock().unwrap_or_else(|e| e.into_inner());
        match resolved {
            Some(resolved) => found.insert(name.to_owned(), resolved),
            None => found.remove(name),
        };
    }
}
pub(crate) type Hosts = Arc<Browsed>;

/// Starts browsing for as long as the app runs, and returns what it finds.
pub(crate) fn start() -> Hosts {
    let hosts = Hosts::default();
    let browsed = hosts.clone();
    let started = std::thread::Builder::new()
        .name("bonjour-browse".into())
        .spawn(move || {
            // Browsing ends only when the daemon's connection fails, as when mDNSResponder
            // restarts; it starts again after a wait that grows while it keeps failing.
            let mut wait = RETRY_FIRST;
            loop {
                let started = Instant::now();
                // A new browse reports every host again, and one gone meanwhile not at all.
                browsed.clear();
                if let Err(error) = browse(&browsed) {
                    eprintln!("Browsing for Wiffletree hosts: {error}");
                }
                if started.elapsed() > RETRY_LAST {
                    wait = RETRY_FIRST;
                }
                std::thread::sleep(wait);
                wait = (wait * 2).min(RETRY_LAST);
            }
        });
    if let Err(error) = started {
        eprintln!("Browsing for Wiffletree hosts: {error}");
    }
    hosts
}

/// What a TXT record's fields say, or `None` when it lacks the id.
pub(crate) fn found(address: String, txt: &[u8]) -> Option<Found> {
    let mut fields = BTreeMap::new();
    let mut rest = txt;
    while let Some((&length, tail)) = rest.split_first() {
        let (entry, after) = tail.split_at((length as usize).min(tail.len()));
        let entry = String::from_utf8_lossy(entry);
        if let Some((key, value)) = entry.split_once('=') {
            fields.insert(key.to_owned(), value.to_owned());
        }
        rest = after;
    }
    let field = |key: &str| fields.get(key).cloned().unwrap_or_default();
    Some(Found {
        address,
        short_id: fields.get("id")?.clone(),
        name: field("name"),
        version: field("version"),
        pairing: field("pairing") == "1",
    })
}

type ServiceRef = *mut c_void;
type BrowseReply = extern "C" fn(
    ServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);
type ResolveReply = extern "C" fn(
    ServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    u16,
    u16,
    *const u8,
    *mut c_void,
);

unsafe extern "C" {
    fn DNSServiceBrowse(
        service: *mut ServiceRef,
        flags: u32,
        interface: u32,
        kind: *const c_char,
        domain: *const c_char,
        reply: BrowseReply,
        context: *mut c_void,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn DNSServiceResolve(
        service: *mut ServiceRef,
        flags: u32,
        interface: u32,
        name: *const c_char,
        kind: *const c_char,
        domain: *const c_char,
        reply: ResolveReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(service: ServiceRef) -> i32;
    fn DNSServiceProcessResult(service: ServiceRef) -> i32;
    fn DNSServiceRefDeallocate(service: ServiceRef);
}

/// `kDNSServiceFlagsAdd`: the service appeared rather than went away.
const FLAG_ADD: u32 = 0x2;

/// A service that came or went, as the browse callback reports it.
struct Seen {
    added: bool,
    name: String,
    domain: String,
    interface: u32,
}

extern "C" fn on_browse(
    _: ServiceRef,
    flags: u32,
    interface: u32,
    error: i32,
    name: *const c_char,
    _: *const c_char,
    domain: *const c_char,
    context: *mut c_void,
) {
    if error != 0 || name.is_null() || domain.is_null() {
        return;
    }
    // SAFETY: `context` is the browse loop's `Vec<Seen>`, alive while it processes results, and
    // the strings are valid for the call.
    let seen = unsafe { &mut *(context as *mut Vec<Seen>) };
    let text = |s: *const c_char| unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
    seen.push(Seen {
        added: flags & FLAG_ADD != 0,
        name: text(name),
        domain: text(domain),
        interface,
    });
}

extern "C" fn on_resolve(
    _: ServiceRef,
    _: u32,
    _: u32,
    error: i32,
    _: *const c_char,
    target: *const c_char,
    port: u16,
    txt_length: u16,
    txt: *const u8,
    context: *mut c_void,
) {
    // SAFETY: `context` is the resolve's `Option<Found>`, alive while it processes results.
    let resolved = unsafe { &mut *(context as *mut Option<Found>) };
    if error != 0 || target.is_null() {
        return;
    }
    // SAFETY: the target is a C string and the TXT bytes are `txt_length` long for the call.
    let target = unsafe { CStr::from_ptr(target) }.to_string_lossy();
    let txt = match txt.is_null() {
        true => &[][..],
        false => unsafe { std::slice::from_raw_parts(txt, txt_length as usize) },
    };
    let address = format!("{}:{}", target.trim_end_matches('.'), u16::from_be(port));
    *resolved = found(address, txt);
}

/// Waits up to `wait` for the daemon's answer to `service` and processes it; fails when the
/// daemon's connection does.
fn process(service: ServiceRef, wait: Duration) -> anyhow::Result<()> {
    // SAFETY: `service` is live for the call.
    let fd = unsafe { DNSServiceRefSockFD(service) };
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd.
    if unsafe { libc::poll(&mut poll, 1, wait.as_millis() as i32) } <= 0 {
        return Ok(());
    }
    // SAFETY: `service` is live, and its callback's context outlives the call.
    let status = unsafe { DNSServiceProcessResult(service) };
    anyhow::ensure!(
        status == 0,
        "The Bonjour daemon stopped answering: {status}"
    );
    Ok(())
}

/// Browses until the daemon's connection fails.
fn browse(hosts: &Hosts) -> anyhow::Result<()> {
    let kind = CString::new(SERVICE_TYPE)?;
    let mut seen: Vec<Seen> = vec![];
    let mut service: ServiceRef = std::ptr::null_mut();
    // SAFETY: the strings are valid for the call, and `seen` outlives every result processed,
    // since this function processes them and never returns while browsing.
    let status = unsafe {
        DNSServiceBrowse(
            &mut service,
            0,
            0,
            kind.as_ptr(),
            std::ptr::null(),
            on_browse,
            (&mut seen as *mut Vec<Seen>).cast(),
        )
    };
    anyhow::ensure!(status == 0, "Bonjour refused to browse: {status}");
    let mut known: BTreeMap<String, Seen> = BTreeMap::new();
    loop {
        if let Err(error) = process(service, FRESH_EVERY) {
            // SAFETY: allocated above and no longer used.
            unsafe { DNSServiceRefDeallocate(service) };
            return Err(error);
        }
        for change in std::mem::take(&mut seen) {
            let resolved = change.added.then(|| resolve(&change)).flatten();
            hosts.set(&change.name, resolved);
            match change.added {
                true => known.insert(change.name.clone(), change),
                false => known.remove(&change.name),
            };
        }
        if hosts.wants_fresh() {
            for (name, service) in &known {
                hosts.set(name, resolve(service));
            }
        }
    }
}

/// The address and TXT fields of a service the browse found, or `None` when it does not answer.
fn resolve(seen: &Seen) -> Option<Found> {
    let (name, kind, domain) = (
        CString::new(seen.name.as_str()).ok()?,
        CString::new(SERVICE_TYPE).ok()?,
        CString::new(seen.domain.as_str()).ok()?,
    );
    let mut resolved: Option<Found> = None;
    let mut service: ServiceRef = std::ptr::null_mut();
    // SAFETY: the strings are valid for the call, and `resolved` outlives the results processed
    // below, before the reference is deallocated.
    let status = unsafe {
        DNSServiceResolve(
            &mut service,
            0,
            seen.interface,
            name.as_ptr(),
            kind.as_ptr(),
            domain.as_ptr(),
            on_resolve,
            (&mut resolved as *mut Option<Found>).cast(),
        )
    };
    if status != 0 {
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while resolved.is_none() && Instant::now() < deadline {
        if process(service, Duration::from_millis(250)).is_err() {
            break;
        }
    }
    // SAFETY: allocated above and no longer used.
    unsafe { DNSServiceRefDeallocate(service) };
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "advertises on this Mac's Bonjour service"]
    fn a_host_that_opens_pairing_after_it_was_found_reads_as_pairing() {
        use workspace_host::remote::bonjour::{Advertisement, txt_record};
        let id = format!("{:016x}", std::process::id());
        let txt = |pairing| {
            txt_record(&[
                ("id", &id),
                ("name", "Probe Mac"),
                ("version", "0.4"),
                ("pairing", pairing),
            ])
            .unwrap()
        };
        let advertised = Advertisement::register(&format!("Probe {id}"), 47999, &txt("0")).unwrap();
        let browsed = start();
        let pairing = |browsed: &Hosts| {
            browsed.keep_fresh();
            (browsed.found().into_iter())
                .find(|found| found.short_id == id)
                .map(|found| found.pairing)
        };
        let until = |want: bool| {
            let deadline = Instant::now() + Duration::from_secs(20);
            while pairing(&browsed) != Some(want) {
                assert!(Instant::now() < deadline, "never read pairing={want}");
                std::thread::sleep(Duration::from_millis(200));
            }
        };
        until(false);
        advertised.update(&txt("1")).unwrap();
        until(true);
    }

    #[test]
    fn the_txt_fields_name_the_host_its_version_and_whether_it_pairs() {
        let mut txt = vec![];
        for field in [
            "id=0123456789abcdef",
            "name=Studio Mac",
            "version=0.4",
            "pairing=1",
        ] {
            txt.push(field.len() as u8);
            txt.extend(field.as_bytes());
        }
        assert_eq!(
            found("studio.local:47821".into(), &txt),
            Some(Found {
                address: "studio.local:47821".into(),
                short_id: "0123456789abcdef".into(),
                name: "Studio Mac".into(),
                version: "0.4".into(),
                pairing: true,
            })
        );
        assert_eq!(found("a:1".into(), &txt[20..]), None, "no id");
        assert_eq!(found("a:1".into(), &[200, b'x']), None, "a cut record");
    }
}
