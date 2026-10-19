//! How the app starts its host, and the client credential it hands a child host.
use crate::OwnDescriptors as _;
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
};

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
    /// Starts `<binary> serve --data-dir <data> --child`.
    pub fn start(binary: &Path, data: &Path) -> io::Result<Self> {
        let mut command = Command::new(binary);
        command
            .arg("serve")
            .arg("--data-dir")
            .arg(data)
            .arg("--child");
        Self::start_command(command)
    }
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
