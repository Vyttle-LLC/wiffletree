//! The host's client socket, `<data folder>/host.sock`: JSON lines, a `Hello` first on every
//! connection, then requests answered in any order and, after `Subscribe`, events. `Session` is
//! that protocol over any transport; remote devices speak it over a WebSocket.
use crate::service::{Requests, Service};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use workspace_core::*;

const MAX_CONNECTIONS: usize = 64;
/// Requests one connection may have waiting for a reply.
const MAX_IN_FLIGHT: usize = 64;
const CAPABILITIES: [&str; 7] = [
    capability::EVENTS,
    capability::FILES,
    capability::FOLDERS,
    capability::RESTART_WHEN_IDLE,
    capability::UPLOADS,
    capability::ATTENTION,
    capability::RESUBSCRIBE,
];
/// How long a stopping listener waits for the replies still being written, such as the answer
/// to the `StopAgentsAndQuit` that stopped the host.
const REPLY_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

pub struct Options {
    /// The app's client credential, which the human-only commands need.
    pub credential: String,
    /// The only user whose connections are served.
    pub owner: libc::uid_t,
    /// What `Welcome` names as the host's binary.
    pub binary: PathBuf,
}

/// `<data>/host.sock`, refused when longer than the platform's socket path limit.
pub fn socket_path(data: &Path) -> Result<PathBuf> {
    let path = data.join(SOCKET_FILE);
    // SAFETY: an all-zero `sockaddr_un` is valid; only its field's size is read.
    let limit = unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
        .sun_path
        .len();
    let length = path.as_os_str().len();
    ensure!(
        length < limit,
        "The socket path {} is {length} bytes, longer than the {} this system allows; choose a data folder with a shorter path",
        path.display(),
        limit - 1
    );
    Ok(path)
}

/// Accepts connections until dropped, then removes the socket. Dropping it waits a moment for
/// replies still being written, which a stopped host answers at once.
pub struct Listener {
    path: PathBuf,
    stopping: Arc<AtomicBool>,
    replying: Arc<AtomicUsize>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.path);
        let _ = fs::remove_file(&self.path);
        let deadline = std::time::Instant::now() + REPLY_WAIT;
        while self.replying.load(Ordering::Relaxed) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// Binds `host.sock` at mode 0600 in `data`, which is kept at 0700. The store lock is already
/// held, so a socket file left there is a previous host's.
pub fn listen(service: Service, data: &Path, options: Options) -> Result<Listener> {
    let path = socket_path(data)?;
    fs::set_permissions(data, fs::Permissions::from_mode(0o700))?;
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path).with_context(|| format!("Bind {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = stopping.clone();
    let replying = Arc::new(AtomicUsize::new(0));
    let all_replies = replying.clone();
    let options = Arc::new(options);
    std::thread::spawn(move || {
        let connections = Arc::new(AtomicUsize::new(0));
        for stream in listener.incoming() {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if peer_user(&stream).ok() != Some(options.owner) {
                continue;
            }
            if connections.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                connections.fetch_sub(1, Ordering::Relaxed);
                continue;
            }
            let (requests, options, count) =
                (service.requests(), options.clone(), connections.clone());
            let replying = all_replies.clone();
            std::thread::spawn(move || {
                if let Err(error) = serve_connection(requests, &options, replying, stream) {
                    eprintln!("Host connection: {error:#}");
                }
                count.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok(Listener {
        path,
        stopping,
        replying,
    })
}

#[cfg(not(target_os = "linux"))]
fn peer_user(stream: &UnixStream) -> std::io::Result<libc::uid_t> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: the descriptor is a live socket and both pointers are valid for writes.
    match unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } {
        0 => Ok(uid),
        _ => Err(std::io::Error::last_os_error()),
    }
}
#[cfg(target_os = "linux")]
fn peer_user(stream: &UnixStream) -> std::io::Result<libc::uid_t> {
    // SAFETY: an all-zero `ucred` is valid, and the length matches it.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    match status {
        0 => Ok(credentials.uid),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Where a session is in its handshake.
enum Stage {
    AwaitingHello,
    /// No protocol in common: only `RestartHost`, and only on the host's own socket.
    Refused,
    Welcomed {
        protocol: u32,
        human: bool,
        subscribed: bool,
    },
}

/// Who is on the other end of a session.
pub(crate) enum Peer {
    /// A client on the host's own socket, which is the host itself. The human-only commands
    /// need `credential` in its `Hello`.
    Local { credential: String },
    /// A paired device over the network; the host checks its access level for each command.
    Device { id: String },
}

/// Where a session's messages go, each one JSON message: a line on the socket, a text frame on
/// a WebSocket.
pub(crate) trait Outbox: Send + Sync {
    fn send(&self, message: Vec<u8>) -> Result<()>;
}

/// The event kinds a subscription sends; every kind when `None`.
type Kinds = Arc<Mutex<Option<Vec<EventKindName>>>>;

/// The host protocol over any transport: `Hello` first, then requests answered in any order
/// and, after `Subscribe`, events.
pub(crate) struct Session {
    requests: Requests,
    peer: Peer,
    binary: PathBuf,
    outbox: Arc<dyn Outbox>,
    in_flight: Arc<AtomicUsize>,
    /// The listener's count of replies being waited for or written, over every connection.
    replying: Arc<AtomicUsize>,
    stage: Stage,
    /// The subscription's queue, closed when the session ends, and the kinds it sends, which a
    /// later `Subscribe` changes.
    events: Option<(async_channel::Receiver<Event>, Kinds)>,
    /// When the message being handled was read, for the `host_ms` of its reply.
    read_at: Instant,
}
impl Session {
    pub(crate) fn new(
        requests: Requests,
        peer: Peer,
        binary: PathBuf,
        outbox: Arc<dyn Outbox>,
        replying: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            requests,
            peer,
            binary,
            outbox,
            in_flight: Arc::default(),
            replying,
            stage: Stage::AwaitingHello,
            events: None,
            read_at: Instant::now(),
        }
    }
    /// Handles one message the client sent, read at `read_at`.
    pub(crate) fn receive(&mut self, message: &[u8], read_at: Instant) -> Result<()> {
        self.read_at = read_at;
        match self.stage {
            Stage::AwaitingHello => self.hello(message),
            _ => self.request(message),
        }
    }
    fn hello(&mut self, line: &[u8]) -> Result<()> {
        let Ok(HelloLine { hello }) = serde_json::from_slice::<HelloLine>(line) else {
            return self.refuse(line, PROTOCOL_VERSION, "Send Hello first");
        };
        let answer = match choose_protocol(PROTOCOLS, hello.protocols) {
            Some(protocol) => {
                let human = match &self.peer {
                    Peer::Local { credential } => hello
                        .credential
                        .as_deref()
                        .is_some_and(|given| same_secret(given, credential)),
                    Peer::Device { .. } => false,
                };
                self.stage = Stage::Welcomed {
                    protocol,
                    human,
                    subscribed: false,
                };
                HelloAnswer::Welcome(Welcome {
                    protocol,
                    host_id: self.requests.host_id().into(),
                    host_version: host_version().into(),
                    epoch: self.requests.epoch(),
                    capabilities: CAPABILITIES.map(String::from).to_vec(),
                    binary: self.binary.clone(),
                })
            }
            None => {
                self.stage = Stage::Refused;
                HelloAnswer::Refused(Refusal {
                    host_protocols: PROTOCOLS,
                    client_protocols: hello.protocols,
                    host_version: host_version().into(),
                    working: self.requests.working(),
                })
            }
        };
        self.outbox.send(serde_json::to_vec(&answer)?)
    }
    fn request(&mut self, line: &[u8]) -> Result<()> {
        let request = match serde_json::from_slice::<Request>(line) {
            Ok(request) => request,
            Err(error) => {
                let protocol = self.protocol();
                return self.refuse(line, protocol, &format!("Invalid command: {error}"));
            }
        };
        let Request {
            version,
            id,
            command,
        } = request;
        let local = matches!(self.peer, Peer::Local { .. });
        let (protocol, human) = match self.stage {
            Stage::Welcomed {
                protocol, human, ..
            } => (protocol, human),
            Stage::Refused if local && matches!(command, Command::RestartHost { .. }) => {
                (version, false)
            }
            Stage::Refused => {
                return self.reply(version, id, Err(
                    "This host shares no protocol with the client; only RestartHost is accepted".into(),
                ));
            }
            Stage::AwaitingHello => unreachable!("answered by hello"),
        };
        if version != protocol {
            return self.reply(
                version,
                id,
                Err(format!("Use protocol {protocol} on this connection")),
            );
        }
        if let Some(needed) = command.capability()
            && !CAPABILITIES.contains(&needed)
        {
            return self.reply(
                version,
                id,
                Err(format!("This host does not offer {needed}")),
            );
        }
        if local && command.is_human_only() && !human {
            return self.reply(version, id, Err(
                "Only the Wiffletree app can give this command: its client credential is missing".into(),
            ));
        }
        match &command {
            Command::Subscribe { after, kinds } => {
                return self.subscribe(version, id, after.as_ref(), kinds.clone());
            }
            Command::Ping => return self.reply(version, id, Ok(serde_json::Value::Null)),
            _ => {}
        }
        if self.in_flight.fetch_add(1, Ordering::Relaxed) >= MAX_IN_FLIGHT {
            self.in_flight.fetch_sub(1, Ordering::Relaxed);
            return self.reply(
                version,
                id,
                Err("Too many requests are waiting for a reply".into()),
            );
        }
        let sent = match &self.peer {
            Peer::Local { .. } => self.requests.request(command),
            Peer::Device { id } => self.requests.request_from(id, command),
        };
        let receive = match sent {
            Ok(receive) => receive,
            Err(error) => {
                self.in_flight.fetch_sub(1, Ordering::Relaxed);
                return self.reply(version, id, Err(error));
            }
        };
        let (outbox, in_flight) = (self.outbox.clone(), self.in_flight.clone());
        let (replying, read_at) = (self.replying.clone(), self.read_at);
        replying.fetch_add(1, Ordering::Relaxed);
        std::thread::spawn(move || {
            let result = receive
                .recv_blocking()
                .unwrap_or_else(|_| Err("Workspace stopped".into()));
            let mut answer = response(version, id, result);
            answer.host_ms = Some(read_at.elapsed().as_millis() as u64);
            if let Ok(message) = serde_json::to_vec(&answer) {
                let _ = outbox.send(message);
            }
            in_flight.fetch_sub(1, Ordering::Relaxed);
            replying.fetch_sub(1, Ordering::Relaxed);
        });
        Ok(())
    }
    fn subscribe(
        &mut self,
        version: u32,
        id: String,
        after: Option<&Cursor>,
        kinds: Option<Vec<EventKindName>>,
    ) -> Result<()> {
        let Stage::Welcomed { subscribed, .. } = &mut self.stage else {
            unreachable!("only a welcomed session subscribes")
        };
        if std::mem::replace(subscribed, true) {
            return self.subscribe_again(version, id, after, kinds);
        }
        let mut subscription = self.requests.subscribe(after);
        // A subscriber with no count yet, or one told to start over, gets the current one.
        let starting = after.is_none()
            || matches!(
                &subscription.missed[..],
                [Event {
                    kind: EventKind::Resync,
                    ..
                }]
            );
        if let Some(count) = subscription.attention.filter(|_| starting) {
            subscription.missed.push(Event {
                cursor: subscription.cursor.clone(),
                kind: EventKind::Attention { count },
            });
        }
        let wanted = Kinds::new(Mutex::new(kinds));
        self.events = Some((subscription.events.clone(), wanted.clone()));
        let cursor = subscription.cursor.clone();
        self.reply(
            version,
            id,
            Ok(serde_json::to_value(Subscribed { cursor })?),
        )?;
        let outbox = self.outbox.clone();
        let wanted = move |event: &Event| {
            let kinds = wanted.lock().unwrap_or_else(|e| e.into_inner());
            match (kinds.as_ref(), event.kind.name()) {
                (Some(kinds), Some(name)) => kinds.contains(&name),
                _ => true,
            }
        };
        std::thread::spawn(move || {
            let live = std::iter::from_fn(|| subscription.events.recv_blocking().ok());
            for event in subscription.missed.into_iter().chain(live) {
                if !wanted(&event) {
                    continue;
                }
                let Ok(message) = serde_json::to_vec(&EventLine { event }) else {
                    continue;
                };
                if outbox.send(message).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }
    /// Changes a live subscription's kinds, answered with `null`: events go on from where they
    /// were, under the new kinds from the next one, with no new cursor and none missed.
    fn subscribe_again(
        &self,
        version: u32,
        id: String,
        after: Option<&Cursor>,
        kinds: Option<Vec<EventKindName>>,
    ) -> Result<()> {
        let Some((_, wanted)) = &self.events else {
            unreachable!("a subscribed session keeps its kinds")
        };
        if after.is_some() {
            return self.reply(version, id, Err(
                "This connection is already subscribed; send Subscribe without after to change its kinds".into(),
            ));
        }
        *wanted.lock().unwrap_or_else(|e| e.into_inner()) = kinds;
        self.reply(version, id, Ok(serde_json::Value::Null))
    }
    fn protocol(&self) -> u32 {
        match self.stage {
            Stage::Welcomed { protocol, .. } => protocol,
            _ => PROTOCOL_VERSION,
        }
    }
    /// Answers a message that is not a command it can run, with its id when it has one.
    fn refuse(&self, line: &[u8], version: u32, error: &str) -> Result<()> {
        let id = serde_json::from_slice::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v["id"].as_str().map(String::from))
            .unwrap_or_else(|| "invalid".into());
        self.reply(version, id, Err(error.into()))
    }
    fn reply(
        &self,
        version: u32,
        id: String,
        result: std::result::Result<serde_json::Value, String>,
    ) -> Result<()> {
        let mut answer = response(version, id, result);
        answer.host_ms = Some(self.read_at.elapsed().as_millis() as u64);
        self.outbox.send(serde_json::to_vec(&answer)?)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some((events, _)) = self.events.take() {
            events.close();
        }
    }
}

/// A socket connection: JSON lines, each written whole.
struct Lines(Mutex<UnixStream>);
impl Outbox for Lines {
    fn send(&self, mut message: Vec<u8>) -> Result<()> {
        message.push(b'\n');
        let mut stream = self.0.lock().unwrap_or_else(|e| e.into_inner());
        stream.write_all(&message)?;
        Ok(stream.flush()?)
    }
}

/// Serves one connection on the socket until it closes.
fn serve_connection(
    requests: Requests,
    options: &Options,
    replying: Arc<AtomicUsize>,
    stream: UnixStream,
) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let peer = Peer::Local {
        credential: options.credential.clone(),
    };
    let outbox = Arc::new(Lines(Mutex::new(stream)));
    let mut session = Session::new(requests, peer, options.binary.clone(), outbox, replying);
    loop {
        let mut line = Vec::new();
        let count = (&mut reader)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        ensure!(
            line.len() <= MAX_LINE_BYTES,
            "A line exceeds the 256 KiB bound"
        );
        session.receive(&line, Instant::now())?;
    }
}

fn response(
    version: u32,
    id: String,
    result: std::result::Result<serde_json::Value, String>,
) -> Response {
    let (result, error) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    Response {
        version,
        id,
        result,
        error,
        host_ms: None,
    }
}

/// Compares in time that does not depend on where the secrets differ.
fn same_secret(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}

pub fn host_version() -> &'static str {
    build_version()
}

/// Reads the child host's credential, the first line of its standard input, one byte at a time
/// so nothing after it is consumed.
pub fn read_credential(input: &mut impl Read) -> Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8];
    while input.read(&mut byte)? == 1 && byte[0] != b'\n' {
        line.push(byte[0]);
        ensure!(line.len() <= 1024, "The credential line is too long");
    }
    let credential = String::from_utf8(line)?.trim().to_owned();
    if credential.is_empty() {
        bail!("No client credential on standard input");
    }
    Ok(credential)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_from_another_user_is_closed_before_any_request_is_read() {
        let data = tempfile::tempdir().unwrap();
        let service = Service::start(data.path().into(), std::env::current_exe().unwrap()).unwrap();
        // SAFETY: getuid cannot fail.
        let me = unsafe { libc::getuid() };
        let options = Options {
            credential: "secret".into(),
            owner: me + 1,
            binary: PathBuf::new(),
        };
        let listener = listen(service.clone(), data.path(), options).unwrap();
        let path = socket_path(data.path()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let mut stream = UnixStream::connect(&path).unwrap();
        let hello = r#"{"hello":{"protocols":[2,2],"app_version":"t","client":{"name":"t","kind":"script"}}}"#;
        let _ = writeln!(stream, "{hello}");
        let mut answer = String::new();
        let read = stream.read_to_string(&mut answer);
        assert!(read.is_err() || answer.is_empty(), "answered: {answer}");
        drop(listener);
        service.shutdown();
    }

    #[test]
    fn a_socket_path_over_the_limit_is_refused() {
        let long = PathBuf::from("/tmp").join("d".repeat(120));
        let error = socket_path(&long).unwrap_err();
        assert!(error.to_string().contains("longer than"), "{error}");
        assert!(socket_path(Path::new("/tmp/short")).is_ok());
    }

    #[test]
    fn the_credential_is_the_first_line_only() {
        let mut input: &[u8] = b"abc\nrest";
        assert_eq!(read_credential(&mut input).unwrap(), "abc");
        assert_eq!(input, b"rest");
        assert!(read_credential(&mut &b""[..]).is_err());
    }

    #[test]
    fn secrets_must_match_exactly() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abd", "abc"));
        assert!(!same_secret("ab", "abc"));
    }
}
