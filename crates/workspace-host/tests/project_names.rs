use workspace_core::{Command, Project, Role, Status};
use workspace_host::Host;

#[test]
fn rename_keeps_project_identity_conversation_and_other_projects_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let project = host.create_project("New project").unwrap();
    let other = host.create_project("Other feature").unwrap();
    let root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id && s.role == Role::ProjectOrchestrator)
        .unwrap();
    host.send("existing-message", None, &root.id, "Keep this conversation")
        .unwrap();
    host.set_status(&root.id, Status::Paused).unwrap();
    let command: Command = serde_json::from_value(serde_json::json!({
        "type": "rename_project", "project_id": project.id, "name": "  Notifications  "
    }))
    .unwrap();
    let renamed: Project = serde_json::from_value(host.execute(command).unwrap()).unwrap();
    assert_eq!(renamed.name, "Notifications");
    assert_eq!(renamed.id, project.id);
    drop(host);

    let host = Host::open(directory.path()).unwrap();
    let saved = host.session(&root.id).unwrap();
    assert_eq!(host.project(&project.id).unwrap().name, "Notifications");
    assert_eq!(saved.name, "Notifications");
    assert_eq!(saved.status, Status::Paused);
    assert_eq!(saved.provider, root.provider);
    assert_eq!(
        host.messages(&root.id, None, 100).unwrap()[0].body,
        "Keep this conversation"
    );
    assert_eq!(host.project(&other.id).unwrap().name, "Other feature");
    assert_eq!(host.projects().unwrap().len(), 2);
}

#[test]
fn invalid_rename_preserves_saved_names() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let project = host.create_project("Original").unwrap();
    for name in [" ".to_string(), "a".repeat(129)] {
        assert!(host.rename_project(&project.id, &name).is_err());
    }
    assert!(host.rename_project("missing-project", "Name").is_err());
    assert_eq!(host.project(&project.id).unwrap().name, "Original");
    assert_eq!(host.sessions().unwrap()[0].name, "Original");
}

#[test]
fn coordinator_update_failure_rolls_back_project_name_and_activity() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let project = host.create_project("Original").unwrap();
    let db = rusqlite::Connection::open(directory.path().join("workspace.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_session_update BEFORE UPDATE ON sessions
        BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;",
    )
    .unwrap();
    assert!(
        host.rename_project(&project.id, "Should roll back")
            .is_err()
    );
    assert_eq!(host.project(&project.id).unwrap().name, "Original");
    assert_eq!(host.sessions().unwrap()[0].name, "Original");
    let events: i64 = db
        .query_row(
            "SELECT count(*) FROM activity WHERE kind='project_renamed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 0);
}
