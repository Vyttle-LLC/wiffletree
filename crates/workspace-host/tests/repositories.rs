use rusqlite::{Connection, params};
use serde_json::json;
use std::{fs, os::unix::fs::symlink, path::Path, process::Command};
use workspace_core::{Provider, Role, Session, Status};
use workspace_host::Host;

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn repo(path: &Path, committed: bool) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    if committed {
        git(
            path,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "Fixture",
            ],
        );
    }
}

#[test]
fn discovery_deduplicates_roots_and_includes_worktrees_without_descending_into_repos() {
    let folders = tempfile::tempdir().unwrap();
    let one = folders.path().join("company");
    let two = folders.path().join("personal");
    let app = one.join("app");
    let tool = two.join("tool");
    repo(&app, true);
    repo(&tool, true);
    repo(&one.join("empty"), false);
    repo(&app.join("nested"), true);
    fs::create_dir_all(one.join("ordinary-folder")).unwrap();
    symlink(&app, one.join("alias")).unwrap();
    let worktree = two.join("linked-worktree");
    git(
        &app,
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            worktree.to_str().unwrap(),
        ],
    );
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let project = host.create_project("Import").unwrap();
    host.attach_repository(&project.id, app.to_str().unwrap(), "HEAD")
        .unwrap();
    let discovery = host
        .discover_repositories(&[
            format!("{}/*", one.display()),
            two.display().to_string(),
            one.display().to_string(),
            folders.path().join("missing").display().to_string(),
        ])
        .unwrap();
    assert_eq!(discovery.repositories.len(), 3);
    assert_eq!(discovery.repositories.iter().filter(|r| r.added).count(), 1);
    assert!(discovery.repositories.iter().all(|r| r.base == "main"));
    assert!(
        discovery
            .repositories
            .iter()
            .any(|r| r.path == fs::canonicalize(&worktree).unwrap().to_str().unwrap())
    );
    assert!(discovery.warnings.iter().any(|w| w.contains("empty")));
    assert!(discovery.warnings.iter().any(|w| w.contains("missing")));
    assert_eq!(host.repositories().unwrap().len(), 1);
}

#[test]
fn bulk_import_keeps_successes_and_existing_bases_and_is_retry_safe() {
    let folder = tempfile::tempdir().unwrap();
    let one = folder.path().join("one");
    let two = folder.path().join("two");
    repo(&one, true);
    repo(&two, true);
    fs::write(two.join("local-draft.txt"), "Keep me").unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let project = host.create_project("Import").unwrap();
    let existing = host
        .attach_repository(&project.id, one.to_str().unwrap(), "HEAD")
        .unwrap();
    let paths = vec![
        one.display().to_string(),
        two.display().to_string(),
        two.display().to_string(),
        folder.path().join("missing").display().to_string(),
    ];
    let result = host
        .import_repositories(Some(&project.id), &paths, &[], &[])
        .unwrap();
    assert_eq!(result.imported.len(), 2);
    assert_eq!(result.failures.len(), 1);
    assert!(result.imported.contains(&existing));
    let retry = host
        .import_repositories(Some(&project.id), &paths, &[], &[])
        .unwrap();
    assert_eq!(result.imported, retry.imported);
    assert_eq!(host.repositories().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(two.join("local-draft.txt")).unwrap(),
        "Keep me"
    );
    assert!(
        host.import_repositories(Some("unknown-project"), &paths, &[], &[])
            .is_err()
    );
    drop(host);
    let host = Host::open(home.path()).unwrap();
    assert_eq!(host.repositories().unwrap().len(), 2);
}

fn team(host: &mut Host, project: &str, repository: &str) -> anyhow::Result<Session> {
    let root = host
        .sessions()?
        .into_iter()
        .find(|s| s.project_id == project && s.role == Role::ProjectOrchestrator)
        .unwrap();
    host.create_session(
        project,
        &root.id,
        Some(repository),
        "Team",
        Role::TaskOrchestrator,
        Provider::Claude,
    )
}

#[test]
fn projects_use_every_repository_until_they_choose_some() {
    let folder = tempfile::tempdir().unwrap();
    let (one, two, three) = (
        folder.path().join("one"),
        folder.path().join("two"),
        folder.path().join("three"),
    );
    for path in [&one, &two, &three] {
        repo(path, true);
    }
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let all = host.create_project("All").unwrap();
    let custom = host.create_project("Custom").unwrap();
    let first = host.add_repository(one.to_str().unwrap(), "HEAD").unwrap();
    let second = host.add_repository(two.to_str().unwrap(), "HEAD").unwrap();
    host.set_project_repositories(&custom.id, Some(vec![first.id.clone()]))
        .unwrap();

    // Later additions reach projects that use everything, not ones that chose.
    let third = host
        .import_repositories(None, &[three.display().to_string()], &[], &[])
        .unwrap()
        .imported
        .remove(0);
    assert_eq!(host.project_repositories(&all.id).unwrap().len(), 3);
    assert_eq!(
        host.project_repositories(&custom.id).unwrap(),
        vec![first.clone()]
    );
    assert!(team(&mut host, &custom.id, &second.id).is_err());
    team(&mut host, &all.id, &second.id).unwrap();

    // Adding from inside a choosing project includes the repository there too.
    host.attach_repository(&custom.id, three.to_str().unwrap(), "HEAD")
        .unwrap();
    assert!(host.project(&custom.id).unwrap().uses(&third.id));

    let working = team(&mut host, &custom.id, &first.id).unwrap();
    assert!(
        host.set_project_repositories(&custom.id, Some(vec![third.id.clone()]))
            .is_err()
    );
    host.set_archived(&working.id, true).unwrap();
    host.set_project_repositories(&custom.id, Some(vec![third.id.clone()]))
        .unwrap();
    host.set_project_repositories(&custom.id, None).unwrap();
    assert_eq!(host.project_repositories(&custom.id).unwrap().len(), 3);
    assert!(
        host.set_project_repositories(&custom.id, Some(vec!["unknown".into()]))
            .is_err()
    );
}

#[test]
fn repositories_with_teams_stay_in_the_workspace() {
    let folder = tempfile::tempdir().unwrap();
    let (used, idle) = (folder.path().join("used"), folder.path().join("idle"));
    repo(&used, true);
    repo(&idle, true);
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let project = host.create_project("Remove").unwrap();
    let used = host.add_repository(used.to_str().unwrap(), "HEAD").unwrap();
    let idle = host.add_repository(idle.to_str().unwrap(), "HEAD").unwrap();
    host.set_project_repositories(&project.id, Some(vec![used.id.clone(), idle.id.clone()]))
        .unwrap();
    let working = team(&mut host, &project.id, &used.id).unwrap();
    host.set_archived(&working.id, true).unwrap();
    assert!(host.remove_repository(&used.id).is_err());
    host.remove_repository(&idle.id).unwrap();
    assert_eq!(host.repositories().unwrap(), vec![used.clone()]);
    assert_eq!(
        host.project(&project.id).unwrap().repositories,
        Some(vec![used.id])
    );
}

#[test]
fn per_project_attachments_migrate_to_one_workspace_list() {
    let home = tempfile::tempdir().unwrap();
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute_batch(include_str!("../src/schema.sql")).unwrap();
    db.execute_batch(include_str!("../src/live.sql")).unwrap();
    db.execute_batch(include_str!("../src/usage.sql")).unwrap();
    let project = |id: &str| json!({"id":id,"name":id,"brain":null,"turn_limit":4}).to_string();
    for id in ["a", "b", "empty"] {
        db.execute(
            "INSERT INTO projects VALUES (?1,?2)",
            params![id, project(id)],
        )
        .unwrap();
    }
    let attach = |id: &str, project: &str, path: &str| {
        let data = json!({"id":id,"project_id":project,"name":"app","path":path,"base":"HEAD"});
        db.execute(
            "INSERT INTO repositories VALUES (?1,?2,?3,?4)",
            params![id, project, path, data.to_string()],
        )
        .unwrap();
    };
    attach("a-app", "a", "/repos/app");
    attach("a-api", "a", "/repos/api");
    attach("b-app", "b", "/repos/app");
    let session = Session {
        id: "b-team".into(),
        project_id: "b".into(),
        parent_id: None,
        repository_id: Some("b-app".into()),
        name: "Team".into(),
        role: Role::TaskOrchestrator,
        provider: Provider::Claude,
        status: Status::Ready,
        archived: false,
    };
    db.execute(
        "INSERT INTO sessions VALUES (?1,?2,NULL,'task_orchestrator',?3)",
        params![
            session.id,
            session.project_id,
            serde_json::to_string(&session).unwrap()
        ],
    )
    .unwrap();
    drop(db);

    let host = Host::open(home.path()).unwrap();
    let ids: Vec<_> = host
        .repositories()
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, ["a-app", "a-api"]);
    assert_eq!(
        host.project("a").unwrap().repositories,
        Some(vec!["a-app".into(), "a-api".into()])
    );
    assert_eq!(
        host.project("b").unwrap().repositories,
        Some(vec!["a-app".into()])
    );
    assert_eq!(host.project("empty").unwrap().repositories, None);
    assert_eq!(
        host.session("b-team").unwrap().repository_id.as_deref(),
        Some("a-app")
    );
    drop(host);
    assert_eq!(
        Host::open(home.path())
            .unwrap()
            .repositories()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn root_folders_pick_up_new_repositories_but_respect_dismissals() {
    let folder = tempfile::tempdir().unwrap();
    let dev = fs::canonicalize(folder.path()).unwrap().join("dev");
    let (alpha, beta, gamma, empty) = (
        dev.join("alpha"),
        dev.join("beta"),
        dev.join("gamma"),
        dev.join("empty"),
    );
    repo(&alpha, true);
    repo(&beta, true);
    repo(&empty, false);
    repo(&dev.join(".hidden"), true);
    fs::create_dir_all(dev.join("notes")).unwrap();
    git(
        &alpha,
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            dev.join("linked").to_str().unwrap(),
        ],
    );
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let path = |p: &Path| p.to_str().unwrap().to_owned();

    // Unticked repositories in a remembered root stay out; a repository root is not a root folder.
    host.import_repositories(
        None,
        &[path(&alpha)],
        &[format!("{}/*", dev.display()), path(&alpha)],
        &[path(&beta)],
    )
    .unwrap();
    assert_eq!(host.repository_roots().unwrap(), vec![path(&dev)]);

    repo(&gamma, true);
    let refresh = host.refresh_repositories().unwrap();
    let added: Vec<_> = refresh.added.iter().map(|r| r.path.clone()).collect();
    assert_eq!(added, vec![path(&gamma)], "{:?}", refresh.warnings);
    assert!(refresh.warnings.is_empty(), "{:?}", refresh.warnings);
    assert!(host.refresh_repositories().unwrap().added.is_empty());

    // An empty repository joins once it has a commit.
    git(
        &empty,
        &[
            "-c",
            "user.name=F",
            "-c",
            "user.email=f@x.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "First",
        ],
    );
    assert_eq!(host.refresh_repositories().unwrap().added.len(), 1);

    // Removing a repository keeps it out of later refreshes until it is added by hand.
    let gamma_id = host
        .repositories()
        .unwrap()
        .into_iter()
        .find(|r| r.path == path(&gamma))
        .unwrap()
        .id;
    host.remove_repository(&gamma_id).unwrap();
    assert!(host.refresh_repositories().unwrap().added.is_empty());
    host.import_repositories(None, &[path(&gamma)], &[], &[])
        .unwrap();
    assert_eq!(host.repositories().unwrap().len(), 3);

    fs::remove_dir_all(&gamma).unwrap();
    assert_eq!(host.missing_repositories().unwrap().len(), 1);

    host.remove_repository_root(&path(&dev)).unwrap();
    repo(&dev.join("delta"), true);
    assert!(host.refresh_repositories().unwrap().added.is_empty());
}
