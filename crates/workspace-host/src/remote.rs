//! Remote access: other paired devices drive this host over TLS and WebSocket while "Let other
//! devices use this Mac's Wiffletree" is on. The actor owns `Remote`; the listener's threads
//! reach the store only through the actor, with `ToActor`.
pub(crate) mod access;
#[cfg(target_os = "macos")]
pub mod bonjour;
pub(crate) mod devices;
mod listener;
mod pairing;
pub(crate) mod uploads;
mod websocket;

use crate::service::Requests;
use anyhow::{Context, Result};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    net::{Shutdown, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
use workspace_core::{AccessLevel, Command, PAIRING_MS, Pairing, pairing::KeyPair};

type Reply = async_channel::Sender<std::result::Result<serde_json::Value, String>>;
/// Milliseconds since the epoch, as the host's clock or a test's says.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Publishes the host on the local network while remote access is on.
pub trait Advertiser: Send {
    /// Advertises `name` at `port` with `txt`, or replaces the TXT fields of the advertisement
    /// already made.
    fn advertise(&mut self, name: &str, port: u16, txt: &[(&str, &str)]) -> Result<()>;
    fn withdraw(&mut self);
}

/// The system's DNS-SD service.
#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct Bonjour(Option<(u16, bonjour::Advertisement)>);
#[cfg(target_os = "macos")]
impl Advertiser for Bonjour {
    fn advertise(&mut self, name: &str, port: u16, txt: &[(&str, &str)]) -> Result<()> {
        let record = bonjour::txt_record(txt)?;
        match &self.0 {
            Some((advertised, advertisement)) if *advertised == port => {
                advertisement.update(&record)
            }
            _ => {
                self.0 = None;
                self.0 = Some((port, bonjour::Advertisement::register(name, port, &record)?));
                Ok(())
            }
        }
    }
    fn withdraw(&mut self) {
        self.0 = None;
    }
}

/// How this host's remote access runs.
pub struct Options {
    /// 47821, or `0` for any free port.
    pub port: u16,
    /// The host's long-term key, read the first time remote access is on.
    pub key: Box<dyn Fn() -> Result<KeyPair> + Send>,
    pub advertiser: Box<dyn Advertiser>,
    /// The name devices see; this Mac's computer name when `None`.
    pub name: Option<String>,
    pub clock: Clock,
    /// What `Welcome` names as the host's binary.
    pub binary: PathBuf,
}
impl Default for Options {
    /// A host whose key and advertising are not set up: turning remote access on fails.
    fn default() -> Self {
        Self {
            port: workspace_core::REMOTE_PORT,
            key: Box::new(|| anyhow::bail!("This host has no key for remote access")),
            advertiser: Box::new(NoAdvertiser),
            name: None,
            clock: Arc::new(crate::now),
            binary: PathBuf::new(),
        }
    }
}
/// Advertises nothing.
pub struct NoAdvertiser;
impl Advertiser for NoAdvertiser {
    fn advertise(&mut self, _: &str, _: u16, _: &[(&str, &str)]) -> Result<()> {
        Ok(())
    }
    fn withdraw(&mut self) {}
}

/// What the listener's threads tell the actor, which owns the store.
pub(crate) enum ToActor {
    /// A command from a paired device, run if its access level allows it.
    Command {
        device: String,
        command: Command,
        reply: Reply,
    },
    /// A device answered the challenge: the actor checks the signature against the stored key
    /// and answers with the device's level, or `None` when the device is unknown or removed.
    Authenticate {
        device: String,
        signed: Vec<u8>,
        signature: Vec<u8>,
        reply: async_channel::Sender<Option<AccessLevel>>,
    },
    /// A pairing exchange confirmed: store the device with Control and close pairing mode.
    Paired {
        key: Vec<u8>,
        name: String,
        reply: async_channel::Sender<std::result::Result<(), String>>,
    },
    /// Pairing mode or the open connections changed on their own.
    Changed,
}

/// Pairing mode while it is open.
struct Mode {
    code: String,
    expires_at: i64,
    wrong: u32,
    /// An exchange is running; another device is told to wait.
    busy: bool,
}

/// What the actor and the listener's threads share.
pub(crate) struct Shared {
    requests: Requests,
    binary: PathBuf,
    clock: Clock,
    pairing: Mutex<Option<Mode>>,
    connections: Mutex<HashMap<u64, Open>>,
    next: AtomicU64,
    replying: Arc<AtomicUsize>,
}
/// An open remote connection: its device once the challenge names one.
struct Open {
    device: Option<String>,
    stream: TcpStream,
}
impl Shared {
    fn now(&self) -> i64 {
        (self.clock)()
    }
    fn pairing(&self) -> std::sync::MutexGuard<'_, Option<Mode>> {
        self.pairing.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn connections(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Open>> {
        self.connections.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Tracks a connection so it can be closed; returns its id.
    fn track(&self, stream: &TcpStream) -> Result<u64> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let stream = stream.try_clone()?;
        self.connections().insert(
            id,
            Open {
                device: None,
                stream,
            },
        );
        Ok(id)
    }
    fn claim(&self, connection: u64, device: &str) {
        if let Some(open) = self.connections().get_mut(&connection) {
            open.device = Some(device.into());
        }
    }
    fn forget(&self, connection: u64) {
        self.connections().remove(&connection);
    }
    /// Closes the connections `which` picks.
    fn close(&self, which: impl Fn(&Open) -> bool) {
        for open in self.connections().values().filter(|open| which(open)) {
            let _ = open.stream.shutdown(Shutdown::Both);
        }
    }
    fn connected(&self) -> HashSet<String> {
        let connections = self.connections();
        connections
            .values()
            .filter_map(|o| o.device.clone())
            .collect()
    }
    /// Pairing mode, closed first if its time is up.
    fn open_pairing(&self) -> std::sync::MutexGuard<'_, Option<Mode>> {
        let mut pairing = self.pairing();
        if pairing
            .as_ref()
            .is_some_and(|mode| self.now() >= mode.expires_at)
        {
            *pairing = None;
        }
        pairing
    }
}

/// The key, name and port a listening host serves with.
pub(crate) struct Identity {
    pub key: KeyPair,
    pub name: String,
}

/// Remote access as the actor runs it.
pub(crate) struct Remote {
    options: Options,
    shared: Arc<Shared>,
    key: Option<KeyPair>,
    name: String,
    listening: Option<listener::Listening>,
    /// Why the host is not listening although remote access is on.
    error: Option<String>,
    /// The `pairing` TXT field last advertised.
    advertised_pairing: Option<bool>,
}
impl Remote {
    pub(crate) fn new(options: Options, requests: Requests) -> Self {
        let name = options.name.clone().unwrap_or_else(computer_name);
        let shared = Arc::new(Shared {
            requests,
            binary: options.binary.clone(),
            clock: options.clock.clone(),
            pairing: Mutex::default(),
            connections: Mutex::default(),
            next: AtomicU64::new(1),
            replying: Arc::default(),
        });
        Self {
            options,
            shared,
            key: None,
            name,
            listening: None,
            error: None,
            advertised_pairing: None,
        }
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    /// Listens and advertises while `enabled`; otherwise closes the listener, every remote
    /// connection and the advertisement, and pairing mode.
    pub(crate) fn apply(&mut self, enabled: bool) {
        if !enabled {
            self.listening = None;
            self.shared.close(|_| true);
            *self.shared.pairing() = None;
            self.options.advertiser.withdraw();
            self.advertised_pairing = None;
            self.error = None;
            return;
        }
        if self.listening.is_some() {
            return;
        }
        self.error = self.listen().err().map(|error| format!("{error:#}"));
    }
    fn listen(&mut self) -> Result<()> {
        if self.key.is_none() {
            self.key = Some((self.options.key)().context("Read the host's key")?);
        }
        let identity = Arc::new(Identity {
            key: self.key.clone().context("The key was just read")?,
            name: self.name.clone(),
        });
        let listening = listener::listen(self.options.port, identity, self.shared.clone())?;
        self.listening = Some(listening);
        self.advertise();
        Ok(())
    }
    /// Advertises the TXT fields as they are now, if they changed.
    fn advertise(&mut self) {
        let (Some(listening), Some(key)) = (&self.listening, &self.key) else {
            return;
        };
        let pairing = self.shared.open_pairing().is_some();
        if self.advertised_pairing == Some(pairing) {
            return;
        }
        let id = key.id();
        let fields = [
            ("id", workspace_core::pairing::short_id(&id)),
            ("name", self.name.as_str()),
            ("version", workspace_core::build_version()),
            ("pairing", if pairing { "1" } else { "0" }),
        ];
        match self
            .options
            .advertiser
            .advertise(&self.name, listening.port, &fields)
        {
            Ok(()) => self.advertised_pairing = Some(pairing),
            Err(error) => eprintln!("Remote access advertising: {error:#}"),
        }
    }
    /// Opens pairing mode with a new code, replacing one already open.
    pub(crate) fn open_pairing(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.listening.is_some(),
            "Turn on remote access before pairing a device"
        );
        let expires_at = self.shared.now() + PAIRING_MS;
        *self.shared.pairing() = Some(Mode {
            code: new_code()?,
            expires_at,
            wrong: 0,
            busy: false,
        });
        // The advertisement says pairing is off again once the time is up.
        let requests = self.shared.requests.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(PAIRING_MS as u64));
            let _ = requests.tell(ToActor::Changed);
        });
        self.advertise();
        Ok(())
    }
    pub(crate) fn close_pairing(&mut self) {
        *self.shared.pairing() = None;
        self.advertise();
    }
    /// Takes in a change pairing mode or the connections made on their own.
    pub(crate) fn refresh(&mut self) {
        self.advertise();
    }
    /// Closes every connection of the device, which reconnects under its new level, or not at
    /// all once removed.
    pub(crate) fn close_device(&self, device: &str) {
        self.shared
            .close(|open| open.device.as_deref() == Some(device));
    }
    pub(crate) fn pairing(&self) -> Option<Pairing> {
        self.shared.open_pairing().as_ref().map(|mode| Pairing {
            code: mode.code.clone(),
            expires_at: mode.expires_at,
        })
    }
    pub(crate) fn connected(&self) -> HashSet<String> {
        self.shared.connected()
    }
    pub(crate) fn port(&self) -> Option<u16> {
        self.listening.as_ref().map(|l| l.port)
    }
    pub(crate) fn error(&self) -> Option<String> {
        self.error.clone()
    }
    pub(crate) fn host_id(&self) -> String {
        self.key.as_ref().map(KeyPair::id).unwrap_or_default()
    }
    /// The time as remote access counts it, for pairing mode and uploads.
    pub(crate) fn now(&self) -> i64 {
        self.shared.now()
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        self.apply(false);
    }
}

/// A random 4-digit code, each one equally likely.
fn new_code() -> Result<String> {
    let mut file = std::fs::File::open("/dev/urandom")?;
    loop {
        let mut bytes = [0u8; 2];
        file.read_exact(&mut bytes)?;
        let value = u16::from_be_bytes(bytes);
        // 65,536 is not a multiple of 10,000, so the top values are drawn again.
        if value < 60_000 {
            return Ok(format!("{:04}", value % 10_000));
        }
    }
}

/// This Mac's name as the Sharing settings give it, or its host name.
fn computer_name() -> String {
    let named = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    };
    named("/usr/sbin/scutil", &["--get", "ComputerName"])
        .or_else(|| named("/bin/hostname", &["-s"]))
        .unwrap_or_else(|| "This Mac".into())
}

/// Where devices can reach this Mac: its Bonjour name and its IPv4 addresses.
pub(crate) fn addresses() -> Vec<String> {
    let mut addresses = vec![];
    if let Ok(output) = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "LocalHostName"])
        .output()
        && output.status.success()
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !name.is_empty() {
            addresses.push(format!("{name}.local"));
        }
    }
    let mut interfaces: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills the pointer with a list freed below.
    if unsafe { libc::getifaddrs(&mut interfaces) } == 0 {
        let mut interface = interfaces;
        while !interface.is_null() {
            // SAFETY: a node of the list getifaddrs returned.
            let node = unsafe { &*interface };
            if !node.ifa_addr.is_null()
                && i32::from(unsafe { (*node.ifa_addr).sa_family }) == libc::AF_INET
            {
                // SAFETY: an AF_INET address is a `sockaddr_in`.
                let address = unsafe { &*(node.ifa_addr as *const libc::sockaddr_in) };
                let ip = std::net::Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
                if !ip.is_loopback() && !ip.is_link_local() {
                    addresses.push(ip.to_string());
                }
            }
            interface = node.ifa_next;
        }
        // SAFETY: the list getifaddrs returned, freed once.
        unsafe { libc::freeifaddrs(interfaces) };
    }
    addresses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_four_digits() {
        for _ in 0..50 {
            assert!(workspace_core::pairing::is_code(&new_code().unwrap()));
        }
    }
}
