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
    let mut host = Host::open(directory.path()).unwrap();
    let (_, session) = project(&mut host, "No repository required");
    host.send("first", None, &session.id, "Instruction")
        .unwrap();
    host.send("second", None, &session.id, "Next instruction")
        .unwrap();
    assert!(host.advance_receipt("second", Receipt::Delivered).is_err());
    host.advance_receipt("first", Receipt::Delivered).unwrap();
    host.set_status(&session.id, Status::Working).unwrap();
    drop(host);
    let mut host = Host::open(directory.path()).unwrap();
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
    let mut host = Host::open(directory.path()).unwrap();
    assert!(Host::open(directory.path()).is_err());
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
    let mut host = Host::open(directory.path()).unwrap();
    host.resolve_attention(&attention.id, "denied").unwrap();
    assert!(host.resolve_attention(&attention.id, "approved").is_err());
    assert_eq!(host.session(&session.id).unwrap().status, Status::Blocked);
}

#[test]
fn messages_and_logs_do_not_cross_project_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
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
    host.append_log(log(&a.id, "one")).unwrap();
    assert!(host.logs(&two.id, None, 100).unwrap().is_empty());
}

#[test]
fn transcript_keyset_and_simulator_are_bounded_and_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let (_, session) = project(&mut host, "Simulation");
    host.send("a", None, &session.id, "Hello").unwrap();
    let response = host.simulate("a").unwrap();
    assert!(response.body.contains("Local simulation"));
    assert_eq!(host.simulate("a").unwrap().id, response.id);
    let last = host.messages(&session.id, None, 1).unwrap();
    let previous = host
        .messages(&session.id, Some(last[0].sequence), 1)
        .unwrap();
    assert_eq!(previous[0].id, "a");
    assert!(host.messages(&session.id, None, 101).is_err());
    assert!(
        host.send("huge", None, &session.id, &"x".repeat(MAX_TEXT_BYTES + 1))
            .is_err()
    );
}

fn log(session: &str, milestone: &str) -> LogInput {
    LogInput {
        session_id: session.into(),
        milestone: milestone.into(),
        kind: LogKind::Decision,
        title: format!("Decision {milestone}"),
        reference: "—".into(),
        changed: "Use stable source entries".into(),
        why: "Session replacement".into(),
        state: "Reported; unverified".into(),
    }
}

#[test]
fn markdown_preserves_external_content_and_recovers_receipt_gap() {
    let directory = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let (project, session) = project(&mut host, "Memory");
    host.attach_brain(&project.id, brain.path().to_str().unwrap())
        .unwrap();
    let first = host.append_log(log(&session.id, "one")).unwrap();
    let second = host.append_log(log(&session.id, "two")).unwrap();
    assert_eq!(
        host.append_log(log(&session.id, "one")).unwrap().id,
        first.id
    );
    let mut changed = log(&session.id, "one");
    changed.changed = "Different".into();
    assert!(host.append_log(changed).is_err());
    let path = brain.path().join("second-brain/logs/project.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original =
        "# Existing logs\n<!-- compiled-through:2026-10-02 -->\nExternal editor content\n";
    std::fs::write(
        &path,
        format!(
            "<!-- workspace-entry:{} -->\nAlready written\n{original}",
            first.id
        ),
    )
    .unwrap();
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 2);
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        content
            .matches(&format!("workspace-entry:{}", first.id))
            .count(),
        1
    );
    assert_eq!(
        content
            .matches(&format!("workspace-entry:{}", second.id))
            .count(),
        1
    );
    assert!(content.contains(original));
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 0);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
    drop(host);
    let host = Host::open(directory.path()).unwrap();
    assert_eq!(host.logs(&project.id, None, 100).unwrap().len(), 2);
    assert_eq!(host.runs(&project.id).unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn markdown_symlink_destination_is_rejected_and_failure_is_recorded() {
    let directory = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let (project, session) = project(&mut host, "Memory");
    host.attach_brain(&project.id, brain.path().to_str().unwrap())
        .unwrap();
    host.append_log(log(&session.id, "one")).unwrap();
    let path = brain.path().join("second-brain/logs/project.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside.path(), &path).unwrap();
    assert!(host.export_logs(&project.id).is_err());
    assert_eq!(std::fs::metadata(outside.path()).unwrap().len(), 0);
    assert!(
        host.runs(&project.id).unwrap()[0]
            .outcome
            .starts_with("failed:")
    );
}

#[test]
fn model_settings_and_versioned_commands_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
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
    let mut host = Host::open(directory.path()).unwrap();
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
    use std::process::Command;
    let directory = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut host = Host::open(directory.path()).unwrap();
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
        host.git_history(&repository.id, 0, 10).unwrap()["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(host);
    let host = Host::open(directory.path()).unwrap();
    assert_eq!(host.session(&worker.id).unwrap(), worker);
}

#[cfg(unix)]
#[test]
fn brain_parent_symlink_is_rejected_before_creating_outside_directories() {
    let directory = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), brain.path().join("second-brain")).unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let (project, session) = project(&mut host, "Memory");
    host.attach_brain(&project.id, brain.path().to_str().unwrap())
        .unwrap();
    host.append_log(log(&session.id, "one")).unwrap();
    assert!(host.export_logs(&project.id).is_err());
    assert!(!outside.path().join("logs").exists());
}

#[test]
fn markdown_batches_keep_header_compiler_marker_and_newest_first_order() {
    let directory = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let (project, session) = project(&mut host, "Batch memory");
    host.attach_brain(&project.id, brain.path().to_str().unwrap())
        .unwrap();
    let path = brain.path().join("second-brain/logs/project.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let prefix = "# project — working log\n<!-- compiled-through: 2026-10-02 -->\n\n";
    std::fs::write(&path, format!("{prefix}## Existing entry\nRetain me\n")).unwrap();
    for i in 0..101 {
        host.append_log(log(&session.id, &format!("batch-{i:03}")))
            .unwrap();
    }
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 100);
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 1);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with(prefix));
    assert!(
        content.find("Decision batch-100").unwrap() < content.find("Decision batch-099").unwrap()
    );
    assert!(
        content.find("Decision batch-099").unwrap() < content.find("Decision batch-000").unwrap()
    );
    assert_eq!(content.matches("<!-- workspace-entry:").count(), 101);
}

#[test]
fn one_host_owns_a_store_until_it_closes() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::open(directory.path()).unwrap();
    let second = Host::open(directory.path()).err().unwrap();
    assert!(
        second
            .to_string()
            .contains("Another host already owns this store")
    );
    drop(host);
    Host::open(directory.path()).unwrap();
}
