//! The copy of the app under test that an app run drives (OpenSpec change `ui-evaluators`).
//! The host starts it for the run's turn on a socket of the run's own, from a clean
//! environment, and stops it and everything it started when the turn ends, however it ends.
use super::*;

/// How long a test copy may take to answer on its socket with a window, as a debug build
/// opening a fresh data folder can.
pub(crate) const SOCKET_WAIT: Duration = Duration::from_secs(90);
/// How long the copy and what it started have to exit after SIGTERM before they are killed. Its
/// own host stops its agents on SIGTERM.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// What an app run's turn starts and drives.
#[derive(Clone, Debug)]
pub struct AppLaunch {
    pub target: EvaluationTarget,
    /// Short, as macOS limits a socket's path to about 100 bytes.
    pub socket: PathBuf,
    /// The copy's output, for the human; the evaluator never sees it.
    pub log: PathBuf,
}
impl AppLaunch {
    /// The copy's command: its arguments and the run's socket, in an environment holding only
    /// `evaluator::ENVIRONMENT`, what the target gives, with its `PATH` first, and the switch
    /// that keeps its host off the login shell's `PATH`.
    pub(crate) fn command(&self) -> ProcessCommand {
        let mut cmd = ProcessCommand::new(&self.target.executable);
        cmd.args(&self.target.arguments)
            .arg("--automation-socket")
            .arg(&self.socket);
        evaluator::clear_environment(&mut cmd);
        for (name, value) in &self.target.environment {
            match name.as_str() {
                "PATH" => {
                    let host = std::env::var_os("PATH").unwrap_or_default();
                    let mut path = std::ffi::OsString::from(value);
                    if !host.is_empty() {
                        path.push(":");
                        path.push(host);
                    }
                    cmd.env("PATH", path)
                }
                _ => cmd.env(name, value),
            };
        }
        cmd.env(crate::login_env::SKIP, "1")
            .stdin(Stdio::null())
            .process_group(0)
            .own_descriptors_only();
        cmd
    }
}

/// A started test copy. Dropping it stops the copy and everything it started.
pub(crate) struct RunningTarget {
    child: Child,
    socket: PathBuf,
}
impl RunningTarget {
    /// Starts the copy and returns once its socket answers with a window. A copy that exits
    /// first, or does not answer within `wait`, fails the turn with the reason.
    pub(crate) fn start(launch: &AppLaunch, wait: Duration, cancel: &AtomicBool) -> Result<Self> {
        if let Some(folder) = launch.log.parent() {
            fs::create_dir_all(folder)?;
        }
        let log = fs::File::create(&launch.log)?;
        let _ = fs::remove_file(&launch.socket);
        let child = launch
            .command()
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .with_context(|| {
                format!(
                    "Could not start the app under test {}",
                    launch.target.executable
                )
            })?;
        let mut running = Self {
            child,
            socket: launch.socket.clone(),
        };
        running.wait_for_window(wait, cancel)?;
        Ok(running)
    }

    fn wait_for_window(&mut self, wait: Duration, cancel: &AtomicBool) -> Result<()> {
        let deadline = Instant::now() + wait;
        loop {
            check_running(cancel)?;
            if let Some(status) = self.child.try_wait()? {
                bail!("The app under test exited ({status}) before its automation socket answered");
            }
            let error = match crate::app_driver::send(&self.socket, &json!({"do": "window"})) {
                Ok(_) => return Ok(()),
                Err(error) => error,
            };
            ensure!(
                Instant::now() < deadline,
                "The app under test did not answer on its automation socket within {:.1} s: {error:#}",
                wait.as_secs_f64()
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// SIGTERM to the copy's group and the group of every process it started, then SIGKILL to
    /// whatever is left after the grace period.
    fn stop(&mut self) {
        let leader = self.child.id() as i32;
        let mut groups: Vec<i32> = (processes::descendants(leader).into_iter())
            .map(|process| process.group)
            .filter(|group| *group > 1)
            .collect();
        groups.push(leader);
        groups.sort_unstable();
        groups.dedup();
        let signal = |signal| {
            for group in &groups {
                // SAFETY: killpg only sends a signal to groups the copy's tree is in.
                unsafe { libc::killpg(*group, signal) };
            }
        };
        let mut settle = |grace: Duration| {
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                let _ = self.child.try_wait();
                if !groups.iter().any(|group| processes::group_runs(*group)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        signal(libc::SIGTERM);
        settle(STOP_GRACE);
        signal(libc::SIGKILL);
        settle(Duration::from_secs(1));
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.socket);
    }
}
impl Drop for RunningTarget {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn launch(executable: &str, arguments: &[&str], folder: &Path) -> AppLaunch {
        AppLaunch {
            target: EvaluationTarget {
                executable: executable.into(),
                arguments: arguments.iter().map(|arg| (*arg).into()).collect(),
                environment: [
                    ("PATH".to_owned(), "/stubs/bin".to_owned()),
                    ("WORKSPACE_CLAUDE_BIN".into(), "/stubs/bin/claude".into()),
                    ("GH_CONFIG_DIR".into(), "/tmp/eval-gh".into()),
                ]
                .into(),
            },
            socket: folder.join("t.sock"),
            log: folder.join("logs/target.log"),
        }
    }

    #[test]
    fn the_copy_gets_its_socket_the_given_path_first_and_no_session_variable() {
        let folder = Path::new("/runs/r");
        let cmd = launch(
            "/build/workspace-desktop",
            &["--data-dir", "/tmp/ux"],
            folder,
        )
        .command();
        let args: Vec<String> = (cmd.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--data-dir",
                "/tmp/ux",
                "--automation-socket",
                "/runs/r/t.sock"
            ]
        );
        let env: std::collections::HashMap<String, Option<String>> = (cmd.get_envs())
            .map(|(name, value)| {
                let value = value.map(|v| v.to_string_lossy().into_owned());
                (name.to_string_lossy().into_owned(), value)
            })
            .collect();
        assert!(env["PATH"].as_deref().unwrap().starts_with("/stubs/bin:"));
        assert_eq!(env[crate::login_env::SKIP].as_deref(), Some("1"));
        assert_eq!(
            env["WORKSPACE_CLAUDE_BIN"].as_deref(),
            Some("/stubs/bin/claude")
        );
        assert_eq!(env["GH_CONFIG_DIR"].as_deref(), Some("/tmp/eval-gh"));
        // The environment is cleared, so only what is set reaches the copy.
        let set: Vec<&String> = (env.iter())
            .filter(|(_, value)| value.is_some())
            .map(|(name, _)| name)
            .collect();
        for name in set {
            assert!(
                ![
                    "WORKSPACE_TOKEN",
                    "WORKSPACE_SOCKET",
                    "GH_TOKEN",
                    "GITHUB_TOKEN"
                ]
                .contains(&name.as_str()),
                "{name}"
            );
        }
    }

    #[test]
    fn a_copy_that_never_answers_fails_the_turn_and_is_stopped() {
        let folder = tempfile::tempdir().unwrap();
        // Ignores its arguments and never opens the socket; its own child must stop with it.
        let script = folder.path().join("never.sh");
        fs::write(
            &script,
            "#!/bin/sh\nsleep 300 &\necho $! > \"$(dirname \"$0\")/child\"\nwait\n",
        )
        .unwrap();
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let launch = launch(script.to_str().unwrap(), &[], folder.path());
        let cancel = AtomicBool::new(false);
        let error = RunningTarget::start(&launch, Duration::from_millis(600), &cancel)
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("did not answer on its automation socket"),
            "{error:#}"
        );
        let child: i32 = fs::read_to_string(folder.path().join("child"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            processes::read(child).is_none(),
            "the copy's child still runs"
        );
    }

    #[test]
    fn a_stop_while_the_copy_starts_stops_it() {
        let folder = tempfile::tempdir().unwrap();
        let script = folder.path().join("slow.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho $$ > \"$(dirname \"$0\")/pid\"\nexec sleep 300\n",
        )
        .unwrap();
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let launch = launch(script.to_str().unwrap(), &[], folder.path());
        let cancel = AtomicBool::new(false);
        let error = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !folder.path().join("pid").exists() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                cancel.store(true, Ordering::Relaxed);
            });
            RunningTarget::start(&launch, Duration::from_secs(20), &cancel)
                .err()
                .unwrap()
        });
        assert!(error.is::<Cancelled>(), "{error:#}");
        let pid: i32 = fs::read_to_string(folder.path().join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(processes::read(pid).is_none(), "the copy still runs");
    }

    #[test]
    fn a_copy_that_exits_fails_the_turn_with_its_status() {
        let folder = tempfile::tempdir().unwrap();
        let launch = launch("/usr/bin/false", &[], folder.path());
        let cancel = AtomicBool::new(false);
        let error = RunningTarget::start(&launch, Duration::from_secs(5), &cancel)
            .err()
            .unwrap();
        assert!(error.to_string().contains("exited"), "{error:#}");
    }

    #[test]
    fn the_copy_and_what_it_started_stop_when_the_turn_ends() {
        let folder = tempfile::tempdir().unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_under_test.py");
        let agent_file = folder.path().join("agent");
        // Python is found on the host's PATH, after the given prefix.
        let mut launch = launch(script.to_str().unwrap(), &[], folder.path());
        let agent_path = agent_file.to_string_lossy().into_owned();
        launch.target.environment.insert("AGENT".into(), agent_path);
        let cancel = AtomicBool::new(false);
        let running = RunningTarget::start(&launch, Duration::from_secs(20), &cancel).unwrap();
        let leader = running.child.id() as i32;
        let agent: i32 = fs::read_to_string(&agent_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(processes::read(agent).is_some());
        drop(running);
        assert!(processes::read(leader).is_none());
        assert!(
            processes::read(agent).is_none(),
            "the agent it started still runs"
        );
        assert!(!launch.socket.exists());
    }
}
