//! Files the human attaches to a message: checked when dropped, copied when sent.
use crate::*;
use std::{io::Read, os::unix::fs::PermissionsExt};

pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Describes a file that can be attached, or says why it cannot.
pub fn inspect(path: &Path) -> Result<Attachment> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    let metadata = fs::metadata(path).with_context(|| format!("{name} cannot be read"))?;
    ensure!(
        !metadata.is_dir(),
        "{name} is a folder; only files can be attached"
    );
    ensure!(metadata.is_file(), "{name} is not a regular file");
    ensure!(
        metadata.len() <= MAX_ATTACHMENT_BYTES,
        "{name} is larger than 50 MB"
    );
    Ok(Attachment {
        path: path.to_path_buf(),
        size: metadata.len(),
        image: image_format(path),
    })
}

/// Judges by the file's first bytes, then by its extension.
fn image_format(path: &Path) -> Option<ImageFormat> {
    let mut head = [0; 12];
    let read = File::open(path)
        .and_then(|mut file| file.read(&mut head))
        .unwrap_or(0);
    let head = &head[..read];
    let by_content = if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some(ImageFormat::Jpeg)
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if head.len() == 12 && head.starts_with(b"RIFF") && &head[8..] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    };
    by_content.or_else(|| {
        match path
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => Some(ImageFormat::Png),
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "gif" => Some(ImageFormat::Gif),
            "webp" => Some(ImageFormat::Webp),
            _ => None,
        }
    })
}

/// Where a message's attachments live: `<store>/projects/<project>/attachments/<message>`.
pub fn directory(home: &Path, project: &str, message: &str) -> Result<PathBuf> {
    ensure!(
        Path::new(message).file_name() == Some(message.as_ref()),
        "Message ID cannot name a folder"
    );
    Ok(home
        .join("projects")
        .join(project)
        .join("attachments")
        .join(message))
}

/// The path Codex is given for a message's `index`th attachment. Codex splits `--image`
/// values at commas, so an image named with one is reached through a comma-free hard link
/// that `store` makes beside it.
pub(crate) fn codex_image_path(attachment: &Attachment, index: usize) -> PathBuf {
    match attachment.image {
        Some(format) if attachment.name().contains(',') => {
            let extension = format.media_type().trim_start_matches("image/");
            attachment
                .path
                .with_file_name(format!(".codex-{index}.{extension}"))
        }
        _ => attachment.path.clone(),
    }
}

/// Copies `sources` into `directory` as read-only files. On failure nothing is left behind.
pub(crate) fn store(directory: &Path, sources: &[PathBuf]) -> Result<Vec<Attachment>> {
    let copied = (|| -> Result<Vec<Attachment>> {
        fs::create_dir_all(directory)?;
        sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let original = inspect(source)?;
                let copy = directory.join(original.name());
                ensure!(
                    !copy.exists(),
                    "Two attachments are named {}",
                    original.name()
                );
                fs::copy(source, &copy)
                    .with_context(|| format!("Could not copy {}", original.name()))?;
                fs::set_permissions(&copy, fs::Permissions::from_mode(0o444))?;
                let stored = Attachment {
                    path: copy,
                    ..original
                };
                let link = codex_image_path(&stored, index);
                if link != stored.path {
                    fs::hard_link(&stored.path, &link)?;
                }
                Ok(stored)
            })
            .collect()
    })();
    if copied.is_err() {
        let _ = fs::remove_dir_all(directory);
    }
    copied
}

/// Adds the messages' attachment column (schema 5). Existing messages have none.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let present: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='attachments'",
        [],
        |r| r.get(0),
    )?;
    if present == 0 {
        db.execute_batch("ALTER TABLE messages ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]'")?;
    }
    db.execute_batch("PRAGMA user_version = 5;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn folders_and_large_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("screens");
        fs::create_dir(&folder).unwrap();
        let error = inspect(&folder).unwrap_err().to_string();
        assert_eq!(error, "screens is a folder; only files can be attached");

        let large = file(dir.path(), "video.mov", b"");
        File::options()
            .write(true)
            .open(&large)
            .unwrap()
            .set_len(MAX_ATTACHMENT_BYTES + 1)
            .unwrap();
        let error = inspect(&large).unwrap_err().to_string();
        assert_eq!(error, "video.mov is larger than 50 MB");

        let limit = file(dir.path(), "limit.bin", b"");
        File::options()
            .write(true)
            .open(&limit)
            .unwrap()
            .set_len(MAX_ATTACHMENT_BYTES)
            .unwrap();
        assert_eq!(inspect(&limit).unwrap().size, MAX_ATTACHMENT_BYTES);
    }

    #[test]
    fn images_are_recognized_by_content_or_extension() {
        let dir = tempfile::tempdir().unwrap();
        let cases = [
            (
                "shot",
                b"\x89PNG\r\n\x1a\n....".as_slice(),
                Some(ImageFormat::Png),
            ),
            ("photo.txt", b"\xff\xd8\xff\xe0", Some(ImageFormat::Jpeg)),
            ("anim", b"GIF89a", Some(ImageFormat::Gif)),
            ("still", b"RIFF\0\0\0\0WEBP", Some(ImageFormat::Webp)),
            ("empty.JPG", b"", Some(ImageFormat::Jpeg)),
            ("notes.txt", b"hello", None),
        ];
        for (name, bytes, expected) in cases {
            let attachment = inspect(&file(dir.path(), name, bytes)).unwrap();
            assert_eq!(attachment.image, expected, "{name}");
            assert_eq!(attachment.size, bytes.len() as u64);
        }
    }

    #[test]
    fn sent_files_are_copied_read_only_under_the_message() {
        let source = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let notes = file(source.path(), "notes.txt", b"hello");
        let target = directory(home.path(), "project", "message").unwrap();
        assert_eq!(
            target,
            home.path().join("projects/project/attachments/message")
        );
        let stored = store(&target, std::slice::from_ref(&notes)).unwrap();
        assert_eq!(
            stored,
            vec![Attachment {
                path: target.join("notes.txt"),
                size: 5,
                image: None
            }]
        );
        assert_eq!(fs::read(target.join("notes.txt")).unwrap(), b"hello");
        let mode = fs::metadata(target.join("notes.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o444);
        assert!(notes.exists(), "the original stays where it was");
    }

    #[test]
    fn codex_reaches_comma_named_images_through_a_link() {
        let source = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let shot = file(source.path(), "odd,shot.png", b"\x89PNG\r\n\x1a\nimage");
        let notes = file(source.path(), "a,b.txt", b"hello");
        let plain = file(source.path(), "plain.png", b"\x89PNG\r\n\x1a\nimage");
        let target = directory(home.path(), "project", "message").unwrap();
        let stored = store(&target, &[shot, notes, plain]).unwrap();
        assert_eq!(stored[0].path, target.join("odd,shot.png"));
        let link = codex_image_path(&stored[0], 0);
        assert_eq!(link, target.join(".codex-0.png"));
        assert_eq!(fs::read(&link).unwrap(), fs::read(&stored[0].path).unwrap());
        // Only comma-named images need a link.
        assert_eq!(codex_image_path(&stored[1], 1), stored[1].path);
        assert_eq!(codex_image_path(&stored[2], 2), stored[2].path);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 4);
    }

    #[test]
    fn a_failed_copy_leaves_nothing_behind() {
        let source = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let notes = file(source.path(), "notes.txt", b"hello");
        let target = directory(home.path(), "project", "message").unwrap();
        let missing = source.path().join("gone.txt");
        assert!(store(&target, &[notes, missing]).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn message_ids_cannot_escape_the_attachment_folder() {
        assert!(directory(Path::new("/store"), "p", "../escape").is_err());
        assert!(directory(Path::new("/store"), "p", "..").is_err());
    }
}
