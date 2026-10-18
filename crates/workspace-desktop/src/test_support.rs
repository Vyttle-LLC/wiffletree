//! What several views' tests set up.
use std::path::Path;
use workspace_core::{Session, Ticket};
use workspace_host::Host;

/// Runs Git in `path` as a test author; it must succeed.
pub(crate) fn git(path: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// A temporary store with a project using `sagechat-sms` and one ticket in it.
pub(crate) fn ticket_store() -> (tempfile::TempDir, Host, Session, Ticket) {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("sagechat-sms");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Base"]);
    let mut host = Host::open_set_up(directory.path().join("home")).unwrap();
    host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let project = host.create_project("Calls").unwrap();
    let root = host.sessions().unwrap().remove(0);
    let repository = host
        .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
        .unwrap();
    let ticket = host
        .create_ticket(&root.id, &repository.id, "Fix inbound calls", "Fix them")
        .unwrap();
    (directory, host, root, ticket)
}
