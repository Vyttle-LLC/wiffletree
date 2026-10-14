//! Removing a ticket's worktree is whole whatever its size. Every test here runs Git through a
//! stand-in that refuses `git worktree remove`, as if the time limit had cut it off, and fails
//! `git worktree prune` in a repository holding a `fail-prune` mark, or after pruning in one
//! holding a `fail-after-prune` mark.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Once,
    time::{Duration, Instant},
};
use workspace_core::{Session, Ticket};
use workspace_host::Host;

fn stand_in_git() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let real = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        let real = String::from_utf8(real.stdout).unwrap();
        let bin = tempfile::tempdir().unwrap().keep();
        let script = bin.join("git");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 case \"$3 $4\" in\n\
                 \"worktree remove\") echo 'fatal: timed out' >&2; exit 1;;\n\
                 \"worktree prune\") [ -e \"$2/.git/fail-prune\" ] && {{ echo 'fatal: injected' >&2; exit 1; }}\n\
                 [ -e \"$2/.git/fail-after-prune\" ] && {{ {real} \"$@\"; echo 'fatal: timed out' >&2; exit 1; }};;\n\
                 esac\n\
                 exec {real} \"$@\"\n",
                real = real.trim()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        // SAFETY: set once, before any test in this binary runs Git.
        unsafe { std::env::set_var("PATH", path) };
    });
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// A host with the `web` repository and a ticket whose worktree holds committed work and
/// `files` ignored build outputs.
fn ticket_with_build_output(directory: &Path, files: usize) -> (Host, Session, Ticket) {
    stand_in_git();
    let mut host = Host::open_set_up(directory.join("home")).unwrap();
    host.set_workspaces_dir(directory.join("workspaces").to_str().unwrap())
        .unwrap();
    let project = host.create_project("Theme rollout").unwrap();
    let coordinator = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id)
        .unwrap();
    let web = directory.join("web");
    fs::create_dir_all(&web).unwrap();
    fs::write(web.join(".gitignore"), "target/\n").unwrap();
    git(&web, &["init", "-q", "-b", "main"]);
    git(&web, &["add", ".gitignore"]);
    git(&web, &["commit", "-q", "-m", "Fixture"]);
    let repository = host
        .attach_repository(&project.id, web.to_str().unwrap(), "HEAD")
        .unwrap();
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, "Toolbar", "Style it")
        .unwrap();
    let worktree = Path::new(&ticket.worktree);
    fs::write(worktree.join("style.css"), "body {}\n").unwrap();
    git(worktree, &["add", "style.css"]);
    git(worktree, &["commit", "-q", "-m", "Style"]);
    for file in 0..files {
        let folder = worktree.join(format!("target/debug/{}", file % 50));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{file}.o")), "binary").unwrap();
    }
    (host, coordinator, ticket)
}

fn removal_folder(ticket: &Ticket) -> PathBuf {
    Path::new(&ticket.worktree).with_file_name(format!(".removing-{}", ticket.id))
}

fn deleted_soon(folder: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while folder.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    !folder.exists()
}

fn registered(directory: &Path, ticket: &Ticket) -> bool {
    git(&directory.join("web"), &["worktree", "list", "--porcelain"]).contains(&ticket.branch)
}

#[test]
fn a_worktree_with_large_build_output_is_removed_completely() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 2_000);

    host.close_ticket(&ticket.id).unwrap();

    assert_eq!(host.ticket(&ticket.id).unwrap().state, "closed");
    assert!(!Path::new(&ticket.worktree).exists());
    assert!(!registered(directory.path(), &ticket));
    assert!(deleted_soon(&removal_folder(&ticket)));
    git(
        &directory.path().join("web"),
        &["rev-parse", "--verify", &ticket.branch],
    );
}

#[test]
fn a_failed_git_step_leaves_the_worktree_whole_and_the_ticket_open() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 10);
    let mark = directory.path().join("web/.git/fail-prune");
    fs::write(&mark, "").unwrap();

    let refused = host.close_ticket(&ticket.id).unwrap_err();

    assert!(format!("{refused:#}").contains("injected"), "{refused:#}");
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "planned");
    let worktree = Path::new(&ticket.worktree);
    assert!(worktree.join("style.css").exists());
    assert!(worktree.join("target/debug/0/0.o").exists());
    assert!(!removal_folder(&ticket).exists());
    assert!(registered(directory.path(), &ticket));
    assert_eq!(git(worktree, &["status", "--porcelain"]), "");

    fs::remove_file(&mark).unwrap();
    host.close_ticket(&ticket.id).unwrap();
    assert!(!worktree.exists());
}

#[test]
fn a_folder_left_aside_by_a_stop_is_deleted_at_start() {
    let directory = tempfile::tempdir().unwrap();
    let (host, _, ticket) = ticket_with_build_output(directory.path(), 0);
    let left = removal_folder(&ticket);
    fs::create_dir_all(left.join("target")).unwrap();
    fs::write(left.join("target/build.o"), "binary").unwrap();

    host.delete_removed_worktrees().unwrap();

    assert!(deleted_soon(&left));
    assert!(Path::new(&ticket.worktree).join("style.css").exists());
}

#[test]
fn a_git_failure_after_it_forgot_the_worktree_still_completes_the_removal() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 10);
    fs::write(directory.path().join("web/.git/fail-after-prune"), "").unwrap();

    host.close_ticket(&ticket.id).unwrap();

    assert_eq!(host.ticket(&ticket.id).unwrap().state, "closed");
    assert!(!Path::new(&ticket.worktree).exists());
    assert!(!registered(directory.path(), &ticket));
    assert!(deleted_soon(&removal_folder(&ticket)));
}
