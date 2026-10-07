use std::{path::Path, process::Command};
use workspace_core::{Provider, Role, Session};
use workspace_host::Host;

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Fixture",
        ],
    ] {
        let output = Command::new("git")
            .current_dir(path)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
}

fn team(host: &mut Host, directory: &Path) -> (Session, Session) {
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
