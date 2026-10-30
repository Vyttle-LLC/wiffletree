//! The build gate: at most "Builds at once" builds compile or run tests at a time across every
//! agent on this machine, whatever its project or role. Each build already gets half the cores,
//! so two at once took the whole machine.
//!
//! Agents reach the gate through hooks the build tools read from the environment, which shell
//! profiles and `nix develop` keep, unlike an order of PATH: Cargo's `RUSTC_WRAPPER` and target
//! runner, and Go's `-toolexec`. Each runs this binary as `wiffletree-build-gate <program>
//! <args…>`, which always ends by executing the program in its place, so signals reach it as
//! before.
//!
//! A slot belongs to a build: the outermost `cargo` or `go` command among the call's ancestors
//! whose subcommand builds, such as `cargo test` or `go vet`, so the nested Cargo of `cargo
//! clippy` or `cargo nextest`, or one a test starts, shares it, while `cargo watch` is no build
//! and each `cargo test` it starts is its own. A build takes its slot at its first compile or
//! test binary and holds it until that command ends, so its test binaries, doctests and docs
//! run inside it. `cargo run` replaces Cargo with the app, so the app holds no slot; `go run`
//! keeps Go running beside its app, so it holds its slot only while its compiles and link run.
//! Waiting builds take free slots oldest first.
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::symlink, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{Duration, SystemTime},
};

/// The name the hooks run this binary by.
const GATE_NAME: &str = "wiffletree-build-gate";
/// The data folder whose `settings.json` holds "Builds at once" and that keeps the slot state.
const DATA_DIR: &str = "WIFFLETREE_BUILD_GATE";
/// The host's process: an owner is looked for only below it.
const ROOT: &str = "WIFFLETREE_BUILD_GATE_ROOT";
const STATE: &str = "build-gate";
const POLL: Duration = Duration::from_millis(200);
/// How long a `go run` keeps its slot after its last compile or link started, so the gaps
/// between them do not hand the slot away.
const GRACE: Duration = Duration::from_secs(2);
/// The Cargo subcommands, aliases included, and Go commands that build.
const CARGO_BUILDS: [&str; 17] = [
    "build", "b", "check", "c", "clippy", "test", "t", "nextest", "bench", "doc", "d", "rustc",
    "rustdoc", "install", "run", "r", "fix",
];
const GO_BUILDS: [&str; 5] = ["build", "test", "vet", "install", "run"];

/// The environment the host gives its agents, once `install` has run.
static HOOKS: OnceLock<Vec<(String, OsString)>> = OnceLock::new();

/// Links `GATE_NAME` to `binary` in a temporary folder without spaces, as Cargo's runner and
/// Go's flags need, one per data folder so restarts reuse it, and records the hooks agents get.
/// A hook the host's own environment already sets is the user's and is left alone.
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
    for name in ["RUSTC_WRAPPER".to_owned(), runner_variable()] {
        if host(&name).is_none() {
            hooks.push((name, gate.into()));
        }
    }
    let flags = host("GOFLAGS").unwrap_or_default();
    if !flags.to_string_lossy().contains("-toolexec=") {
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

/// What a call through the gate is.
#[derive(Debug, PartialEq)]
enum Call {
    /// rustc or clippy-driver compiling a crate; it says it waits on standard output, which
    /// Cargo shows and, unlike rustc's errors, does not replay from its cache.
    Compile,
    /// A Go compiler, linker or vet run. It waits silently: Go caches and replays a tool's
    /// output.
    GoTool,
    /// A test or bench binary Cargo or nextest runs from a `deps` folder.
    TestBinary,
    /// Anything else, such as the version and target queries behind `cargo tree` and `go env`,
    /// or the app `cargo run` starts.
    Other,
}

fn classify(program: &Path, args: &[OsString]) -> Call {
    let arg = |test: fn(&str) -> bool| args.iter().any(|a| test(&a.to_string_lossy()));
    let folder = |path: &Path| {
        path.parent()
            .and_then(Path::file_name)
            .map(|n| n.to_owned())
    };
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
    if folder(program).is_some_and(|n| n == "deps") {
        return Call::TestBinary;
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
    // A `go run` call's lease stays open in the program it becomes.
    let _lease = std::env::var_os(DATA_DIR).and_then(|data| {
        let data = Path::new(&data);
        // The gate never stops a build: one it cannot keep track of runs ungated.
        match classify(Path::new(&program), &args) {
            Call::Compile => claim(data, &mut io::stdout()),
            Call::TestBinary => claim(data, &mut io::stderr()),
            Call::GoTool => claim(data, &mut io::sink()),
            Call::Other => Ok(None),
        }
        .ok()
        .flatten()
    });
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
        for part in ["held", "waiting", "leases"] {
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
    fn leases(&self, build: u32) -> PathBuf {
        self.folder.join("leases").join(build.to_string())
    }
    /// The builds holding a slot. Clears those whose process ended or became something else,
    /// as `cargo run` becomes its app, and a `go run` whose compiles and link are done.
    fn holders(&self) -> io::Result<Vec<u32>> {
        let mut holders = vec![];
        for (build, path) in entries(&self.folder.join("held"))? {
            let saved = fs::read_to_string(&path).unwrap_or_default();
            let (identity, run) = saved.split_once('\n').unwrap_or((&saved, ""));
            let same = process(build).is_some_and(|p| p.identity() == identity);
            if same && (run.is_empty() || self.leased(build)?) {
                holders.push(build);
            } else {
                fs::remove_file(path)?;
                let _ = fs::remove_dir_all(self.leases(build));
            }
        }
        Ok(holders)
    }
    /// Whether one of a `go run`'s calls runs, or the last started within `GRACE`.
    fn leased(&self, build: u32) -> io::Result<bool> {
        let mut latest = SystemTime::UNIX_EPOCH;
        for (_, lease) in entries(&self.leases(build)).unwrap_or_default() {
            let file = File::open(&lease)?;
            if file.try_lock().is_err() {
                return Ok(true);
            }
            latest = latest.max(file.metadata()?.modified()?);
        }
        Ok(latest.elapsed().unwrap_or_default() < GRACE)
    }
    /// A lease for this call on a `go run`'s slot, kept open across `exec`.
    fn lease(&self, build: u32) -> io::Result<File> {
        fs::create_dir_all(self.leases(build))?;
        let file = File::create(self.leases(build).join(std::process::id().to_string()))?;
        file.try_lock().map_err(io::Error::other)?;
        // SAFETY: clearing a descriptor's flags is a plain system call on a descriptor we own.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(file)
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

/// Waits until this call's build holds a slot, saying once on `say` that it waits. Returns a
/// `go run`'s lease for this call.
fn claim(data: &Path, say: &mut impl Write) -> io::Result<Option<File>> {
    let build = owner();
    let parent = std::os::unix::process::parent_id();
    let mut told = false;
    loop {
        let state = State::open(data)?;
        let holders = state.holders()?;
        let waiting = state.waiting(build.pid);
        let mut holds = holders.contains(&build.pid);
        if !holds {
            // Only the build's first call to wait says so.
            let first = !waiting.exists() && File::create_new(&waiting).is_ok();
            told |= !first;
            let free = limit(data).saturating_sub(holders.len());
            let rank = state.waiters()?.iter().position(|&w| w == build.pid);
            if rank.is_some_and(|rank| rank < free) {
                let run = if build.run { "\nrun" } else { "" };
                fs::write(state.held(build.pid), format!("{}{run}", build.identity))?;
                let _ = fs::remove_file(&waiting);
                holds = true;
            }
        }
        if holds {
            return build.run.then(|| state.lease(build.pid)).transpose();
        }
        drop(state);
        if !told {
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

/// The build a call belongs to.
struct Build {
    pid: u32,
    identity: String,
    /// A `go run`, which holds its slot only while its calls run.
    run: bool,
}

/// The outermost `cargo` or `go` among the call's ancestors below the host whose subcommand
/// builds, else the call's parent.
fn owner() -> Build {
    let root: Option<u32> = std::env::var(ROOT).ok().and_then(|r| r.parse().ok());
    let parent = std::os::unix::process::parent_id();
    let mut owner = Build {
        pid: parent,
        identity: process(parent).map(|p| p.identity()).unwrap_or_default(),
        run: false,
    };
    let mut pid = parent;
    for _ in 0..64 {
        if pid <= 1 || Some(pid) == root {
            break;
        }
        let Some(process) = process(pid) else {
            break;
        };
        let tool = process
            .name
            .trim_start_matches('.')
            .trim_end_matches("-wrapped");
        let builds = match tool {
            "cargo" => CARGO_BUILDS.as_slice(),
            "go" => GO_BUILDS.as_slice(),
            _ => &[],
        };
        let command = arguments(pid).and_then(|args| subcommand(&args).map(str::to_owned));
        if let Some(command) = command.filter(|c| builds.contains(&c.as_str())) {
            owner = Build {
                pid,
                identity: process.identity(),
                run: tool == "go" && command == "run",
            };
        }
        pid = process.parent;
    }
    owner
}

/// A `cargo` or `go` command line's subcommand: its first word after the global flags.
fn subcommand(args: &[String]) -> Option<&str> {
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-C" | "--config" | "-Z" | "--color" => {
                rest.next();
            }
            flag if flag.starts_with(['-', '+']) => {}
            command => return Some(command),
        }
    }
    None
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

/// A process's command line.
#[cfg(target_os = "macos")]
fn arguments(pid: u32) -> Option<Vec<String>> {
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;
    let ask = |buffer: *mut libc::c_void, size: &mut libc::size_t, name: &mut [libc::c_int; 3]| {
        // SAFETY: `buffer` is null, to ask for the size, or holds `size` writable bytes.
        unsafe { libc::sysctl(name.as_mut_ptr(), 3, buffer, size, std::ptr::null_mut(), 0) }
    };
    if ask(std::ptr::null_mut(), &mut size, &mut name) != 0 {
        return None;
    }
    let mut buffer = vec![0u8; size];
    if ask(buffer.as_mut_ptr().cast(), &mut size, &mut name) != 0 {
        return None;
    }
    buffer.truncate(size);
    // The argument count, the executable's path, padding, then the arguments.
    let count = i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?) as usize;
    let rest = &buffer[4..];
    let rest = &rest[rest.iter().position(|&b| b == 0)?..];
    let rest = &rest[rest.iter().position(|&b| b != 0)?..];
    Some(
        (rest.split(|&b| b == 0).take(count))
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect(),
    )
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

/// A process's command line.
#[cfg(not(target_os = "macos"))]
fn arguments(pid: u32) -> Option<Vec<String>> {
    let line = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        (line.split(|&b| b == 0).filter(|arg| !arg.is_empty()))
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_compiles_and_test_binaries_take_a_slot() {
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
        let deps = Path::new("/w/target/debug/deps/core-1a2b");
        assert_eq!(classify(deps, &args(&["--nocapture"])), Call::TestBinary);
        // `cargo run`'s app.
        assert_eq!(classify(Path::new("/w/target/debug/app"), &[]), Call::Other);
    }

    #[test]
    fn the_subcommand_is_the_first_word_after_global_flags() {
        let line = |line: &str| line.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let cases = [
            ("cargo test -p core", Some("test")),
            (
                "cargo +nightly --locked -C crates/core nextest run",
                Some("nextest"),
            ),
            ("cargo --config a=1 watch -x test", Some("watch")),
            ("go -C dir run .", Some("run")),
            ("cargo --version", None),
        ];
        for (command, expected) in cases {
            assert_eq!(subcommand(&line(command)), expected, "{command}");
        }
        assert!(!CARGO_BUILDS.contains(&"watch") && CARGO_BUILDS.contains(&"t"));
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
