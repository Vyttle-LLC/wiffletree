//! The app's link to its host, over `host.sock`: a launchd job for a signed app on its default
//! data folder, otherwise the app's own child host. Views send commands through it and are woken
//! by the host's events.
use async_channel::{Receiver, Sender};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use workspace_core::{ask::Ask, *};

/// How long the app waits for a reply it needs before it can go on, such as which file a link
/// names.
const BLOCKING_REPLY: Duration = Duration::from_secs(2);

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
    /// Set by `resync`: the host may have restarted, and its progress revisions with it.
    progress_resynced: Arc<AtomicBool>,
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
            progress_resynced: Arc::default(),
        }
    }
    fn wake(&self, kind: &EventKind) {
        let woken = match kind {
            EventKind::State => vec![&self.state, &self.menu_bar],
            EventKind::Steps { .. } => vec![&self.steps],
            EventKind::Progress => vec![&self.progress],
            EventKind::Usage => vec![&self.usage],
            EventKind::Resync => {
                self.progress_resynced.store(true, Ordering::Relaxed);
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

#[derive(Clone)]
pub(crate) struct Bridge {
    client: Result<HostClient, String>,
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
        let client = Client {
            name: "Wiffletree".into(),
            kind: "desktop".into(),
        };
        let client = HostClient::connect(launcher, client, Box::new(move |kind| woken.wake(kind)));
        Self {
            client: client.map_err(|e| format!("The host did not start: {e}")),
            wakes,
            outlives_app,
        }
    }
    fn unavailable(error: String) -> Self {
        Self {
            client: Err(error),
            wakes: Wakes::new(),
            outlives_app: false,
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
        self.client.as_ref().map_err(Clone::clone)?.request(command)
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
    /// Sends `ask` and waits briefly for its answer, for a reply the app needs before it can go
    /// on. Fails at once while the host is out of reach.
    pub(crate) fn ask_now<R: DeserializeOwned>(&self, ask: Ask<R>) -> Result<R, String> {
        let client = self.client.as_ref().map_err(Clone::clone)?;
        let reply = client.request_blocking(ask.command().clone(), BLOCKING_REPLY)?;
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
    /// Wakes when some session's work steps changed; fetch them with `Command::Steps`.
    pub(crate) fn step_changes(&self) -> Option<Receiver<()>> {
        self.wake(&self.wakes.steps)
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
    }
    impl Drop for Served {
        fn drop(&mut self) {
            self.bridge.shutdown();
            self.service.shutdown();
        }
    }
    struct Running(Endpoint);
    impl Launch for Running {
        fn endpoint(&mut self) -> std::io::Result<Endpoint> {
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
}
