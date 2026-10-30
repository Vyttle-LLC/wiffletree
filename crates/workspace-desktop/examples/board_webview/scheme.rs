//! The minimal `wiffletree-board://` scheme: `wiffletree-board://<board-id>/<path>` serves
//! `<path>` from that board's folder and nothing else. Task 4.2 builds the full sandbox.

use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};

use wry::http::{Response, StatusCode, Uri, header};

pub const SCHEME: &str = "wiffletree-board";

/// The policy design decision 6 puts on every board response.
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'self'; \
    style-src 'unsafe-inline' 'self'; img-src 'self' data: blob:; font-src 'self' data:; \
    connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

pub struct BoardFolder {
    id: String,
    root: PathBuf,
}

impl BoardFolder {
    pub fn new(id: &str, root: &Path) -> io::Result<Self> {
        Ok(Self {
            id: id.to_owned(),
            root: root.canonicalize()?,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{SCHEME}://{}/{path}", self.id)
    }

    pub fn respond(&self, uri: &Uri) -> Response<Cow<'static, [u8]>> {
        let response = Response::builder().header("Content-Security-Policy", CSP);
        let response = match self.read(uri) {
            Some((body, content_type)) => response
                .header(header::CONTENT_TYPE, content_type)
                .body(Cow::Owned(body)),
            None => response
                .status(StatusCode::NOT_FOUND)
                .body(Cow::Borrowed(&[][..])),
        };
        response.expect("static headers are valid")
    }

    /// Reads a file only when the URL names this board and the resolved path, after symbolic
    /// links, is still inside its folder.
    fn read(&self, uri: &Uri) -> Option<(Vec<u8>, &'static str)> {
        if uri.scheme_str() != Some(SCHEME) || uri.host() != Some(self.id.as_str()) {
            return None;
        }
        let file = self
            .root
            .join(uri.path().trim_start_matches('/'))
            .canonicalize()
            .ok()?;
        if !file.starts_with(&self.root) || !file.is_file() {
            return None;
        }
        Some((std::fs::read(&file).ok()?, content_type(&file)))
    }
}

fn content_type(file: &Path) -> &'static str {
    match file.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> (tempfile::TempDir, BoardFolder) {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("board");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), "<p>board</p>").unwrap();
        std::fs::write(parent.path().join("secret.txt"), "secret").unwrap();
        let folder = BoardFolder::new("fixture", &root).unwrap();
        (parent, folder)
    }

    fn get(folder: &BoardFolder, url: &str) -> Response<Cow<'static, [u8]>> {
        folder.respond(&url.parse().unwrap())
    }

    #[test]
    fn serves_the_board_folder_with_the_policy() {
        let (_parent, folder) = board();
        let response = get(&folder, &folder.url("index.html"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"<p>board</p>");
        assert_eq!(response.headers()["Content-Security-Policy"], CSP);
    }

    #[test]
    fn refuses_anything_outside_the_board_folder() {
        let (parent, folder) = board();
        std::os::unix::fs::symlink(
            parent.path().join("secret.txt"),
            parent.path().join("board/link.txt"),
        )
        .unwrap();
        for url in [
            "wiffletree-board://fixture/../secret.txt",
            "wiffletree-board://fixture/link.txt",
            "wiffletree-board://other/index.html",
            "https://fixture/index.html",
        ] {
            let response = get(&folder, url);
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{url}");
            assert_eq!(response.headers()["Content-Security-Policy"], CSP, "{url}");
        }
    }
}
