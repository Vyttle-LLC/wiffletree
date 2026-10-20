//! A paired host as `HostClient` reaches it (design 5 and 7 of `remote-hosts`): TLS pinned to
//! the host's key and the challenge, through `remote_client::connect`, then a relay between the
//! WebSocket and one end of a local socket pair. The client speaks to the other end as it does
//! to `host.sock`: each line it writes goes as one text frame, and each text frame comes back as
//! one line.
use crate::{pairing::KeyPair, remote_client::RemoteError, *};
use std::{
    io::{self, ErrorKind, Read, Write},
    net::IpAddr,
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tungstenite::Message;

/// The largest message either way: a `read_file` reply carries up to 50 MB as base64.
const MAX_MESSAGE: usize = 80 << 20;

/// Why a paired host turned this device away, which trying again does not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turnaway {
    /// Its certificate carries another key than the one stored at pairing.
    KeyChanged,
    /// It no longer knows this device.
    Unpaired,
}

/// What the last try at a paired host found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteState {
    /// What the host lets this device do, as its last connection said.
    pub level: Option<AccessLevel>,
    /// Why the host turned the device away, until a connection succeeds.
    pub turnaway: Option<Turnaway>,
    /// The address that last answered.
    pub address: Option<String>,
}

/// What browsing found for a paired host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovered {
    /// The addresses advertised under the host's id.
    pub addresses: Vec<String>,
    /// A host advertises the paired host's name under another id, as one whose key was made
    /// anew does after a reinstall.
    pub same_name_other_id: bool,
}

/// Browses for a paired host by its id and name.
pub type Discover = Box<dyn Fn() -> Discovered + Send>;

/// Reaches a paired host by its stored addresses, then by those `discover` finds.
pub struct RemoteLaunch {
    host_key: Vec<u8>,
    device: KeyPair,
    addresses: Vec<String>,
    discover: Discover,
    state: Arc<Mutex<RemoteState>>,
}
impl RemoteLaunch {
    pub fn new(
        host_key: Vec<u8>,
        device: KeyPair,
        addresses: Vec<String>,
        discover: Discover,
    ) -> Self {
        Self {
            host_key,
            device,
            addresses,
            discover,
            state: Arc::default(),
        }
    }
    /// What the last try found, shared with whoever shows it.
    pub fn state(&self) -> Arc<Mutex<RemoteState>> {
        self.state.clone()
    }
    fn set(&self, change: impl FnOnce(&mut RemoteState)) {
        change(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()));
    }
    fn try_address(&mut self, address: &str) -> Result<(UnixStream, Option<String>), Tried> {
        match remote_client::connect(address, &self.host_key, &self.device) {
            Ok(remote) => {
                self.set(|state| {
                    state.level = Some(remote.level);
                    state.turnaway = None;
                    state.address = Some(address.into());
                });
                // The address that answered is tried first next time.
                self.addresses.retain(|a| a != address);
                self.addresses.insert(0, address.into());
                relay(remote.socket).map_err(Tried::Failed)
            }
            Err(RemoteError::KeyChanged) => Err(Tried::OtherKey),
            Err(RemoteError::Unpaired) => {
                self.set(|state| state.turnaway = Some(Turnaway::Unpaired));
                Err(Tried::Unpaired)
            }
            Err(error) => Err(Tried::Failed(io::Error::other(format!(
                "{address}: {error}"
            )))),
        }
    }
}
impl Launch for RemoteLaunch {
    fn endpoint(&mut self, _: &AtomicBool) -> io::Result<Endpoint> {
        Err(io::Error::other(
            "A paired host is reached over the network",
        ))
    }
    /// Tries the addresses the host last answered at, newest first, and only then browses for
    /// it, so a host reached by name, as over Tailscale, comes back without Bonjour. Another key
    /// at a name or a Tailscale address is the host's key changed, since those name one machine.
    /// Another key at any other IP address, which a LAN may give to another machine, passes it
    /// over. When nothing shows the pinned key, the host's key has changed too if an address
    /// browsed for under its id shows another, or a host advertises its name under another id.
    fn connect(&mut self, stopped: &AtomicBool) -> io::Result<(UnixStream, Option<String>)> {
        let mut last = io::Error::other("No address is known for this host");
        let (mut tried, mut other_key) = (vec![], vec![]);
        let mut discovered = Discovered::default();
        for browsing in [false, true] {
            if stopped.load(Ordering::Relaxed) {
                break;
            }
            let addresses = match browsing {
                false => self.addresses.clone(),
                true => {
                    discovered = (self.discover)();
                    discovered.addresses.clone()
                }
            };
            for address in addresses {
                if stopped.load(Ordering::Relaxed) {
                    break;
                }
                if tried.contains(&address) {
                    continue;
                }
                tried.push(address.clone());
                match self.try_address(&address) {
                    Ok(connection) => return Ok(connection),
                    Err(Tried::Unpaired) => {
                        return Err(io::Error::other(RemoteError::Unpaired.to_string()));
                    }
                    Err(Tried::OtherKey) if names_one_machine(&address) => {
                        self.set(|state| state.turnaway = Some(Turnaway::KeyChanged));
                        return Err(io::Error::other(RemoteError::KeyChanged.to_string()));
                    }
                    Err(Tried::OtherKey) => {
                        last = io::Error::other(format!("{address} is another machine now"));
                        other_key.push(address);
                    }
                    Err(Tried::Failed(error)) => last = error,
                }
            }
        }
        let browsed_other_key = (discovered.addresses.iter()).any(|a| other_key.contains(a));
        if browsed_other_key || discovered.same_name_other_id {
            self.set(|state| state.turnaway = Some(Turnaway::KeyChanged));
            return Err(io::Error::other(RemoteError::KeyChanged.to_string()));
        }
        Err(last)
    }
}

/// Whether `address` always names the same machine: a DNS name, as typed for Tailscale or
/// `.local`, or a Tailscale address (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`). Any other IP
/// address may pass to another machine on a LAN.
fn names_one_machine(address: &str) -> bool {
    let (host, _) = remote_client::split_address(address);
    match host.parse::<IpAddr>() {
        Err(_) => true,
        Ok(IpAddr::V4(ip)) => ip.octets()[0] == 100 && ip.octets()[1] & 0xc0 == 64,
        Ok(IpAddr::V6(ip)) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// How one address failed: the host no longer knows the device, which ends the try; it showed
/// another key than the pinned one; or it did not answer.
enum Tried {
    Unpaired,
    OtherKey,
    Failed(io::Error),
}

/// Relays `socket` to a new socket pair on a thread of its own, and returns the client's end.
fn relay(mut socket: remote_client::RemoteWebSocket) -> io::Result<(UnixStream, Option<String>)> {
    let (ours, theirs) = UnixStream::pair()?;
    socket.set_config(|config| {
        config.max_message_size = Some(MAX_MESSAGE);
        config.max_frame_size = Some(MAX_MESSAGE);
    });
    socket.get_mut().sock.set_nonblocking(true)?;
    theirs.set_nonblocking(true)?;
    std::thread::Builder::new()
        .name("remote-relay".into())
        .spawn(move || {
            let mut relay = Relay {
                socket,
                local: theirs,
                to_local: vec![],
                from_local: vec![],
                sending: false,
            };
            relay.run();
            let _ = relay.local.shutdown(Shutdown::Both);
            let _ = relay.socket.close(None);
            let _ = relay.socket.flush();
        })?;
    Ok((ours, None))
}

struct Relay {
    socket: remote_client::RemoteWebSocket,
    local: UnixStream,
    /// Lines from the host not yet written to the client.
    to_local: Vec<u8>,
    /// What the client wrote after its last full line.
    from_local: Vec<u8>,
    /// Frames to the host wait for the network.
    sending: bool,
}
impl Relay {
    /// Moves messages both ways until either side closes.
    fn run(&mut self) {
        while self.read_host().is_ok() && self.write_client().is_ok() && self.read_client().is_ok()
        {
            self.wait();
        }
    }
    /// Reads every message the host has sent so far.
    fn read_host(&mut self) -> io::Result<()> {
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    self.to_local.extend_from_slice(text.as_bytes());
                    self.to_local.push(b'\n');
                }
                Ok(Message::Close(_)) => return Err(ErrorKind::ConnectionAborted.into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(error) => return Err(io::Error::other(error)),
            }
        }
    }
    fn write_client(&mut self) -> io::Result<()> {
        while !self.to_local.is_empty() {
            match self.local.write(&self.to_local) {
                Ok(0) => return Err(ErrorKind::WriteZero.into()),
                Ok(written) => drop(self.to_local.drain(..written)),
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    /// Sends each full line the client has written as a text frame.
    fn read_client(&mut self) -> io::Result<()> {
        let mut buffer = [0u8; 64 * 1024];
        loop {
            match self.local.read(&mut buffer) {
                Ok(0) => return Err(ErrorKind::UnexpectedEof.into()),
                Ok(read) => self.from_local.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        while let Some(end) = self.from_local.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.from_local.drain(..=end).collect();
            let text = String::from_utf8_lossy(&line[..end]).into_owned();
            match self.socket.write(Message::text(text)) {
                Ok(()) => {}
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        self.sending = match self.socket.flush() {
            Ok(()) => false,
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => true,
            Err(error) => return Err(io::Error::other(error)),
        };
        Ok(())
    }
    /// Sleeps until either side can be read, or written where something waits for it.
    fn wait(&self) {
        let events = |out: bool| libc::POLLIN | if out { libc::POLLOUT } else { 0 };
        let mut fds = [
            libc::pollfd {
                fd: self.socket.get_ref().sock.as_raw_fd(),
                events: events(self.sending),
                revents: 0,
            },
            libc::pollfd {
                fd: self.local.as_raw_fd(),
                events: events(!self.to_local.is_empty()),
                revents: 0,
            },
        ];
        // TLS may hold a decrypted message the socket no longer signals, so wake at least once
        // a second.
        // SAFETY: `fds` is a valid array of two pollfd for the call.
        unsafe { libc::poll(fds.as_mut_ptr(), 2, 1000) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_tailscale_addresses_name_one_machine_and_lan_addresses_do_not() {
        for one in [
            "studio.tail4c2e.ts.net",
            "studio.local:9000",
            "100.64.0.1",
            "100.127.255.254:47821",
            "[fd7a:115c:a1e0::1]:47821",
        ] {
            assert!(names_one_machine(one), "{one}");
        }
        for any in [
            "192.168.1.20",
            "100.128.0.1",
            "10.0.0.5:47821",
            "[fe80::1]:47821",
        ] {
            assert!(!names_one_machine(any), "{any}");
        }
    }
}
