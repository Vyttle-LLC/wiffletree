//! The build gate as agents' build tools run it: the host binary called `wiffletree-build-gate`.
//! Every step waits for an event, never for a length of time, so a busy machine only slows it.
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
        symlink(
            env!("CARGO_BIN_EXE_workspace-host"),
            home.path().join("wiffletree-build-gate"),
        )
        .unwrap();
        // A compiler that logs when it starts, then runs until its release file appears, or
        // until a failed test's folder is gone.
        let rustc = home.path().join("rustc");
        fs::write(
            &rustc,
            format!(
                "#!/bin/sh\necho $$ > {home}/pid-$2\necho \"start $2\" >> {home}/log\nwhile [ -d {home} ] && [ ! -f {home}/release-$2 ]; do sleep 0.02; done\necho \"end $2\" >> {home}/log\n",
                home = home.path().display()
            ),
        )
        .unwrap();
        fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755)).unwrap();
        Gate { home }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env("WIFFLETREE_BUILD_GATE", self.home.path())
            .env("WIFFLETREE_BUILD_GATE_ROOT", std::process::id().to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    /// Starts the compile of crate `name` through the gate as its own build: a shell that stays
    /// its parent, as Cargo would.
    fn compile(&self, name: &str) -> Child {
        self.command("sh")
            .arg("-c")
            .arg("\"$0\" \"$@\"; exit $?")
            .arg(self.path("wiffletree-build-gate"))
            .arg(self.path("rustc"))
            .args(["--crate-name", name])
            .spawn()
            .unwrap()
    }
    fn release(&self, name: &str) {
        fs::write(self.path(&format!("release-{name}")), "").unwrap();
    }
    fn log(&self) -> Vec<String> {
        let log = fs::read_to_string(self.path("log")).unwrap_or_default();
        log.lines().map(str::to_owned).collect()
    }
    fn wait_for_log(&self, line: &str) {
        wait_until(&format!("{line:?} in the log"), || {
            self.log().iter().any(|l| l == line)
        });
    }
    /// Waits until some build is queued for a slot.
    fn wait_for_a_waiter(&self) {
        let waiting = self.path("build-gate/waiting");
        wait_until("a waiting build", || {
            fs::read_dir(&waiting).is_ok_and(|mut d| d.next().is_some())
        });
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done() {
        assert!(Instant::now() < deadline, "no {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn finish(child: Child) -> Output {
    child.wait_with_output().unwrap()
}

#[test]
fn with_one_build_at_once_a_second_build_waits_for_the_first_and_says_so() {
    let gate = Gate::new(1);
    let first = gate.compile("a");
    gate.wait_for_log("start a");
    let second = gate.compile("b");
    gate.wait_for_a_waiter();
    assert_eq!(gate.log(), ["start a"]);
    gate.release("a");
    gate.release("b");
    let (first, second) = (finish(first), finish(second));
    assert!(first.status.success() && second.status.success());
    assert_eq!(gate.log(), ["start a", "end a", "start b", "end b"]);
    // On standard output, which Cargo shows without replaying it from its cache.
    assert_eq!(
        String::from_utf8_lossy(&second.stdout),
        "Waiting for a build slot (1 in use)\n"
    );
    assert!(first.stdout.is_empty());
}

#[test]
fn with_two_builds_at_once_two_builds_overlap() {
    let gate = Gate::new(2);
    let first = gate.compile("a");
    gate.wait_for_log("start a");
    let second = gate.compile("b");
    gate.wait_for_log("start b");
    gate.release("a");
    gate.release("b");
    let (first, second) = (finish(first), finish(second));
    assert!(first.status.success() && second.status.success());
    assert!(second.stdout.is_empty());
}

#[test]
fn the_gate_becomes_the_program_so_killing_it_kills_the_program() {
    let gate = Gate::new(1);
    let mut compile = gate
        .command(gate.path("wiffletree-build-gate"))
        .arg(gate.path("rustc"))
        .args(["--crate-name", "k"])
        .spawn()
        .unwrap();
    gate.wait_for_log("start k");
    let pid = fs::read_to_string(gate.path("pid-k")).unwrap();
    assert_eq!(pid.trim(), compile.id().to_string());
    compile.kill().unwrap();
    compile.wait().unwrap();
    assert!(!gate.log().contains(&"end k".to_owned()));
}

#[test]
fn a_slot_frees_when_its_build_is_killed() {
    let gate = Gate::new(1);
    let mut first = gate.compile("a");
    gate.wait_for_log("start a");
    let second = gate.compile("b");
    gate.wait_for_a_waiter();
    first.kill().unwrap();
    first.wait().unwrap();
    gate.wait_for_log("start b");
    gate.release("a");
    gate.release("b");
    assert!(finish(second).status.success());
}

/// A crate with no dependencies, whose target folder nothing has built yet.
fn fresh_crate(home: &Path) -> PathBuf {
    let folder = home.join("fresh");
    fs::create_dir_all(folder.join("src")).unwrap();
    fs::write(
        folder.join("Cargo.toml"),
        "[package]\nname = \"fresh\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(folder.join("src/lib.rs"), "").unwrap();
    folder
}

#[test]
fn cargo_commands_that_build_nothing_run_while_a_build_holds_the_only_slot() {
    let gate = Gate::new(1);
    let holder = gate.compile("a");
    gate.wait_for_log("start a");
    let folder = fresh_crate(gate.home.path());
    let cargo = std::env::var_os("CARGO").unwrap_or("cargo".into());
    for command in [
        &["tree"][..],
        &["metadata", "--format-version", "1"],
        &["fmt"],
    ] {
        // A build would wait for the holder, which is released only afterwards.
        let output = gate
            .command(&cargo)
            .args(command)
            .current_dir(&folder)
            .env("RUSTC_WRAPPER", gate.path("wiffletree-build-gate"))
            .env(
                "CARGO_TARGET_DIR",
                gate.path(&format!("target-{}", command[0])),
            )
            .output()
            .unwrap();
        let fmt_missing = command[0] == "fmt"
            && String::from_utf8_lossy(&output.stderr).contains("no such command");
        assert!(
            output.status.success() || fmt_missing,
            "{command:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(gate.log(), ["start a"]);
    gate.release("a");
    assert!(finish(holder).status.success());
}

/// `CARGO_TARGET_<host triple>_RUNNER`, as the host sets it.
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

#[test]
fn a_cached_cargo_test_rerun_that_compiles_nothing_still_waits_for_a_slot() {
    let gate = Gate::new(1);
    let folder = fresh_crate(gate.home.path());
    fs::write(folder.join("src/lib.rs"), "#[test]\nfn passes() {}\n").unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or("cargo".into());
    let cargo_test = |extra: &[&str]| {
        let mut command = gate.command(&cargo);
        command
            .arg("test")
            .args(extra)
            .current_dir(&folder)
            .env("RUSTC_WRAPPER", gate.path("wiffletree-build-gate"))
            .env(runner_variable(), gate.path("wiffletree-build-gate"))
            .env("CARGO_TARGET_DIR", gate.path("target"));
        command
    };
    assert!(cargo_test(&["--no-run"]).status().unwrap().success());
    let holder = gate.compile("a");
    gate.wait_for_log("start a");

    let rerun = cargo_test(&[]).spawn().unwrap();
    gate.wait_for_a_waiter();
    gate.release("a");
    let rerun = finish(rerun);
    assert!(rerun.status.success());
    // The test binary waited, through the runner; nothing was compiled, so rustc never did.
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).contains("Waiting for a build slot");
    assert!(
        said(&rerun.stderr),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert!(!said(&rerun.stdout));
    assert!(finish(holder).status.success());
}

#[test]
fn a_cargo_watch_style_parent_holds_no_slot_between_the_builds_it_starts() {
    let gate = Gate::new(1);
    // A program named `cargo` that runs `<subcommand>.sh` beside it and waits for it: as
    // `cargo watch` it starts two `cargo build`s, waiting between them.
    let tools = gate.path("tools");
    fs::create_dir(&tools).unwrap();
    let cargo = tools.join("cargo");
    fs::write(
        tools.join("cargo.rs"),
        "fn main() {\n    let args: Vec<String> = std::env::args().collect();\n    let script = std::path::Path::new(&args[0]).with_file_name(format!(\"{}.sh\", args[1]));\n    let status = std::process::Command::new(\"/bin/sh\").arg(script).status().unwrap();\n    std::process::exit(status.code().unwrap_or(1));\n}\n",
    )
    .unwrap();
    let built = Command::new("rustc")
        .arg("-o")
        .arg(&cargo)
        .arg(tools.join("cargo.rs"))
        .status()
        .unwrap();
    assert!(built.success());
    let home = gate.home.path().display();
    let script = |name: &str, body: String| {
        fs::write(tools.join(format!("{name}.sh")), body).unwrap();
    };
    script(
        "build",
        format!("{home}/wiffletree-build-gate {home}/rustc --crate-name $NAME"),
    );
    script(
        "watch",
        format!(
            "NAME=w1 {cargo} build\ntouch {home}/between\nwhile [ -d {home} ] && [ ! -f {home}/go-on ]; do sleep 0.02; done\nNAME=w2 {cargo} build",
            cargo = cargo.display()
        ),
    );
    let watch = gate
        .command(&cargo)
        .arg("watch")
        .current_dir(&tools)
        .spawn()
        .unwrap();
    gate.wait_for_log("start w1");
    gate.release("w1");
    wait_until("the watch between builds", || gate.path("between").exists());

    let other = gate.compile("b");
    gate.wait_for_log("start b");
    gate.release("b");
    assert!(finish(other).status.success());
    fs::write(gate.path("go-on"), "").unwrap();
    gate.wait_for_log("start w2");
    gate.release("w2");
    assert!(finish(watch).status.success());
}

/// A Go module whose `name` program, or test, writes `go-started` and runs until `release-go`
/// appears.
fn go_module(gate: &Gate, file: &str, code: &str) -> Option<PathBuf> {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("go is not installed; skipped");
        return None;
    }
    let module = gate.path("module");
    fs::create_dir_all(&module).unwrap();
    fs::write(module.join("go.mod"), "module gated\n\ngo 1.21\n").unwrap();
    let wait = format!(
        "\tos.WriteFile({started:?}, nil, 0o644)\n\tfor {{\n\t\tif _, err := os.Stat({release:?}); err == nil {{\n\t\t\treturn\n\t\t}}\n\t\ttime.Sleep(20 * time.Millisecond)\n\t}}\n",
        started = gate.path("go-started").display().to_string(),
        release = gate.path("release-go").display().to_string(),
    );
    fs::write(module.join(file), code.replace("WAIT", &wait)).unwrap();
    Some(module)
}

fn go(gate: &Gate, module: &Path, args: &[&str]) -> Child {
    gate.command("go")
        .args(args)
        .current_dir(module)
        .env(
            "GOFLAGS",
            format!("-toolexec={}", gate.path("wiffletree-build-gate").display()),
        )
        .spawn()
        .unwrap()
}

#[test]
fn a_go_test_holds_its_slot_until_its_tests_end() {
    let gate = Gate::new(1);
    let test = "package gated\n\nimport (\n\t\"os\"\n\t\"testing\"\n\t\"time\"\n)\n\nfunc TestRunsUntilReleased(t *testing.T) {\nWAIT}\n";
    let Some(module) = go_module(&gate, "gated_test.go", test) else {
        return;
    };
    let go_test = go(&gate, &module, &["test", "-count=1", "./..."]);
    wait_until("the Go test running", || gate.path("go-started").exists());
    let compile = gate.compile("b");
    gate.wait_for_a_waiter();
    assert!(gate.log().is_empty(), "{:?}", gate.log());
    gate.release("go");
    gate.wait_for_log("start b");
    gate.release("b");
    assert!(finish(go_test).status.success());
    assert!(finish(compile).status.success());
}

#[test]
fn a_go_run_frees_its_slot_once_its_program_runs() {
    let gate = Gate::new(1);
    let program = "package main\n\nimport (\n\t\"os\"\n\t\"time\"\n)\n\nfunc main() {\nWAIT}\n";
    let Some(module) = go_module(&gate, "main.go", program) else {
        return;
    };
    let go_run = go(&gate, &module, &["run", "."]);
    wait_until("the Go program running", || {
        gate.path("go-started").exists()
    });
    // It would wait for the program, which is released only afterwards.
    let compile = gate.compile("b");
    gate.wait_for_log("start b");
    gate.release("b");
    assert!(finish(compile).status.success());
    gate.release("go");
    assert!(finish(go_run).status.success());
}
