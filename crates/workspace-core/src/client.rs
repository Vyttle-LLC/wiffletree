//! A client of a host's socket, as `docs/host-protocol.md` describes it: `Hello` with the client
//! credential, requests matched to their replies by id, and one event subscription. When the
//! connection drops, it connects again as `Backoff` says, resumes its subscription from its
//! last cursor, and sends again each request that never got its reply and is safe to repeat: a
//! read, or a `Send`, which the host stores once by its message id.
//!
//! A host that shares no protocol with the client refuses it. The client keeps trying, since an
//! older host restarts into the new version once asked, and says why it is not connected.
use crate::{backoff::Backoff, *};
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
    time::{Duration, Instant, SystemTime},
};

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
/// a host refuses the client or one that had refused it answers, or, on the caller's thread, when
/// `subscribe_to` widens a subscription in place, since the events it skipped are not sent.
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
    /// Its command's `type`, which the latency log names.
    kind: String,
    /// When it was written on the current connection, for its round trip.
    written_at: Option<Instant>,
}

/// One reply's timing (design 12 of `remote-hosts`): its round trip, the part the host spent
/// between reading the request and writing the reply, and the rest, the network's delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub round_trip_ms: u64,
    pub host_ms: u64,
    pub network_ms: u64,
}
impl Timing {
    fn of(round_trip: Duration, host_ms: u64) -> Self {
        let round_trip_ms = round_trip.as_millis() as u64;
        Self {
            round_trip_ms,
            host_ms,
            network_ms: round_trip_ms.saturating_sub(host_ms),
        }
    }
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
    /// When the host last sent a line.
    heard_at: Option<SystemTime>,
    /// The event kinds to subscribe to, from a host that can narrow them; every kind when `None`.
    kinds: Option<Vec<EventKindName>>,
    /// The host last connected to, as the latency log names it.
    host_id: String,
    /// The latest reply's timing.
    timing: Option<Timing>,
}

/// Whether the client is connected, and when it last heard from the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkState {
    pub connected: bool,
    pub heard_at: Option<SystemTime>,
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
    /// Try at once rather than wait out the delay.
    retry_now: AtomicBool,
    /// Woken, holding at most one wake, when the connection drops or comes back.
    link_changes: (async_channel::Sender<()>, async_channel::Receiver<()>),
    /// The connection was closed to subscribe again, so the next one opens at once.
    resubscribing: AtomicBool,
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
        let shared = Self::shared(launcher, client, on_event);
        let reader = match shared.open() {
            Ok(reader) => Some(reader),
            Err(_) if shared.state().refusal.is_some() => None,
            Err(error) => return Err(error),
        };
        Self::run(shared, move |_| reader)
    }
    /// Connects to the host `launcher` reaches, trying again as `Backoff` says until it answers,
    /// as for a paired host that may be away. Even the first try is the worker's, so a host that
    /// is slow to answer never holds up the caller.
    pub fn keep_connecting(
        launcher: Box<dyn Launch>,
        client: Client,
        on_event: OnEvent,
    ) -> io::Result<Self> {
        let shared = Self::shared(launcher, client, on_event);
        Self::run(shared, |shared| shared.open().ok())
    }
    /// Starts the worker on the connection `first` opens, or with none to try again later.
    fn run(
        shared: Arc<Shared>,
        first: impl FnOnce(&Shared) -> Option<BufReader<UnixStream>> + Send + 'static,
    ) -> io::Result<Self> {
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("host-client".into())
            .spawn(move || {
                let reader = first(&worker);
                worker.run(reader)
            })?;
        Ok(Self(shared))
    }
    fn shared(launcher: Box<dyn Launch>, client: Client, on_event: OnEvent) -> Arc<Shared> {
        Arc::new(Shared {
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
            retry_now: AtomicBool::new(false),
            link_changes: async_channel::bounded(1),
            resubscribing: AtomicBool::new(false),
        })
    }
    /// Subscribes to only `kinds` of event, or to every kind with `None`, from a host that can
    /// narrow them. A host that takes `Subscribe` again changes the live subscription; with an
    /// older one, a change connects again. Widening it in place gives `Resync`, so what reads
    /// the host reads it whole.
    pub fn subscribe_to(&self, kinds: Option<Vec<EventKindName>>) {
        let mut writer = self.0.writer();
        let (widened, in_place) = {
            let mut state = self.0.state();
            if state.kinds == kinds {
                return;
            }
            let widened = std::mem::replace(&mut state.kinds, kinds.clone()).is_some();
            let in_place = writer.is_some()
                && (state.capabilities.iter()).any(|c| c == capability::RESUBSCRIBE);
            (widened, in_place)
        };
        if !in_place {
            if let Some(stream) = writer.take() {
                self.0.resubscribing.store(true, Ordering::Relaxed);
                let _ = stream.shutdown(Shutdown::Both);
            }
            return;
        }
        drop(writer);
        // Should the connection drop first, the next one subscribes with these kinds anyway.
        let subscribe = Command::Subscribe { after: None, kinds };
        if let Err(error) = self.0.submit_as(subscribe, Box::new(|_| {}), false) {
            eprintln!("Change the subscription: {error}");
        }
        if widened {
            (self.0.on_event)(&EventKind::Resync);
        }
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
    /// The latest reply's timing, once one has come.
    pub fn timing(&self) -> Option<Timing> {
        self.0.state().timing
    }
    /// Whether connected now, and when the host last sent anything.
    pub fn link(&self) -> LinkState {
        let connected = self.connected();
        LinkState {
            connected,
            heard_at: self.0.state().heard_at,
        }
    }
    /// Wakes when the connection drops or comes back; read `link` then.
    pub fn link_changes(&self) -> async_channel::Receiver<()> {
        self.0.link_changes.1.clone()
    }
    /// While disconnected, tries to connect at once and starts the delays over, as the network
    /// changing or the human pressing Try now does.
    pub fn try_now(&self) {
        self.0.retry_now.store(true, Ordering::Relaxed);
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
        let (mut stream, credential) = self.launcher().connect(&self.stopped)?;
        if self.stopped.load(Ordering::Relaxed) {
            return Err(io::Error::other(DISCONNECTED));
        }
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let hello = Hello {
            credential,
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
        let narrows = (welcome.capabilities.iter()).any(|c| c == capability::ATTENTION);
        let wanted = |state: &State| state.kinds.clone().filter(|_| narrows);
        let (after, kinds) = {
            let state = self.state();
            (state.cursor.clone(), wanted(&state))
        };
        let subscribe = Request {
            version: welcome.protocol,
            id: new_id(),
            command: Command::Subscribe {
                after,
                kinds: kinds.clone(),
            },
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
        // `subscribe_to` changed the kinds during the handshake and found no connection to
        // close, so this one subscribes again at once.
        if wanted(&self.state()) != kinds {
            self.resubscribing.store(true, Ordering::Relaxed);
            return Err(io::Error::other(
                "The subscription changed while connecting",
            ));
        }
        let (was_refused, restart_when_idle) = {
            let mut state = self.state();
            state.protocol = welcome.protocol;
            state.cursor.get_or_insert(subscribed.cursor);
            let restart_when_idle = welcome.host_version != self.hello.app_version
                && (welcome.capabilities.iter()).any(|c| c == capability::RESTART_WHEN_IDLE);
            state.capabilities = welcome.capabilities;
            state.host_version = Some(welcome.host_version);
            state.host_id = welcome.host_id;
            state.heard_at = Some(SystemTime::now());
            (state.refusal.take().is_some(), restart_when_idle)
        };
        *writer = Some(stream);
        self.flush(&mut writer);
        drop(writer);
        let _ = self.link_changes.0.try_send(());
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
        let (mut stream, credential) = self.launcher().connect(&self.stopped)?;
        stream.set_read_timeout(Some(RESTART_TIMEOUT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let hello = Hello {
            credential,
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
            self.retry_now.store(false, Ordering::Relaxed);
            let mut backoff = Backoff::dropped(Instant::now());
            if self.resubscribing.swap(false, Ordering::Relaxed) {
                backoff.try_now(Instant::now());
            }
            reader = loop {
                if !self.wait(&mut backoff) {
                    return;
                }
                match self.open() {
                    Ok(reader) => break Some(reader),
                    Err(error) => eprintln!("Connect to the host: {error}"),
                }
                backoff.failed(Instant::now());
                if self.resubscribing.swap(false, Ordering::Relaxed) {
                    backoff.try_now(Instant::now());
                }
            };
        }
    }
    /// Sleeps until `backoff` is due, or until asked to try now; whether still running.
    fn wait(&self, backoff: &mut Backoff) -> bool {
        let step = Duration::from_millis(50);
        while !self.stopped.load(Ordering::Relaxed) {
            let now = Instant::now();
            if self.retry_now.swap(false, Ordering::Relaxed) {
                backoff.try_now(now);
            }
            if now >= backoff.due() {
                break;
            }
            std::thread::sleep(step.min(backoff.due() - now));
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
        self.state().heard_at = Some(SystemTime::now());
        if value.get("event").is_some() {
            let EventLine { event } = serde_json::from_value(value)?;
            self.state().cursor = Some(event.cursor);
            (self.on_event)(&event.kind);
            return Ok(());
        }
        let response: Response = serde_json::from_value(value)?;
        let pending = self.state().pending.remove(&response.id);
        if let Some(pending) = pending {
            if let (Some(host_ms), Some(written_at)) = (response.host_ms, pending.written_at) {
                self.timed(&pending.kind, Timing::of(written_at.elapsed(), host_ms));
            }
            (pending.reply)(match response.error {
                Some(error) => Err(error),
                None => Ok(response.result.unwrap_or_default()),
            });
        }
        Ok(())
    }
    /// Keeps `timing` as the latest and logs it, so a local and a remote host can be compared.
    fn timed(&self, kind: &str, timing: Timing) {
        let mut state = self.state();
        state.timing = Some(timing);
        let Timing {
            round_trip_ms,
            host_ms,
            network_ms,
        } = timing;
        let host = crate::pairing::short_id(&state.host_id);
        eprintln!(
            "Latency to host {host}, {kind}: round trip {round_trip_ms} ms, on the host {host_ms} ms, network {network_ms} ms"
        );
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
                pending.written_at = None;
            }
            ids.iter()
                .filter_map(|id| state.pending.remove(id))
                .collect()
        };
        drop(writer);
        let _ = self.link_changes.0.try_send(());
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
            let line = serde_json::to_value(&request).map_err(|e| e.to_string())?;
            let kind = line["command"]["type"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if serde_json::to_vec(&line).map_err(|e| e.to_string())?.len() >= MAX_LINE_BYTES {
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
                kind,
                written_at: None,
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
                    pending.written_at = Some(Instant::now());
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
        os::unix::net::UnixListener,
        path::{Path, PathBuf},
        sync::atomic::AtomicUsize,
        time::Instant,
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
                                host_ms: Some(0),
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

    /// A host that welcomes each connection, answers `Subscribe`, records every `Send` by its
    /// message id, and closes the first connection that brings a `Send` without answering it,
    /// as when the connection drops after the host stored the message.
    fn losing_first_answer(folder: &Path) -> Arc<Mutex<Vec<String>>> {
        let listener = UnixListener::bind(folder.join(SOCKET_FILE)).unwrap();
        let sends = Arc::new(Mutex::new(vec![]));
        let recorded = sends.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (Ok(mut stream), sends) = (stream, recorded.clone()) else {
                    continue;
                };
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let _: HelloLine = read_line(&mut reader).unwrap();
                    write_line(&mut stream, &welcome("t")).unwrap();
                    while let Ok(request) = read_line::<Request>(&mut reader) {
                        let result = match request.command {
                            Command::Subscribe { .. } => {
                                serde_json::json!({"cursor": {"epoch": "e", "seq": 0}})
                            }
                            Command::Send { id, .. } => {
                                let mut sends = sends.lock().unwrap();
                                sends.push(id);
                                if sends.len() == 1 {
                                    let _ = stream.shutdown(Shutdown::Both);
                                    return;
                                }
                                Value::Null
                            }
                            _ => Value::Null,
                        };
                        let response = Response {
                            version: request.version,
                            id: request.id,
                            result: Some(result),
                            error: None,
                            host_ms: None,
                        };
                        let _ = write_line(&mut stream, &response);
                    }
                });
            }
        });
        sends
    }
    fn send(body: &str) -> Command {
        Command::Send {
            id: new_id(),
            sender: None,
            recipient: "coordinator".into(),
            body: body.into(),
            attachments: vec![],
            uploads: vec![],
        }
    }
    fn found(folder: &Path) -> Box<Found> {
        Box::new(Found {
            socket: folder.join(SOCKET_FILE),
            outlives_app: false,
        })
    }
    fn test_client() -> Client {
        Client {
            name: "test".into(),
            kind: "script".into(),
        }
    }

    #[test]
    fn a_send_whose_answer_was_lost_goes_again_with_its_id() {
        let folder = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let sends = losing_first_answer(folder.path());
        let client =
            HostClient::connect(found(folder.path()), test_client(), Box::new(|_| {})).unwrap();

        let message = send("Ship it");
        let Command::Send { id, .. } = &message else {
            unreachable!()
        };
        let reply = client.request(message.clone()).unwrap();
        let answered = reply.recv_blocking().unwrap();

        assert!(answered.is_ok(), "{answered:?}");
        assert_eq!(
            *sends.lock().unwrap(),
            [id.clone(), id.clone()],
            "the same id twice"
        );
        client.stop();
    }

    #[test]
    fn a_message_waiting_when_the_client_stops_is_never_sent() {
        let folder = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        // No host answers yet, so the message waits.
        let away =
            HostClient::keep_connecting(found(folder.path()), test_client(), Box::new(|_| {}))
                .unwrap();
        let _waiting = away.request(send("Written while away")).unwrap();
        away.stop();

        let sends = losing_first_answer(folder.path());
        let back =
            HostClient::connect(found(folder.path()), test_client(), Box::new(|_| {})).unwrap();
        // The new client is answered, so anything the old one held would have arrived by now.
        back.request_blocking(Command::Snapshot, Duration::from_secs(5))
            .unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            sends.lock().unwrap().is_empty(),
            "{:?}",
            sends.lock().unwrap()
        );
        back.stop();
    }

    #[test]
    fn a_client_subscribes_narrowly_only_where_the_host_offers_it() {
        let mut offering = welcome("t");
        if let HelloAnswer::Welcome(welcome) = &mut offering {
            welcome.capabilities.push(capability::ATTENTION.into());
        }
        for (answer, narrowed) in [(welcome("t"), false), (offering, true)] {
            let host = FakeHost::start(answer);
            let client = connect(&host, false, Box::new(|_| {}));
            client.subscribe_to(Some(vec![EventKindName::Attention]));
            let deadline = Instant::now() + Duration::from_secs(10);
            let kinds = loop {
                assert!(Instant::now() < deadline, "no second subscription");
                match host.requests.recv_blocking().unwrap() {
                    Command::Subscribe {
                        kinds: Some(kinds), ..
                    } => break Some(kinds),
                    Command::Subscribe {
                        kinds: None,
                        after: Some(_),
                    } => break None,
                    _ => {}
                }
            };
            let expected = narrowed.then(|| vec![EventKindName::Attention]);
            assert_eq!(kinds, expected, "narrowed: {narrowed}");
            client.stop();
        }
    }

    #[test]
    fn a_subscription_changed_while_connecting_is_made_on_that_connection_or_the_next() {
        let folder = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let listener = UnixListener::bind(folder.path().join(SOCKET_FILE)).unwrap();
        let (subscribed, subscriptions) = async_channel::unbounded();
        let (release, released) = async_channel::unbounded::<()>();
        std::thread::spawn(move || {
            for (index, stream) in listener.incoming().enumerate() {
                let (Ok(mut stream), subscribed) = (stream, subscribed.clone()) else {
                    continue;
                };
                let released = released.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let _: HelloLine = read_line(&mut reader).unwrap();
                    let mut answer = welcome("t");
                    if let HelloAnswer::Welcome(welcome) = &mut answer {
                        welcome.capabilities.push(capability::ATTENTION.into());
                    }
                    write_line(&mut stream, &answer).unwrap();
                    while let Ok(request) = read_line::<Request>(&mut reader) {
                        if let Command::Subscribe { kinds, .. } = &request.command {
                            let _ = subscribed.send_blocking(kinds.clone());
                            match index {
                                // The first connection drops once subscribed.
                                0 => return drop(stream.shutdown(Shutdown::Both)),
                                // The second holds its answer until the test lets it go.
                                1 => drop(released.recv_blocking()),
                                _ => {}
                            }
                        }
                        let response = Response {
                            version: request.version,
                            id: request.id,
                            result: Some(serde_json::json!({"cursor": {"epoch": "e", "seq": 0}})),
                            error: None,
                            host_ms: None,
                        };
                        let _ = write_line(&mut stream, &response);
                    }
                });
            }
        });
        let client =
            HostClient::keep_connecting(found(folder.path()), test_client(), Box::new(|_| {}))
                .unwrap();
        let next = || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match subscriptions.try_recv() {
                    Ok(kinds) => return kinds,
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => panic!("no subscription in time"),
                }
            }
        };
        assert_eq!(next(), None, "the first connection, which drops");
        assert_eq!(next(), None, "the second, held mid-handshake");

        client.subscribe_to(Some(vec![EventKindName::Attention]));
        release.send_blocking(()).unwrap();

        assert_eq!(
            next(),
            Some(vec![EventKindName::Attention]),
            "subscribed again"
        );
        client.stop();
    }

    #[test]
    fn a_host_slow_to_answer_does_not_hold_up_keep_connecting() {
        struct Slow;
        impl Launch for Slow {
            fn endpoint(&mut self, stopped: &AtomicBool) -> io::Result<Endpoint> {
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline && !stopped.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(io::Error::other("no answer"))
            }
        }
        let started = Instant::now();
        let client =
            HostClient::keep_connecting(Box::new(Slow), test_client(), Box::new(|_| {})).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert!(!client.link().connected);
        client.stop();
    }

    #[test]
    fn a_host_that_takes_subscribe_again_changes_the_kinds_on_the_same_connection() {
        let mut offering = welcome("t");
        if let HelloAnswer::Welcome(welcome) = &mut offering {
            (welcome.capabilities)
                .extend([capability::ATTENTION, capability::RESUBSCRIBE].map(String::from));
        }
        let host = FakeHost::start(offering);
        let resyncs = Arc::new(AtomicUsize::new(0));
        let counted = resyncs.clone();
        let on_event: OnEvent = Box::new(move |kind| {
            if *kind == EventKind::Resync {
                counted.fetch_add(1, Ordering::Relaxed);
            }
        });
        let client = connect(&host, false, on_event);
        let subscriptions = || loop {
            if let Command::Subscribe { after, kinds } = host.requests.recv_blocking().unwrap() {
                return (after.is_some(), kinds);
            }
        };
        assert_eq!(subscriptions(), (false, None), "the first subscription");

        client.subscribe_to(Some(vec![EventKindName::Attention]));
        // A new connection would resume from its cursor; this one names none.
        let attention = Some(vec![EventKindName::Attention]);
        assert_eq!(subscriptions(), (false, attention));
        assert_eq!(
            resyncs.load(Ordering::Relaxed),
            0,
            "narrowing skips nothing"
        );

        client.subscribe_to(None);
        assert_eq!(subscriptions(), (false, None));
        assert_eq!(resyncs.load(Ordering::Relaxed), 1, "widening reads anew");
        assert!(client.link().connected);
        client.stop();
    }

    #[test]
    fn a_reply_splits_its_round_trip_into_the_hosts_time_and_the_networks() {
        let timing = Timing::of(Duration::from_millis(240), 12);
        assert_eq!((timing.host_ms, timing.network_ms), (12, 228));
        assert_eq!(timing.round_trip_ms, 240);
        // A clock that disagrees never makes the network's share negative.
        assert_eq!(Timing::of(Duration::from_millis(5), 12).network_ms, 0);

        let host = FakeHost::start(welcome("t"));
        let client = connect(&host, false, Box::new(|_| {}));
        assert_eq!(client.timing(), None);
        client
            .request_blocking(Command::Snapshot, Duration::from_secs(5))
            .unwrap();
        let timing = client.timing().expect("the reply was timed");
        assert_eq!(timing.host_ms, 0);
        assert_eq!(timing.network_ms, timing.round_trip_ms);
        client.stop();
    }
}
