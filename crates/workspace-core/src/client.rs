//! A client of a host's socket, as `docs/host-protocol.md` describes it: `Hello` with the client
//! credential, requests matched to their replies by id, and one event subscription. When the
//! connection drops, it connects again after a growing delay, resumes its subscription from its
//! last cursor, and sends again each request that never got its reply and is safe to repeat: a
//! read, or a `Send`, which the host stores once by its message id.
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
const LOST: &str = "The connection to the host closed before it answered";
const DISCONNECTED: &str = "Not connected to the host";

type Reply = Box<dyn FnOnce(Result<Value, String>) + Send>;
/// Called on the client's reading thread with each event the host sends.
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
    /// when the first connection does.
    pub fn connect(
        launcher: Box<dyn Launch>,
        client: Client,
        on_event: OnEvent,
    ) -> io::Result<Self> {
        let app_version = option_env!("WIFFLETREE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
        let shared = Arc::new(Shared {
            hello: Hello {
                protocols: [PROTOCOL_VERSION, PROTOCOL_VERSION],
                app_version: app_version.into(),
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
        let reader = shared.open()?;
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
    pub fn connected(&self) -> bool {
        self.0.writer().is_some()
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

    /// Connects, says `Hello`, subscribes from the last cursor and writes what waits.
    fn open(&self) -> io::Result<BufReader<UnixStream>> {
        let endpoint = self.launcher().endpoint()?;
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
            HelloAnswer::Refused(refusal) => return Err(io::Error::other(refused(&refusal))),
        };
        let after = self.state().cursor.clone();
        let subscribe = Request {
            version: welcome.protocol,
            id: new_id(),
            command: Command::Subscribe { after },
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
        {
            let mut state = self.state();
            state.protocol = welcome.protocol;
            state.capabilities = welcome.capabilities;
            state.cursor.get_or_insert(subscribed.cursor);
        }
        *writer = Some(stream);
        self.flush(&mut writer);
        Ok(reader)
    }

    /// Reads until the connection drops, then connects again until stopped.
    fn run(&self, mut reader: BufReader<UnixStream>) {
        loop {
            self.read(&mut reader);
            self.disconnected();
            let mut delay = FIRST_RETRY;
            reader = loop {
                if !self.wait(delay) {
                    return;
                }
                match self.open() {
                    Ok(reader) => break reader,
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
    /// Asks the host's actor something cheap, so a host that is only quiet answers.
    fn probe(&self) {
        let _ = self.submit_as(Command::Settings, Box::new(|_| {}), false);
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
                fn endpoint(&mut self) -> io::Result<Endpoint> {
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
        assert!(host.commands().contains(&"settings".to_string()));
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
        };
        let error = client.request(send).unwrap_err();
        assert!(error.contains("too large to send"), "{error}");
        let small = Command::Send {
            id: new_id(),
            sender: None,
            recipient: "coordinator".into(),
            body: "Hello".into(),
            attachments: vec![],
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
