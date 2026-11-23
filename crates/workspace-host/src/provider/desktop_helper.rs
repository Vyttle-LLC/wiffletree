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

/// Names another helper for runs to start, such as a development build's; otherwise they start
/// the copy `serve` installs (`install_helper`).
pub const EVALUATOR_APP: &str = "WORKSPACE_EVALUATOR_APP";
const HELPER_NAME: &str = "Wiffletree Evaluator.app";

/// The helper `EVALUATOR_APP` names, or else the installed copy's usual place under `data`.
pub fn helper_app(data: &Path) -> PathBuf {
    match std::env::var_os(EVALUATOR_APP) {
        Some(app) => app.into(),
        None => installed_helper(data),
    }
}

/// The helper Wiffletree ships in `bundle`, `Wiffletree.app`.
pub fn bundled_helper(bundle: &Path) -> PathBuf {
    bundle.join("Contents/Helpers").join(HELPER_NAME)
}

/// The app bundle whose `Contents/MacOS` holds `executable`.
pub fn bundle_of(executable: &Path) -> Option<&Path> {
    executable.ancestors().nth(3)
}

fn installed_helper(data: &Path) -> PathBuf {
    data.join("evaluator").join(HELPER_NAME)
}

/// Installs the helper shipped in the app as `<data>/evaluator/Wiffletree Evaluator.app`, and
/// returns that path. macOS charges Screen Recording to the outermost bundle, so a helper
/// launched inside `Wiffletree.app` would be charged as Wiffletree itself; the copy is its own
/// outermost bundle. Its grants stay with its bundle ID and signature, so replacing it with an
/// update signed by the same team keeps them. An unchanged copy is left in place.
pub fn install_helper(bundled: &Path, data: &Path) -> Result<PathBuf> {
    let installed = installed_helper(data);
    if same_helper(bundled, &installed) {
        return Ok(installed);
    }
    let folder = installed.parent().context("No evaluator folder")?;
    fs::create_dir_all(folder)?;
    ensure!(
        !fs::symlink_metadata(folder)?.file_type().is_symlink(),
        "{} is a link",
        folder.display()
    );
    let staged = folder.join(format!(".{HELPER_NAME}.new"));
    let _ = fs::remove_dir_all(&staged);
    // `ditto` keeps the bundle's files, modes and links exactly, so its signature holds.
    let copied = ProcessCommand::new("/usr/bin/ditto")
        .arg(bundled)
        .arg(&staged)
        .output()
        .context("Could not run ditto")?;
    ensure!(
        copied.status.success(),
        "Could not copy Wiffletree Evaluator: {}",
        String::from_utf8_lossy(&copied.stderr).trim()
    );
    let _ = fs::remove_dir_all(&installed);
    fs::rename(&staged, &installed)?;
    Ok(installed)
}

/// Whether the installed copy is the bundled helper: the same executable, `Info.plist` and
/// sealed resources.
fn same_helper(bundled: &Path, installed: &Path) -> bool {
    [
        "MacOS/wiffletree-evaluator",
        "Info.plist",
        "_CodeSignature/CodeResources",
    ]
    .iter()
    .all(|file| {
        let (a, b) = (
            bundled.join("Contents").join(file),
            installed.join("Contents").join(file),
        );
        matches!((fs::read(a), fs::read(b)), (Ok(a), Ok(b)) if a == b)
    })
}

/// A running helper. Dropping it closes the control connection, which quits the helper, and
/// kills it if it has not quit within the grace period.
pub(crate) struct RunningHelper {
    control: UnixStream,
    /// The helper's process id and its start time when it answered: LaunchServices started
    /// it, so the host cannot hold it unreaped, and a reused id names another process.
    pid: i32,
    started: Option<i64>,
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
            "macOS could not open Wiffletree Evaluator at {}: {}. Wiffletree installs it there from its app when the host starts; a development build names it with WORKSPACE_EVALUATOR_APP (docs/DEVELOPMENT.md).",
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
        let pid = hello["pid"].as_i64().unwrap_or_default() as i32;
        let mut helper = Self {
            pid,
            started: (pid > 1).then(|| processes::process_start(pid)).flatten(),
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
        // The helper still runs only while its id names the process that answered.
        let runs = || self.started.is_some() && processes::process_start(self.pid) == self.started;
        let deadline = Instant::now() + QUIT_GRACE;
        while runs() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if runs() {
            // SAFETY: a signal to the helper this turn started, confirmed by its start time,
            // which outlived its grace.
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
    fn the_helper_ships_in_the_bundle_of_the_app_or_the_host() {
        let host = Path::new("/Applications/Wiffletree.app/Contents/MacOS/wiffletree-host");
        let bundle = bundle_of(host).unwrap();
        assert_eq!(bundle, Path::new("/Applications/Wiffletree.app"));
        assert_eq!(
            bundled_helper(bundle),
            Path::new("/Applications/Wiffletree.app/Contents/Helpers/Wiffletree Evaluator.app")
        );
    }

    /// A stand-in bundle with the three files `same_helper` compares.
    fn fake_helper(at: &Path, binary: &[u8]) {
        for (file, bytes) in [
            ("MacOS/wiffletree-evaluator", binary),
            ("Info.plist", b"plist".as_slice()),
            ("_CodeSignature/CodeResources", b"sealed".as_slice()),
        ] {
            let path = at.join("Contents").join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }

    /// macOS only: it copies with `ditto`.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_helper_is_installed_outside_the_app_bundle_and_replaced_only_when_it_changed() {
        let root = tempfile::tempdir().unwrap();
        let bundled = root
            .path()
            .join("Wiffletree.app/Contents/Helpers/Wiffletree Evaluator.app");
        let data = root.path().join("data");
        fake_helper(&bundled, b"v1");
        let installed = install_helper(&bundled, &data).unwrap();
        assert_eq!(installed, data.join("evaluator/Wiffletree Evaluator.app"));
        assert!(!installed.starts_with(root.path().join("Wiffletree.app")));
        let binary = installed.join("Contents/MacOS/wiffletree-evaluator");
        assert_eq!(fs::read(&binary).unwrap(), b"v1");
        // An unchanged copy is kept as it is.
        let kept = fs::metadata(&binary).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        install_helper(&bundled, &data).unwrap();
        assert_eq!(fs::metadata(&binary).unwrap().modified().unwrap(), kept);
        // An update replaces it whole, leaving nothing staged.
        fake_helper(&bundled, b"v2");
        install_helper(&bundled, &data).unwrap();
        assert_eq!(fs::read(&binary).unwrap(), b"v2");
        assert_eq!(fs::read_dir(data.join("evaluator")).unwrap().count(), 1);
        assert_eq!(helper_app(&data), installed);
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

    /// macOS only, as above.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_process_that_took_the_helpers_id_is_never_killed() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("h.sock");
        let mut other = std::process::Command::new("/bin/sleep")
            .arg("300")
            .spawn()
            .unwrap();
        let closed = helper(&socket, granted(other.id()), vec![]);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut running = RunningHelper::connect(&socket, Duration::from_secs(5), &cancel).unwrap();
        // As if the helper had exited and its id now named a process started later.
        running.started = running.started.map(|started| started - 1);
        drop(running);
        closed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            other.try_wait().unwrap(),
            None,
            "the other process was killed"
        );
        other.kill().unwrap();
        other.wait().unwrap();
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
