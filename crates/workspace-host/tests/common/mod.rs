//! Helpers shared by the host's integration tests. Each test binary uses only some of them.
#![allow(dead_code)]
use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

/// A detection that finds `signed_in` installed and signed in, each listing the built-in
/// catalog, as the one-time upgrade to the Who runs what table reads it.
pub fn signed_in(signed_in: &[Provider]) -> impl Fn(Provider) -> ProviderReport + use<> {
    let signed_in = signed_in.to_vec();
    move |provider| {
        let found = signed_in.contains(&provider);
        ProviderReport {
            status: ProviderStatus {
                installed: found,
                signed_in: found,
                plan: None,
            },
            models: (fallback_catalog().into_iter())
                .filter(|m| found && m.provider == provider)
                .collect(),
        }
    }
}

/// `verify_ticket`'s arguments for the level "Full".
pub fn verify_args(ticket_id: &str) -> Value {
    json!({"ticket_id": ticket_id, "level": "Full"})
}

/// Verification by the default tester alone, for tests that pass or fail one verifier.
pub fn one_tester(host: &mut Host) {
    let mut verification = VerificationSettings::default();
    verification.verifiers.truncate(1);
    verification.levels[0].verifiers.truncate(1);
    host.set_verification(verification).unwrap();
}

/// A process's start time in microseconds, as the kernel reports it.
pub fn process_started(pid: i32) -> i64 {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    assert_eq!(read, size, "process {pid} is not running");
    (info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec) as i64
}

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use workspace_host::service::Service;

pub fn git_output(path: &Path, args: &[&str]) -> std::process::Output {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    if let Some(repository) = created(path, args).filter(|_| output.status.success()) {
        without_background_maintenance(&repository);
    }
    output
}

/// The repository `git init` or `git clone` made in `path`: the folder its last argument names,
/// or `path` itself when that argument is an option or its value.
fn created(path: &Path, args: &[&str]) -> Option<PathBuf> {
    if !matches!(args.first(), Some(&("init" | "clone"))) {
        return None;
    }
    let named = path.join(args.last().unwrap());
    Some(if named.is_dir() { named } else { path.into() })
}

/// Keeps Git from repacking a test repository in the background, as it may after any commit or
/// push. The repack deletes loose objects that a local clone of the repository may be copying.
fn without_background_maintenance(repository: &Path) {
    git(repository, &["config", "maintenance.auto", "false"]);
    git(repository, &["config", "gc.auto", "0"]);
}

pub fn git(path: &Path, args: &[&str]) -> String {
    let output = git_output(path, args);
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub fn request(service: &Service, command: Command) -> Value {
    service
        .request(command)
        .unwrap()
        .recv_blocking()
        .unwrap()
        .unwrap()
}

pub fn snapshot(service: &Service) -> Snapshot {
    serde_json::from_value(request(service, Command::Snapshot)).unwrap()
}

pub fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Reads snapshots until one satisfies `done`; reading them never triggers admission.
pub fn until_snapshot(service: &Service, what: &str, mut done: impl FnMut(&Snapshot) -> bool) {
    until(what, || done(&snapshot(service)));
}

/// Waits for `done`, looking again on the host's change signal, which every turn start and
/// finish sends, so the result does not depend on the machine's load. Fails at `deadline`;
/// the panic drops the service, which stops the turns still running.
pub fn until_changed(
    service: &Service,
    deadline: Instant,
    done: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    let changes = service.changes();
    loop {
        let snapshot = snapshot(service);
        if done(&snapshot) {
            return snapshot;
        }
        while changes.try_recv().is_err() {
            assert!(
                Instant::now() < deadline,
                "the turns did not finish before the deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Setup shared by fake-provider tests. Tests own their temporary folders and store setup.
pub struct ServiceFixture {
    home: PathBuf,
    provider: Option<PathBuf>,
    environment: Vec<(String, PathBuf)>,
}
impl ServiceFixture {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.into(),
            provider: None,
            environment: vec![],
        }
    }
    pub fn fake_provider(self, usage: &Path) -> Self {
        self.provider(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider.py"))
            .env("WORKSPACE_TEST_USAGE_DIR", usage)
    }
    pub fn provider(mut self, provider: &Path) -> Self {
        self.provider = Some(provider.into());
        self
    }
    pub fn env(mut self, key: &str, value: &Path) -> Self {
        self.environment.push((key.into(), value.into()));
        self
    }
    /// The caller must serialize tests using the process environment and keep it unchanged
    /// while the service runs. Each fixture binary has one test or holds an environment lock.
    pub unsafe fn start(self) -> Service {
        unsafe {
            if let Some(provider) = self.provider {
                std::env::set_var("WORKSPACE_CLAUDE_BIN", &provider);
                std::env::set_var("WORKSPACE_CODEX_BIN", &provider);
            }
            for (key, value) in self.environment {
                std::env::set_var(key, value);
            }
        }
        Service::start(self.home, env!("CARGO_BIN_EXE_workspace-host").into()).unwrap()
    }
}
