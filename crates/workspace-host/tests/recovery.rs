mod common;
use workspace_core::*;
use workspace_host::Host;

fn project(host: &mut Host, name: &str) -> (Project, Session) {
    let project = host.create_project(name).unwrap();
    let session = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id)
        .unwrap();
    (project, session)
}

#[test]
fn restart_preserves_queue_and_holds_ambiguous_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(directory.path()).unwrap();
    let (_, session) = project(&mut host, "No repository required");
    host.send("first", None, &session.id, "Instruction")
        .unwrap();
    host.send("second", None, &session.id, "Next instruction")
        .unwrap();
    assert!(host.advance_receipt("second", Receipt::Delivered).is_err());
    host.advance_receipt("first", Receipt::Delivered).unwrap();
    host.set_status(&session.id, Status::Working).unwrap();
    drop(host);
    let mut host = Host::open_set_up(directory.path()).unwrap();
    assert_eq!(
        host.session(&session.id).unwrap().status,
        Status::Disconnected
    );
    assert_eq!(host.message("first").unwrap().receipt, Receipt::Held);
    assert_eq!(host.message("second").unwrap().receipt, Receipt::Queued);
    assert!(host.advance_receipt("second", Receipt::Delivered).is_err());
    assert!(host.advance_receipt("first", Receipt::Delivered).is_err());
    host.send("second", None, &session.id, "Next instruction")
        .unwrap();
    assert_eq!(host.messages(&session.id, None, 100).unwrap().len(), 2);
    assert!(
        host.send("second", None, &session.id, "Changed instruction")
            .is_err()
    );
}

#[test]
fn store_owner_attention_and_blocked_state_are_independent() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(directory.path()).unwrap();
    assert!(Host::open_set_up(directory.path()).is_err());
    let (_, session) = project(&mut host, "Attention");
    host.set_status(&session.id, Status::Blocked).unwrap();
    assert!(host.snapshot().unwrap().attention.is_empty());
    let attention = host
        .request_attention(&session.id, "local", "exact-operation", "Allow test?", &[])
        .unwrap();
    assert_eq!(
        host.request_attention(&session.id, "local", "exact-operation", "Allow test?", &[])
            .unwrap()
            .id,
        attention.id
    );
    assert!(
        host.request_attention(
            &session.id,
            "other-host",
            "exact-operation",
            "Allow test?",
            &[]
        )
        .is_err()
    );
    drop(host);
    let mut host = Host::open_set_up(directory.path()).unwrap();
    host.resolve_attention(&attention.id, "denied").unwrap();
    assert!(host.resolve_attention(&attention.id, "approved").is_err());
    assert_eq!(host.session(&session.id).unwrap().status, Status::Blocked);
}

#[test]
fn messages_and_sessions_do_not_cross_project_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(directory.path()).unwrap();
    let (one, a) = project(&mut host, "One");
    let (two, b) = project(&mut host, "Two");
    assert!(
        host.send("peer", Some(&a.id), &b.id, "Cross scope")
            .is_err()
    );
    assert!(
        host.create_session(
            &two.id,
            &a.id,
            None,
            "Worker",
            Role::Implementer,
            Provider::Codex
        )
        .is_err()
    );
    assert!(
        host.create_session(
            &one.id,
            &a.id,
            None,
            "Task",
            Role::TaskOrchestrator,
            Provider::Codex
        )
        .is_err()
    );
}

#[test]
fn transcript_pages_and_input_are_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(directory.path()).unwrap();
    let (_, session) = project(&mut host, "Bounds");
    assert!(host.messages(&session.id, None, 101).is_err());
    assert!(
        host.send("huge", None, &session.id, &"x".repeat(MAX_TEXT_BYTES + 1))
            .is_err()
    );
}

#[test]
fn model_settings_and_versioned_commands_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(directory.path()).unwrap();
    let mut selection = host.model_selection().unwrap();
    selection.guide = "Raise effort for migrations.".into();
    let selection = host.set_model_selection(&selection).unwrap();
    let response = host.respond(Request {
        version: 99,
        id: "wrong-version".into(),
        command: Command::Snapshot,
    });
    assert_eq!(response.id, "wrong-version");
    assert!(response.error.is_some());
    drop(host);
    let mut host = Host::open_set_up(directory.path()).unwrap();
    assert_eq!(host.model_selection().unwrap(), selection);
    let response = host.respond(Request {
        version: PROTOCOL_VERSION,
        id: "snapshot".into(),
        command: Command::Snapshot,
    });
    assert!(response.error.is_none());
    assert!(response.result.unwrap()["projects"].is_array());
}

#[test]
fn repository_coordinators_cannot_be_created_and_workers_keep_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    common::git(repo.path(), &["init", "-q"]);
    common::git(repo.path(), &["commit", "--allow-empty", "-qm", "fixture"]);
    let mut host = Host::open_set_up(directory.path()).unwrap();
    let (project, root) = project(&mut host, "Hierarchy");
    let repository = host
        .attach_repository(&project.id, repo.path().to_str().unwrap(), "HEAD")
        .unwrap();
    let sessions = host.sessions().unwrap().len();
    let refused = host
        .create_session(
            &project.id,
            &root.id,
            Some(&repository.id),
            "One",
            Role::TaskOrchestrator,
            Provider::Codex,
        )
        .unwrap_err();
    assert!(refused.to_string().contains("removed"), "{refused}");
    assert_eq!(
        host.sessions().unwrap().len(),
        sessions,
        "no session stored"
    );
    let other = host.create_project("Another outcome").unwrap();
    let other_root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == other.id)
        .unwrap();
    let other_repository = host
        .attach_repository(&other.id, repo.path().to_str().unwrap(), "HEAD")
        .unwrap();
    let worker = host
        .create_session(
            &other.id,
            &other_root.id,
            Some(&other_repository.id),
            "Tester",
            Role::Tester,
            Provider::Codex,
        )
        .unwrap();
    // Projects share the workspace's repository while keeping their own agents.
    assert_eq!(repository, other_repository);
    assert_eq!(worker.parent_id.as_deref(), Some(other_root.id.as_str()));
    assert_eq!(
        worker.repository_id.as_deref(),
        Some(other_repository.id.as_str())
    );
    host.send("direct", None, &worker.id, "Inspect tests")
        .unwrap();
    assert!(
        host.activity(&other.id, None, 100)
            .unwrap()
            .iter()
            .any(|e| e.kind == "direct_instruction"
                && e.session_id.as_deref() == Some(&other_root.id))
    );
    assert_eq!(
        host.git_history(&repository.id, 0, 10)
            .unwrap()
            .commits
            .len(),
        1
    );
    drop(host);
    let host = Host::open_set_up(directory.path()).unwrap();
    assert_eq!(host.session(&worker.id).unwrap(), worker);
}

#[test]
fn one_host_owns_a_store_until_it_closes() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::open_set_up(directory.path()).unwrap();
    let second = Host::open_set_up(directory.path()).err().unwrap();
    assert!(
        second
            .to_string()
            .contains("Another host already owns this store")
    );
    drop(host);
    Host::open_set_up(directory.path()).unwrap();
}
