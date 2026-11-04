//! The Wiffletree Evaluator helper a full-desktop run drives the screen through (OpenSpec
//! change `ui-evaluators`). The host starts it for the run's turn through LaunchServices, so
//! macOS charges the helper's own grants and never Wiffletree's, and holds its control
//! connection for the turn. The helper reports the human's Esc and its menu bar Stop on that
//! connection, which stop the run at once, and quits when the connection closes.
use super::*;
use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixStream,
    sync::Mutex,
};

/// How long the helper may take to open its socket after LaunchServices starts it.
pub(crate) const HELPER_WAIT: Duration = Duration::from_secs(20);
/// How long the helper has to quit once its control connection closes, before it is killed.
const QUIT_GRACE: Duration = Duration::from_secs(3);
/// Why a run ends when the human stops it on the screen, or the helper goes.
pub const STOPPED_BY_ESCAPE: &str = "Stopped: you pressed Esc.";
pub const STOPPED_FROM_MENU: &str = "Stopped from the menu bar.";
pub const HELPER_QUIT: &str = "Stopped: Wiffletree Evaluator quit during the run.";

/// What a full-desktop run's turn starts: the helper, on a socket of the run's own, limited to
/// the apps the run names.
#[derive(Clone, Debug)]
pub struct DesktopLaunch {
    /// `Wiffletree Evaluator.app`.
    pub helper: PathBuf,
    /// Short, as macOS limits a socket's path to about 100 bytes.
    pub socket: PathBuf,
    pub run: String,
    /// Bundle ids; the helper refuses every other app.
    pub apps: Vec<String>,
}
impl DesktopLaunch {
    /// `open -n -g`: a new instance in the background, started by LaunchServices so that it is
    /// its own responsible process.
    pub(crate) fn command(&self) -> ProcessCommand {
        let mut cmd = ProcessCommand::new("/usr/bin/open");
        cmd.args(["-n", "-g"])
            .arg(&self.helper)
            .arg("--args")
            .arg("serve")
            .arg(&self.socket)
            .arg(&self.run)
            .args(&self.apps)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        cmd
    }
}

/// Names the helper app. `serve` sets it to the one bundled with the app; a development build
/// may name another.
pub const EVALUATOR_APP: &str = "WORKSPACE_EVALUATOR_APP";

/// The helper `EVALUATOR_APP` names, or the one bundled beside the running host.
pub fn helper_app() -> PathBuf {
    match std::env::var_os(EVALUATOR_APP) {
        Some(app) => app.into(),
        None => helper_beside(&std::env::current_exe().unwrap_or_default()),
    }
}

/// The helper in the bundle whose `Contents/MacOS` holds `executable`:
/// `Contents/Helpers/Wiffletree Evaluator.app`.
pub fn helper_beside(executable: &Path) -> PathBuf {
    let contents = (executable.parent().and_then(Path::parent)).unwrap_or(Path::new("/"));
    contents.join("Helpers/Wiffletree Evaluator.app")
}

/// A running helper. Dropping it closes the control connection, which quits the helper, and
/// kills it if it has not quit within the grace period.
pub(crate) struct RunningHelper {
    control: UnixStream,
    pid: i32,
    socket: PathBuf,
    /// Why the human stopped the run, until the turn takes it.
    stopped: Arc<Mutex<Option<String>>>,
    /// The host is closing the connection, so its end is no stop.
    closing: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl RunningHelper {
    /// Starts the helper and returns once it holds every grant the run needs. A stop the human
    /// makes on the screen sets `cancel` and is kept for `take_stop`.
    pub(crate) fn start(launch: &DesktopLaunch, cancel: &Arc<AtomicBool>) -> Result<Self> {
        let _ = fs::remove_file(&launch.socket);
        let opened = launch
            .command()
            .output()
            .context("Could not start Wiffletree Evaluator")?;
        ensure!(
            opened.status.success(),
            "macOS could not open Wiffletree Evaluator at {}: {}. A packaged Wiffletree has it in Contents/Helpers; a development build names it with WORKSPACE_EVALUATOR_APP (docs/DEVELOPMENT.md).",
            launch.helper.display(),
            String::from_utf8_lossy(&opened.stderr).trim()
        );
        Self::connect(&launch.socket, HELPER_WAIT, cancel)
    }

    /// Opens the run's control connection once the helper's socket answers, and checks the
    /// grants the helper reports for itself.
    pub(crate) fn connect(socket: &Path, wait: Duration, cancel: &Arc<AtomicBool>) -> Result<Self> {
        let deadline = Instant::now() + wait;
        let mut control = loop {
            check_running(cancel)?;
            match UnixStream::connect(socket) {
                Ok(stream) => break stream,
                Err(error) => ensure!(
                    Instant::now() < deadline,
                    "Wiffletree Evaluator did not open its socket within {:.0} s: {error}",
                    wait.as_secs_f64()
                ),
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        control.set_read_timeout(Some(Duration::from_secs(10)))?;
        writeln!(control, "{}", json!({"do": "control"}))?;
        let mut reader = BufReader::new(control.try_clone()?);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let hello: Value =
            serde_json::from_str(&line).context("Wiffletree Evaluator did not answer")?;
        ensure!(
            hello["ok"] == true,
            "Wiffletree Evaluator refused the run: {}",
            hello["error"].as_str().unwrap_or_default()
        );
        let hello = &hello["result"];
        // macOS refuses this once the helper has closed its end, which the reader then sees.
        let _ = control.set_read_timeout(None);
        let mut helper = Self {
            pid: hello["pid"].as_i64().unwrap_or_default() as i32,
            control,
            socket: socket.to_owned(),
            stopped: Default::default(),
            closing: Default::default(),
            reader: None,
        };
        // Dropping the helper here quits it: nothing touched the screen.
        if let Some(missing) = missing_grant(hello) {
            bail!("{missing}");
        }
        let (stopped, closing, cancel) = (
            helper.stopped.clone(),
            helper.closing.clone(),
            cancel.clone(),
        );
        helper.reader = Some(std::thread::spawn(move || {
            let mut line = String::new();
            let reason = loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(read) if read > 0 => {
                        let event: Value = serde_json::from_str(&line).unwrap_or_default();
                        match event["event"].as_str() {
                            Some("escape") => break STOPPED_BY_ESCAPE,
                            Some("stop") => break STOPPED_FROM_MENU,
                            _ => continue,
                        }
                    }
                    _ => break HELPER_QUIT,
                }
            };
            if !closing.load(Ordering::SeqCst) {
                *stopped.lock().unwrap() = Some(reason.to_owned());
                cancel.store(true, Ordering::Relaxed);
            }
        }));
        Ok(helper)
    }

    /// Why the human stopped the run on the screen, once.
    pub(crate) fn take_stop(&self) -> Option<String> {
        self.stopped.lock().unwrap().take()
    }
}
impl Drop for RunningHelper {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let deadline = Instant::now() + QUIT_GRACE;
        while self.pid > 1 && processes::read(self.pid).is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if self.pid > 1 && processes::read(self.pid).is_some() {
            // SAFETY: a signal to the helper this turn started, which outlived its grace.
            unsafe { libc::kill(self.pid, libc::SIGKILL) };
        }
        let _ = fs::remove_file(&self.socket);
    }
}

/// What the helper lacks for a run, and where the human gives it: the run is refused before
/// anything touches the screen.
pub(crate) fn missing_grant(hello: &Value) -> Option<String> {
    let pane = |grant: &str, pane: &str| {
        format!(
            "Wiffletree Evaluator needs {grant}. Open System Settings › Privacy & Security › {pane}, turn on Wiffletree Evaluator, then start the run again."
        )
    };
    if hello["screen_recording"] != true {
        return Some(pane("Screen Recording", "Screen & System Audio Recording"));
    }
    if hello["accessibility"] != true {
        return Some(pane("Accessibility", "Accessibility"));
    }
    if hello["escape"] != true {
        return Some(
            "Wiffletree Evaluator could not listen for Esc, so you could not stop the run from the keyboard. Check its Accessibility grant in System Settings › Privacy & Security › Accessibility.".into(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn launch() -> DesktopLaunch {
        DesktopLaunch {
            helper: "/Applications/Wiffletree.app/Contents/Helpers/Wiffletree Evaluator.app".into(),
            socket: "/tmp/wiffletree-run-1-desktop.sock".into(),
            run: "run-1".into(),
            apps: vec!["com.apple.calculator".into(), "com.apple.TextEdit".into()],
        }
    }

    #[test]
    fn launch_services_starts_the_helper_with_the_runs_socket_and_apps() {
        let cmd = launch().command();
        assert_eq!(cmd.get_program(), "/usr/bin/open");
        let args: Vec<String> = (cmd.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-n",
                "-g",
                "/Applications/Wiffletree.app/Contents/Helpers/Wiffletree Evaluator.app",
                "--args",
                "serve",
                "/tmp/wiffletree-run-1-desktop.sock",
                "run-1",
                "com.apple.calculator",
                "com.apple.TextEdit"
            ]
        );
    }

    #[test]
    fn the_helper_is_found_in_the_bundle_of_the_app_or_the_host() {
        for executable in [
            "/Applications/Wiffletree.app/Contents/MacOS/wiffletree",
            "/Applications/Wiffletree.app/Contents/MacOS/wiffletree-host",
        ] {
            assert_eq!(
                helper_beside(Path::new(executable)),
                Path::new("/Applications/Wiffletree.app/Contents/Helpers/Wiffletree Evaluator.app")
            );
        }
    }

    #[test]
    fn a_missing_grant_names_the_pane_to_open() {
        let hello = |screen: bool, access: bool, escape: bool| json!({"pid":1,"screen_recording":screen,"accessibility":access,"escape":escape});
        assert_eq!(missing_grant(&hello(true, true, true)), None);
        let screen = missing_grant(&hello(false, true, true)).unwrap();
        assert!(
            screen.contains("Screen & System Audio Recording"),
            "{screen}"
        );
        let access = missing_grant(&hello(true, false, false)).unwrap();
        assert!(access.contains("› Accessibility,"), "{access}");
        assert!(
            missing_grant(&hello(true, true, false))
                .unwrap()
                .contains("Esc")
        );
    }

    /// A stand-in helper: it answers the control connection with `hello`, then sends `events`
    /// and reports whether the host closed the connection.
    fn helper(socket: &Path, hello: Value, events: Vec<Value>) -> std::sync::mpsc::Receiver<()> {
        let listener = UnixListener::bind(socket).unwrap();
        let (closed, closed_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap(),
                json!({"do":"control"})
            );
            writeln!(stream, "{}", json!({"ok":true,"result":hello})).unwrap();
            for event in events {
                writeln!(stream, "{event}").unwrap();
            }
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                let _ = closed.send(());
            }
        });
        closed_rx
    }
    fn granted(pid: u32) -> Value {
        json!({"pid":pid,"screen_recording":true,"accessibility":true,"escape":true})
    }

    #[test]
    fn esc_stops_the_turn_at_once_and_says_so() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("h.sock");
        let closed = helper(&socket, granted(0), vec![json!({"event":"escape"})]);
        let cancel = Arc::new(AtomicBool::new(false));
        let running = RunningHelper::connect(&socket, Duration::from_secs(5), &cancel).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cancel.load(Ordering::Relaxed) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(running.take_stop().as_deref(), Some(STOPPED_BY_ESCAPE));
        assert_eq!(running.take_stop(), None);
        drop(running);
        closed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!socket.exists());
    }

    #[test]
    fn the_menu_bars_stop_and_a_helper_that_quits_stop_the_turn() {
        for (events, reason) in [
            (vec![json!({"event":"stop"})], STOPPED_FROM_MENU),
            (vec![], HELPER_QUIT),
        ] {
            let folder = tempfile::tempdir().unwrap();
            let socket = folder.path().join("h.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            // Answers, sends its events, then quits.
            std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                writeln!(stream, "{}", json!({"ok":true,"result":granted(0)})).unwrap();
                for event in events {
                    writeln!(stream, "{event}").unwrap();
                }
            });
            let cancel = Arc::new(AtomicBool::new(false));
            let running = RunningHelper::connect(&socket, Duration::from_secs(5), &cancel).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while running.stopped.lock().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(cancel.load(Ordering::Relaxed), "{reason}");
            assert_eq!(running.take_stop().as_deref(), Some(reason));
        }
    }

    /// macOS only: elsewhere `processes::read` sees no process to kill.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_turn_that_ends_closes_the_helper_without_stopping_anything_and_kills_one_that_stays() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("h.sock");
        // A process standing in for a helper that ignores its closed connection.
        let mut stays = std::process::Command::new("/bin/sleep")
            .arg("300")
            .spawn()
            .unwrap();
        let closed = helper(&socket, granted(stays.id()), vec![]);
        let cancel = Arc::new(AtomicBool::new(false));
        let running = RunningHelper::connect(&socket, Duration::from_secs(5), &cancel).unwrap();
        drop(running);
        closed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            !cancel.load(Ordering::Relaxed),
            "the turn's own end is no stop"
        );
        let status = stays.wait().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGKILL)
        );
    }

    #[test]
    fn a_helper_without_its_grants_is_closed_before_the_run_starts() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("h.sock");
        let hello = json!({"pid":0,"screen_recording":false,"accessibility":true,"escape":true});
        let closed = helper(&socket, hello, vec![]);
        let cancel = Arc::new(AtomicBool::new(false));
        let refused = RunningHelper::connect(&socket, Duration::from_secs(5), &cancel)
            .err()
            .unwrap();
        assert!(
            refused.to_string().contains("Screen Recording"),
            "{refused}"
        );
        closed.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn a_helper_that_never_opens_its_socket_fails_the_turn_and_a_stop_ends_the_wait() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("never.sock");
        let cancel = Arc::new(AtomicBool::new(false));
        let error = RunningHelper::connect(&socket, Duration::from_millis(300), &cancel)
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("did not open its socket"),
            "{error}"
        );
        cancel.store(true, Ordering::Relaxed);
        let error = RunningHelper::connect(&socket, Duration::from_secs(20), &cancel)
            .err()
            .unwrap();
        assert!(error.is::<Cancelled>(), "{error:#}");
    }
}
