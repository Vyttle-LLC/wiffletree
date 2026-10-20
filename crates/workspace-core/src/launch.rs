//! How the app starts or finds its host, and the client credential it connects with.
use crate::{OwnDescriptors as _, SOCKET_FILE, build_version, host_copy};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// The Keychain item holding a launchd host's client credential; its account is the data folder,
/// so each store has its own.
pub const CREDENTIAL_SERVICE: &str = "com.vyttle.wiffletree.client-credential";
/// How long a host that is starting may take to listen.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// A fresh client credential: 256 random bits, hex-encoded.
pub fn new_credential() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Where the app's host runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostLaunch {
    /// A launchd job, which outlives the app.
    Launchd,
    /// The app's own child, which stops when the app does.
    Child,
}
impl HostLaunch {
    /// Only an app signed with a Developer ID and opened on its default data folder uses
    /// launchd: an unsigned build cannot be named in the credential's Keychain access list, and
    /// a `--data-dir` store must never leave a background job behind.
    pub fn choose(developer_id_signed: bool, default_data_folder: bool) -> Self {
        if developer_id_signed && default_data_folder {
            Self::Launchd
        } else {
            Self::Child
        }
    }
}

/// Whether `binary` carries a Developer ID signature, as `codesign` reports it. Ad-hoc and
/// unsigned builds do not.
pub fn developer_id_signed(binary: &Path) -> bool {
    Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=2"])
        .arg(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .any(|line| line.starts_with("Authority=Developer ID Application:"))
        })
}

/// A host the app started as its child. Its credential exists only in the two processes: it is
/// written once to the child's standard input, never to its environment or a file.
pub struct ChildHost {
    child: Child,
    stdin: Option<ChildStdin>,
    credential: String,
}
impl ChildHost {
    /// Starts `command`, a `serve --child`, passing a fresh credential as the first line of its
    /// standard input and keeping the pipe open: closing it stops the host.
    pub fn start_command(mut command: Command) -> io::Result<Self> {
        let credential = new_credential()?;
        let mut child = command
            .stdin(Stdio::piped())
            .own_descriptors_only()
            .spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        if let Err(error) = writeln!(stdin, "{credential}").and_then(|_| stdin.flush()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(Self {
            child,
            stdin: Some(stdin),
            credential,
        })
    }
    pub fn credential(&self) -> &str {
        &self.credential
    }
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    /// Whether the host has exited, without waiting.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
    /// Closes the host's standard input, which runs its shutdown, and waits for it to exit.
    pub fn stop(mut self) -> io::Result<ExitStatus> {
        self.close()
    }
    fn close(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        self.child.wait()
    }
}
impl Drop for ChildHost {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// What a client needs to reach a host: its socket and, for the app, the client credential.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub socket: PathBuf,
    pub credential: Option<String>,
}

/// Finds the app's host, starting it when none answers. A client asks again before each
/// connection, so a host that stopped is started again.
pub trait Launch: Send {
    /// Waiting for a host that is starting ends early once `stopped` is set, so a client that
    /// stops is not held up and can stop the host just started.
    fn endpoint(&mut self, stopped: &AtomicBool) -> io::Result<Endpoint>;
    /// A new connection to the host for its protocol, with the credential to say `Hello` with:
    /// the socket `endpoint` names, or for a remote host, its relayed stream.
    fn connect(
        &mut self,
        stopped: &AtomicBool,
    ) -> io::Result<(std::os::unix::net::UnixStream, Option<String>)> {
        let endpoint = self.endpoint(stopped)?;
        let stream = std::os::unix::net::UnixStream::connect(&endpoint.socket)?;
        Ok((stream, endpoint.credential))
    }
    /// Stops the host if it is the app's own child; a launchd host keeps running.
    fn stop(&mut self) {}
    /// The running child host, when the host is the app's own.
    fn child(&mut self) -> Option<&mut ChildHost> {
        None
    }
    /// The host keeps running after the app exits, so quitting may leave agents working and an
    /// update finds an older host still running.
    fn outlives_app(&self) -> bool {
        false
    }
}

/// The launcher `choice` names, for the host binary `binary`, the store `data` and the app
/// bundle `app`, which a launchd host's credential trusts.
pub fn launcher(choice: HostLaunch, binary: &Path, data: &Path, app: &Path) -> Box<dyn Launch> {
    match choice {
        HostLaunch::Child => Box::new(ChildLaunch::new(binary, data)),
        HostLaunch::Launchd => Box::new(LaunchdJob::new(binary, data, app)),
    }
}

/// A child host, started again with a fresh credential whenever it has exited.
pub struct ChildLaunch {
    binary: PathBuf,
    data: PathBuf,
    environment: Vec<(String, std::ffi::OsString)>,
    host: Option<ChildHost>,
}
impl ChildLaunch {
    pub fn new(binary: &Path, data: &Path) -> Self {
        Self {
            binary: binary.into(),
            data: data.into(),
            environment: vec![],
            host: None,
        }
    }
    /// Sets an environment variable for each host it starts, as tests do to keep providers out.
    pub fn env(mut self, key: &str, value: impl Into<std::ffi::OsString>) -> Self {
        self.environment.push((key.into(), value.into()));
        self
    }
}
impl Launch for ChildLaunch {
    fn endpoint(&mut self, stopped: &AtomicBool) -> io::Result<Endpoint> {
        let running = match &mut self.host {
            Some(host) => host.try_wait()?.is_none(),
            None => false,
        };
        if !running {
            self.host = None;
            let mut command = Command::new(&self.binary);
            command
                .arg("serve")
                .arg("--data-dir")
                .arg(&self.data)
                .arg("--child")
                .envs(self.environment.iter().map(|(k, v)| (k, v)));
            self.host = Some(ChildHost::start_command(command)?);
        }
        let host = self.host.as_mut().expect("started above");
        let socket = self.data.join(SOCKET_FILE);
        wait_for_socket(&socket, stopped, || Ok(host.try_wait()?.is_none()))?;
        Ok(Endpoint {
            socket,
            credential: Some(host.credential().into()),
        })
    }
    fn stop(&mut self) {
        if let Some(host) = self.host.take() {
            let _ = host.stop();
        }
    }
    fn child(&mut self) -> Option<&mut ChildHost> {
        self.host.as_mut()
    }
}

/// A signed app's host on its default data folder: a launchd job loaded from a plist in
/// `<data folder>/host/`, never from `~/Library/LaunchAgents`, so it does not start at login.
/// It runs this version's copy of `binary`, the bundle's host, so an update of the bundle never
/// replaces a running host.
pub struct LaunchdJob {
    binary: PathBuf,
    data: PathBuf,
    app: PathBuf,
}
impl LaunchdJob {
    fn new(binary: &Path, data: &Path, app: &Path) -> Self {
        Self {
            binary: binary.into(),
            data: data.into(),
            app: app.into(),
        }
    }
    /// A beta keeps its own host, as it keeps its own store.
    pub fn label() -> &'static str {
        if crate::data_dir::is_beta() {
            "com.vyttle.wiffletree.beta.host"
        } else {
            "com.vyttle.wiffletree.host"
        }
    }
    fn plist_path(&self) -> PathBuf {
        self.data
            .join("host")
            .join(format!("{}.plist", Self::label()))
    }
    /// The copy of the host the job runs.
    fn program(&self) -> PathBuf {
        host_copy::path(&self.data, build_version())
    }
    /// The job: `serve` for the data folder from this version's copy, trusting the app,
    /// restarted only after an unsuccessful exit (which also starts it when loaded), logging to
    /// `logs/host.log`.
    fn plist(&self) -> String {
        let program = self.program();
        let arguments = [
            program.as_path(),
            Path::new("serve"),
            Path::new("--data-dir"),
            self.data.as_path(),
            Path::new("--app"),
            self.app.as_path(),
        ];
        let arguments: String = arguments
            .iter()
            .map(|argument| format!("\t\t<string>{}</string>\n", xml(argument)))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
{arguments}	</array>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
            label = Self::label(),
            log = xml(&self.data.join("logs/host.log")),
        )
    }
    fn target() -> String {
        // SAFETY: getuid cannot fail.
        format!("gui/{}", unsafe { libc::getuid() })
    }
    fn service() -> String {
        format!("{}/{}", Self::target(), Self::label())
    }
    fn running() -> bool {
        Command::new("/bin/launchctl")
            .args(["print", &Self::service()])
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("state = running")
            })
    }
    /// Copies the bundle's host, writes the plist and bootstraps it, after unloading a job whose
    /// host has exited.
    fn load(&self) -> io::Result<()> {
        host_copy::install(&self.binary, &self.data, build_version())?;
        let plist = self.plist_path();
        fs::create_dir_all(plist.parent().expect("in host/"))?;
        fs::create_dir_all(self.data.join("logs"))?;
        fs::write(&plist, self.plist())?;
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &Self::service()])
            .output();
        let output = Command::new("/bin/launchctl")
            .args(["bootstrap", &Self::target()])
            .arg(&plist)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "launchctl could not load the host: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }
}
impl Launch for LaunchdJob {
    fn endpoint(&mut self, stopped: &AtomicBool) -> io::Result<Endpoint> {
        let socket = self.data.join(SOCKET_FILE);
        if UnixStream::connect(&socket).is_err() && !Self::running() {
            self.load()?;
        }
        wait_for_socket(&socket, stopped, || Ok(true))?;
        Ok(Endpoint {
            socket,
            credential: Some(keychain_credential(&self.data)?),
        })
    }
    fn outlives_app(&self) -> bool {
        true
    }
}

/// The launchd host's credential, from the Keychain item the host created at its first start.
#[cfg(target_os = "macos")]
fn keychain_credential(data: &Path) -> io::Result<String> {
    use security_framework::os::macos::passwords::find_generic_password;
    // The host names the item by its data folder as it resolved it.
    let account = fs::canonicalize(data)?;
    let (password, _) = find_generic_password(None, CREDENTIAL_SERVICE, &account.to_string_lossy())
        .map_err(io::Error::other)?;
    String::from_utf8(password.to_owned()).map_err(io::Error::other)
}
#[cfg(not(target_os = "macos"))]
fn keychain_credential(_data: &Path) -> io::Result<String> {
    Err(io::Error::other("A launchd host needs the macOS Keychain"))
}

/// Waits until `socket` accepts a connection, while `alive` says the host is still starting and
/// the client has not `stopped`.
fn wait_for_socket(
    socket: &Path,
    stopped: &AtomicBool,
    mut alive: impl FnMut() -> io::Result<bool>,
) -> io::Result<()> {
    let deadline = Instant::now() + START_TIMEOUT;
    while UnixStream::connect(socket).is_err() {
        if stopped.load(Ordering::Relaxed) {
            return Err(io::Error::other("The client stopped"));
        }
        if !alive()? {
            return Err(io::Error::other(
                "The host stopped as it started; its reason is in the app's log",
            ));
        }
        if Instant::now() > deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "The host did not start listening within a minute",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn xml(path: &Path) -> String {
    path.to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_signed_app_on_its_default_folder_uses_launchd() {
        assert_eq!(HostLaunch::choose(true, true), HostLaunch::Launchd);
        assert_eq!(HostLaunch::choose(true, false), HostLaunch::Child);
        assert_eq!(HostLaunch::choose(false, true), HostLaunch::Child);
        assert_eq!(HostLaunch::choose(false, false), HostLaunch::Child);
    }

    #[test]
    fn a_development_build_is_not_developer_id_signed() {
        let binary = std::env::current_exe().unwrap();
        assert!(!developer_id_signed(&binary));
        assert_eq!(
            HostLaunch::choose(developer_id_signed(&binary), true),
            HostLaunch::Child
        );
    }

    #[test]
    fn the_launchd_plist_runs_the_versioned_copy_for_the_app_and_restarts_only_after_a_failure() {
        let job = LaunchdJob::new(
            Path::new("/Applications/Wiffletree.app/Contents/MacOS/workspace-host"),
            Path::new("/Users/me/Library/Application Support/R&D"),
            Path::new("/Applications/Wiffletree.app"),
        );
        assert_eq!(
            job.plist_path(),
            Path::new(
                "/Users/me/Library/Application Support/R&D/host/com.vyttle.wiffletree.host.plist"
            )
        );
        let plist = job.plist();
        let version = build_version();
        let arguments = format!(
            "<string>/Users/me/Library/Application Support/R&amp;D/host/{version}/workspace-host</string>\n\t\t<string>serve</string>\n\t\t<string>--data-dir</string>\n\t\t<string>/Users/me/Library/Application Support/R&amp;D</string>\n\t\t<string>--app</string>\n\t\t<string>/Applications/Wiffletree.app</string>"
        );
        assert!(plist.contains(&arguments), "{plist}");
        assert!(plist.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
        assert!(plist.contains("R&amp;D/logs/host.log</string>"));
        assert!(
            !plist.contains("RunAtLoad"),
            "KeepAlive starts it when loaded"
        );
        // `plutil` accepts what launchd will read.
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("job.plist");
        fs::write(&file, &plist).unwrap();
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&file)
            .output();
        if let Ok(lint) = lint {
            assert!(lint.status.success(), "{lint:?}");
        }
    }

    #[test]
    fn credentials_are_fresh_256_bit_secrets() {
        let (first, second) = (new_credential().unwrap(), new_credential().unwrap());
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn a_child_reads_its_credential_from_standard_input_only() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "read line; printf '%s' \"$line\"; env"]);
        command.stdout(Stdio::piped());
        let mut host = ChildHost::start_command(command).unwrap();
        let credential = host.credential().to_owned();
        let mut output = String::new();
        host.child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .unwrap();
        let (read, environment) = output.split_at(credential.len());
        assert_eq!(read, credential);
        assert!(!environment.contains(&credential));
        assert!(host.stop().unwrap().success());
    }
}
