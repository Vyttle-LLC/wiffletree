//! Removing a ticket's worktree is whole whatever its size. Every test here runs Git through a
//! stand-in that refuses `git worktree remove`, as if the time limit had cut it off, and fails
//! `git worktree prune` in a repository holding a `fail-prune` mark, after pruning in one holding
//! a `fail-after-prune` mark, or after deleting the worktrees' administrative file the
//! `fail-mid-prune` mark names, as a prune cut off partway can.
mod common;
use common::git;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Once,
    time::{Duration, Instant},
};
use workspace_core::{Session, Ticket, TicketState};
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
                 [ -e \"$2/.git/fail-after-prune\" ] && {{ {real} \"$@\"; echo 'fatal: timed out' >&2; exit 1; }}\n\
                 [ -e \"$2/.git/fail-mid-prune\" ] && {{ rm -f \"$2\"/.git/worktrees/*/$(cat \"$2/.git/fail-mid-prune\"); echo 'fatal: timed out' >&2; exit 1; }};;\n\
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

    assert_eq!(host.ticket(&ticket.id).unwrap().state, TicketState::Closed);
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
    assert_eq!(host.ticket(&ticket.id).unwrap().state, TicketState::Planned);
    let worktree = Path::new(&ticket.worktree);
    assert!(worktree.join("style.css").exists());
    assert!(worktree.join("target/debug/0/0.o").exists());
    assert!(!removal_folder(&ticket).exists());
    assert!(registered(directory.path(), &ticket));
    assert_eq!(git(worktree, &["status", "--porcelain"]), "");
    git(worktree, &["rev-parse", "--verify", "HEAD"]);

    fs::remove_file(&mark).unwrap();
    host.close_ticket(&ticket.id).unwrap();
    assert!(!worktree.exists());
}

fn store(directory: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(directory.join("home/workspace.sqlite3")).unwrap()
}

fn recorded_deletions(directory: &Path) -> Vec<(String, i64, Option<String>)> {
    store(directory)
        .prepare("SELECT path,attempts,last_error FROM worktree_deletions ORDER BY path")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// Settles at `at` once every delete running has finished, as the service does when told.
fn settle_finished(host: &mut Host, at: i64) -> Option<i64> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while host.worktree_deletions_running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        host.settle_worktree_deletions(at);
    }
    host.settle_worktree_deletions(at)
}

#[test]
fn a_removal_is_recorded_until_its_folder_is_gone() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 10);

    host.close_ticket(&ticket.id).unwrap();
    let recorded = recorded_deletions(directory.path());
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, removal_folder(&ticket).to_string_lossy());

    assert_eq!(settle_finished(&mut host, workspace_host::now()), None);
    assert!(!removal_folder(&ticket).exists());
    assert!(recorded_deletions(directory.path()).is_empty());
}

#[test]
fn a_delete_cut_off_by_a_stop_finishes_at_the_next_start() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 10);
    // The host stopped partway: the folder was renamed aside and half deleted.
    let left = removal_folder(&ticket);
    fs::create_dir_all(left.join("target/debug")).unwrap();
    fs::write(left.join("target/debug/build.o"), "binary").unwrap();
    store(directory.path())
        .execute(
            "INSERT INTO worktree_deletions(path,ticket_id,recorded_at,attempts,next_attempt_at)
             VALUES (?1,?2,1,0,?3)",
            rusqlite::params![left.to_string_lossy(), ticket.id, i64::MAX],
        )
        .unwrap();
    drop(host);
    host = Host::open(directory.path().join("home")).unwrap();

    host.delete_removed_worktrees().unwrap();

    assert_eq!(settle_finished(&mut host, workspace_host::now()), None);
    assert!(!left.exists());
    assert!(recorded_deletions(directory.path()).is_empty());
    assert!(Path::new(&ticket.worktree).join("style.css").exists());
}

#[test]
fn a_removed_folder_nothing_records_is_found_and_deleted_at_start() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 0);
    let repository_folder = Path::new(&ticket.worktree).parent().unwrap();
    let unknown = repository_folder.join(".removing-from-an-unknown-ticket");
    fs::create_dir_all(unknown.join("target")).unwrap();
    let other = repository_folder.join("notes");
    fs::create_dir_all(&other).unwrap();
    // Inside a project's repository folder no ticket names any more.
    let elsewhere = directory
        .path()
        .join("workspaces/tasks/old-project/old-repo/.removing-gone");
    fs::create_dir_all(&elsewhere).unwrap();

    host.delete_removed_worktrees().unwrap();

    assert_eq!(settle_finished(&mut host, workspace_host::now()), None);
    assert!(!unknown.exists());
    assert!(!elsewhere.exists());
    assert!(other.exists());
    assert!(Path::new(&ticket.worktree).join("style.css").exists());
}

#[test]
fn a_delete_that_keeps_failing_is_retried_then_reported_once_with_folder_reason_and_size() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, coordinator, ticket) = ticket_with_build_output(directory.path(), 10);
    // Ignored build output in a read-only folder, whose file cannot be deleted.
    let stuck = Path::new(&ticket.worktree).join("target/locked");
    fs::create_dir_all(&stuck).unwrap();
    fs::write(stuck.join("build.o"), vec![0u8; 64 * 1024]).unwrap();
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o555)).unwrap();
    let left = removal_folder(&ticket);
    let stuck = left.join("target/locked");

    let mut at = workspace_host::now();
    host.close_ticket(&ticket.id).unwrap();
    let mut waits = vec![];
    for attempt in 1..=4 {
        if attempt > 1 {
            // Starts the attempt that came due.
            host.settle_worktree_deletions(at);
        }
        let due = settle_finished(&mut host, at).unwrap();
        waits.push(due - at);
        at = due;
    }

    assert_eq!(waits, [15_000, 30_000, 60_000, 120_000]);
    let recorded = recorded_deletions(directory.path());
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].1, 4);
    assert!(
        recorded[0]
            .2
            .as_deref()
            .unwrap()
            .contains("ermission denied")
    );
    // The human's inbox, under the project's coordinator, is told.
    let notices: Vec<String> = store(directory.path())
        .prepare(
            "SELECT json_extract(data,'$.prompt') FROM attention
             WHERE operation_id LIKE 'worktree-delete-failed:%' AND session_id=?1",
        )
        .unwrap()
        .query_map([&coordinator.id], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    let notice = &notices[0];
    assert!(notice.contains(&*left.to_string_lossy()), "{notice}");
    assert!(notice.contains("ermission denied"), "{notice}");
    assert!(notice.contains(" MB)"), "{notice}");

    // Once the folder can go, the next attempt deletes it and clears the record.
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o755)).unwrap();
    host.settle_worktree_deletions(at);
    assert_eq!(settle_finished(&mut host, at), None);
    assert!(!left.exists());
    assert!(recorded_deletions(directory.path()).is_empty());
}

#[test]
fn a_folder_a_process_wiffletree_did_not_start_still_uses_waits_for_it() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 10);
    // A build outside Wiffletree's records, working in the build output.
    let mut build = Command::new("sleep")
        .arg("60")
        .current_dir(Path::new(&ticket.worktree).join("target/debug"))
        .spawn()
        .unwrap();
    let left = removal_folder(&ticket);

    let at = workspace_host::now();
    host.close_ticket(&ticket.id).unwrap();
    let due = settle_finished(&mut host, at);

    let alive = build.try_wait().unwrap().is_none();
    build.kill().unwrap();
    build.wait().unwrap();
    assert!(alive, "the build was signalled");
    assert!(left.join("target/debug/0/0.o").exists());
    let recorded = recorded_deletions(directory.path());
    let error = recorded[0].2.as_deref().unwrap();
    assert!(
        error.contains(&format!("{} (sleep)", build.id())),
        "{error}"
    );

    let at = due.unwrap();
    host.settle_worktree_deletions(at);
    assert_eq!(settle_finished(&mut host, at), None);
    assert!(!left.exists());
    assert!(recorded_deletions(directory.path()).is_empty());
}

#[test]
fn a_failing_delete_no_ticket_names_is_reported_in_the_inbox() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, coordinator, ticket) = ticket_with_build_output(directory.path(), 0);
    let orphan = Path::new(&ticket.worktree).with_file_name(".removing-gone");
    let stuck = orphan.join("target");
    fs::create_dir_all(&stuck).unwrap();
    fs::write(stuck.join("build.o"), "binary").unwrap();
    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o555)).unwrap();

    host.delete_removed_worktrees().unwrap();
    let mut at = workspace_host::now();
    for attempt in 1..=4 {
        if attempt > 1 {
            host.settle_worktree_deletions(at);
        }
        at = settle_finished(&mut host, at).unwrap();
    }

    fs::set_permissions(&stuck, fs::Permissions::from_mode(0o755)).unwrap();
    let prompts: Vec<String> = store(directory.path())
        .prepare("SELECT json_extract(data,'$.prompt') FROM attention WHERE operation_id LIKE 'worktree-delete-failed:%' AND session_id=?1")
        .unwrap()
        .query_map([&coordinator.id], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(prompts[0].contains(".removing-gone"), "{}", prompts[0]);
}

#[test]
fn a_folder_left_aside_by_a_stop_is_deleted_at_start() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 0);
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

    assert_eq!(host.ticket(&ticket.id).unwrap().state, TicketState::Closed);
    assert!(!Path::new(&ticket.worktree).exists());
    assert!(!registered(directory.path(), &ticket));
    assert!(deleted_soon(&removal_folder(&ticket)));
}

#[test]
fn a_prune_cut_off_partway_completes_the_removal_and_the_next_start_prunes() {
    for deleted in ["HEAD", "index"] {
        prune_cut_off_after_deleting(deleted);
    }
}

fn prune_cut_off_after_deleting(file: &str) {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, _, ticket) = ticket_with_build_output(directory.path(), 10);
    let mark = directory.path().join("web/.git/fail-mid-prune");
    fs::write(&mark, file).unwrap();

    host.close_ticket(&ticket.id).unwrap();

    assert_eq!(host.ticket(&ticket.id).unwrap().state, TicketState::Closed);
    assert!(!Path::new(&ticket.worktree).exists());
    assert!(deleted_soon(&removal_folder(&ticket)));
    git(
        &directory.path().join("web"),
        &["rev-parse", "--verify", &ticket.branch],
    );

    fs::remove_file(&mark).unwrap();
    host.delete_removed_worktrees().unwrap();
    assert!(!registered(directory.path(), &ticket));
}
