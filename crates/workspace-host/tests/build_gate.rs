//! The build gate as agents' build tools run it: the host binary called `wiffletree-build-gate`.
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Gate {
    home: tempfile::TempDir,
}
impl Gate {
    fn new(builds_at_once: usize) -> Self {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join("settings.json"),
            format!(r#"{{"workspaces_dir":"/w","builds_at_once":{builds_at_once}}}"#),
        )
        .unwrap();
        fs::create_dir(home.path().join("deps")).unwrap();
        symlink(
            env!("CARGO_BIN_EXE_workspace-host"),
            home.path().join("wiffletree-build-gate"),
        )
        .unwrap();
        // A test binary, as Cargo leaves them in `deps`, that logs when it runs.
        let suite = home.path().join("deps/suite");
        fs::write(
            &suite,
            format!(
                "#!/bin/sh\necho \"start $1\" >> {log}\nsleep 0.6\necho \"end $1\" >> {log}\nexit $2\n",
                log = home.path().join("log").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&suite, fs::Permissions::from_mode(0o755)).unwrap();
        Gate { home }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }
    /// Runs `program` through the gate as its own build: a shell that stays its parent, as a
    /// build tool would.
    fn start(&self, program: &Path, args: &[&str]) -> Child {
        Command::new("sh")
            .arg("-c")
            .arg("\"$0\" \"$@\"; exit $?")
            .arg(self.path("wiffletree-build-gate"))
            .arg(program)
            .args(args)
            .env("WIFFLETREE_BUILD_GATE", self.home.path())
            .env("WIFFLETREE_BUILD_GATE_ROOT", std::process::id().to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
    fn log(&self) -> Vec<String> {
        let log = fs::read_to_string(self.path("log")).unwrap_or_default();
        log.lines().map(str::to_owned).collect()
    }
    fn wait_for_log(&self, line: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.log().iter().any(|l| l == line) {
            assert!(Instant::now() < deadline, "no {line:?} in {:?}", self.log());
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn finish(child: Child) -> Output {
    child.wait_with_output().unwrap()
}

#[test]
fn with_one_build_at_once_a_second_build_waits_for_the_first_and_says_so() {
    let gate = Gate::new(1);
    let first = gate.start(&gate.path("deps/suite"), &["a", "0"]);
    gate.wait_for_log("start a");
    let second = gate.start(&gate.path("deps/suite"), &["b", "3"]);
    let (first, second) = (finish(first), finish(second));
    assert!(first.status.success());
    // The program's exit status comes back through the gate.
    assert_eq!(second.status.code(), Some(3));
    assert_eq!(gate.log(), ["start a", "end a", "start b", "end b"]);
    assert_eq!(
        String::from_utf8_lossy(&second.stderr),
        "Waiting for a build slot (1 in use)\n"
    );
    assert!(first.stderr.is_empty());
}

#[test]
fn with_two_builds_at_once_two_builds_overlap() {
    let gate = Gate::new(2);
    let first = gate.start(&gate.path("deps/suite"), &["a", "0"]);
    gate.wait_for_log("start a");
    let second = gate.start(&gate.path("deps/suite"), &["b", "0"]);
    let (first, second) = (finish(first), finish(second));
    assert!(first.status.success() && second.status.success());
    assert_eq!(&gate.log()[..2], ["start a", "start b"]);
    assert!(second.stderr.is_empty());
}

#[test]
fn a_call_that_neither_compiles_nor_runs_tests_is_not_gated() {
    let gate = Gate::new(1);
    let build = gate.start(&gate.path("deps/suite"), &["a", "0"]);
    gate.wait_for_log("start a");
    // Cargo's version query, as `cargo fmt`, `metadata` and `tree` make through the wrapper.
    let started = Instant::now();
    let query = finish(gate.start(Path::new("/bin/echo"), &["-vV"]));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(String::from_utf8_lossy(&query.stdout), "-vV\n");
    assert!(query.stderr.is_empty());
    assert!(gate.log().iter().all(|l| l != "end a"), "{:?}", gate.log());
    assert!(finish(build).status.success());
}

#[test]
fn a_slot_frees_when_its_build_is_killed() {
    let gate = Gate::new(1);
    let mut first = gate.start(&gate.path("deps/suite"), &["a", "0"]);
    gate.wait_for_log("start a");
    first.kill().unwrap();
    first.wait().unwrap();
    let second = finish(gate.start(&gate.path("deps/suite"), &["b", "0"]));
    assert!(second.status.success());
    assert!(gate.log().contains(&"end b".to_owned()));
}
