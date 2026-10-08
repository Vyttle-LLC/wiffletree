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
fn unsaved_work_blocks_the_whole_archive() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let clean = worked_ticket(&mut host, &coordinator, "Clean");
    let modified = worked_ticket(&mut host, &coordinator, "Modified");
    let untracked = worked_ticket(&mut host, &coordinator, "Untracked");
    fs::write(Path::new(&modified.worktree).join("style.css"), "edited").unwrap();
    fs::write(Path::new(&untracked.worktree).join("notes.md"), "draft").unwrap();

    let refused = host.set_archived(&root.id, true).unwrap_err().to_string();

    for blocker in [modified.id.as_str(), &untracked.id, "\"Modified\""] {
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

/// The store, for opening and finishing provider runs as the service would.
fn store(directory: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(directory.join("home/workspace.sqlite3")).unwrap()
}

fn start_turn(db: &rusqlite::Connection, agent: &Session) {
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?1,'[]',1,'{}')",
        [&agent.id],
    )
    .unwrap();
}

fn finish_turn(db: &rusqlite::Connection, host: &mut Host, agent: &Session) {
    db.execute(
        "UPDATE provider_runs SET finished_at=2 WHERE id=?1",
        [&agent.id],
    )
    .unwrap();
    host.settle_pending_worktrees();
}

/// A passed ticket whose tester reported and is still writing its summary.
fn reported_in_turn(
    host: &mut Host,
    db: &rusqlite::Connection,
    coordinator: &Session,
) -> (Ticket, Session) {
    let ticket = worked_ticket(host, coordinator, "Toolbar");
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
        .unwrap();
    start_turn(db, &tester);
    host.set_status(&tester.id, Status::Working).unwrap();
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"done","kind":"passed","body":"Green"}),
    )
    .unwrap();
    (ticket, tester)
}

fn notices(host: &Host, session: &Session) -> Vec<String> {
    host.messages(&session.id, None, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.id.starts_with("worktree-kept:"))
        .map(|m| m.body)
        .collect()
}

#[test]
fn accepting_while_the_tester_is_in_its_turn_removes_the_worktree_when_the_turn_ends() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let (ticket, tester) = reported_in_turn(&mut host, &db, &coordinator);

    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "accepted");
    assert!(Path::new(&ticket.worktree).exists());
    assert_eq!(
        host.pending_worktree_removals().unwrap(),
        std::slice::from_ref(&ticket.id)
    );
    host.settle_pending_worktrees();
    assert!(Path::new(&ticket.worktree).exists());

    // The pending removal survives a restart.
    drop(host);
    let mut host = Host::open(directory.path().join("home")).unwrap();
    assert_eq!(
        host.pending_worktree_removals().unwrap(),
        std::slice::from_ref(&ticket.id)
    );

    finish_turn(&db, &mut host, &tester);

    assert!(!Path::new(&ticket.worktree).exists());
    assert!(branch_exists(directory.path(), &ticket));
    assert!(host.pending_worktree_removals().unwrap().is_empty());
}

#[test]
fn work_left_after_acceptance_keeps_the_worktree_and_tells_the_coordinator_once() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let (ticket, tester) = reported_in_turn(&mut host, &db, &coordinator);
    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    fs::write(Path::new(&ticket.worktree).join("summary.md"), "late").unwrap();

    finish_turn(&db, &mut host, &tester);
    host.settle_pending_worktrees();

    assert!(Path::new(&ticket.worktree).join("summary.md").exists());
    assert!(host.pending_worktree_removals().unwrap().is_empty());
    let sent = notices(&host, &coordinator);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains(&ticket.id) && sent[0].contains(&ticket.worktree));
    assert!(sent[0].contains("untracked"), "{}", sent[0]);
}

#[test]
fn closing_while_the_reviewer_is_in_its_turn_removes_the_worktree_when_the_turn_ends() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Review");
    let reviewer = host
        .assign_ticket(&ticket.id, Role::Reviewer, Provider::Claude, "Review", None)
        .unwrap();
    start_turn(&db, &reviewer);
    host.agent_tool(
        &reviewer.id,
        "report",
        json!({"message_id":"done","kind":"completed","body":"Findings"}),
    )
    .unwrap();

    host.close_ticket(&ticket.id).unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "closed");
    assert!(Path::new(&ticket.worktree).exists());

    finish_turn(&db, &mut host, &reviewer);
    assert!(!Path::new(&ticket.worktree).exists());
}

#[test]
fn archiving_mid_turn_defers_removal_and_an_archived_tree_asks_the_human_about_kept_work() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let clean = worked_ticket(&mut host, &coordinator, "Clean");
    let left = worked_ticket(&mut host, &coordinator, "Left");
    let mut agents = vec![];
    for ticket in [&clean, &left] {
        let agent = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
            .unwrap();
        start_turn(&db, &agent);
        host.set_status(&agent.id, Status::Working).unwrap();
        agents.push(agent);
    }

    host.set_archived(&root.id, true).unwrap();
    assert!(host.session(&coordinator.id).unwrap().archived);
    assert_eq!(host.pending_worktree_removals().unwrap().len(), 2);
    // The cancelled agent left a file behind.
    fs::write(Path::new(&left.worktree).join("partial.rs"), "fn").unwrap();
    for agent in &agents {
        finish_turn(&db, &mut host, agent);
    }

    assert!(!Path::new(&clean.worktree).exists());
    assert!(Path::new(&left.worktree).join("partial.rs").exists());
    assert!(host.pending_worktree_removals().unwrap().is_empty());
    let asked = host.open_attention(&root.project_id).unwrap();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].prompt.contains(&left.id), "{}", asked[0].prompt);

    // Restoring brings back the clean worktree and keeps the other.
    host.set_archived(&coordinator.id, false).unwrap();
    assert!(Path::new(&clean.worktree).join("style.css").exists());
}

#[test]
fn restoring_cancels_a_pending_removal() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let agent = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    start_turn(&db, &agent);

    host.set_archived(&coordinator.id, true).unwrap();
    host.set_archived(&coordinator.id, false).unwrap();
    finish_turn(&db, &mut host, &agent);

    assert!(host.pending_worktree_removals().unwrap().is_empty());
    assert!(Path::new(&ticket.worktree).exists());
}

#[test]
fn an_unmarked_finished_ticket_keeps_its_worktree_when_turns_end() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Legacy");
    let agent = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
        .unwrap();
    // Accepted before removal existed: its worktree is still on disk.
    db.execute(
        "UPDATE tickets SET data=json_set(data,'$.state','accepted') WHERE id=?1",
        [&ticket.id],
    )
    .unwrap();
    start_turn(&db, &agent);

    finish_turn(&db, &mut host, &agent);
    drop(host);
    let mut host = Host::open(directory.path().join("home")).unwrap();
    host.settle_pending_worktrees();

    assert!(Path::new(&ticket.worktree).exists());
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

#[test]
fn a_close_whose_commit_fails_rolls_back_and_leaves_no_open_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Review");
    let db = rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
    // Only the commit checks the deferred key, after close_ticket's change succeeded.
    db.execute_batch(
        "CREATE TABLE commit_check (ticket TEXT REFERENCES tickets(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER fail_commit AFTER INSERT ON activity WHEN NEW.kind='ticket_closed'
         BEGIN INSERT INTO commit_check VALUES ('missing'); END;",
    )
    .unwrap();

    let refused = host.close_ticket(&ticket.id).unwrap_err().to_string();

    assert!(refused.contains("FOREIGN KEY"), "{refused}");
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "planned");
    assert!(Path::new(&ticket.worktree).join("style.css").exists());
    host.send("after", None, &root.id, "Still writable")
        .unwrap();
    drop(host);
    let host = Host::open(directory.path().join("home")).unwrap();
    assert_eq!(
        host.messages(&root.id, None, 10).unwrap()[0].body,
        "Still writable"
    );
}

#[test]
fn a_failed_restore_keeps_the_pending_removal() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let agent = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    start_turn(&db, &agent);
    host.set_archived(&coordinator.id, true).unwrap();
    let failing = fail_activity(directory.path(), "restored");

    assert!(host.set_archived(&coordinator.id, false).is_err());

    assert!(host.session(&coordinator.id).unwrap().archived);
    assert_eq!(
        host.pending_worktree_removals().unwrap(),
        std::slice::from_ref(&ticket.id)
    );
    failing.execute_batch("DROP TRIGGER fail_activity").unwrap();
    finish_turn(&db, &mut host, &agent);
    assert!(!Path::new(&ticket.worktree).exists());
}

#[test]
fn a_refused_notice_falls_back_to_the_inbox_and_settles_once() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let (ticket, tester) = reported_in_turn(&mut host, &db, &coordinator);
    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    fs::write(Path::new(&ticket.worktree).join("summary.md"), "late").unwrap();
    // A full queue refuses the coordinator's notice.
    db.execute_batch(
        "CREATE TRIGGER refuse_notice BEFORE INSERT ON messages WHEN NEW.id LIKE 'worktree-kept:%'
         BEGIN SELECT RAISE(ABORT,'queue full'); END;",
    )
    .unwrap();

    finish_turn(&db, &mut host, &tester);
    host.settle_pending_worktrees();

    assert!(host.pending_worktree_removals().unwrap().is_empty());
    let asked = host.open_attention(&root.project_id).unwrap();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].prompt.contains(&ticket.id), "{}", asked[0].prompt);
    let kept = host
        .activity(&root.project_id, None, 100)
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "worktree_kept")
        .count();
    assert_eq!(kept, 1);
}
