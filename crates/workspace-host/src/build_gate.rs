//! The build gate: at most "Builds at once" builds compile or run tests at a time across every
//! agent on this machine, whatever its project or role. Each build already gets half the cores,
//! so two at once took the whole machine.
//!
//! Agents reach the gate through hooks the build tools read from the environment, which shell
//! profiles and `nix develop` keep, unlike an order of PATH: Cargo's `RUSTC_WRAPPER` and test
//! runner, and Go's `-toolexec`. Each runs this binary as `wiffletree-build-gate <program>
//! <args…>`. A compile or a test binary waits for a slot, then runs; anything else, such as the
//! version queries behind `cargo fmt`, `cargo metadata` or `go env`, runs straight away.
//!
//! A slot belongs to a build: the outermost Cargo or Go process the call runs under, so the
//! nested Cargo of `cargo clippy` or `cargo nextest`, or one a test starts, shares its slot. A
//! build holds it while any of its compiles or test binaries runs and for a moment after, and
//! loses it when it exits. Waiting builds take free slots oldest first. The state is a folder of
//! lock files that the system unlocks when a holder dies, so a killed build never keeps a slot.
use std::{
    ffi::OsString,
    fs::{self, File},
    io,
    os::unix::{fs::symlink, process::ExitStatusExt},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{Duration, SystemTime},
};

/// The name the hooks run this binary by.
pub const GATE_NAME: &str = "wiffletree-build-gate";
/// The data folder whose `settings.json` holds "Builds at once" and that keeps the slot state.
const DATA_DIR: &str = "WIFFLETREE_BUILD_GATE";
/// The host's process: an owner is looked for only below it.
const ROOT: &str = "WIFFLETREE_BUILD_GATE_ROOT";
const STATE: &str = "build-gate";
/// How long a build keeps its slot after its last compile or test binary ends, so the gaps
/// between them do not hand the slot to another build.
const GRACE: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(200);

/// The environment the host gives its agents, once `install` has run.
static HOOKS: OnceLock<Vec<(String, OsString)>> = OnceLock::new();

/// Links `GATE_NAME` to `binary` in a folder without spaces, as Cargo's runner and Go's flags
/// need, and records the hooks agents get. A tool the host's own environment already wraps keeps
/// its wrapper and is not gated.
pub fn install(data: &Path, binary: &Path) -> io::Result<()> {
    let folder = std::env::temp_dir().join(format!("wiffletree-{}", std::process::id()));
    fs::create_dir_all(&folder)?;
    let gate = folder.join(GATE_NAME);
    let _ = fs::remove_file(&gate);
    symlink(binary, &gate)?;
    let _ = HOOKS.set(hooks(data, &gate, |name| std::env::var_os(name)));
    Ok(())
}

/// The variables that send `data`'s agents' builds through `gate`; `host` reads the host's own.
fn hooks(
    data: &Path,
    gate: &Path,
    host: impl Fn(&str) -> Option<OsString>,
) -> Vec<(String, OsString)> {
    let mut hooks = vec![
        (DATA_DIR.to_owned(), data.into()),
        (ROOT.to_owned(), std::process::id().to_string().into()),
    ];
    for name in ["RUSTC_WRAPPER".to_owned(), runner_variable()] {
        if host(&name).is_none() {
            hooks.push((name, gate.into()));
        }
    }
    let flags = host("GOFLAGS").unwrap_or_default();
    if !flags.to_string_lossy().contains("-toolexec") {
        let mut flags = flags;
        if !flags.is_empty() {
            flags.push(" ");
        }
        flags.push("-toolexec=");
        flags.push(gate);
        hooks.push(("GOFLAGS".to_owned(), flags));
    }
    hooks
}

/// `CARGO_TARGET_<host triple>_RUNNER`, which Cargo and nextest run test binaries through.
fn runner_variable() -> String {
    let arch = std::env::consts::ARCH;
    let triple = match std::env::consts::OS {
        "macos" => format!("{arch}-apple-darwin"),
        os => format!("{arch}-unknown-{os}-gnu"),
    };
    format!(
        "CARGO_TARGET_{}_RUNNER",
        triple.to_uppercase().replace('-', "_")
    )
}

/// The hooks for an agent's environment; none when the host installed no gate.
pub(crate) fn agent_env() -> &'static [(String, OsString)] {
    HOOKS.get().map_or(&[], Vec::as_slice)
}

/// Runs the gate when this binary was started by `GATE_NAME`, and never returns then.
pub fn run_if_called() {
    let mut args = std::env::args_os();
    let called = args.next().map(PathBuf::from);
    if called.as_deref().and_then(Path::file_name) != Some(GATE_NAME.as_ref()) {
        return;
    }
    let Some(program) = args.next() else {
        eprintln!("{GATE_NAME}: name a program to run");
        std::process::exit(2);
    };
    let args: Vec<OsString> = args.collect();
    // The gate never stops a build: one it cannot keep track of runs ungated.
    let lease = match std::env::var_os(DATA_DIR) {
        Some(data) if is_build(Path::new(&program), &args) => wait_for_slot(Path::new(&data)).ok(),
        _ => None,
    };
    let status = Command::new(&program).args(&args).status();
    if let Some(lease) = lease {
        lease.release();
    }
    match status {
        Ok(status) => exit_like(status),
        Err(error) => {
            eprintln!(
                "{GATE_NAME}: cannot run {}: {error}",
                program.to_string_lossy()
            );
            std::process::exit(127);
        }
    }
}

/// Whether a call compiles or runs tests: a rustc or clippy-driver call naming a crate, a test
/// or bench binary from a `deps` folder, or a Go tool other than its version query. Cargo runs
/// `cargo run` binaries from elsewhere, so a running app holds no slot.
fn is_build(program: &Path, args: &[OsString]) -> bool {
    let parent = |path: &Path| {
        path.parent()
            .and_then(Path::file_name)
            .map(|n| n.to_owned())
    };
    let folder = parent(program);
    let go_tool = program
        .parent()
        .and_then(parent)
        .is_some_and(|n| n == "tool");
    args.iter().any(|a| a == "--crate-name")
        || folder.is_some_and(|n| n == "deps")
        || (go_tool && !args.iter().any(|a| a.to_string_lossy().starts_with("-V")))
}

/// Exits as the program did; one ended by a signal ends by the same signal.
fn exit_like(status: std::process::ExitStatus) -> ! {
    if let Some(signal) = status.signal() {
        // SAFETY: resetting a handler and raising a signal are plain system calls.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
    std::process::exit(status.code().unwrap_or(1))
}

/// "Builds at once" from `data`'s `settings.json`, read on every check so a change applies to
/// builds already waiting.
pub(crate) fn limit(data: &Path) -> usize {
    fs::read(data.join(crate::settings::FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|settings| settings["builds_at_once"].as_u64())
        .map_or(workspace_core::DEFAULT_BUILDS_AT_ONCE, |n| n as usize)
        .clamp(1, workspace_core::MAX_BUILDS_AT_ONCE)
}

/// A compile or test binary running in its build's slot.
struct Lease {
    _file: File,
    owner: PathBuf,
}
impl Lease {
    /// Marks when the build last used its slot, before unlocking, so the slot never looks free
    /// in between.
    fn release(self) {
        let _ = File::create(self.owner.join("released"));
    }
}

/// Waits until this call's build holds a slot. Says once per build that it waits.
fn wait_for_slot(data: &Path) -> io::Result<Lease> {
    let state = data.join(STATE);
    fs::create_dir_all(state.join("waiting"))?;
    let owner = owner();
    let parent = std::os::unix::process::parent_id();
    let waiting = state.join("waiting").join(owner.to_string());
    // Only the build's first call to wait says so.
    let mut first = false;
    let mut told = false;
    loop {
        let gate = File::create(state.join("gate.lock"))?;
        gate.lock()?;
        let active = active_builds(&state)?;
        let free = limit(data).saturating_sub(active.len());
        let holds = active.contains(&owner);
        if !holds && !waiting.exists() {
            File::create_new(&waiting)?;
            first = true;
        }
        let rank = waiters(&state)?.iter().position(|&w| w == owner);
        if holds || rank.is_some_and(|rank| rank < free) {
            let _ = fs::remove_file(&waiting);
            let folder = state.join(owner.to_string());
            fs::create_dir_all(&folder)?;
            let file = File::create(folder.join(format!("lease-{}", std::process::id())))?;
            file.try_lock().map_err(io::Error::other)?;
            return Ok(Lease {
                _file: file,
                owner: folder,
            });
        }
        drop(gate);
        if first && !told {
            eprintln!("Waiting for a build slot ({} in use)", active.len());
            told = true;
        }
        std::thread::sleep(POLL);
        // A build that went away no longer needs this call.
        if std::os::unix::process::parent_id() != parent {
            std::process::exit(1);
        }
    }
}

/// The builds holding a slot. Clears what dead builds and finished calls left behind.
fn active_builds(state: &Path) -> io::Result<Vec<u32>> {
    let mut active = vec![];
    for entry in fs::read_dir(state)? {
        let path = entry?.path();
        let Some(owner) = path
            .file_name()
            .and_then(|n| n.to_str()?.parse::<u32>().ok())
        else {
            continue;
        };
        if !alive(owner) {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        let recent = fs::metadata(path.join("released"))
            .and_then(|m| m.modified())
            .is_ok_and(|at| SystemTime::now().duration_since(at).unwrap_or_default() < GRACE);
        let mut running = false;
        for lease in fs::read_dir(&path)? {
            let lease = lease?.path();
            if !lease
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("lease-"))
            {
                continue;
            }
            match File::open(&lease)?.try_lock() {
                Ok(()) => fs::remove_file(&lease)?,
                Err(_) => running = true,
            }
        }
        if running || recent {
            active.push(owner);
        }
    }
    Ok(active)
}

/// The builds waiting for a slot, oldest first. Clears dead ones.
fn waiters(state: &Path) -> io::Result<Vec<u32>> {
    let mut waiting = vec![];
    for entry in fs::read_dir(state.join("waiting"))? {
        let entry = entry?;
        let Some(owner) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if !alive(owner) {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        waiting.push((entry.metadata()?.modified()?, owner));
    }
    waiting.sort();
    Ok(waiting.into_iter().map(|(_, owner)| owner).collect())
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let found = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
    found || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The build a call belongs to: the outermost Cargo, Go or Rust tool among its ancestors below
/// the host, else its parent.
fn owner() -> u32 {
    let root: Option<u32> = std::env::var(ROOT).ok().and_then(|r| r.parse().ok());
    let mut pid = std::os::unix::process::parent_id();
    let mut owner = pid;
    for _ in 0..64 {
        if pid <= 1 || Some(pid) == root {
            break;
        }
        let Some((parent, name)) = process(pid) else {
            break;
        };
        if is_build_tool(&name) {
            owner = pid;
        }
        pid = parent;
    }
    owner
}

/// Names the build tools a slot belongs to, as a wrapped Nix binary such as `.cargo-wrapped`
/// is named too.
fn is_build_tool(name: &str) -> bool {
    let name = name.trim_start_matches('.').trim_end_matches("-wrapped");
    matches!(name, "cargo" | "rustc" | "rustdoc" | "clippy-driver" | "go")
        || name.starts_with("cargo-")
}

/// A process's parent and name.
#[cfg(target_os = "macos")]
fn process(pid: u32) -> Option<(u32, String)> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is a writable buffer of `size` bytes.
    let read = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if read != size {
        return None;
    }
    // SAFETY: the kernel ends the name with a NUL inside the array.
    let name = unsafe { std::ffi::CStr::from_ptr(info.pbi_name.as_ptr()) };
    let name = match name.is_empty() {
        // SAFETY: as above, for the shorter name.
        true => unsafe { std::ffi::CStr::from_ptr(info.pbi_comm.as_ptr()) },
        false => name,
    };
    Some((info.pbi_ppid, name.to_string_lossy().into_owned()))
}

/// A process's parent and name.
#[cfg(not(target_os = "macos"))]
fn process(pid: u32) -> Option<(u32, String)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (head, tail) = stat.rsplit_once(')')?;
    let name = head.split_once('(')?.1.to_owned();
    let parent = tail.split_whitespace().nth(1)?.parse().ok()?;
    Some((parent, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_compiles_and_test_binaries_are_builds() {
        let rustc = Path::new("/toolchain/bin/rustc");
        assert!(is_build(
            rustc,
            &args(&["--crate-name", "core", "src/lib.rs"])
        ));
        // Cargo's version and target queries, as `cargo fmt`, `metadata` and `tree` make.
        assert!(!is_build(rustc, &args(&["-vV"])));
        assert!(!is_build(rustc, &args(&["-", "--print=cfg"])));
        assert!(is_build(Path::new("/w/target/debug/deps/core-1a2b"), &[]));
        assert!(!is_build(Path::new("/w/target/debug/app"), &[]));
        let compile = Path::new("/go/pkg/tool/darwin_arm64/compile");
        assert!(is_build(compile, &args(&["-o", "x.a", "a.go"])));
        assert!(!is_build(compile, &args(&["-V=full"])));
    }

    #[test]
    fn wrapped_tools_count_as_build_tools_and_shells_do_not() {
        for tool in [
            "cargo",
            ".cargo-wrapped",
            "cargo-nextest",
            "cargo-clippy",
            "go",
            "rustc",
        ] {
            assert!(is_build_tool(tool), "{tool}");
        }
        for other in ["zsh", "claude", "build-script-build", "make", "gopls"] {
            assert!(!is_build_tool(other), "{other}");
        }
    }

    #[test]
    fn agents_get_every_hook_the_host_does_not_set_itself() {
        let data = Path::new("/data");
        let gate = Path::new("/tmp/w/wiffletree-build-gate");
        let fresh = hooks(data, gate, |_| None);
        let value = |hooks: &[(String, OsString)], name: &str| {
            hooks
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(value(&fresh, "RUSTC_WRAPPER"), Some(gate.into()));
        assert_eq!(value(&fresh, &runner_variable()), Some(gate.into()));
        assert_eq!(
            value(&fresh, "GOFLAGS"),
            Some("-toolexec=/tmp/w/wiffletree-build-gate".into())
        );
        assert_eq!(value(&fresh, DATA_DIR), Some("/data".into()));
        // An existing wrapper or toolexec is the user's; other Go flags are kept.
        let wrapped = hooks(data, gate, |name| match name {
            "RUSTC_WRAPPER" => Some("sccache".into()),
            "GOFLAGS" => Some("-mod=mod".into()),
            _ => None,
        });
        assert_eq!(value(&wrapped, "RUSTC_WRAPPER"), None);
        assert_eq!(
            value(&wrapped, "GOFLAGS"),
            Some("-mod=mod -toolexec=/tmp/w/wiffletree-build-gate".into())
        );
        let toolexec = hooks(data, gate, |name| {
            (name == "GOFLAGS").then(|| "-toolexec=mine".into())
        });
        assert_eq!(value(&toolexec, "GOFLAGS"), None);
    }

    #[test]
    fn the_limit_comes_from_the_settings_file_within_bounds() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(limit(data.path()), 1);
        for (saved, read) in [(2, 2), (0, 1), (9, 4)] {
            fs::write(
                data.path().join("settings.json"),
                format!(r#"{{"workspaces_dir":"/w","builds_at_once":{saved}}}"#),
            )
            .unwrap();
            assert_eq!(limit(data.path()), read);
        }
    }
}
