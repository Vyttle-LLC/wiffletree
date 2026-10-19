//! What the host reads and lists for a client, through the store's own roots.
mod common;

use base64::Engine as _;
use std::{fs, path::Path};
use workspace_core::*;
use workspace_host::Host;

/// A store whose project has a ticket worktree in the workspace folder.
struct Store {
    data: tempfile::TempDir,
    workspaces: tempfile::TempDir,
    _repository: tempfile::TempDir,
    host: Host,
    project: Project,
    worktree: std::path::PathBuf,
}
fn store() -> Store {
    let data = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    common::git(repository.path(), &["init"]);
    common::git(
        repository.path(),
        &["commit", "--allow-empty", "-m", "Initial"],
    );
    let mut host = Host::open_set_up(data.path()).unwrap();
    let workspaces = tempfile::tempdir().unwrap();
    host.set_workspaces_dir(workspaces.path().to_str().unwrap())
        .unwrap();
    let project = host.create_project("Tide alerts").unwrap();
    let coordinator = host.sessions().unwrap().remove(0);
    let attached = host
        .attach_repository(&project.id, repository.path().to_str().unwrap(), "HEAD")
        .unwrap();
    let ticket = host
        .create_ticket(&coordinator.id, &attached.id, "Files", "Read them")
        .unwrap();
    let worktree = std::path::PathBuf::from(&ticket.worktree);
    fs::create_dir_all(&worktree).unwrap();
    Store {
        data,
        workspaces,
        _repository: repository,
        host,
        project,
        worktree,
    }
}
fn read(host: &mut Host, path: &Path) -> Result<Vec<u8>, String> {
    let response = host.respond(Request {
        version: PROTOCOL_VERSION,
        id: new_id(),
        command: Command::ReadFile {
            path: path.into(),
            max_bytes: MAX_ATTACHMENT_BYTES,
        },
    });
    match response.error {
        Some(error) => Err(error),
        None => {
            let contents: FileContents = serde_json::from_value(response.result.unwrap()).unwrap();
            Ok(base64::engine::general_purpose::STANDARD
                .decode(contents.data)
                .unwrap())
        }
    }
}

#[test]
fn a_worktree_file_is_served_and_a_symlink_out_of_it_is_not() {
    let Store {
        data: _data,
        mut host,
        worktree,
        workspaces: _workspaces,
        ..
    } = store();
    fs::write(worktree.join("notes.md"), "# Notes").unwrap();
    assert_eq!(
        read(&mut host, &worktree.join("notes.md")).unwrap(),
        b"# Notes"
    );
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("hosts.yml"), "token").unwrap();
    std::os::unix::fs::symlink(outside.path().join("hosts.yml"), worktree.join("creds")).unwrap();
    let error = read(&mut host, &worktree.join("creds")).unwrap_err();
    assert!(error.contains("outside what the host serves"), "{error}");
    // `..` is resolved before the check too.
    let escape = worktree.join("../../../../../../../../../../etc/hosts");
    assert!(read(&mut host, &escape).is_err());
    assert!(read(&mut host, Path::new("/etc/hosts")).is_err());
}

#[test]
fn a_file_over_50_mb_is_refused() {
    let Store {
        data: _data,
        mut host,
        worktree,
        workspaces: _workspaces,
        ..
    } = store();
    let large = worktree.join("large.bin");
    fs::File::create(&large)
        .unwrap()
        .set_len(60 * 1024 * 1024)
        .unwrap();
    let error = read(&mut host, &large).unwrap_err();
    assert!(error.contains("over 50 MB"), "{error}");
}

#[test]
fn the_store_and_the_rest_of_the_data_folder_are_never_served() {
    let Store {
        data,
        mut host,
        project,
        workspaces: _workspaces,
        ..
    } = store();
    fs::write(data.path().join("workspace.sqlite3-wal"), "journal").unwrap();
    fs::create_dir_all(data.path().join("backups")).unwrap();
    fs::write(data.path().join("backups/workspace.sqlite3"), "copy").unwrap();
    for file in [
        "workspace.sqlite3",
        "workspace.sqlite3-wal",
        "backups/workspace.sqlite3",
        "host.lock",
    ] {
        let error = read(&mut host, &data.path().join(file)).unwrap_err();
        assert!(
            error.contains("only attachments and project folders"),
            "{file}: {error}"
        );
    }
    // Nor does an attachment store that is a symlink to `backups/`.
    let attachments = data
        .path()
        .join("projects")
        .join(&project.id)
        .join("attachments");
    fs::create_dir_all(attachments.parent().unwrap()).unwrap();
    let _ = fs::remove_dir_all(&attachments);
    std::os::unix::fs::symlink(data.path().join("backups"), &attachments).unwrap();
    assert!(read(&mut host, &attachments.join("workspace.sqlite3")).is_err());
    assert!(read(&mut host, &data.path().join("backups/workspace.sqlite3")).is_err());
    // Nor does a workspace folder or a repository set inside the data folder serve its copies.
    host.set_workspaces_dir(data.path().join("backups").to_str().unwrap())
        .unwrap();
    assert!(read(&mut host, &data.path().join("backups/workspace.sqlite3")).is_err());
}

#[test]
fn an_attachment_is_served_whatever_its_projects_home() {
    let Store {
        data,
        mut host,
        project,
        workspaces,
        ..
    } = store();
    let home = host.project_directory(&project.id).unwrap();
    assert!(home.starts_with(workspaces.path()));
    let attachments = data
        .path()
        .join("projects")
        .join(&project.id)
        .join("attachments/message");
    fs::create_dir_all(&attachments).unwrap();
    fs::write(attachments.join("photo.png"), b"\x89PNG").unwrap();
    assert_eq!(
        read(&mut host, &attachments.join("photo.png")).unwrap(),
        b"\x89PNG"
    );
}

#[test]
fn listings_name_folders_under_the_roots() {
    let Store {
        data: _data,
        mut host,
        worktree,
        workspaces: _workspaces,
        ..
    } = store();
    fs::create_dir_all(worktree.join("src")).unwrap();
    fs::create_dir_all(worktree.join("vendored/.git")).unwrap();
    fs::write(worktree.join("README.md"), "").unwrap();
    let listing: FolderListing = serde_json::from_value(
        host.execute(Command::ListFolder {
            path: worktree.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    let names: Vec<_> = listing
        .folders
        .iter()
        .map(|f| (f.name.as_str(), f.repository))
        .collect();
    assert_eq!(names, [("src", false), ("vendored", true)]);
    assert!(
        host.execute(Command::ListFolder {
            path: "/etc".into()
        })
        .is_err()
    );
}

#[test]
fn a_project_home_in_a_workspace_folder_inside_the_data_folder_is_served() {
    let data = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(data.path()).unwrap();
    let workspaces = data.path().join("workspaces");
    host.set_workspaces_dir(workspaces.to_str().unwrap())
        .unwrap();
    let project = host.create_project("x").unwrap();
    let home = host.project_directory(&project.id).unwrap();
    assert_eq!(home, workspaces.join("projects/x"));
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("plan.md"), "the plan").unwrap();
    assert_eq!(read(&mut host, &home.join("plan.md")).unwrap(), b"the plan");
}

#[test]
fn a_project_home_reached_through_a_workspace_folder_alias_is_served() {
    let data = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(data.path()).unwrap();
    let workspaces = data.path().join("workspaces");
    fs::create_dir_all(&workspaces).unwrap();
    let alias = outside.path().join("workspace-alias");
    std::os::unix::fs::symlink(&workspaces, &alias).unwrap();
    host.set_workspaces_dir(alias.to_str().unwrap()).unwrap();
    let project = host.create_project("y").unwrap();
    let home = host.project_directory(&project.id).unwrap();
    assert_eq!(home, alias.join("projects/y"));
    fs::create_dir_all(home.join("src")).unwrap();
    fs::write(home.join("plan.md"), "the plan").unwrap();
    assert_eq!(read(&mut host, &home.join("plan.md")).unwrap(), b"the plan");
    let listing = host.execute(Command::ListFolder { path: home }).unwrap();
    assert_eq!(listing["folders"][0]["name"], "src");
}
