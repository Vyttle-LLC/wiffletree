use std::{os::unix::fs::PermissionsExt, path::Path, process::Command as Git};
use workspace_core::{Command, HostSettings, Project};
use workspace_host::Host;

fn git(path: &Path, args: &[&str]) {
    let output = Git::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(
        path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Fixture",
        ],
    );
}

/// The project coordinator and the repository its tickets are created in.
struct Owner {
    id: String,
    repository_id: String,
}

/// A host whose workspaces live in `<directory>/workspaces`, with one project using the
/// repository folder `Web App`.
fn team(directory: &Path, project: &str) -> (Host, Project, Owner) {
    let mut host = Host::open(directory.join("home")).unwrap();
    host.set_workspaces_dir(directory.join("workspaces").to_str().unwrap())
        .unwrap();
    let project = host.create_project(project).unwrap();
    let coordinator = coordinator(&mut host, &project, &directory.join("Web App"));
    (host, project, coordinator)
}

fn coordinator(host: &mut Host, project: &Project, repo: &Path) -> Owner {
    if !repo.exists() {
        repository(repo);
    }
    let root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id && s.parent_id.is_none())
        .unwrap();
    let attached = host
        .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
        .unwrap();
    Owner {
        id: root.id,
        repository_id: attached.id,
    }
}

fn current_branch(path: &str) -> String {
    let output = Git::new("git")
        .current_dir(path)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn new_projects_and_tickets_get_readable_folders_and_branches() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, project, coordinator) = team(directory.path(), "Theme Rollout");
    let workspaces = directory.path().join("workspaces");
    assert_eq!(
        project.home.as_deref(),
        workspaces.join("projects/theme-rollout").to_str()
    );
    assert_eq!(
        host.project_directory(&project.id).unwrap(),
        workspaces.join("projects/theme-rollout")
    );

    let ticket = host
        .create_ticket(
            &coordinator.id,
            &coordinator.repository_id,
            "Fix: Toolbar colours",
            "Style it",
        )
        .unwrap();

    let expected = workspaces.join("tasks/theme-rollout/web-app/fix-toolbar-colours");
    assert_eq!(Path::new(&ticket.worktree), expected);
    assert!(expected.is_dir());
    assert_eq!(ticket.branch, "wiffletree/fix-toolbar-colours");
    assert_eq!(current_branch(&ticket.worktree), ticket.branch);
}

#[test]
fn slugs_never_collide_with_projects_folders_tickets_or_branches() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, first, coordinator) = team(directory.path(), "Notes");
    let workspaces = directory.path().join("workspaces");
    std::fs::create_dir_all(workspaces.join("projects/notes-3")).unwrap();
    let second = host.create_project("notes").unwrap();
    let third = host.create_project("Notes!").unwrap();
    let homes: Vec<_> = [&first, &second, &third]
        .iter()
        .map(|p| {
            Path::new(p.home.as_deref().unwrap())
                .file_name()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(homes, ["notes", "notes-2", "notes-4"]);

    git(
        &directory.path().join("Web App"),
        &["branch", "wiffletree/sidebar-2"],
    );
    let tickets: Vec<_> = ["Sidebar", "sidebar", "Sidebar?"]
        .into_iter()
        .map(|title| {
            host.create_ticket(
                &coordinator.id,
                &coordinator.repository_id,
                title,
                "Style it",
            )
            .unwrap()
        })
        .collect();
    let branches: Vec<_> = tickets.iter().map(|t| t.branch.as_str()).collect();
    assert_eq!(
        branches,
        [
            "wiffletree/sidebar",
            "wiffletree/sidebar-3",
            "wiffletree/sidebar-4"
        ]
    );
    for ticket in &tickets {
        assert_eq!(
            Path::new(&ticket.worktree).file_name().unwrap(),
            ticket.branch.strip_prefix("wiffletree/").unwrap()
        );
    }
}

#[test]
fn renaming_or_moving_workspaces_keeps_existing_folders() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, project, coordinator) = team(directory.path(), "Payments");
    let home = project.home.clone();
    host.rename_project(&project.id, "Billing").unwrap();
    host.set_workspaces_dir(directory.path().join("elsewhere").to_str().unwrap())
        .unwrap();

    assert_eq!(host.project(&project.id).unwrap().home, home);
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &coordinator.repository_id,
            "Invoices",
            "Send them",
        )
        .unwrap();
    assert_eq!(
        Path::new(&ticket.worktree),
        directory
            .path()
            .join("elsewhere/tasks/payments/web-app/invoices")
    );
}

#[test]
fn legacy_projects_and_tickets_keep_their_stored_paths() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, project, coordinator) = team(directory.path(), "Legacy");
    let legacy_worktree = directory.path().join("home/worktrees/0b7c1a2e-uuid");
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &coordinator.repository_id,
            "Old work",
            "Kept",
        )
        .unwrap();
    drop(host);
    let db = rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE projects SET data=json_remove(data,'$.home') WHERE id=?1",
        [&project.id],
    )
    .unwrap();
    db.execute(
        "UPDATE tickets SET data=json_set(data,'$.worktree',?2,'$.branch','codex/workspace-0b7c1a2e') WHERE id=?1",
        [&ticket.id, legacy_worktree.to_str().unwrap()],
    )
    .unwrap();
    drop(db);

    let mut host = Host::open(directory.path().join("home")).unwrap();
    assert_eq!(host.project(&project.id).unwrap().home, None);
    assert_eq!(
        host.project_directory(&project.id).unwrap(),
        host.home.join("projects").join(&project.id)
    );
    let saved = host.ticket(&ticket.id).unwrap();
    assert_eq!(Path::new(&saved.worktree), legacy_worktree);
    assert_eq!(saved.branch, "codex/workspace-0b7c1a2e");

    // Its first new ticket fixes a slug; `legacy` is taken by the folder made before the downgrade.
    let first = host
        .create_ticket(
            &coordinator.id,
            &coordinator.repository_id,
            "Before rename",
            "Kept",
        )
        .unwrap();
    host.rename_project(&project.id, "Renamed").unwrap();
    let second = host
        .create_ticket(
            &coordinator.id,
            &coordinator.repository_id,
            "After rename",
            "Kept",
        )
        .unwrap();
    let tasks = directory.path().join("workspaces/tasks/legacy-2/web-app");
    assert_eq!(Path::new(&first.worktree), tasks.join("before-rename"));
    assert_eq!(Path::new(&second.worktree), tasks.join("after-rename"));
    let project = host.project(&project.id).unwrap();
    assert_eq!(
        (project.home, project.slug.as_deref()),
        (None, Some("legacy-2"))
    );
    assert_eq!(
        host.project_directory(&project.id).unwrap(),
        host.home.join("projects").join(&project.id)
    );
}

#[test]
fn settings_round_trip_keep_unknown_fields_and_fall_back_when_missing_or_corrupt() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("settings.json");
    let mut host = Host::open(directory.path()).unwrap();
    let default = host.settings();
    // Beta builds, like the host they test, default to their own folder.
    let folder = match option_env!("WIFFLETREE_CHANNEL") {
        Some("beta") => "wiffletree-beta",
        _ => "wiffletree",
    };
    assert_eq!(
        default.workspaces_dir,
        std::env::home_dir().unwrap().join(folder).to_str().unwrap()
    );

    std::fs::write(&file, r#"{"future_option":true}"#).unwrap();
    let chosen = directory.path().join("chosen");
    let saved: HostSettings = serde_json::from_value(
        host.execute(Command::SetWorkspacesDir {
            path: chosen.to_str().unwrap().into(),
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.workspaces_dir, chosen.to_str().unwrap());
    assert!(chosen.is_dir() && std::fs::read_dir(&chosen).unwrap().next().is_none());
    drop(host);

    let mut host = Host::open(directory.path()).unwrap();
    let reloaded: HostSettings =
        serde_json::from_value(host.execute(Command::Settings).unwrap()).unwrap();
    assert_eq!(reloaded, saved);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(raw["future_option"], true);

    std::fs::write(&file, b"{ not json").unwrap();
    assert_eq!(host.settings(), default);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(host.settings(), default);
}

#[test]
fn unusable_workspace_folders_are_refused_and_keep_the_saved_setting() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let kept = host
        .set_workspaces_dir(directory.path().join("kept").to_str().unwrap())
        .unwrap();
    let file = directory.path().join("a file");
    std::fs::write(&file, b"").unwrap();

    let inside = file.join("inside");
    let blocked = host
        .set_workspaces_dir(inside.to_str().unwrap())
        .unwrap_err();
    assert_names_only(&blocked, "Cannot create", &inside);
    let read_only = directory.path().join("read only");
    std::fs::create_dir(&read_only).unwrap();
    std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o555)).unwrap();
    let unwritable = host
        .set_workspaces_dir(read_only.to_str().unwrap())
        .unwrap_err();
    assert_names_only(&unwritable, "Cannot write to", &read_only);
    let relative = host.set_workspaces_dir("relative/folder").unwrap_err();
    assert!(relative.to_string().contains("absolute"), "{relative}");
    assert_eq!(host.settings(), kept);
}

/// One line naming the chosen folder and the system's reason, never a probe file inside it.
fn assert_names_only(error: &anyhow::Error, action: &str, folder: &Path) {
    let message = format!("{error:#}");
    let reason = message
        .strip_prefix(&format!("{action} {}: ", folder.display()))
        .unwrap_or_else(|| panic!("{message}"));
    assert!(
        !reason.is_empty() && !reason.contains('/') && !reason.contains('\n'),
        "{message}"
    );
}

#[test]
fn checking_a_workspace_folder_never_writes_through_an_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let outside = directory.path().join("outside.txt");
    std::fs::write(&outside, b"keep me").unwrap();
    let chosen = directory.path().join("chosen");
    std::fs::create_dir(&chosen).unwrap();
    std::os::unix::fs::symlink(&outside, chosen.join(".wiffletree-write-check")).unwrap();

    host.set_workspaces_dir(chosen.to_str().unwrap()).unwrap();

    assert_eq!(std::fs::read(&outside).unwrap(), b"keep me");
    assert!(chosen.join(".wiffletree-write-check").is_symlink());
    assert_eq!(std::fs::read_dir(&chosen).unwrap().count(), 1);
}

#[test]
fn nested_branches_take_their_prefix_and_a_wiffletree_branch_is_reported() {
    let directory = tempfile::tempdir().unwrap();
    let (mut host, project, web) = team(directory.path(), "Calls");
    git(
        &directory.path().join("Web App"),
        &["branch", "wiffletree/fix/nested"],
    );
    let ticket = host
        .create_ticket(&web.id, &web.repository_id, "Fix", "Fix it")
        .unwrap();
    assert_eq!(ticket.branch, "wiffletree/fix-2");

    let blocked = directory.path().join("Blocked");
    repository(&blocked);
    git(&blocked, &["branch", "wiffletree"]);
    let other = coordinator(&mut host, &project, &blocked);
    let error = host
        .create_ticket(&other.id, &other.repository_id, "Fix", "Fix it")
        .unwrap_err()
        .to_string();
    assert!(error.contains("branch named \"wiffletree\""), "{error}");
}
