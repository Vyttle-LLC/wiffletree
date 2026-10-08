use serde_json::json;
use std::{fs, path::Path, process::Command};
use workspace_core::{Provider, Receipt, Role, Session, Status, Ticket};
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

/// A project using the `web` repository; the second session is its coordinator, which owns the
/// tickets, returned again for readability.
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
    assert_eq!(host.repositories().unwrap(), [attached]);
    (root.clone(), root)
}

/// A ticket with committed work and ignored build output in its worktree.
fn worked_ticket(host: &mut Host, coordinator: &Session, title: &str) -> Ticket {
    let repository = host.repositories().unwrap().remove(0);
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, title, "Style it")
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

/// Starts verification of a ticket its implementer reported ready, returning the verifier.
fn verifying(host: &mut Host, ticket: &Ticket) -> Session {
    let implementer = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    host.agent_tool(
        &implementer.id,
        "report",
        json!({"message_id":"ready","kind":"ready_for_testing","body":"Committed"}),
    )
    .unwrap();
    let verified: Ticket = serde_json::from_value(
        host.agent_tool(
            &ticket.coordinator_id,
            "verify_ticket",
            json!({"ticket_id":ticket.id}),
        )
        .unwrap(),
    )
    .unwrap();
    let verifier = &verified.verification.unwrap().rounds[0].verifiers[0];
    host.session(&verifier.session_id).unwrap()
}

/// What a turn does first: takes every queued message as its input.
fn take_input(host: &mut Host, session: &str) {
    for message in host.messages(session, None, 100).unwrap() {
        if message.receipt == Receipt::Queued {
            host.advance_receipt(&message.id, Receipt::Delivered)
                .unwrap();
        }
    }
}

fn pass(host: &mut Host, verifier: &Session) {
    take_input(host, &verifier.id);
    host.agent_tool(
        &verifier.id,
        "report",
        json!({"message_id":"done","kind":"passed","body":"Green"}),
    )
    .unwrap();
}

fn passed(host: &mut Host, ticket: &Ticket) -> Session {
    let verifier = verifying(host, ticket);
    pass(host, &verifier);
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "passed");
    verifier
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
fn archiving_a_project_removes_clean_worktrees_and_restoring_re_creates_them() {
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

/// A passed ticket whose verifier reported and is still writing its summary.
fn reported_in_turn(
    host: &mut Host,
    db: &rusqlite::Connection,
    coordinator: &Session,
) -> (Ticket, Session) {
    let ticket = worked_ticket(host, coordinator, "Toolbar");
    let tester = verifying(host, &ticket);
    start_turn(db, &tester);
    host.set_status(&tester.id, Status::Working).unwrap();
    pass(host, &tester);
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

/// A provider process a crashed host left running in its own process group.
struct Orphan(std::process::Child);
impl Orphan {
    fn spawn() -> Self {
        use std::os::unix::process::CommandExt;
        Self(
            Command::new("sleep")
                .arg("30")
                .process_group(0)
                .spawn()
                .unwrap(),
        )
    }
    /// Records it on a finished run of `agent` with `outcome`, as a stopped host leaves it.
    fn record(&self, db: &rusqlite::Connection, agent: &Session, outcome: &str) {
        db.execute(
            "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail,process_group)
             VALUES (?1,?1,'[]',1,2,?3,'{}',?2)",
            rusqlite::params![agent.id, self.0.id(), outcome],
        )
        .unwrap();
    }
}
impl Drop for Orphan {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn an_orphaned_provider_process_defers_removal_until_it_exits() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let tester = passed(&mut host, &ticket);
    let orphan = Orphan::spawn();
    orphan.record(&db, &tester, "interrupted");

    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    host.settle_pending_worktrees();
    assert!(Path::new(&ticket.worktree).exists());
    assert_eq!(
        host.pending_worktree_removals().unwrap(),
        std::slice::from_ref(&ticket.id)
    );

    drop(orphan);
    host.settle_pending_worktrees();
    assert!(!Path::new(&ticket.worktree).exists());
    assert!(host.pending_worktree_removals().unwrap().is_empty());
}

#[test]
fn archiving_while_only_an_orphan_runs_defers_removal_whatever_its_run_outcome() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let (_root, coordinator) = team(&mut host, directory.path());
    let db = store(directory.path());
    let ticket = worked_ticket(&mut host, &coordinator, "Toolbar");
    let agent = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    let orphan = Orphan::spawn();
    // Shutdown finished the run as failed, then the host stopped before its process exited.
    orphan.record(&db, &agent, "failed");

    host.set_archived(&coordinator.id, true).unwrap();
    host.settle_pending_worktrees();

    assert!(Path::new(&ticket.worktree).exists());
    drop(orphan);
    host.settle_pending_worktrees();
    assert!(!Path::new(&ticket.worktree).exists());
}
