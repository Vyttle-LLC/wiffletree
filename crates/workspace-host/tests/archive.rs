use serde_json::json;
use std::{fs, path::Path, process::Command};
use workspace_core::{Provider, Role, Session, Status, Ticket};
use workspace_host::Host;

fn git(path: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap()
}

fn succeed(path: &Path, args: &[&str]) {
    let output = git(path, args);
    assert!(output.status.success(), "{output:?}");
}

/// A repository whose build output under target/ is ignored.
fn repository(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join(".gitignore"), "target/\n").unwrap();
    succeed(path, &["init", "-q", "-b", "main"]);
    succeed(path, &["add", ".gitignore"]);
    succeed(path, &["commit", "-q", "-m", "Fixture"]);
}

fn team(host: &mut Host, directory: &Path) -> (Session, Session) {
    host.set_workspaces_dir(directory.join("workspaces").to_str().unwrap())
        .unwrap();
    let project = host.create_project("Theme rollout").unwrap();
    let root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id)
        .unwrap();
    repository(&directory.join("web"));
    let attached = host
        .attach_repository(&project.id, directory.join("web").to_str().unwrap(), "HEAD")
        .unwrap();
    let coordinator = host
        .create_session(
            &project.id,
            &root.id,
            Some(&attached.id),
            "Web",
            Role::TaskOrchestrator,
            Provider::Codex,
        )
        .unwrap();
    (root, coordinator)
}

/// A ticket with committed work and ignored build output in its worktree.
fn worked_ticket(host: &mut Host, coordinator: &Session, title: &str) -> Ticket {
    let ticket = host
        .create_ticket(&coordinator.id, title, "Style it")
        .unwrap();
    let worktree = Path::new(&ticket.worktree);
    fs::write(worktree.join("style.css"), "body {}\n").unwrap();
    succeed(worktree, &["add", "style.css"]);
    succeed(worktree, &["commit", "-q", "-m", "Style"]);
    fs::create_dir_all(worktree.join("target")).unwrap();
    fs::write(worktree.join("target/build.o"), "binary").unwrap();
    ticket
}

fn branch_exists(directory: &Path, ticket: &Ticket) -> bool {
    git(
        &directory.join("web"),
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", ticket.branch),
        ],
    )
    .status
    .success()
}

fn passed(host: &mut Host, ticket: &Ticket) {
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
        .unwrap();
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"done","kind":"passed","body":"Green"}),
    )
    .unwrap();
}

#[test]
fn archiving_a_project_hides_its_tree_stops_it_and_keeps_its_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    host.send("kept", None, &root.id, "Keep this conversation")
        .unwrap();
    host.set_live(&root.project_id, true).unwrap();

    host.set_archived(&root.id, true).unwrap();

    assert!(host.session(&root.id).unwrap().archived);
    assert!(host.session(&coordinator.id).unwrap().archived);
    assert!(host.live_projects().unwrap().is_empty());
    let refused = host.send("late", None, &root.id, "Hello").unwrap_err();
    assert!(refused.to_string().contains("archived"), "{refused}");
    drop(host);

    let host = Host::open(directory.path().join("home")).unwrap();
    assert!(host.session(&root.id).unwrap().archived);
    assert_eq!(
        host.messages(&root.id, None, 100).unwrap()[0].body,
        "Keep this conversation"
    );
}

#[test]
fn restoring_a_team_restores_its_project_but_archiving_a_team_leaves_the_project() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());

    host.set_archived(&coordinator.id, true).unwrap();
    assert!(!host.session(&root.id).unwrap().archived);
    let refused = host.create_ticket(&coordinator.id, "Toolbar", "Style it");
    assert!(refused.is_err());

    host.set_archived(&root.id, true).unwrap();
    host.set_archived(&coordinator.id, false).unwrap();
    assert!(!host.session(&root.id).unwrap().archived);
    assert!(!host.session(&coordinator.id).unwrap().archived);
    host.send("after", None, &coordinator.id, "Welcome back")
        .unwrap();
}

#[test]
fn archiving_a_team_removes_clean_worktrees_and_restoring_re_creates_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    host.send("kept", None, &coordinator.id, "Keep this conversation")
        .unwrap();

    host.set_archived(&coordinator.id, true).unwrap();

    assert!(!Path::new(&ticket.worktree).exists());
    assert!(branch_exists(directory.path(), &ticket));
    drop(host);
    let mut host = Host::open(directory.path().join("home")).unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "planned");
    assert_eq!(
        host.messages(&coordinator.id, None, 100).unwrap()[0].body,
        "Keep this conversation"
    );

    host.set_archived(&coordinator.id, false).unwrap();

    let worktree = Path::new(&ticket.worktree);
    assert_eq!(
        String::from_utf8(git(worktree, &["branch", "--show-current"]).stdout)
            .unwrap()
            .trim(),
        ticket.branch
    );
    assert!(worktree.join("style.css").exists());
    assert!(!host.session(&root.id).unwrap().archived);
}

#[test]
fn restoring_without_the_ticket_branch_names_the_ticket() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    host.set_archived(&coordinator.id, true).unwrap();
    succeed(
        &directory.path().join("web"),
        &["branch", "-D", &ticket.branch],
    );

    let refused = host
        .set_archived(&coordinator.id, false)
        .unwrap_err()
        .to_string();

    assert!(refused.contains(&ticket.id), "{refused}");
    assert!(refused.contains("\"Toolbar\""), "{refused}");
    assert!(host.session(&coordinator.id).unwrap().archived);
}

#[test]
fn unsaved_work_or_a_working_agent_blocks_the_whole_archive() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let clean = worked_ticket(&mut host, &coordinator, "Clean");
    let modified = worked_ticket(&mut host, &coordinator, "Modified");
    let untracked = worked_ticket(&mut host, &coordinator, "Untracked");
    fs::write(Path::new(&modified.worktree).join("style.css"), "edited").unwrap();
    fs::write(Path::new(&untracked.worktree).join("notes.md"), "draft").unwrap();
    let worker = host
        .assign_ticket(&clean.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    host.set_status(&worker.id, Status::Working).unwrap();

    let refused = host.set_archived(&root.id, true).unwrap_err().to_string();

    for blocker in [
        worker.name.as_str(),
        &modified.id,
        &untracked.id,
        "\"Modified\"",
    ] {
        assert!(refused.contains(blocker), "{refused}");
    }
    for ticket in [&clean, &modified, &untracked] {
        assert!(Path::new(&ticket.worktree).exists());
    }
    assert!(host.sessions().unwrap().iter().all(|s| !s.archived));
}

#[test]
fn closing_or_accepting_removes_a_clean_worktree_and_refuses_unsaved_work() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let closing = worked_ticket(&mut host, &coordinator, "Review");
    let accepting = worked_ticket(&mut host, &coordinator, "Toolbar");
    passed(&mut host, &accepting);
    let accept = json!({"ticket_id":accepting.id});
    for ticket in [&closing, &accepting] {
        fs::write(Path::new(&ticket.worktree).join("notes.md"), "draft").unwrap();
    }

    let refused = host.close_ticket(&closing.id).unwrap_err().to_string();
    assert!(refused.contains(&closing.id), "{refused}");
    let refused = host
        .agent_tool(&coordinator.id, "accept_ticket", accept.clone())
        .unwrap_err()
        .to_string();
    assert!(refused.contains(&accepting.id), "{refused}");
    assert_eq!(host.ticket(&closing.id).unwrap().state, "planned");
    assert_eq!(host.ticket(&accepting.id).unwrap().state, "passed");

    for ticket in [&closing, &accepting] {
        fs::remove_file(Path::new(&ticket.worktree).join("notes.md")).unwrap();
    }
    host.close_ticket(&closing.id).unwrap();
    host.agent_tool(&coordinator.id, "accept_ticket", accept)
        .unwrap();

    for ticket in [&closing, &accepting] {
        assert!(!Path::new(&ticket.worktree).exists());
        assert!(branch_exists(directory.path(), ticket));
    }
    assert_eq!(host.ticket(&accepting.id).unwrap().state, "accepted");
}

#[test]
fn missing_worktrees_neither_block_archiving_nor_opening_and_restore_re_creates_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let open = worked_ticket(&mut host, &coordinator, "Toolbar");
    let closing = worked_ticket(&mut host, &coordinator, "Review");
    for ticket in [&open, &closing] {
        fs::remove_dir_all(&ticket.worktree).unwrap();
    }

    host.close_ticket(&closing.id).unwrap();
    host.set_archived(&coordinator.id, true).unwrap();
    drop(host);
    let mut host = Host::open(directory.path().join("home")).unwrap();
    host.set_archived(&coordinator.id, false).unwrap();

    assert!(Path::new(&open.worktree).join("style.css").exists());
    assert!(!Path::new(&closing.worktree).exists());
}

#[test]
fn an_agent_that_reported_but_is_still_in_its_turn_blocks_removal() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
        .unwrap();
    let db = rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES ('live',?1,'[]',1,'{}')",
        [&tester.id],
    )
    .unwrap();
    host.set_status(&tester.id, Status::Working).unwrap();
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"done","kind":"passed","body":"Green"}),
    )
    .unwrap();
    let accept = json!({"ticket_id":ticket.id});

    for refused in [
        host.agent_tool(&coordinator.id, "accept_ticket", accept.clone())
            .unwrap_err(),
        host.close_ticket(&ticket.id).unwrap_err(),
        host.set_archived(&coordinator.id, true).unwrap_err(),
    ] {
        assert!(refused.to_string().contains(&tester.name), "{refused}");
    }
    assert!(Path::new(&ticket.worktree).exists());
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "passed");

    db.execute("UPDATE provider_runs SET finished_at=2 WHERE id='live'", [])
        .unwrap();
    host.agent_tool(&coordinator.id, "accept_ticket", accept)
        .unwrap();
    assert!(!Path::new(&ticket.worktree).exists());
}

#[test]
fn a_locked_worktree_blocks_the_archive_before_anything_is_removed() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let first = worked_ticket(&mut host, &coordinator, "First");
    let locked = worked_ticket(&mut host, &coordinator, "Locked");
    succeed(
        &directory.path().join("web"),
        &["worktree", "lock", &locked.worktree],
    );

    let refused = host
        .set_archived(&coordinator.id, true)
        .unwrap_err()
        .to_string();

    assert!(
        refused.contains(&locked.id) && refused.contains("locked"),
        "{refused}"
    );
    assert!(Path::new(&first.worktree).exists());
    assert!(!host.session(&coordinator.id).unwrap().archived);
}

#[test]
fn a_failed_archive_re_creates_the_worktrees_it_removed() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let db = rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_archive BEFORE INSERT ON activity WHEN NEW.kind='archived'
         BEGIN SELECT RAISE(ABORT,'injected'); END;",
    )
    .unwrap();

    assert!(host.set_archived(&coordinator.id, true).is_err());

    assert!(Path::new(&ticket.worktree).join("style.css").exists());
    assert!(!host.session(&coordinator.id).unwrap().archived);
}

#[test]
fn an_empty_folder_left_on_a_stale_worktree_is_replaced_and_any_other_folder_refused() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let worktree = Path::new(&ticket.worktree);
    fs::remove_dir_all(worktree).unwrap();
    fs::create_dir(worktree).unwrap();

    host.set_archived(&coordinator.id, false).unwrap();
    assert!(worktree.join("style.css").exists());

    fs::remove_dir_all(worktree).unwrap();
    fs::create_dir(worktree).unwrap();
    fs::write(worktree.join("stray.txt"), "keep me").unwrap();
    let refused = host
        .set_archived(&coordinator.id, false)
        .unwrap_err()
        .to_string();
    assert!(refused.contains(&ticket.id), "{refused}");
    assert!(worktree.join("stray.txt").exists());
}

/// Makes recording the activity `kind` fail until the returned connection drops the trigger.
fn fail_activity(directory: &Path, kind: &str) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(directory.join("home/workspace.sqlite3")).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER fail_activity BEFORE INSERT ON activity WHEN NEW.kind='{kind}'
         BEGIN SELECT RAISE(ABORT,'injected'); END;"
    ))
    .unwrap();
    db
}

#[test]
fn a_failed_close_keeps_the_ticket_its_agents_and_worktree_and_a_retry_closes_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Review");
    let reviewer = host
        .assign_ticket(&ticket.id, Role::Reviewer, Provider::Claude, "Review", None)
        .unwrap();
    let db = fail_activity(directory.path(), "ticket_closed");

    assert!(host.close_ticket(&ticket.id).is_err());

    assert_eq!(host.ticket(&ticket.id).unwrap().state, "assigned");
    assert!(!host.session(&reviewer.id).unwrap().archived);
    assert!(Path::new(&ticket.worktree).join("style.css").exists());

    db.execute_batch("DROP TRIGGER fail_activity").unwrap();
    host.close_ticket(&ticket.id).unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "closed");
    assert!(host.session(&reviewer.id).unwrap().archived);
    assert!(!Path::new(&ticket.worktree).exists());
}

#[test]
fn a_failed_team_archive_keeps_accepted_tickets_open_to_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    passed(&mut host, &ticket);
    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    let archive = json!({"session_id":coordinator.id});
    let db = fail_activity(directory.path(), "team_archived");

    assert!(
        host.agent_tool(&root.id, "archive_team", archive.clone())
            .is_err()
    );

    assert_eq!(host.ticket(&ticket.id).unwrap().state, "accepted");
    assert!(!host.session(&coordinator.id).unwrap().archived);
    assert!(!Path::new(&ticket.worktree).exists());

    db.execute_batch("DROP TRIGGER fail_activity").unwrap();
    host.agent_tool(&root.id, "archive_team", archive).unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "closed");
    assert!(host.session(&coordinator.id).unwrap().archived);
}
