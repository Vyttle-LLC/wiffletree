use std::{fs, path::PathBuf};
use workspace_core::{Attachment, ImageFormat, Session};
use workspace_host::Host;

fn coordinator(host: &mut Host) -> Session {
    let project = host.create_project("Screenshots").unwrap();
    host.sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id)
        .unwrap()
}

fn dropped(dir: &std::path::Path) -> Vec<PathBuf> {
    let shot = dir.join("shot.png");
    let notes = dir.join("notes.txt");
    fs::write(&shot, b"\x89PNG\r\n\x1a\nimage").unwrap();
    fs::write(&notes, b"hello").unwrap();
    vec![shot, notes]
}

#[test]
fn attachments_are_stored_with_the_message_and_survive_a_restart() {
    let directory = tempfile::tempdir().unwrap();
    let files = dropped(directory.path());
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let root = coordinator(&mut host);
    let sent = host
        .send_with_files("m1", None, &root.id, "", &files)
        .unwrap();
    let folder = host
        .home
        .join("projects")
        .join(&root.project_id)
        .join("attachments/m1");
    let expected = vec![
        Attachment {
            path: folder.join("shot.png"),
            size: 13,
            image: Some(ImageFormat::Png),
        },
        Attachment {
            path: folder.join("notes.txt"),
            size: 5,
            image: None,
        },
    ];
    assert_eq!(sent.attachments, expected);
    // A retried send is recognized and copies nothing again.
    assert_eq!(
        host.send_with_files("m1", None, &root.id, "", &files)
            .unwrap(),
        sent
    );
    drop(host);

    let host = Host::open(directory.path().join("home")).unwrap();
    let stored = host.messages(&root.id, None, 10).unwrap();
    assert_eq!(stored[0].attachments, expected);
    assert_eq!(fs::read(folder.join("notes.txt")).unwrap(), b"hello");
}

#[test]
fn a_failed_send_leaves_no_attachment_folder() {
    let directory = tempfile::tempdir().unwrap();
    let mut files = dropped(directory.path());
    files.push(directory.path().join("missing.txt"));
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let root = coordinator(&mut host);
    assert!(
        host.send_with_files("m1", None, &root.id, "Look", &files)
            .is_err()
    );
    let attachments = host
        .home
        .join("projects")
        .join(&root.project_id)
        .join("attachments");
    assert!(!attachments.join("m1").exists());
    assert!(host.messages(&root.id, None, 10).unwrap().is_empty());

    host.set_archived(&root.id, true).unwrap();
    files.pop();
    assert!(
        host.send_with_files("m2", None, &root.id, "Look", &files)
            .is_err()
    );
    assert!(!attachments.join("m2").exists());
}

#[test]
fn only_the_human_attaches_files() {
    let directory = tempfile::tempdir().unwrap();
    let files = dropped(directory.path());
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let root = coordinator(&mut host);
    let error = host
        .send_with_files("m1", Some(&root.id), &root.id, "Look", &files)
        .unwrap_err();
    assert_eq!(error.to_string(), "Only the human can attach files");
    assert!(host.send("m2", None, &root.id, " ").is_err());
}

#[test]
fn existing_stores_gain_attachments_without_losing_messages() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let mut host = Host::open(&home).unwrap();
    let root = coordinator(&mut host);
    host.send("old", None, &root.id, "Before attachments")
        .unwrap();
    drop(host);
    // Return the store to schema 4, as an older release left it.
    rusqlite::Connection::open(home.join("workspace.sqlite3"))
        .unwrap()
        .execute_batch("ALTER TABLE messages DROP COLUMN attachments; PRAGMA user_version = 4;")
        .unwrap();

    let host = Host::open(&home).unwrap();
    let messages = host.messages(&root.id, None, 10).unwrap();
    assert_eq!(messages[0].body, "Before attachments");
    assert!(messages[0].attachments.is_empty());
}
