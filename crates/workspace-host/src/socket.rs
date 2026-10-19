//! The host's client socket, `<data folder>/host.sock`: JSON lines, a `Hello` first on every
//! connection, then requests answered in any order and, after `Subscribe`, events.
use crate::service::Service;
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
};
use workspace_core::*;

pub const SOCKET_FILE: &str = "host.sock";
const MAX_CONNECTIONS: usize = 64;
/// Requests one connection may have waiting for a reply.
const MAX_IN_FLIGHT: usize = 64;
const CAPABILITIES: [&str; 3] = [capability::EVENTS, capability::FILES, capability::FOLDERS];

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

/// Accepts connections until dropped, then removes the socket.
pub struct Listener {
    path: PathBuf,
    stopping: Arc<AtomicBool>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.path);
        let _ = fs::remove_file(&self.path);
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
            let (service, options, count) = (service.clone(), options.clone(), connections.clone());
            std::thread::spawn(move || {
                if let Err(error) =
                    Connection::new(service, options, stream).and_then(Connection::run)
                {
                    eprintln!("Host connection: {error:#}");
                }
                count.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok(Listener { path, stopping })
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

/// Where a connection is in its handshake.
enum Stage {
    AwaitingHello,
    /// No protocol in common: only `RestartHost`.
    Refused,
    Welcomed {
        protocol: u32,
        human: bool,
        subscribed: bool,
    },
}

struct Connection {
    service: Service,
    options: Arc<Options>,
    reader: BufReader<UnixStream>,
    writer: Arc<Mutex<UnixStream>>,
    in_flight: Arc<AtomicUsize>,
    stage: Stage,
    /// The subscription's queue, closed when the connection ends.
    events: Option<async_channel::Receiver<Event>>,
}
impl Connection {
    fn new(service: Service, options: Arc<Options>, stream: UnixStream) -> Result<Self> {
        Ok(Self {
            service,
            options,
            reader: BufReader::new(stream.try_clone()?),
            writer: Arc::new(Mutex::new(stream)),
            in_flight: Arc::default(),
            stage: Stage::AwaitingHello,
            events: None,
        })
    }
    fn run(mut self) -> Result<()> {
        let result = self.serve();
        if let Some(events) = self.events.take() {
            events.close();
        }
        result
    }
    fn serve(&mut self) -> Result<()> {
        while let Some(line) = self.read_line()? {
            match self.stage {
                Stage::AwaitingHello => self.hello(&line)?,
                _ => self.request(&line)?,
            }
        }
        Ok(())
    }
    fn read_line(&mut self) -> Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let count = (&mut self.reader)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(None);
        }
        ensure!(
            line.len() <= MAX_LINE_BYTES,
            "A line exceeds the 256 KiB bound"
        );
        Ok(Some(line))
    }
    fn hello(&mut self, line: &[u8]) -> Result<()> {
        let Ok(HelloLine { hello }) = serde_json::from_slice::<HelloLine>(line) else {
            return self.refuse(line, PROTOCOL_VERSION, "Send Hello first");
        };
        let answer = match choose_protocol(PROTOCOLS, hello.protocols) {
            Some(protocol) => {
                let human = hello
                    .credential
                    .as_deref()
                    .is_some_and(|given| same_secret(given, &self.options.credential));
                self.stage = Stage::Welcomed {
                    protocol,
                    human,
                    subscribed: false,
                };
                HelloAnswer::Welcome(Welcome {
                    protocol,
                    host_id: self.service.host_id().into(),
                    host_version: host_version().into(),
                    epoch: self.service.epoch(),
                    capabilities: CAPABILITIES.map(String::from).to_vec(),
                    binary: self.options.binary.clone(),
                })
            }
            None => {
                self.stage = Stage::Refused;
                HelloAnswer::Refused(Refusal {
                    host_protocols: PROTOCOLS,
                    client_protocols: hello.protocols,
                    host_version: host_version().into(),
                    working: self.service.working(),
                })
            }
        };
        self.write(&answer)
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
        let (protocol, human) = match self.stage {
            Stage::Welcomed {
                protocol, human, ..
            } => (protocol, human),
            Stage::Refused if matches!(command, Command::RestartHost { .. }) => (version, false),
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
        if command.is_human_only() && !human {
            return self.reply(version, id, Err(
                "Only the Wiffletree app can give this command: its client credential is missing".into(),
            ));
        }
        if let Command::Subscribe { after } = &command {
            return self.subscribe(version, id, after.as_ref());
        }
        if self.in_flight.fetch_add(1, Ordering::Relaxed) >= MAX_IN_FLIGHT {
            self.in_flight.fetch_sub(1, Ordering::Relaxed);
            return self.reply(
                version,
                id,
                Err("Too many requests are waiting for a reply".into()),
            );
        }
        let receive = match self.service.request(command) {
            Ok(receive) => receive,
            Err(error) => {
                self.in_flight.fetch_sub(1, Ordering::Relaxed);
                return self.reply(version, id, Err(error));
            }
        };
        let (writer, in_flight) = (self.writer.clone(), self.in_flight.clone());
        std::thread::spawn(move || {
            let result = receive
                .recv_blocking()
                .unwrap_or_else(|_| Err("Workspace stopped".into()));
            let _ = write_line(&writer, &response(version, id, result));
            in_flight.fetch_sub(1, Ordering::Relaxed);
        });
        Ok(())
    }
    fn subscribe(&mut self, version: u32, id: String, after: Option<&Cursor>) -> Result<()> {
        let Stage::Welcomed { subscribed, .. } = &mut self.stage else {
            unreachable!("only a welcomed connection subscribes")
        };
        if std::mem::replace(subscribed, true) {
            return self.reply(
                version,
                id,
                Err("This connection is already subscribed".into()),
            );
        }
        let subscription = self.service.subscribe(after);
        self.events = Some(subscription.events.clone());
        let cursor = subscription.cursor.clone();
        self.reply(
            version,
            id,
            Ok(serde_json::to_value(Subscribed { cursor })?),
        )?;
        let writer = self.writer.clone();
        std::thread::spawn(move || {
            let live = std::iter::from_fn(|| subscription.events.recv_blocking().ok());
            for event in subscription.missed.into_iter().chain(live) {
                if write_line(&writer, &EventLine { event }).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }
    fn protocol(&self) -> u32 {
        match self.stage {
            Stage::Welcomed { protocol, .. } => protocol,
            _ => PROTOCOL_VERSION,
        }
    }
    /// Answers a line that is not a command it can run, with its id when it has one.
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
        write_line(&self.writer, &response(version, id, result))
    }
    fn write(&self, value: &impl serde::Serialize) -> Result<()> {
        write_line(&self.writer, value)
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
    }
}

/// Writes one whole line, so replies and events never interleave.
fn write_line(writer: &Mutex<UnixStream>, value: &impl serde::Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let mut stream = writer.lock().unwrap_or_else(|e| e.into_inner());
    stream.write_all(&line)?;
    Ok(stream.flush()?)
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
    option_env!("WIFFLETREE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
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
