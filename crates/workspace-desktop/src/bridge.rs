//! The app's link to a host: This Mac's over `host.sock`, a launchd job for a signed app on its
//! default data folder and otherwise the app's own child host, or a paired host's over the
//! network through `RemoteLaunch`. Views send commands through it and are woken by the host's
//! events.
use async_channel::{Receiver, Sender};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use workspace_core::{
    ask::Ask,
    remote_launch::{RemoteLaunch, RemoteState},
    *,
};

/// Waits for the host's answer to `ask`, before the app starts.
fn answer<R: DeserializeOwned>(client: &HostClient, ask: Ask<R>) -> anyhow::Result<R> {
    let reply = client
        .request(ask.command().clone())
        .map_err(anyhow::Error::msg)?;
    let value = reply.recv_blocking()?.map_err(anyhow::Error::msg)?;
    ask.decode(value).map_err(anyhow::Error::msg)
}

/// One wake per event kind, each holding at most one wake until it is read, and the menu bar
/// icon's own wake for state changes, since it runs beside the window's.
#[derive(Clone)]
struct Wakes {
    state: (Sender<()>, Receiver<()>),
    menu_bar: (Sender<()>, Receiver<()>),
    steps: (Sender<()>, Receiver<()>),
    progress: (Sender<()>, Receiver<()>),
    usage: (Sender<()>, Receiver<()>),
    attention: (Sender<()>, Receiver<()>),
    /// The host's count of things that need the human, from its last `attention` event.
    needs_you: Arc<Mutex<Option<usize>>>,
    /// Set by `resync`: the host may have restarted, and its progress revisions with it.
    progress_resynced: Arc<AtomicBool>,
    /// Whose steps changed since the step wake was last read.
    changed_steps: Arc<Mutex<ChangedSteps>>,
}
impl Wakes {
    fn new() -> Self {
        let wake = || async_channel::bounded(1);
        Self {
            state: wake(),
            menu_bar: wake(),
            steps: wake(),
            progress: wake(),
            usage: wake(),
            attention: wake(),
            needs_you: Arc::default(),
            progress_resynced: Arc::default(),
            changed_steps: Arc::default(),
        }
    }
    fn changed_steps(&self) -> std::sync::MutexGuard<'_, ChangedSteps> {
        self.changed_steps.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn wake(&self, kind: &EventKind) {
        let woken = match kind {
            EventKind::State => vec![&self.state, &self.menu_bar],
            EventKind::Steps { sessions } => {
                (self.changed_steps().sessions).extend(sessions.iter().cloned());
                vec![&self.steps]
            }
            EventKind::Progress => vec![&self.progress],
            EventKind::Usage => vec![&self.usage],
            EventKind::Attention { count } => {
                *self.needs_you.lock().unwrap_or_else(|e| e.into_inner()) = Some(*count);
                vec![&self.attention]
            }
            EventKind::Resync => {
                self.progress_resynced.store(true, Ordering::Relaxed);
                self.changed_steps().all = true;
                vec![
                    &self.state,
                    &self.menu_bar,
                    &self.steps,
                    &self.progress,
                    &self.usage,
                ]
            }
        };
        for (sender, _) in woken {
            let _ = sender.try_send(());
        }
    }
}

/// The sessions whose work steps changed, or every session's after a resync, when events
/// may have been lost.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct ChangedSteps {
    pub(crate) sessions: BTreeSet<String>,
    pub(crate) all: bool,
}
impl ChangedSteps {
    pub(crate) fn includes(&self, session_id: &str) -> bool {
        self.all || self.sessions.contains(session_id)
    }
}

/// A paired host, as the app names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteName {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
pub(crate) struct Bridge {
    client: Result<HostClient, String>,
    /// The paired host this bridge reaches; `None` for This Mac's own host.
    remote: Option<RemoteName>,
    /// What the last try at a paired host found.
    reached: Option<Arc<Mutex<RemoteState>>>,
    wakes: Wakes,
    /// The host is a launchd job, which keeps running after the app exits.
    outlives_app: bool,
}
impl Bridge {
    /// Connects to the host of `home`, the default data folder when `default_folder`, starting
    /// it as `HostLaunch::choose` says.
    pub(crate) fn start(
        home: &Path,
        default_folder: bool,
        demo_repository: Option<String>,
    ) -> Self {
        let launcher = (|| -> anyhow::Result<_> {
            let app = std::env::current_exe()?;
            let binary = app.parent().unwrap().join("workspace-host");
            let choice = HostLaunch::choose(developer_id_signed(&app), default_folder);
            // `Wiffletree.app/Contents/MacOS/workspace-desktop`
            let bundle = app.ancestors().nth(3).unwrap_or(&app);
            Ok(launcher(choice, &binary, home, bundle))
        })();
        let bridge = match launcher {
            Ok(launcher) => Self::connect(launcher),
            Err(error) => Self::unavailable(format!("{error:#}")),
        };
        if let (Some(path), Ok(client)) = (demo_repository, &bridge.client)
            && let Err(error) = Self::start_demo(client, path)
        {
            return Self::unavailable(format!("{error:#}"));
        }
        bridge
    }
    pub(crate) fn connect(launcher: Box<dyn Launch>) -> Self {
        let outlives_app = launcher.outlives_app();
        let wakes = Wakes::new();
        let woken = wakes.clone();
        let client = HostClient::connect(
            launcher,
            Self::client(),
            Box::new(move |kind| woken.wake(kind)),
        );
        Self {
            client: client.map_err(|e| format!("The host did not start: {e}")),
            remote: None,
            reached: None,
            wakes,
            outlives_app,
        }
    }
    /// Keeps a connection to the paired host `host` through `launcher`, while it can be reached.
    pub(crate) fn remote_host(host: RemoteName, launcher: RemoteLaunch) -> Self {
        let wakes = Wakes::new();
        let woken = wakes.clone();
        let reached = launcher.state();
        let client = HostClient::keep_connecting(
            Box::new(launcher),
            Self::client(),
            Box::new(move |kind| woken.wake(kind)),
        );
        Self {
            client: client.map_err(|e| format!("Not connected to {}: {e}", host.name)),
            remote: Some(host),
            reached: Some(reached),
            wakes,
            outlives_app: false,
        }
    }
    fn client() -> Client {
        Client {
            name: "Wiffletree".into(),
            kind: "desktop".into(),
        }
    }
    /// A paired host the app has no connection to.
    pub(crate) fn unreachable(host: RemoteName) -> Self {
        Self {
            remote: Some(host.clone()),
            ..Self::unavailable(format!("Not connected to {}", host.name))
        }
    }
    fn unavailable(error: String) -> Self {
        Self {
            client: Err(error),
            remote: None,
            reached: None,
            wakes: Wakes::new(),
            outlives_app: false,
        }
    }
    /// The paired host this bridge reaches, or `None` for This Mac's.
    pub(crate) fn remote(&self) -> Option<&RemoteName> {
        self.remote.as_ref()
    }
    /// Whether the app is connected to the host now.
    pub(crate) fn connected(&self) -> bool {
        self.client.as_ref().is_ok_and(|c| c.link().connected)
    }
    /// What the last try at a paired host found: the level it grants and why it turned this
    /// device away.
    pub(crate) fn reached(&self) -> RemoteState {
        (self.reached.as_ref())
            .map(|state| state.lock().unwrap_or_else(|e| e.into_inner()).clone())
            .unwrap_or_default()
    }
    /// The host's count of things that need the human, once it has sent one.
    pub(crate) fn needs_you(&self) -> Option<usize> {
        *self
            .wakes
            .needs_you
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
    /// Wakes when the host's count of things that need the human changed.
    pub(crate) fn attention_changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.attention)
    }
    /// Subscribes to every event while the window shows this host, and otherwise only to
    /// `attention`, which the switcher counts.
    pub(crate) fn follow(&self, shown: bool) {
        if let Ok(client) = &self.client {
            client.subscribe_to((!shown).then(|| vec![EventKindName::Attention]));
        }
    }
    /// Whether the host keeps running, with its agents, after the app exits.
    pub(crate) fn outlives_app(&self) -> bool {
        self.outlives_app
    }
    /// Why the host refuses the app, while it does: it shares no protocol with it.
    pub(crate) fn refusal(&self) -> Option<Refusal> {
        self.client.as_ref().ok()?.refusal()
    }
    /// The version of the host, once it has answered.
    pub(crate) fn host_version(&self) -> Option<String> {
        self.client.as_ref().ok()?.host_version()
    }
    /// When the host last sent anything, in Unix milliseconds.
    pub(crate) fn heard_at(&self) -> Option<i64> {
        let heard = self.client.as_ref().ok()?.link().heard_at?;
        let since = heard.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(since.as_millis() as i64)
    }
    /// Wakes when the connection drops or comes back.
    pub(crate) fn link_changes(&self) -> Option<Receiver<()>> {
        Some(self.client.as_ref().ok()?.link_changes())
    }
    /// While disconnected, tries to connect at once and starts the delays over.
    pub(crate) fn try_now(&self) {
        if let Ok(client) = &self.client {
            client.try_now();
        }
    }
    /// Asks the host to restart and waits for its answer; run it off the main thread.
    pub(crate) fn restart_host(&self, when: RestartWhen) -> Result<(), String> {
        self.client
            .as_ref()
            .map_err(Clone::clone)?
            .restart_host(when)
    }
    fn start_demo(client: &HostClient, path: String) -> anyhow::Result<()> {
        let snapshot = answer(client, ask::snapshot())?;
        // A store without a saved setup starts nothing, the demo project included.
        if snapshot.projects.is_empty() && snapshot.model_selection.is_saved() {
            let project = answer(client, ask::create_project("Workspace trial".into()))?;
            let base = "HEAD".into();
            answer(client, ask::attach_repository(Some(project.id), path, base))?;
        }
        Ok(())
    }
    pub(crate) fn request(
        &self,
        command: Command,
    ) -> Result<Receiver<Result<Value, String>>, String> {
        if let Some(reason) = self.refuses(&command) {
            return Err(reason);
        }
        self.client.as_ref().map_err(Clone::clone)?.request(command)
    }
    /// Why a paired host takes no change now: it does not answer, so the change would wait and
    /// apply later, or it lets this device only view. Reads, and messages, which wait with their
    /// ids, always go.
    fn refuses(&self, command: &Command) -> Option<String> {
        let host = self.remote.as_ref()?;
        if command.is_read() || matches!(command, Command::Send { .. }) {
            return None;
        }
        let link = crate::hosts::Link::of(self);
        crate::hosts::HostView::remote(&host.name, link, None).changes_refused()
    }
    /// Sends `command`; its reply arrives when the host answers, or says the host is gone.
    pub(crate) fn send(
        &self,
        command: Command,
    ) -> Result<impl Future<Output = Result<Value, String>> + use<>, String> {
        let receiver = self.request(command)?;
        Ok(
            async move { (receiver.recv().await).unwrap_or_else(|_| Err("Host disconnected".into())) },
        )
    }
    /// Sends `ask`; its reply arrives decoded as what the host answers it with.
    pub(crate) fn ask<R: DeserializeOwned>(
        &self,
        ask: Ask<R>,
    ) -> Result<impl Future<Output = Result<R, String>> + use<R>, String> {
        let reply = self.send(ask.command().clone())?;
        Ok(async move { reply.await.and_then(|reply| ask.decode(reply)) })
    }
    /// Sends `ask` and waits briefly for its answer, for a test. The app never waits for the
    /// host on its UI thread. Fails at once while the host is out of reach.
    #[cfg(test)]
    pub(crate) fn ask_now<R: DeserializeOwned>(&self, ask: Ask<R>) -> Result<R, String> {
        self.ask_within(ask, Duration::from_secs(2))
    }
    /// Sends `ask` and waits at most `timeout` for its answer. Fails at once while the host is
    /// out of reach or refuses the app; run it off the main thread.
    pub(crate) fn ask_within<R: DeserializeOwned>(
        &self,
        ask: Ask<R>,
        timeout: Duration,
    ) -> Result<R, String> {
        let client = self.client.as_ref().map_err(Clone::clone)?;
        if let Some(reason) = self.refuses(ask.command()) {
            return Err(reason);
        }
        let reply = client.request_blocking(ask.command().clone(), timeout)?;
        ask.decode(reply)
    }
    /// Disconnects. A child host stops, which stops its running turns; a launchd host keeps
    /// running.
    pub(crate) fn shutdown(&self) {
        if let Ok(client) = &self.client {
            client.stop();
        }
    }
    /// Runs `with` on the app's child host, while it has one.
    #[cfg(test)]
    pub(crate) fn with_child<R>(&self, with: impl FnOnce(&mut ChildHost) -> R) -> Option<R> {
        let client = self.client.as_ref().ok()?;
        client.with_launcher(|launcher| launcher.child().map(with))
    }
    fn wake(&self, wake: &(Sender<()>, Receiver<()>)) -> Option<Receiver<()>> {
        self.client.is_ok().then(|| wake.1.clone())
    }
    /// Wakes when the snapshot should be read again.
    pub(crate) fn changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.state)
    }
    /// Wakes the menu bar icon when the snapshot, and so how many agents work, may have changed.
    pub(crate) fn menu_bar_changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.menu_bar)
    }
    /// Wakes when some session's work steps changed; `take_changed_steps` says whose, and
    /// `Command::Steps` fetches them.
    pub(crate) fn step_changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.steps)
    }
    /// Whose steps changed since this was last taken.
    pub(crate) fn take_changed_steps(&self) -> ChangedSteps {
        std::mem::take(&mut *self.wakes.changed_steps())
    }
    /// Wakes when a figure on the progress board changed; fetch it with `Command::Progress`.
    pub(crate) fn progress_changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.progress)
    }
    /// Whether the progress board must be read whole, once: true once after each `resync`.
    pub(crate) fn take_progress_resync(&self) -> bool {
        self.wakes.progress_resynced.swap(false, Ordering::Relaxed)
    }
    /// Wakes when usage was recorded; fetch usage and quotas.
    pub(crate) fn usage_changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.usage)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use workspace_core::remote_client;
    use workspace_host::{Host, service::Service, socket};

    /// A host in this process serving `host.sock` in a temporary data folder, as a launchd host
    /// would, and the app's bridge to it.
    pub(crate) struct Served {
        pub(crate) data: tempfile::TempDir,
        pub(crate) bridge: Bridge,
        endpoint: Endpoint,
        _listener: socket::Listener,
        service: Service,
    }
    impl Served {
        /// A second bridge to the same host, as the app has after it opens again.
        pub(crate) fn reconnect(&self) -> Bridge {
            Bridge::connect(Box::new(Running(self.endpoint.clone())))
        }
        /// A bridge to this host through a relay that holds the app's requests while the flag is
        /// set, as a busy host would.
        pub(crate) fn busy(&self) -> (Bridge, Arc<AtomicBool>) {
            use std::io::{BufRead, BufReader, Write};
            use std::os::unix::net::{UnixListener, UnixStream};
            let relay = self.data.path().join("relay.sock");
            let listener = UnixListener::bind(&relay).unwrap();
            let (host, held) = (
                self.endpoint.socket.clone(),
                Arc::new(AtomicBool::new(false)),
            );
            let holding = held.clone();
            std::thread::spawn(move || {
                for app in listener.incoming() {
                    let (Ok(app), Ok(mut to_host)) = (app, UnixStream::connect(&host)) else {
                        return;
                    };
                    let (mut to_app, mut from_host) =
                        (app.try_clone().unwrap(), to_host.try_clone().unwrap());
                    std::thread::spawn(move || std::io::copy(&mut from_host, &mut to_app));
                    let holding = holding.clone();
                    std::thread::spawn(move || {
                        for line in BufReader::new(app).split(b'\n') {
                            let Ok(mut line) = line else { return };
                            while holding.load(Ordering::Relaxed) {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            line.push(b'\n');
                            if to_host.write_all(&line).is_err() {
                                return;
                            }
                        }
                    });
                }
            });
            let endpoint = Endpoint {
                socket: relay,
                credential: self.endpoint.credential.clone(),
            };
            (Bridge::connect(Box::new(Running(endpoint))), held)
        }
    }
    impl Drop for Served {
        fn drop(&mut self) {
            self.bridge.shutdown();
            self.service.shutdown();
        }
    }
    struct Running(Endpoint);
    impl Launch for Running {
        fn endpoint(&mut self, _: &std::sync::atomic::AtomicBool) -> std::io::Result<Endpoint> {
            Ok(self.0.clone())
        }
    }

    /// Serves a set-up store in a fresh data folder, after `prepare` has added to it.
    pub(crate) fn serve(prepare: impl FnOnce(&mut Host)) -> Served {
        let data = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let mut host = Host::open_set_up(data.path()).unwrap();
        let workspaces = data.path().join("workspaces");
        (host.set_workspaces_dir(workspaces.to_str().unwrap())).unwrap();
        prepare(&mut host);
        drop(host);
        let binary = std::env::current_exe().unwrap();
        let service = Service::start(data.path().into(), binary.clone()).unwrap();
        let credential = new_credential().unwrap();
        let options = socket::Options {
            credential: credential.clone(),
            // SAFETY: getuid cannot fail.
            owner: unsafe { libc::getuid() },
            binary,
        };
        let listener = socket::listen(service.clone(), data.path(), options).unwrap();
        let endpoint = Endpoint {
            socket: data.path().join(SOCKET_FILE),
            credential: Some(credential),
        };
        Served {
            bridge: Bridge::connect(Box::new(Running(endpoint.clone()))),
            endpoint,
            data,
            _listener: listener,
            service,
        }
    }

    #[test]
    fn after_a_resync_the_progress_board_is_read_whole_once() {
        let bridge = Bridge::unavailable(String::new());
        bridge.wakes.wake(&EventKind::Progress);
        assert!(!bridge.take_progress_resync());
        bridge.wakes.wake(&EventKind::Resync);
        assert!(bridge.take_progress_resync());
        assert!(!bridge.take_progress_resync());
    }

    #[test]
    fn step_wakes_say_whose_steps_changed_and_a_resync_says_everyone_s() {
        let bridge = Bridge::unavailable(String::new());
        let steps = |sessions: &[&str]| EventKind::Steps {
            sessions: sessions.iter().map(|s| s.to_string()).collect(),
        };
        bridge.wakes.wake(&steps(&["a"]));
        bridge.wakes.wake(&steps(&["b", "a"]));
        let changed = bridge.take_changed_steps();
        assert!(changed.includes("a") && changed.includes("b") && !changed.includes("c"));
        assert_eq!(bridge.take_changed_steps(), ChangedSteps::default());
        bridge.wakes.wake(&EventKind::Resync);
        assert!(bridge.take_changed_steps().includes("c"));
        assert!(!bridge.take_changed_steps().includes("c"), "once");
    }

    #[test]
    fn requests_and_events_cross_the_socket() {
        // Renaming changes state without starting a coordinator, so no provider is needed.
        let project = std::cell::OnceCell::new();
        let served = serve(|host| {
            project
                .set(host.create_project("Socket").unwrap().id)
                .unwrap();
        });
        let project = project.into_inner().unwrap();
        let changes = served.bridge.changes().unwrap();
        let rename = ask::rename_project(project.clone(), "Renamed".into());
        served.bridge.ask_now(rename).unwrap();
        let snapshot = served.bridge.ask_now(ask::snapshot()).unwrap();
        assert!((snapshot.projects.iter()).any(|p| p.id == project && p.name == "Renamed"));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while changes.try_recv().is_err() {
            assert!(std::time::Instant::now() < deadline, "no state event");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A host in this process with remote access on, named Studio Mac, and a device paired
    /// with it, as a second Mac is.
    pub(crate) struct Studio {
        pub(crate) data: tempfile::TempDir,
        service: Option<Service>,
        key: workspace_core::pairing::KeyPair,
        port: u16,
        pub(crate) paired: remote_client::PairedHost,
        device: workspace_core::pairing::KeyPair,
        /// The project's coordinator, which takes no turns.
        pub(crate) coordinator: String,
    }
    impl Studio {
        pub(crate) fn start() -> Self {
            let data = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
            let mut host = Host::open_set_up(data.path()).unwrap();
            let workspaces = data.path().join("workspaces");
            host.set_workspaces_dir(workspaces.to_str().unwrap())
                .unwrap();
            let project = host.create_project("Tide alerts").unwrap();
            host.set_live(&project.id, false).unwrap();
            let coordinator = (host.sessions().unwrap().into_iter())
                .find(|s| s.project_id == project.id)
                .unwrap()
                .id;
            drop(host);
            let key = workspace_core::pairing::KeyPair::generate().unwrap();
            let service = Self::serve(data.path(), &key, 0);
            let state = |service: &Service, command| {
                let reply = service.request(command).unwrap().recv_blocking().unwrap();
                serde_json::from_value::<RemoteAccess>(reply.unwrap()).unwrap()
            };
            let port = state(&service, Command::SetRemoteAccess { enabled: true })
                .port
                .expect("listening");
            let code = state(&service, Command::OpenPairing).pairing.unwrap().code;
            let device = workspace_core::pairing::KeyPair::generate().unwrap();
            let address = format!("127.0.0.1:{port}");
            let paired = remote_client::pair(&address, &code, &device, "Test Mac").unwrap();
            Self {
                data,
                service: Some(service),
                key,
                port,
                paired,
                device,
                coordinator,
            }
        }
        fn serve(data: &Path, key: &workspace_core::pairing::KeyPair, port: u16) -> Service {
            let key = key.clone();
            let binary = std::env::current_exe().unwrap();
            let options = workspace_host::remote::Options {
                port,
                key: Box::new(move || Ok(key.clone())),
                name: Some("Studio Mac".into()),
                binary: binary.clone(),
                ..Default::default()
            };
            Service::start_with(data.into(), binary, options).unwrap()
        }
        /// This device's bridge to the host, as the switcher keeps one.
        pub(crate) fn bridge(&self) -> Bridge {
            let name = RemoteName {
                id: self.paired.id.clone(),
                name: self.paired.name.clone(),
            };
            let address = format!("127.0.0.1:{}", self.port);
            let launcher = RemoteLaunch::new(
                self.paired.public_key.clone(),
                self.device.clone(),
                vec![address],
                Box::new(Vec::new),
            );
            Bridge::remote_host(name, launcher)
        }
        /// Stops the host, as a restart does, and starts it again on the same port.
        fn restart(&mut self) {
            if let Some(service) = self.service.take() {
                service.shutdown();
            }
            self.service = Some(Self::serve(self.data.path(), &self.key, self.port));
        }
        fn stop(&mut self) {
            if let Some(service) = self.service.take() {
                service.shutdown();
            }
        }
    }
    impl Drop for Studio {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Reaches `host.sock` through a relay that cuts the first connection once the host has
    /// answered a `Send`, before the answer reaches the client, as a drop after the host stored
    /// the message does. Later connections go straight to the socket.
    struct CutAfterSend {
        endpoint: Endpoint,
        cut: bool,
    }
    impl Launch for CutAfterSend {
        fn endpoint(&mut self, _: &AtomicBool) -> std::io::Result<Endpoint> {
            Ok(self.endpoint.clone())
        }
        fn connect(
            &mut self,
            _: &AtomicBool,
        ) -> std::io::Result<(std::os::unix::net::UnixStream, Option<String>)> {
            use std::io::{BufRead, BufReader, Write};
            use std::os::unix::net::UnixStream;
            let host = UnixStream::connect(&self.endpoint.socket)?;
            let credential = self.endpoint.credential.clone();
            if std::mem::replace(&mut self.cut, true) {
                return Ok((host, credential));
            }
            let (ours, theirs) = UnixStream::pair()?;
            let send_id = Arc::new(Mutex::new(None::<String>));
            let (mut to_host, from_client) = (host.try_clone()?, theirs.try_clone()?);
            let noted = send_id.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(from_client).lines().map_while(Result::ok) {
                    let request: Value = serde_json::from_str(&line).unwrap_or_default();
                    if request["command"]["type"] == "send" {
                        *noted.lock().unwrap() = request["id"].as_str().map(str::to_owned);
                    }
                    if writeln!(to_host, "{line}").is_err() {
                        break;
                    }
                }
            });
            let mut to_client = theirs;
            std::thread::spawn(move || {
                for line in BufReader::new(host.try_clone().unwrap()).lines() {
                    let Ok(line) = line else { break };
                    let answer: Value = serde_json::from_str(&line).unwrap_or_default();
                    let id = send_id.lock().unwrap().clone();
                    if id.is_some() && answer["id"].as_str() == id.as_deref() {
                        let _ = host.shutdown(std::net::Shutdown::Both);
                        let _ = to_client.shutdown(std::net::Shutdown::Both);
                        break;
                    }
                    if writeln!(to_client, "{line}").is_err() {
                        break;
                    }
                }
            });
            Ok((ours, credential))
        }
    }

    #[test]
    fn a_message_whose_answer_was_cut_off_is_sent_again_and_shows_once() {
        let coordinator = std::cell::OnceCell::new();
        let served = serve(|host| {
            let project = host.create_project("Cut").unwrap();
            host.set_live(&project.id, false).unwrap();
            let main = (host.sessions().unwrap().into_iter())
                .find(|s| s.project_id == project.id)
                .unwrap();
            coordinator.set(main.id).unwrap();
        });
        let coordinator = coordinator.into_inner().unwrap();
        let launcher = CutAfterSend {
            endpoint: served.endpoint.clone(),
            cut: false,
        };
        let bridge = Bridge::connect(Box::new(launcher));
        let send = Command::Send {
            id: new_id(),
            sender: None,
            recipient: coordinator.clone(),
            body: "Stored before the drop".into(),
            attachments: vec![],
            uploads: vec![],
        };

        let answered = bridge.request(send).unwrap().recv_blocking().unwrap();

        assert!(answered.is_ok(), "{answered:?}");
        let page = bridge
            .ask_within(
                ask::messages(coordinator, None, 50),
                Duration::from_secs(10),
            )
            .unwrap();
        let shown = page.iter().filter(|m| m.body == "Stored before the drop");
        assert_eq!(shown.count(), 1);
        bridge.shutdown();
    }

    fn until(what: &str, done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_paired_host_that_does_not_answer_takes_no_change_but_reads_and_messages_wait() {
        let name = RemoteName {
            id: "studio".into(),
            name: "Studio Mac".into(),
        };
        let device = workspace_core::pairing::KeyPair::generate().unwrap();
        let nowhere = vec!["127.0.0.1:1".to_owned()];
        let launcher = RemoteLaunch::new(vec![4; 65], device, nowhere, Box::new(Vec::new));
        let away = Bridge::remote_host(name, launcher);

        let error = away.request(Command::SetCheckIns {
            child_turns: 7,
            human_turns: 11,
        });
        assert_eq!(error.unwrap_err(), "Waiting for Studio Mac to answer");
        assert!(away.request(Command::Snapshot).is_ok(), "a read waits");
        let send = Command::Send {
            id: new_id(),
            sender: None,
            recipient: "coordinator".into(),
            body: "Written while away".into(),
            attachments: vec![],
            uploads: vec![],
        };
        assert!(away.request(send).is_ok(), "a message waits");
        away.shutdown();
    }

    #[test]
    fn a_paired_host_is_driven_through_its_bridge_and_a_save_there_changes_only_it() {
        let here = serve(|_| {});
        let studio = Studio::start();
        let remote = studio.bridge();
        until("connected", || remote.connected());
        assert_eq!(remote.reached().level, Some(AccessLevel::Control));
        let snapshot = remote.ask_now(ask::snapshot()).unwrap();
        assert!(snapshot.projects.iter().any(|p| p.name == "Tide alerts"));

        let before = here.bridge.ask_now(ask::settings()).unwrap();
        let saved = remote
            .ask_within(ask::set_check_ins(7, 11), Duration::from_secs(10))
            .unwrap();
        assert_eq!(
            (saved.checkin_child_turns, saved.checkin_human_turns),
            (7, 11)
        );
        let there = remote.ask_now(ask::settings()).unwrap();
        assert_eq!(
            (there.checkin_child_turns, there.checkin_human_turns),
            (7, 11)
        );
        assert_eq!(
            here.bridge.ask_now(ask::settings()).unwrap(),
            before,
            "This Mac's own"
        );
        remote.shutdown();
    }

    #[test]
    fn a_host_restarted_during_a_drop_is_read_whole_again_and_nothing_repeats() {
        let mut studio = Studio::start();
        let remote = studio.bridge();
        until("connected", || remote.connected());
        let coordinator = studio.coordinator.clone();
        let sent = Command::Send {
            id: new_id(),
            sender: None,
            recipient: coordinator.clone(),
            body: "Before the drop".into(),
            attachments: vec![],
            uploads: vec![],
        };
        let stored = remote.request(sent).unwrap().recv_blocking().unwrap();
        stored.unwrap();
        let messages = || {
            let page = remote.ask_now(ask::messages(coordinator.clone(), None, 50));
            page.map(|page| page.iter().filter(|m| m.body == "Before the drop").count())
        };
        assert_eq!(messages(), Ok(1));
        let _ = remote.take_progress_resync();

        studio.restart();
        until("disconnected and back", || {
            remote.connected() && remote.take_progress_resync()
        });

        assert_eq!(messages(), Ok(1), "the message is not repeated");
        let snapshot = remote.ask_now(ask::snapshot()).unwrap();
        assert_eq!(snapshot.projects.len(), 1);
        remote.shutdown();
    }
}
