//! Adopts the user's login-shell environment. Apps launched from the Dock inherit launchd's bare
//! PATH, so without this the host, providers and agent shells cannot find the user's tools.

use std::{
    ffi::{OsStr, OsString},
    io::Read,
    os::unix::{ffi::OsStrExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(10);

/// Describe the terminal session the probe ran in, not the user's environment.
const PER_SESSION: &[&str] = &[
    "SHLVL",
    "PWD",
    "OLDPWD",
    "_",
    "TERM_SESSION_ID",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "ITERM_SESSION_ID",
    "WINDOWID",
    "ZSH_EXECUTION_STRING",
];

/// Merges the login shell's environment into this process so every child inherits it. Keeps the
/// inherited environment if the shell fails, and never stops the app from starting.
///
/// # Safety
/// Mutates the process environment, so it must run before any other thread exists.
pub unsafe fn adopt() {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    match resolve(Path::new(&shell), TIMEOUT) {
        Ok(variables) => {
            for (key, value) in variables {
                // SAFETY: the caller guarantees no other thread can read the environment.
                unsafe { std::env::set_var(key, value) };
            }
        }
        Err(error) => eprintln!("Warning: keeping inherited environment: {error:#}"),
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    eprintln!("PATH: {}", path.to_string_lossy());
}

/// Runs `shell` as an interactive login shell and returns its environment without per-session
/// variables. The reader thread has finished by the time this returns successfully.
fn resolve(shell: &Path, timeout: Duration) -> anyhow::Result<Vec<(OsString, OsString)>> {
    let marker = format!("__WIFFLETREE_ENV_{}__", uuid::Uuid::new_v4().simple());
    let script = format!("printf '%s' {marker}; /usr/bin/env -0; printf '%s' {marker}");
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A new session, unlike a new process group, detaches the interactive shell from the
    // controlling terminal, so the terminal cannot stop it. Its group id is still its pid.
    // SAFETY: setsid is async-signal-safe and touches no memory in the forked child.
    unsafe {
        command.pre_exec(|| match libc::setsid() {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        })
    };
    let mut child = command
        .spawn()
        .map_err(|error| anyhow::anyhow!("starting {}: {error}", shell.display()))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = sender.send(stdout.read_to_end(&mut output).map(|_| output));
    });
    let output = match receiver.recv_timeout(timeout) {
        Ok(output) => output?,
        Err(_) => {
            // Kill the whole group so processes started by rc files die with the shell.
            // SAFETY: killpg only sends a signal; the group id is the shell's pid.
            unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
            let _ = child.wait();
            anyhow::bail!("{} timed out after {timeout:?}", shell.display());
        }
    };
    let _ = reader.join();
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "{} exited with {status}", shell.display());
    parse(&output, marker.as_bytes())
        .ok_or_else(|| anyhow::anyhow!("{} printed no environment", shell.display()))
}

/// Reads the NUL-separated `env -0` output between the first and last markers, ignoring rc-file
/// noise around them, malformed entries and per-session variables. Searching from both ends keeps
/// a value that contains the marker, such as the `-c` script itself, from truncating the output.
fn parse(output: &[u8], marker: &[u8]) -> Option<Vec<(OsString, OsString)>> {
    let start = find(output, marker)? + marker.len();
    let end = start + rfind(&output[start..], marker)?;
    Some(
        output[start..end]
            .split(|&byte| byte == 0)
            .filter_map(|entry| {
                let split = entry.iter().position(|&byte| byte == b'=')?;
                let (key, value) = (&entry[..split], &entry[split + 1..]);
                let per_session = PER_SESSION.iter().any(|name| name.as_bytes() == key);
                (!key.is_empty() && !per_session).then(|| {
                    (
                        OsStr::from_bytes(key).to_owned(),
                        OsStr::from_bytes(value).to_owned(),
                    )
                })
            })
            .collect(),
    )
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Instant};

    fn fake_shell(directory: &Path, body: &str) -> PathBuf {
        let path = directory.join("shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn pairs(variables: &[(OsString, OsString)]) -> Vec<(&str, &str)> {
        variables
            .iter()
            .map(|(key, value)| (key.to_str().unwrap(), value.to_str().unwrap()))
            .collect()
    }

    #[test]
    fn parses_between_markers_and_drops_malformed_and_per_session_entries() {
        let output = b"rc noise\nwelcome\nMARKPATH=/nix/bin:/usr/bin\0NOTE=two\nlines\0\
            SHLVL=2\0=empty\0no-equals\0TERM_PROGRAM=iTerm\0EMPTY=\0MARKtrailing noise";
        let variables = parse(output, b"MARK").unwrap();
        assert_eq!(
            pairs(&variables),
            [
                ("PATH", "/nix/bin:/usr/bin"),
                ("NOTE", "two\nlines"),
                ("EMPTY", "")
            ]
        );
    }

    #[test]
    fn value_containing_the_marker_does_not_truncate_the_environment() {
        let output = b"MARKSCRIPT=printf MARK; env\0PATH=/nix/bin\0MARK";
        let variables = parse(output, b"MARK").unwrap();
        assert_eq!(
            pairs(&variables),
            [("SCRIPT", "printf MARK; env"), ("PATH", "/nix/bin")]
        );
    }

    #[test]
    fn missing_closing_marker_is_not_an_environment() {
        assert_eq!(parse(b"noise MARKPATH=/bin\0", b"MARK"), None);
    }

    #[test]
    fn resolves_a_noisy_login_shell() {
        let directory = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            directory.path(),
            r#"[ "$1 $2 $3" = "-l -i -c" ] || exit 9
echo "rc file says hello"
export WIFFLETREE_PROBE=found SHLVL=7
eval "$4"
echo "rc file says goodbye""#,
        );
        let variables = resolve(&shell, Duration::from_secs(5)).unwrap();
        let variables = pairs(&variables);
        assert!(variables.contains(&("WIFFLETREE_PROBE", "found")));
        assert!(variables.iter().all(|(key, _)| !PER_SESSION.contains(key)));
    }

    #[test]
    fn failing_shell_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let shell = fake_shell(directory.path(), r#"eval "$4"; exit 1"#);
        assert!(resolve(&shell, Duration::from_secs(5)).is_err());
    }

    #[test]
    fn shell_without_markers_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let shell = fake_shell(directory.path(), "echo PATH=/bin");
        assert!(resolve(&shell, Duration::from_secs(5)).is_err());
    }

    #[test]
    fn hanging_shell_is_killed_after_the_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let shell = fake_shell(directory.path(), "exec sleep 30");
        let started = Instant::now();
        let error = resolve(&shell, Duration::from_millis(200)).unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn timeout_kills_processes_started_by_the_shell() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("grandchild.pid");
        let shell = fake_shell(
            directory.path(),
            &format!("/bin/sleep 42 &\necho $! > '{}'\nwait", pid_file.display()),
        );
        resolve(&shell, Duration::from_millis(200)).unwrap_err();
        let grandchild: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only checks whether the process still exists.
        while unsafe { libc::kill(grandchild, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "grandchild {grandchild} survived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn missing_shell_is_an_error() {
        assert!(resolve(Path::new("/nonexistent/shell"), Duration::from_secs(1)).is_err());
    }
}
