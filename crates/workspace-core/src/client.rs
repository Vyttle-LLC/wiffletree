//! A client of a host's socket, as `docs/host-protocol.md` describes it: `Hello` with the client
//! credential, requests matched to their replies by id, and one event subscription. When the
//! connection drops, it connects again after a growing delay, resumes its subscription from its
//! last cursor, and sends again each request that never got its reply and is safe to repeat: a
//! read, or a `Send`, which the host stores once by its message id.
//!
//! A host that shares no protocol with the client refuses it. The client keeps trying, since an
//! older host restarts into the new version once asked, and says why it is not connected.
use crate::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, ErrorKind, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// The delay before the first try after a drop, doubled after each failed try up to the last.
const FIRST_RETRY: Duration = Duration::from_millis(500);
const LAST_RETRY: Duration = Duration::from_secs(30);
/// How long the host may take to answer `Hello` and `Subscribe`.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// After this long without a line from the host the client asks it something; if that goes
/// unanswered as long again, the host has stopped answering and the client connects again.
#[cfg(not(test))]
const QUIET: Duration = Duration::from_secs(20);
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(300);
/// A restart now waits for the host's shutdown, which waits for its turns to stop.
const RESTART_TIMEOUT: Duration = Duration::from_secs(60);
const LOST: &str = "The connection to the host closed before it answered";
const DISCONNECTED: &str = "Not connected to the host";

type Reply = Box<dyn FnOnce(Result<Value, String>) + Send>;
/// Called on the client's reading thread with each event the host sends, and with `Resync` when
/// a host refuses the client or one that had refused it answers.
pub type OnEvent = Box<dyn Fn(&EventKind) + Send + Sync>;

struct Pending {
    command: Command,
    reply: Reply,
    /// Written on the current connection.
    written: bool,
    /// Sent again after a drop rather than failed.
    repeatable: bool,
    /// When it was asked, so requests are written in order.
    order: u64,
}

#[derive(Default)]
struct State {
    pending: HashMap<String, Pending>,
    asked: u64,
    protocol: u32,
    capabilities: Vec<String>,
    /// The last event read, to resume from.
    cursor: Option<Cursor>,
    /// The version of the host last connected to.
    host_version: Option<String>,
    /// Why the host last refused the client, until one welcomes it.
    refusal: Option<Refusal>,
}

struct Shared {
    /// This client's `Hello`, whose credential each connection's endpoint fills in.
    hello: Hello,
    launcher: Mutex<Box<dyn Launch>>,
    on_event: OnEvent,
    /// The current connection, while there is one; lock it before `state`.
    writer: Mutex<Option<UnixStream>>,
    state: Mutex<State>,
    stopped: AtomicBool,
}

/// A connection to a host that keeps itself connected until stopped.
#[derive(Clone)]
pub struct HostClient(Arc<Shared>);
impl HostClient {
    /// Connects to the host `launcher` finds or starts, and keeps connected until `stop`. Fails
    /// when the first connection does, unless the host refused it.
    pub fn connect(
        launcher: Box<dyn Launch>,
        client: Client,
        on_event: OnEvent,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            hello: Hello {
                protocols: [PROTOCOL_VERSION, PROTOCOL_VERSION],
                app_version: build_version().into(),
                client,
                capabilities: vec![],
                credential: None,
            },
            launcher: Mutex::new(launcher),
            on_event,
            writer: Mutex::default(),
            state: Mutex::default(),
            stopped: AtomicBool::new(false),
        });
        let reader = match shared.open() {
            Ok(reader) => Some(reader),
            Err(_) if shared.state().refusal.is_some() => None,
            Err(error) => return Err(error),
        };
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("host-client".into())
            .spawn(move || worker.run(reader))?;
        Ok(Self(shared))
    }
    /// Sends `command`; its reply arrives on the receiver. While disconnected it waits to be
    /// written once the client connects again.
    pub fn request(
        &self,
        command: Command,
    ) -> Result<async_channel::Receiver<Result<Value, String>>, String> {
        let (sender, receiver) = async_channel::bounded(1);
        self.0.submit(
            command,
            Box::new(move |result| {
                let _ = sender.try_send(result);
            }),
        )?;
        Ok(receiver)
    }
    /// Sends `command` and waits at most `timeout` for its reply; while disconnected it fails at
    /// once.
    pub fn request_blocking(&self, command: Command, timeout: Duration) -> Result<Value, String> {
        if !self.connected() {
            return Err(DISCONNECTED.into());
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.0.submit(
            command,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        )?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| "The host did not answer in time".to_string())?
    }
    fn connected(&self) -> bool {
        self.0.writer().is_some()
    }
    /// Why the host refuses this client, while it does.
    pub fn refusal(&self) -> Option<Refusal> {
        self.0.state().refusal.clone()
    }
    /// The version of the host last connected to.
    pub fn host_version(&self) -> Option<String> {
        self.0.state().host_version.clone()
    }
    /// Asks the host to restart, waiting for its answer, which `Now` gives once its turns have
    /// stopped. A host that refused the client is asked on a connection of its own, as the
    /// handshake allows after a refusal.
    pub fn restart_host(&self, when: RestartWhen) -> Result<(), String> {
        let command = Command::RestartHost { when };
        if self.refusal().is_none() {
            return self.request_blocking(command, RESTART_TIMEOUT).map(|_| ());
        }
        self.0.restart_refused(command).map_err(|e| e.to_string())
    }
    /// Closes the connection for good, and stops the host if it is the client's own child.
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Relaxed);
        if let Some(stream) = self.0.writer().take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.0.launcher().stop();
    }
    /// Runs `with` on the launcher, as tests do to reach a child host.
    pub fn with_launcher<R>(&self, with: impl FnOnce(&mut dyn Launch) -> R) -> R {
        with(self.0.launcher().as_mut())
    }
}

impl Shared {
    fn writer(&self) -> MutexGuard<'_, Option<UnixStream>> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn launcher(&self) -> MutexGuard<'_, Box<dyn Launch>> {
        self.launcher.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Connects, says `Hello`, subscribes from the last cursor and writes what waits. A client
    /// stopped meanwhile gives up instead, so it never keeps a host it started.
    fn open(&self) -> io::Result<BufReader<UnixStream>> {
        let endpoint = self.launcher().endpoint(&self.stopped)?;
        if self.stopped.load(Ordering::Relaxed) {
            return Err(io::Error::other(DISCONNECTED));
        }
        let mut stream = UnixStream::connect(&endpoint.socket)?;
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let hello = Hello {
            credential: endpoint.credential,
            ..self.hello.clone()
        };
        write_line(&mut stream, &HelloLine { hello })?;
        let welcome = match read_line::<HelloAnswer>(&mut reader)? {
            HelloAnswer::Welcome(welcome) => welcome,
            HelloAnswer::Refused(refusal) => {
                let message = refused(&refusal);
                self.state().refusal = Some(refusal);
                (self.on_event)(&EventKind::Resync);
                return Err(io::Error::other(message));
            }
        };
        let after = self.state().cursor.clone();
        let subscribe = Request {
            version: welcome.protocol,
            id: new_id(),
            command: Command::Subscribe { after, kinds: None },
        };
        write_line(&mut stream, &subscribe)?;
        // Nothing else is written yet, so the next line answers it.
        let answer: Response = read_line(&mut reader)?;
        let subscribed: Subscribed = match answer {
            Response {
                result: Some(result),
                error: None,
                ..
            } => serde_json::from_value(result)?,
            Response { error, .. } => return Err(io::Error::other(error.unwrap_or_default())),
        };
        reader.get_ref().set_read_timeout(None)?;
        let mut writer = self.writer();
        // `stop` sets the flag before it takes the writer, so it either finds this connection
        // or this sees the flag.
        if self.stopped.load(Ordering::Relaxed) {
            return Err(io::Error::other(DISCONNECTED));
        }
        let (was_refused, restart_when_idle) = {
            let mut state = self.state();
            state.protocol = welcome.protocol;
            state.cursor.get_or_insert(subscribed.cursor);
            let restart_when_idle = welcome.host_version != self.hello.app_version
                && (welcome.capabilities.iter()).any(|c| c == capability::RESTART_WHEN_IDLE);
            state.capabilities = welcome.capabilities;
            state.host_version = Some(welcome.host_version);
            (state.refusal.take().is_some(), restart_when_idle)
        };
        *writer = Some(stream);
        self.flush(&mut writer);
        drop(writer);
        if was_refused {
            (self.on_event)(&EventKind::Resync);
        }
        // An update left an older host running: it restarts into this version once no agent
        // works. A child host is the app's own binary, so only a launchd host can differ.
        if restart_when_idle && self.launcher().outlives_app() {
            let restart = Command::RestartHost {
                when: RestartWhen::Idle,
            };
            if let Err(error) = self.submit(restart, Box::new(|_| {})) {
                eprintln!("Ask the older host to restart: {error}");
            }
        }
        Ok(reader)
    }
    /// Sends `command`, a `RestartHost`, on a connection of its own to a host that refuses the
    /// client, and waits for the answer.
    fn restart_refused(&self, command: Command) -> io::Result<()> {
        let endpoint = self.launcher().endpoint(&self.stopped)?;
        let mut stream = UnixStream::connect(&endpoint.socket)?;
        stream.set_read_timeout(Some(RESTART_TIMEOUT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let hello = Hello {
            credential: endpoint.credential,
            ..self.hello.clone()
        };
        write_line(&mut stream, &HelloLine { hello })?;
        let version = match read_line::<HelloAnswer>(&mut reader)? {
            HelloAnswer::Welcome(welcome) => welcome.protocol,
            HelloAnswer::Refused(_) => PROTOCOL_VERSION,
        };
        let request = Request {
            version,
            id: new_id(),
            command,
        };
        write_line(&mut stream, &request)?;
        match read_line::<Response>(&mut reader)?.error {
            Some(error) => Err(io::Error::other(error)),
            None => Ok(()),
        }
    }

    /// Reads until the connection drops, then connects again until stopped. With no reader, the
    /// host refused the first connection.
    fn run(&self, mut reader: Option<BufReader<UnixStream>>) {
        loop {
            if let Some(reader) = reader.as_mut() {
                self.read(reader);
                self.disconnected();
            }
            let mut delay = FIRST_RETRY;
            reader = loop {
                if !self.wait(delay) {
                    return;
                }
                match self.open() {
                    Ok(reader) => break Some(reader),
                    Err(error) => eprintln!("Connect to the host: {error}"),
                }
                delay = (delay * 2).min(LAST_RETRY);
            };
        }
    }
    /// Sleeps `delay`, or less once stopped; whether still running.
    fn wait(&self, delay: Duration) -> bool {
        let step = Duration::from_millis(50);
        let mut waited = Duration::ZERO;
        while waited < delay && !self.stopped.load(Ordering::Relaxed) {
            std::thread::sleep(step);
            waited += step;
        }
        !self.stopped.load(Ordering::Relaxed)
    }
    /// Reads until the connection drops or the host stops answering: quiet for `QUIET`, then
    /// for `QUIET` again after a probe.
    fn read(&self, reader: &mut BufReader<UnixStream>) {
        if reader.get_ref().set_read_timeout(Some(QUIET)).is_err() {
            return;
        }
        let mut line = vec![];
        let mut probed_at = None;
        loop {
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => return,
                Ok(_) => {
                    if let Err(error) = self.dispatch(&line) {
                        eprintln!("Host line: {error}");
                    }
                    line.clear();
                    probed_at = None;
                }
                // A timed-out read keeps what it read so far in `line`.
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    match probed_at {
                        Some(read) if read == line.len() => {
                            eprintln!("The host stopped answering; connecting again");
                            return;
                        }
                        _ => {
                            probed_at = Some(line.len());
                            self.probe();
                        }
                    }
                }
                Err(_) => return,
            }
        }
    }
    /// Pings the host's connection, which answers without waiting for the host's actor, so a
    /// host that is quiet or busy answers. A host older than `Ping` answers it with an error,
    /// which is an answer all the same.
    fn probe(&self) {
        let _ = self.submit_as(Command::Ping, Box::new(|_| {}), false);
    }
    fn dispatch(&self, line: &[u8]) -> serde_json::Result<()> {
        let value: Value = serde_json::from_slice(line)?;
        if value.get("event").is_some() {
            let EventLine { event } = serde_json::from_value(value)?;
            self.state().cursor = Some(event.cursor);
            (self.on_event)(&event.kind);
            return Ok(());
        }
        let response: Response = serde_json::from_value(value)?;
        let pending = self.state().pending.remove(&response.id);
        if let Some(pending) = pending {
            (pending.reply)(match response.error {
                Some(error) => Err(error),
                None => Ok(response.result.unwrap_or_default()),
            });
        }
        Ok(())
    }
    /// Forgets the connection: what may be repeated waits for the next, the rest fails.
    fn disconnected(&self) {
        let mut writer = self.writer();
        *writer = None;
        let lost: Vec<_> = {
            let mut state = self.state();
            let ids: Vec<_> = (state.pending.iter())
                .filter(|(_, pending)| pending.written && !pending.repeatable)
                .map(|(id, _)| id.clone())
                .collect();
            for pending in state.pending.values_mut() {
                pending.written = false;
            }
            ids.iter()
                .filter_map(|id| state.pending.remove(id))
                .collect()
        };
        drop(writer);
        for pending in lost {
            (pending.reply)(Err(LOST.into()));
        }
    }

    fn submit(&self, command: Command, reply: Reply) -> Result<(), String> {
        let repeatable = command.is_read() || matches!(command, Command::Send { .. });
        self.submit_as(command, reply, repeatable)
    }
    fn submit_as(&self, command: Command, reply: Reply, repeatable: bool) -> Result<(), String> {
        if self.stopped.load(Ordering::Relaxed) {
            return Err(DISCONNECTED.into());
        }
        {
            let mut state = self.state();
            if let Some(capability) = command.capability()
                && !state.capabilities.iter().any(|c| c == capability)
            {
                return Err(format!("This host does not offer {capability}"));
            }
            // The host closes the connection on a longer line, so it would never be answered.
            let request = Request {
                version: state.protocol,
                id: new_id(),
                command,
            };
            if serde_json::to_vec(&request)
                .map_err(|e| e.to_string())?
                .len()
                >= MAX_LINE_BYTES
            {
                return Err(format!(
                    "This is too large to send: the host takes at most {} KiB at once",
                    MAX_LINE_BYTES / 1024
                ));
            }
            state.asked += 1;
            let Request { id, command, .. } = request;
            let pending = Pending {
                repeatable,
                command,
                reply,
                written: false,
                order: state.asked,
            };
            state.pending.insert(id, pending);
        }
        self.flush(&mut self.writer());
        Ok(())
    }
    /// Writes each request not yet written on the connection, in the order they were asked. A
    /// failed write leaves the rest to the next connection, once the reader sees the drop.
    fn flush(&self, writer: &mut Option<UnixStream>) {
        let Some(stream) = writer.as_mut() else {
            return;
        };
        let requests = {
            let mut state = self.state();
            let version = state.protocol;
            let mut waiting: Vec<_> = (state.pending.iter_mut())
                .filter(|(_, pending)| !pending.written)
                .collect();
            waiting.sort_by_key(|(_, pending)| pending.order);
            let requests: Vec<_> = (waiting.into_iter())
                .map(|(id, pending)| {
                    pending.written = true;
                    Request {
                        version,
                        id: id.clone(),
                        command: pending.command.clone(),
                    }
                })
                .collect();
            requests
        };
        for request in requests {
            if write_line(stream, &request).is_err() {
                break;
            }
        }
    }
}

/// Why no protocol is in common, and which side to update.
fn refused(refusal: &Refusal) -> String {
    let [host_oldest, host_newest] = refusal.host_protocols;
    let [app_oldest, app_newest] = refusal.client_protocols;
    let update = if app_newest < host_oldest {
        "Update Wiffletree"
    } else {
        "Restart the host so it runs this version"
    };
    format!(
        "The host {} speaks protocols {host_oldest}–{host_newest} and this app {app_oldest}–{app_newest}. {update}.",
        refusal.host_version
    )
}

fn write_line(stream: &mut UnixStream, value: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    stream.write_all(&line)
}

fn read_line<T: DeserializeOwned>(reader: &mut BufReader<UnixStream>) -> io::Result<T> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "The host closed the connection",
        ));
    }
    Ok(serde_json::from_str(&line)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::net::UnixListener, path::PathBuf, sync::atomic::AtomicUsize, time::Instant,
    };

    /// A host that completes the handshake on each connection, then reads every request and
    /// answers none, as a hung host does.
    struct Silent {
        socket: PathBuf,
        _folder: tempfile::TempDir,
        /// Every request line read, on any connection.
        requests: Arc<Mutex<Vec<Value>>>,
    }
    impl Silent {
        fn start() -> Self {
            let folder = tempfile::tempdir().unwrap();
            let socket = folder.path().join(SOCKET_FILE);
            let listener = UnixListener::bind(&socket).unwrap();
            let requests = Arc::new(Mutex::new(vec![]));
            let read = requests.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let (mut stream, read) = (stream.unwrap(), read.clone());
                    std::thread::spawn(move || {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let _: HelloLine = read_line(&mut reader).unwrap();
                        let welcome = HelloAnswer::Welcome(Welcome {
                            protocol: PROTOCOL_VERSION,
                            host_id: "h".into(),
                            host_version: "t".into(),
                            epoch: "e".into(),
                            capabilities: vec![capability::EVENTS.into()],
                            binary: PathBuf::new(),
                        });
                        write_line(&mut stream, &welcome).unwrap();
                        let subscribe: Request = read_line(&mut reader).unwrap();
                        let cursor = Cursor {
                            epoch: "e".into(),
                            seq: 0,
                        };
                        let subscribed = serde_json::to_value(Subscribed { cursor }).unwrap();
                        let answer = Response {
                            version: PROTOCOL_VERSION,
                            id: subscribe.id,
                            result: Some(subscribed),
                            error: None,
                            host_ms: None,
                        };
                        write_line(&mut stream, &answer).unwrap();
                        while let Ok(request) = read_line::<Value>(&mut reader) {
                            read.lock().unwrap().push(request);
                        }
                    });
                }
            });
            Self {
                socket,
                _folder: folder,
                requests,
            }
        }
        /// A client of it, and how many times the client has asked for the host.
        fn client(&self) -> (HostClient, Arc<AtomicUsize>) {
            struct Counted(Endpoint, Arc<AtomicUsize>);
            impl Launch for Counted {
                fn endpoint(&mut self, _: &AtomicBool) -> io::Result<Endpoint> {
                    self.1.fetch_add(1, Ordering::Relaxed);
                    Ok(self.0.clone())
                }
            }
            let endpoint = Endpoint {
                socket: self.socket.clone(),
                credential: None,
            };
            let calls = Arc::new(AtomicUsize::new(0));
            let client = Client {
                name: "test".into(),
                kind: "script".into(),
            };
            let launch = Box::new(Counted(endpoint, calls.clone()));
            let client = HostClient::connect(launch, client, Box::new(|_| {})).unwrap();
            (client, calls)
        }
        fn commands(&self) -> Vec<String> {
            let requests = self.requests.lock().unwrap();
            (requests.iter())
                .map(|r| r["command"]["type"].as_str().unwrap_or_default().to_owned())
                .collect()
        }
    }

    #[test]
    fn a_host_that_stops_answering_is_probed_then_connected_to_again() {
        let host = Silent::start();
        let (client, calls) = host.client();
        let snapshot = client.request(Command::Snapshot).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while calls.load(Ordering::Relaxed) < 2 {
            assert!(Instant::now() < deadline, "never connected again");
            std::thread::sleep(Duration::from_millis(20));
        }
        // A probe went unanswered first, and the snapshot is asked again on the new connection.
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.commands().iter().filter(|c| *c == "snapshot").count() < 2 {
            assert!(Instant::now() < deadline, "{:?}", host.commands());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(host.commands().contains(&"ping".to_string()));
        assert!(snapshot.try_recv().is_err());
        client.stop();
    }

    #[test]
    fn a_request_over_the_hosts_line_bound_fails_at_once_and_is_never_sent() {
        let host = Silent::start();
        let (client, _) = host.client();
        let send = Command::Send {
            id: new_id(),
            sender: None,
            recipient: "coordinator".into(),
            body: "x".repeat(300 * 1024),
            attachments: vec![],
            uploads: vec![],
        };
        let error = client.request(send).unwrap_err();
        assert!(error.contains("too large to send"), "{error}");
        let small = Command::Send {
            id: new_id(),
            sender: None,
            recipient: "coordinator".into(),
            body: "Hello".into(),
            attachments: vec![],
            uploads: vec![],
        };
        let _reply = client.request(small).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.commands().is_empty() {
            assert!(Instant::now() < deadline, "nothing written");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(host.commands()[0], "send");
        assert_eq!(host.requests.lock().unwrap()[0]["command"]["body"], "Hello");
        client.stop();
    }

    /// A host on a socket in a temporary folder that answers every `Hello` with `answer`, every
    /// `Subscribe` with a cursor and every other request with nothing, and records the requests.
    struct FakeHost {
        folder: tempfile::TempDir,
        requests: async_channel::Receiver<Command>,
    }
    impl FakeHost {
        fn start(answer: HelloAnswer) -> Self {
            let folder = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
            let listener = UnixListener::bind(folder.path().join(SOCKET_FILE)).unwrap();
            let (record, requests) = async_channel::unbounded();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let (Ok(mut stream), answer, record) = (stream, answer.clone(), record.clone())
                    else {
                        continue;
                    };
                    std::thread::spawn(move || {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let _: HelloLine = read_line(&mut reader).unwrap();
                        write_line(&mut stream, &answer).unwrap();
                        while let Ok(request) = read_line::<Request>(&mut reader) {
                            let result = match request.command {
                                Command::Subscribe { .. } => serde_json::json!({
                                    "cursor": {"epoch": "e", "seq": 0}
                                }),
                                _ => Value::Null,
                            };
                            let _ = record.send_blocking(request.command);
                            let response = Response {
                                version: request.version,
                                id: request.id,
                                result: Some(result),
                                error: None,
                                host_ms: None,
                            };
                            write_line(&mut stream, &response).unwrap();
                        }
                    });
                }
            });
            Self { folder, requests }
        }
        /// The next request other than `Subscribe` and the client's probes, waiting up to
        /// `wait` for it.
        fn next_request(&self, wait: Duration) -> Option<Command> {
            let deadline = std::time::Instant::now() + wait;
            while std::time::Instant::now() < deadline {
                match self.requests.try_recv() {
                    Ok(Command::Subscribe { .. } | Command::Ping) => {}
                    Ok(command) => return Some(command),
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            None
        }
    }
    struct Found {
        socket: std::path::PathBuf,
        outlives_app: bool,
    }
    impl Launch for Found {
        fn endpoint(&mut self, _: &AtomicBool) -> io::Result<Endpoint> {
            Ok(Endpoint {
                socket: self.socket.clone(),
                credential: None,
            })
        }
        fn outlives_app(&self) -> bool {
            self.outlives_app
        }
    }
    fn connect(host: &FakeHost, outlives_app: bool, on_event: OnEvent) -> HostClient {
        let launcher = Found {
            socket: host.folder.path().join(SOCKET_FILE),
            outlives_app,
        };
        let client = Client {
            name: "test".into(),
            kind: "script".into(),
        };
        HostClient::connect(Box::new(launcher), client, on_event).unwrap()
    }
    fn welcome(host_version: &str) -> HelloAnswer {
        HelloAnswer::Welcome(Welcome {
            protocol: PROTOCOL_VERSION,
            host_id: "h".into(),
            host_version: host_version.into(),
            epoch: "e".into(),
            capabilities: vec![
                capability::EVENTS.into(),
                capability::RESTART_WHEN_IDLE.into(),
            ],
            binary: "/host".into(),
        })
    }

    #[test]
    fn a_refusing_host_is_reported_and_restarted_on_a_connection_of_its_own() {
        let host = FakeHost::start(HelloAnswer::Refused(Refusal {
            host_protocols: [PROTOCOL_VERSION + 1, PROTOCOL_VERSION + 2],
            client_protocols: [PROTOCOL_VERSION, PROTOCOL_VERSION],
            host_version: "9.0.0".into(),
            working: 2,
        }));
        let (woken, wakes) = async_channel::unbounded();
        let on_event: OnEvent = Box::new(move |kind| {
            let _ = woken.send_blocking(kind.clone());
        });

        let client = connect(&host, true, on_event);

        assert_eq!(client.refusal().map(|r| r.working), Some(2));
        assert_eq!(wakes.try_recv().ok(), Some(EventKind::Resync));
        client.restart_host(RestartWhen::Idle).unwrap();
        assert!(matches!(
            host.next_request(Duration::from_secs(5)),
            Some(Command::RestartHost {
                when: RestartWhen::Idle
            })
        ));
        client.stop();
    }

    #[test]
    fn an_older_launchd_host_is_asked_to_restart_once_idle() {
        let older = FakeHost::start(welcome("0.0.1-older"));
        let client = connect(&older, true, Box::new(|_| {}));
        assert_eq!(client.host_version().as_deref(), Some("0.0.1-older"));
        assert!(matches!(
            older.next_request(Duration::from_secs(5)),
            Some(Command::RestartHost {
                when: RestartWhen::Idle
            })
        ));
        client.stop();

        // The same version, or a child host, is left alone. The client asks as it connects, so
        // a moment is enough to see that it did not.
        for (version, outlives_app) in [(build_version(), true), ("0.0.1-older", false)] {
            let host = FakeHost::start(welcome(version));
            let client = connect(&host, outlives_app, Box::new(|_| {}));
            assert!(
                host.next_request(Duration::from_millis(500)).is_none(),
                "{version} {outlives_app}"
            );
            client.stop();
        }
    }

    #[test]
    fn a_refusal_names_the_side_to_update() {
        let refusal = |host, client| Refusal {
            host_protocols: host,
            client_protocols: client,
            host_version: "1.4.0".into(),
            working: 2,
        };
        assert_eq!(
            refused(&refusal([3, 4], [2, 2])),
            "The host 1.4.0 speaks protocols 3–4 and this app 2–2. Update Wiffletree."
        );
        assert!(
            refused(&refusal([1, 2], [4, 4]))
                .ends_with("Restart the host so it runs this version.")
        );
    }
}
