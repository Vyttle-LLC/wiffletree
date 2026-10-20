//! Attaching this Mac's files to a message on a remote host (design 8 of `remote-hosts`): each
//! file goes up in pieces of `UPLOAD_PIECE_BYTES` with `UploadAttachment`, and the message names
//! the uploads. A piece cut off by a dropped connection is sent again once the host answers;
//! the host keeps only what continues where its upload ends, and says where the next starts.
use crate::bridge::Bridge;
use base64::Engine as _;
use std::{path::Path, time::Duration};
use workspace_core::{UPLOAD_PIECE_BYTES, ask, new_id};

/// How long one piece may take to be answered.
const PIECE_WAIT: Duration = Duration::from_secs(60);
/// How long a dropped connection is waited for before the upload gives up.
const RECONNECT_WAIT: Duration = Duration::from_secs(10 * 60);

/// Why a piece was not stored.
#[derive(Debug)]
enum Failed {
    /// The connection dropped or the host did not answer: send it again once it does.
    Lost,
    /// The host answered with a refusal.
    Refused(String),
}

/// Uploads `path` to `bridge`'s host and returns the upload's id. Blocks; run it off the main
/// thread.
pub(crate) fn upload(bridge: &Bridge, path: &Path) -> Result<String, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let bytes = std::fs::read(path).map_err(|e| format!("{name} cannot be read: {e}"))?;
    let upload_id = new_id();
    let mut lost_since = None;
    let sent = send_pieces(&bytes, |offset, piece| {
        let encoded = base64::engine::general_purpose::STANDARD.encode(piece);
        let ask = ask::upload_attachment(upload_id.clone(), name.clone(), offset, encoded);
        match bridge.ask_within(ask, PIECE_WAIT) {
            Ok(uploaded) => {
                lost_since = None;
                Ok(uploaded.stored)
            }
            Err(error) if bridge.connected() => Err(Failed::Refused(error)),
            Err(_) => {
                let since = *lost_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed() > RECONNECT_WAIT {
                    return Err(Failed::Refused(format!(
                        "{name} was not uploaded: the host did not come back"
                    )));
                }
                std::thread::sleep(Duration::from_secs(1));
                Err(Failed::Lost)
            }
        }
    });
    sent.map(|()| upload_id)
}

/// Sends `bytes` piece by piece through `store`, which stores the piece at an offset and answers
/// how much of the upload the host holds, continuing from there; a lost piece goes again.
fn send_pieces(
    bytes: &[u8],
    mut store: impl FnMut(u64, &[u8]) -> Result<u64, Failed>,
) -> Result<(), String> {
    let mut offset = 0usize;
    loop {
        let end = (offset + UPLOAD_PIECE_BYTES).min(bytes.len());
        match store(offset as u64, &bytes[offset..end]) {
            Ok(stored) => offset = (stored as usize).min(bytes.len()),
            Err(Failed::Lost) => continue,
            Err(Failed::Refused(error)) => return Err(error),
        }
        if offset == bytes.len() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upload_cut_off_resumes_from_what_the_host_holds() {
        let bytes: Vec<u8> = (0..UPLOAD_PIECE_BYTES * 3 + 10).map(|i| i as u8).collect();
        let mut held = vec![];
        let mut calls = 0;
        send_pieces(&bytes, |offset, piece| {
            calls += 1;
            // The second piece reaches the host, but its answer is lost.
            if calls == 2 {
                held.extend_from_slice(piece);
                return Err(Failed::Lost);
            }
            // A piece is kept only where the upload ends.
            if offset as usize == held.len() {
                held.extend_from_slice(piece);
            }
            Ok(held.len() as u64)
        })
        .unwrap();
        assert_eq!(held, bytes, "each byte stored once, in order");
    }

    #[test]
    fn a_refused_piece_ends_the_upload() {
        let bytes = vec![0u8; UPLOAD_PIECE_BYTES * 2];
        let refused = send_pieces(&bytes, |offset, _| match offset {
            0 => Ok(UPLOAD_PIECE_BYTES as u64),
            _ => Err(Failed::Refused("larger than 50 MB".into())),
        });
        assert_eq!(refused.unwrap_err(), "larger than 50 MB");
    }

    #[test]
    fn a_file_uploaded_to_a_paired_host_is_stored_with_the_message_that_names_it() {
        use crate::bridge::tests::Studio;
        let studio = Studio::start();
        let bridge = studio.bridge();
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("notes.md");
        let text = "x".repeat(UPLOAD_PIECE_BYTES + 1000);
        std::fs::write(&file, &text).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !bridge.connected() {
            assert!(std::time::Instant::now() < deadline, "never connected");
            std::thread::sleep(Duration::from_millis(50));
        }

        let upload = upload(&bridge, &file).unwrap();
        let send = workspace_core::Command::Send {
            id: new_id(),
            sender: None,
            recipient: studio.coordinator.clone(),
            body: "See the notes".into(),
            attachments: vec![],
            uploads: vec![upload],
        };
        let stored = bridge.request(send).unwrap().recv_blocking().unwrap();
        stored.unwrap();

        let page = bridge
            .ask_within(
                ask::messages(studio.coordinator.clone(), None, 50),
                PIECE_WAIT,
            )
            .unwrap();
        let message = page.iter().find(|m| m.body == "See the notes").unwrap();
        let attached = &message.attachments[0];
        assert_eq!(
            (attached.name(), attached.size),
            ("notes.md".into(), text.len() as u64)
        );
        bridge.shutdown();
    }

    #[test]
    fn an_empty_file_is_one_empty_piece() {
        let mut pieces = vec![];
        send_pieces(&[], |offset, piece| {
            pieces.push((offset, piece.len()));
            Ok(0)
        })
        .unwrap();
        assert_eq!(pieces, [(0, 0)]);
    }
}
