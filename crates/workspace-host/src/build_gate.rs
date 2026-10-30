//! The build gate: at most "Builds at once" builds compile or run tests at a time across every
//! agent on this machine, whatever its project or role. Each build already gets half the cores,
//! so two at once took the whole machine.
//!
//! Agents reach the gate through hooks the build tools read from the environment, which shell
//! profiles and `nix develop` keep, unlike an order of PATH: Cargo's `RUSTC_WRAPPER`, and Go's
//! `-toolexec` and `-exec`. Each runs this binary as `wiffletree-build-gate <program> <args…>`,
//! which always ends by executing the program in its place, so signals reach it as before.
//!
//! A slot belongs to a build: the outermost Cargo or Go process the call runs under, so the
//! nested Cargo of `cargo clippy` or `cargo nextest`, or one a test starts, shares it. A build
//! takes its slot at its first compile, or at a Go test binary, and holds it until that process
//! ends, so its test binaries, doctests and docs run inside it. `cargo run` replaces Cargo with
//! the app and `go run` hands its app to the gate, so a running app holds no slot. Waiting builds
//! take free slots oldest first.
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Duration,
};

/// The name the hooks run this binary by.
const GATE_NAME: &str = "wiffletree-build-gate";
/// The data folder whose `settings.json` holds "Builds at once" and that keeps the slot state.
const DATA_DIR: &str = "WIFFLETREE_BUILD_GATE";
/// The host's process: an owner is looked for only below it.
const ROOT: &str = "WIFFLETREE_BUILD_GATE_ROOT";
const STATE: &str = "build-gate";
const POLL: Duration = Duration::from_millis(200);

/// The environment the host gives its agents, once `install` has run.
static HOOKS: OnceLock<Vec<(String, OsString)>> = OnceLock::new();

/// Links `GATE_NAME` to `binary` in a temporary folder without spaces, as Go's flags need, one
/// per data folder so restarts reuse it, and records the hooks agents get. A hook the host's own
/// environment already sets is the user's and is left alone.
pub fn install(data: &Path, binary: &Path) -> io::Result<()> {
    let folder = std::env::temp_dir().join(format!("wiffletree-{:016x}", fnv(data)));
    fs::create_dir_all(&folder)?;
    let gate = folder.join(GATE_NAME);
    let staged = folder.join(format!(".{GATE_NAME}-{}", std::process::id()));
    let _ = fs::remove_file(&staged);
    symlink(binary, &staged)?;
    fs::rename(&staged, &gate)?;
    let _ = HOOKS.set(hooks(data, &gate, |name| std::env::var_os(name)));
    Ok(())
}

/// A stable name for a path, so each data folder keeps one gate folder.
fn fnv(path: &Path) -> u64 {
    (path.as_os_str().as_encoded_bytes().iter()).fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
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
    if host("RUSTC_WRAPPER").is_none() {
        hooks.push(("RUSTC_WRAPPER".to_owned(), gate.into()));
    }
    let mut flags = host("GOFLAGS").unwrap_or_default();
    let before = flags.len();
    for flag in ["-toolexec", "-exec"] {
        if !flags.to_string_lossy().contains(&format!("{flag}=")) {
            if !flags.is_empty() {
                flags.push(" ");
            }
            flags.push(format!("{flag}="));
            flags.push(gate);
        }
    }
    if flags.len() != before {
        hooks.push(("GOFLAGS".to_owned(), flags));
    }
    hooks
}

/// The hooks for an agent's environment; none when the host installed no gate.
pub(crate) fn agent_env() -> &'static [(String, OsString)] {
    HOOKS.get().map_or(&[], Vec::as_slice)
}

/// What a call through the gate is.
#[derive(Debug, PartialEq)]
enum Call {
    /// rustc or clippy-driver compiling a crate; it says it waits on standard output, which
    /// Cargo shows and, unlike rustc's errors, does not replay from its cache.
    Compile,
    /// A Go compiler, linker or vet run. It waits silently: Go caches and replays a tool's
    /// output.
    GoTool,
    /// A Go test binary.
    GoTest,
    /// The program `go run` built; its build gives up its slot.
    GoRun,
    /// Anything else, such as the version and target queries behind `cargo tree` and `go env`.
    Other,
}

fn classify(program: &Path, args: &[OsString]) -> Call {
    let arg = |test: fn(&str) -> bool| args.iter().any(|a| test(&a.to_string_lossy()));
    let folder = |path: &Path| {
        path.parent()
            .and_then(Path::file_name)
            .map(|n| n.to_owned())
    };
    let name = program.file_name().unwrap_or_default().to_string_lossy();
    if arg(|a| a == "--crate-name") {
        // Cargo's target query reads its input from standard input and only prints.
        return match arg(|a| a == "-" || a.starts_with("--print")) {
            true => Call::Other,
            false => Call::Compile,
        };
    }
    if program
        .parent()
        .and_then(folder)
        .is_some_and(|n| n == "tool")
    {
        return match arg(|a| a.starts_with("-V")) {
            true => Call::Other,
            false => Call::GoTool,
        };
    }
    if name.ends_with(".test") {
        return Call::GoTest;
    }
    if folder(program).is_some_and(|n| n == "exe") {
        return Call::GoRun;
    }
    Call::Other
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
    if let Some(data) = std::env::var_os(DATA_DIR) {
        let data = Path::new(&data);
        // The gate never stops a build: one it cannot keep track of runs ungated.
        let _ = match classify(Path::new(&program), &args) {
            Call::Compile => claim(data, &mut io::stdout()),
            Call::GoTest => claim(data, &mut io::stderr()),
            Call::GoTool => claim(data, &mut io::sink()),
            Call::GoRun => release(data),
            Call::Other => Ok(()),
        };
    }
    let error = Command::new(&program).args(&args).exec();
    eprintln!(
        "{GATE_NAME}: cannot run {}: {error}",
        program.to_string_lossy()
    );
    std::process::exit(127);
}

/// "Builds at once" from `data`'s `settings.json`, read on every check so a change applies to
/// builds already waiting.
fn limit(data: &Path) -> usize {
    fs::read(data.join(crate::settings::FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|settings| settings["builds_at_once"].as_u64())
        .map_or(workspace_core::DEFAULT_BUILDS_AT_ONCE, |n| n as usize)
        .clamp(1, workspace_core::MAX_BUILDS_AT_ONCE)
}

/// The slot state, with its folders made, locked against other calls while it lives.
struct State {
    folder: PathBuf,
    _lock: File,
}
impl State {
    fn open(data: &Path) -> io::Result<Self> {
        let folder = data.join(STATE);
        for part in ["held", "waiting"] {
            fs::create_dir_all(folder.join(part))?;
        }
        let lock = File::create(folder.join("gate.lock"))?;
        lock.lock()?;
        Ok(State {
            folder,
            _lock: lock,
        })
    }
    fn held(&self, build: u32) -> PathBuf {
        self.folder.join("held").join(build.to_string())
    }
    fn waiting(&self, build: u32) -> PathBuf {
        self.folder.join("waiting").join(build.to_string())
    }
    /// The builds holding a slot. Clears those whose process ended or became something else,
    /// as `cargo run` becomes its app.
    fn holders(&self) -> io::Result<Vec<u32>> {
        let mut holders = vec![];
        for (build, path) in entries(&self.folder.join("held"))? {
            match fs::read_to_string(&path) {
                Ok(identity) if process(build).is_some_and(|p| p.identity() == identity) => {
                    holders.push(build)
                }
                _ => fs::remove_file(path)?,
            }
        }
        Ok(holders)
    }
    /// The builds waiting for a slot, oldest first. Clears those that are gone.
    fn waiters(&self) -> io::Result<Vec<u32>> {
        let mut waiting = vec![];
        for (build, path) in entries(&self.folder.join("waiting"))? {
            match process(build) {
                Some(_) => waiting.push((fs::metadata(&path)?.modified()?, build)),
                None => fs::remove_file(path)?,
            }
        }
        waiting.sort();
        Ok(waiting.into_iter().map(|(_, build)| build).collect())
    }
}

/// The files in `folder` named by a process id.
fn entries(folder: &Path) -> io::Result<Vec<(u32, PathBuf)>> {
    let mut entries = vec![];
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if let Some(build) = path.file_name().and_then(|n| n.to_str()?.parse().ok()) {
            entries.push((build, path));
        }
    }
    Ok(entries)
}

/// Waits until this call's build holds a slot, saying once on `say` that it waits.
fn claim(data: &Path, say: &mut impl Write) -> io::Result<()> {
    let build = owner();
    let identity = process(build).map(|p| p.identity()).unwrap_or_default();
    let parent = std::os::unix::process::parent_id();
    let mut told = false;
    loop {
        let state = State::open(data)?;
        let holders = state.holders()?;
        if holders.contains(&build) {
            return Ok(());
        }
        let waiting = state.waiting(build);
        // Only the build's first call to wait says so.
        let first = !waiting.exists() && File::create_new(&waiting).is_ok();
        let free = limit(data).saturating_sub(holders.len());
        let rank = state.waiters()?.iter().position(|&w| w == build);
        if rank.is_some_and(|rank| rank < free) {
            fs::write(state.held(build), &identity)?;
            let _ = fs::remove_file(&waiting);
            return Ok(());
        }
        drop(state);
        if first && !told {
            let _ = writeln!(say, "Waiting for a build slot ({} in use)", holders.len());
            told = true;
        }
        std::thread::sleep(POLL);
        // A build that went away no longer needs this call.
        if std::os::unix::process::parent_id() != parent {
            std::process::exit(1);
        }
    }
}

/// Gives up the slot of the `go run` that built this app, so the running app holds none.
fn release(data: &Path) -> io::Result<()> {
    let build = owner();
    if build == std::os::unix::process::parent_id() {
        let state = State::open(data)?;
        let _ = fs::remove_file(state.held(build));
    }
    Ok(())
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
        let Some(process) = process(pid) else {
            break;
        };
        if is_build_tool(&process.name) {
            owner = pid;
        }
        pid = process.parent;
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

struct Process {
    parent: u32,
    name: String,
    /// When it started, which with its name tells it from a later process given its id.
    started: u64,
}
impl Process {
    fn identity(&self) -> String {
        format!("{} {}", self.started, self.name)
    }
}

#[cfg(target_os = "macos")]
fn process(pid: u32) -> Option<Process> {
    // SAFETY: an all-zero `proc_bsdinfo` is a valid value.
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
    // SAFETY: the kernel ends each name with a NUL inside its array.
    let (name, short) = unsafe {
        (
            std::ffi::CStr::from_ptr(info.pbi_name.as_ptr()),
            std::ffi::CStr::from_ptr(info.pbi_comm.as_ptr()),
        )
    };
    let name = if name.is_empty() { short } else { name };
    Some(Process {
        parent: info.pbi_ppid,
        name: name.to_string_lossy().into_owned(),
        started: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
    })
}

#[cfg(not(target_os = "macos"))]
fn process(pid: u32) -> Option<Process> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (head, tail) = stat.rsplit_once(')')?;
    let fields: Vec<&str> = tail.split_whitespace().collect();
    Some(Process {
        parent: fields.get(1)?.parse().ok()?,
        name: head.split_once('(')?.1.to_owned(),
        started: fields.get(19)?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_compiles_and_go_test_binaries_take_a_slot() {
        let rustc = Path::new("/toolchain/bin/rustc");
        let compile = args(&["--crate-name", "core", "--edition=2024", "src/lib.rs"]);
        assert_eq!(classify(rustc, &compile), Call::Compile);
        // Cargo's version and target queries, as `cargo tree` makes them on a cold target.
        assert_eq!(classify(rustc, &args(&["-vV"])), Call::Other);
        let query = "- --crate-name ___ --print=file-names --crate-type bin --print=cfg";
        let query: Vec<&str> = query.split(' ').collect();
        assert_eq!(classify(rustc, &args(&query)), Call::Other);
        let tool = Path::new("/go/pkg/tool/darwin_arm64/compile");
        assert_eq!(classify(tool, &args(&["-o", "x.a", "a.go"])), Call::GoTool);
        assert_eq!(classify(tool, &args(&["-V=full"])), Call::Other);
        let work = Path::new("/tmp/go-build1/b001");
        assert_eq!(classify(&work.join("fmt.test"), &[]), Call::GoTest);
        assert_eq!(classify(&work.join("exe/server"), &[]), Call::GoRun);
        assert_eq!(classify(Path::new("/w/target/debug/app"), &[]), Call::Other);
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
        let value = |hooks: &[(String, OsString)], name: &str| {
            hooks
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        let fresh = hooks(data, gate, |_| None);
        assert_eq!(value(&fresh, "RUSTC_WRAPPER"), Some(gate.into()));
        let go = "-toolexec=/tmp/w/wiffletree-build-gate -exec=/tmp/w/wiffletree-build-gate";
        assert_eq!(value(&fresh, "GOFLAGS"), Some(go.into()));
        assert_eq!(value(&fresh, DATA_DIR), Some("/data".into()));
        assert!(
            fresh
                .iter()
                .all(|(name, _)| !name.starts_with("CARGO_TARGET_"))
        );
        // An existing wrapper or Go hook is the user's; other Go flags are kept.
        let wrapped = hooks(data, gate, |name| match name {
            "RUSTC_WRAPPER" => Some("sccache".into()),
            "GOFLAGS" => Some("-mod=mod -exec=mine".into()),
            _ => None,
        });
        assert_eq!(value(&wrapped, "RUSTC_WRAPPER"), None);
        assert_eq!(
            value(&wrapped, "GOFLAGS"),
            Some("-mod=mod -exec=mine -toolexec=/tmp/w/wiffletree-build-gate".into())
        );
        let both = hooks(data, gate, |name| {
            (name == "GOFLAGS").then(|| "-toolexec=a -exec=b".into())
        });
        assert_eq!(value(&both, "GOFLAGS"), None);
    }

    #[test]
    fn each_data_folder_keeps_one_gate_folder() {
        assert_eq!(fnv(Path::new("/a")), fnv(Path::new("/a")));
        assert_ne!(fnv(Path::new("/a")), fnv(Path::new("/b")));
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
