//! The service learns which CLIs are signed in before it handles a request, without holding
//! up its client, so nothing is pinned to a provider that is not signed in.
//! Kept in its own test binary because it sets the provider environment for the process.
mod common;
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::Host;

/// A stand-in CLI that answers its sign-in status after `delay` seconds.
fn cli(folder: &Path, name: &str, delay: u32, status: &str) -> std::path::PathBuf {
    let path = folder.join(name);
    let script = format!("#!/bin/sh\nsleep {delay}\n{status}\n");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_signed_out_cli_is_never_chosen_even_while_its_status_is_pending() {
    let home = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let workspaces = folder.path().join("workspaces");
    host.set_workspaces_dir(workspaces.to_str().unwrap())
        .unwrap();
    drop(host);
    // Claude is installed but signed out, and slow to say so; Codex is signed in.
    let claude = cli(folder.path(), "claude", 3, r#"echo '{"loggedIn":false}'"#);
    let codex = cli(
        folder.path(),
        "codex",
        1,
        "echo 'Logged in using ChatGPT' >&2",
    );
    let started = Instant::now();
    // SAFETY: this test binary holds one test, so nothing else reads the environment meanwhile.
    let service = unsafe {
        common::ServiceFixture::new(home.path())
            .env("WORKSPACE_CLAUDE_BIN", &claude)
            .env("WORKSPACE_CODEX_BIN", &codex)
            .start()
    };
    // The client is not held up while the statuses are read.
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );

    // Asked at once, while Claude's status is still pending.
    let project = common::request(
        &service,
        Command::CreateProject {
            name: "Signed out".into(),
        },
    );
    let project: Project = serde_json::from_value(project).unwrap();
    let snapshot = common::snapshot(&service);
    let coordinator = (snapshot.sessions.iter())
        .find(|s| s.project_id == project.id)
        .unwrap();
    assert_eq!(coordinator.provider, Provider::Codex);
    let runtime = (snapshot.runtimes.iter())
        .find(|r| r.session_id == coordinator.id)
        .unwrap();
    let selection = runtime.selection.clone().unwrap();
    assert_eq!(
        selection.reason,
        "Started on Codex: Claude is not signed in on this Mac. First choice was Opus · high."
    );
    assert!(!runtime.waiting);
}
