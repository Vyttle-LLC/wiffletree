//! Files a remote device attaches, staged in pieces under `<data folder>/uploads/` until a
//! message names them, for at most an hour. They live only as long as the host runs.
use crate::*;
use base64::Engine as _;
use std::{collections::HashMap, io::Write as _};

const FOLDER: &str = "uploads";

/// An upload the host holds.
#[derive(Debug)]
struct Upload {
    name: String,
    stored: u64,
    /// When the last piece arrived, or a message used it.
    touched_at: i64,
    /// A message stored it; only its name is kept, so a resent message is still recognised.
    used: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Uploads(HashMap<String, Upload>);

/// An id or a file name that cannot leave its folder.
fn plain(name: &str, limit: usize) -> bool {
    !name.is_empty()
        && name.len() <= limit
        && name != "."
        && name != ".."
        && !name.contains(['/', '\0'])
}

/// Adds the table of the upload ids each message was stored with (schema 8), so a message sent
/// again is recognised by exactly those ids after its uploads are gone.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_uploads (
             message_id TEXT NOT NULL,
             position INTEGER NOT NULL,
             upload_id TEXT NOT NULL,
             PRIMARY KEY (message_id, position)
         );",
    )?;
    Ok(())
}

/// Records the upload ids a message is stored with, in order, in the message's own transaction.
pub(crate) fn record_message_uploads(
    db: &Connection,
    message: &str,
    uploads: &[String],
) -> Result<()> {
    for (position, upload) in uploads.iter().enumerate() {
        db.execute(
            "INSERT INTO message_uploads (message_id, position, upload_id) VALUES (?1, ?2, ?3)",
            params![message, position as i64, upload],
        )?;
    }
    Ok(())
}

impl Host {
    /// The upload ids a message was stored with, in order.
    pub(crate) fn message_uploads(&self, message: &str) -> Result<Vec<String>> {
        let mut statement = self.db.prepare(
            "SELECT upload_id FROM message_uploads WHERE message_id = ?1 ORDER BY position",
        )?;
        let ids = statement.query_map([message], |r| r.get(0))?;
        Ok(ids.collect::<rusqlite::Result<_>>()?)
    }
    fn uploads_folder(&self) -> PathBuf {
        self.home.join(FOLDER)
    }
    fn upload_path(&self, id: &str, name: &str) -> PathBuf {
        self.uploads_folder().join(id).join(name)
    }
    /// Removes the uploads a previous run of the host left.
    pub(crate) fn clear_uploads(&self) -> Result<()> {
        match fs::remove_dir_all(self.uploads_folder()) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
    /// Stores a piece where the upload ends; a piece elsewhere, such as one sent again after a
    /// drop, changes nothing. Answers how much is stored.
    pub fn upload_piece(
        &mut self,
        id: &str,
        name: &str,
        offset: u64,
        bytes: &str,
        at: i64,
    ) -> Result<Uploaded> {
        self.expire_uploads(at);
        ensure!(plain(id, 64), "Name the upload with a plain id");
        ensure!(plain(name, 255), "Name the file without a folder");
        let piece = base64::engine::general_purpose::STANDARD
            .decode(bytes)
            .context("The piece is not base64")?;
        ensure!(
            piece.len() <= UPLOAD_PIECE_BYTES,
            "A piece is at most 128 KiB"
        );
        let path = self.upload_path(id, name);
        let upload = match self.uploads.0.get_mut(id) {
            Some(upload) => {
                ensure!(!upload.used, "That upload was already sent");
                ensure!(upload.name == name, "That upload has another file name");
                upload
            }
            None if offset == 0 => {
                fs::create_dir_all(path.parent().context("An upload has a folder")?)?;
                fs::write(&path, b"")?;
                self.uploads.0.entry(id.into()).or_insert(Upload {
                    name: name.into(),
                    stored: 0,
                    touched_at: at,
                    used: false,
                })
            }
            None => bail!("No upload has that id; start it at offset 0"),
        };
        if offset == upload.stored && !piece.is_empty() {
            ensure!(
                upload.stored + piece.len() as u64 <= MAX_ATTACHMENT_BYTES,
                "{name} is over 50 MB"
            );
            fs::OpenOptions::new()
                .append(true)
                .open(&path)?
                .write_all(&piece)?;
            upload.stored += piece.len() as u64;
        }
        upload.touched_at = at;
        Ok(Uploaded {
            upload_id: id.into(),
            stored: upload.stored,
        })
    }
    /// The staged files `ids` name, in order, for a message to attach.
    pub(crate) fn upload_files(&self, ids: &[String]) -> Result<Vec<PathBuf>> {
        ids.iter()
            .map(|id| {
                let upload = self.uploads.0.get(id).context("No upload has that id")?;
                Ok(self.upload_path(id, &upload.name))
            })
            .collect()
    }
    /// Deletes the files of uploads a message stored, keeping their names for an hour so the
    /// message can be sent again.
    pub(crate) fn uploads_used(&mut self, ids: &[String], at: i64) {
        for id in ids {
            if let Some(upload) = self.uploads.0.get_mut(id) {
                let _ = fs::remove_dir_all(self.home.join(FOLDER).join(id));
                upload.used = true;
                upload.touched_at = at;
            }
        }
    }
    /// Deletes uploads untouched for an hour.
    pub(crate) fn expire_uploads(&mut self, at: i64) {
        let folder = self.uploads_folder();
        self.uploads.0.retain(|id, upload| {
            let kept = at - upload.touched_at < UPLOAD_KEPT_MS;
            if !kept {
                let _ = fs::remove_dir_all(folder.join(id));
            }
            kept
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn an_unused_upload_is_deleted_after_an_hour() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        host.upload_piece("a", "shot.png", 0, &piece(b"png"), 0)
            .unwrap();
        let path = host.upload_files(&["a".into()]).unwrap().remove(0);
        assert_eq!(fs::read(&path).unwrap(), b"png");
        host.expire_uploads(UPLOAD_KEPT_MS - 1);
        assert!(path.exists(), "kept just under an hour");
        host.expire_uploads(UPLOAD_KEPT_MS);
        assert!(!path.exists());
        assert!(host.upload_files(&["a".into()]).is_err());
    }

    #[test]
    fn the_piece_that_would_pass_50_mb_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let full = vec![0u8; UPLOAD_PIECE_BYTES];
        host.upload_piece("big", "video.mov", 0, &piece(&full), 0)
            .unwrap();
        // As though pieces up to just under the limit had arrived.
        let path = host.upload_files(&["big".into()]).unwrap().remove(0);
        let almost = MAX_ATTACHMENT_BYTES - 10;
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(almost)
            .unwrap();
        host.uploads.0.get_mut("big").unwrap().stored = almost;
        let fits = host.upload_piece("big", "video.mov", almost, &piece(&[1; 10]), 1);
        assert_eq!(fits.unwrap().stored, MAX_ATTACHMENT_BYTES);
        let crossing = host.upload_piece("big", "video.mov", MAX_ATTACHMENT_BYTES, &piece(&[1]), 2);
        assert!(crossing.unwrap_err().to_string().contains("over 50 MB"));
    }

    #[test]
    fn a_piece_sent_again_is_not_stored_twice_and_names_stay_in_their_folder() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let first = host.upload_piece("u", "notes.txt", 0, &piece(b"abc"), 0);
        assert_eq!(first.unwrap().stored, 3);
        let again = host.upload_piece("u", "notes.txt", 0, &piece(b"abc"), 0);
        assert_eq!(again.unwrap().stored, 3, "the resent piece changes nothing");
        let next = host.upload_piece("u", "notes.txt", 3, &piece(b"def"), 0);
        assert_eq!(next.unwrap().stored, 6);
        let path = host.upload_files(&["u".into()]).unwrap().remove(0);
        assert_eq!(fs::read(path).unwrap(), b"abcdef");
        for (id, name) in [("../x", "a"), ("u2", "../a"), ("u3", ".."), ("", "a")] {
            assert!(host.upload_piece(id, name, 0, &piece(b"x"), 0).is_err());
        }
        assert!(host.upload_piece("late", "a", 5, &piece(b"x"), 0).is_err());
    }
}
