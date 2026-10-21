//! Copies of the files in drafts, made by this app when they are dropped or pasted. macOS lets
//! only the app that received a drop read some dropped files, such as a screenshot's floating
//! thumbnail, and only around the drop; the host, which stores a message's files when it is
//! sent, reads these copies instead. Each copy has its own folder,
//! `<data dir>/staged-attachments/<id>/<file name>`, so it keeps its name. A copy is removed
//! when its draft is sent or it is taken out of the draft, and the whole folder when the app
//! starts.
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use workspace_core::{MAX_ATTACHMENT_BYTES, new_id};

#[derive(Clone)]
pub(crate) struct Staging {
    folder: PathBuf,
}
impl gpui::Global for Staging {}
impl Staging {
    /// The staging folder in `home`, emptied of copies an earlier run left.
    pub(crate) fn open(home: &Path) -> Self {
        let folder = home.join("staged-attachments");
        let _ = fs::remove_dir_all(&folder);
        Self { folder }
    }

    /// A copy of a dropped file to attach in its place, or why it cannot be copied. Folders and
    /// files over the size limit are not copied: the path comes back as it is, for the host's
    /// check to refuse with its reason.
    pub(crate) fn stage(&self, source: &Path) -> Result<PathBuf, String> {
        let name = file_name(source);
        let metadata = fs::metadata(source).map_err(|e| format!("{name} cannot be read: {e}"))?;
        if !metadata.is_file() || metadata.len() > MAX_ATTACHMENT_BYTES {
            return Ok(source.to_path_buf());
        }
        self.write(&name, |copy| fs::copy(source, copy).map(drop))
            .map_err(|e| format!("Could not copy {name}: {e}"))
    }

    /// Writes a pasted image as a PNG named `name`, converting it from another format.
    pub(crate) fn stage_image(&self, name: &str, image: &gpui::Image) -> Result<PathBuf, String> {
        let png = png(image).map_err(|e| format!("{name} could not be read as an image: {e}"))?;
        self.write(name, |copy| fs::write(copy, &png))
            .map_err(|e| format!("Could not save {name}: {e}"))
    }

    /// Removes a copy this staging made, with its folder. Any other path is left alone.
    pub(crate) fn release(&self, path: &Path) {
        if let Some(folder) = path.parent().filter(|f| f.parent() == Some(&self.folder)) {
            let _ = fs::remove_dir_all(folder);
        }
    }

    fn write(
        &self,
        name: &str,
        write: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> std::io::Result<PathBuf> {
        let folder = self.folder.join(new_id());
        let copy = folder.join(name);
        let written = fs::create_dir_all(&folder).and_then(|()| write(&copy));
        if let Err(error) = written {
            let _ = fs::remove_dir_all(&folder);
            return Err(error);
        }
        Ok(copy)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The image's bytes as a PNG.
fn png(image: &gpui::Image) -> Result<Vec<u8>, String> {
    if image.format == gpui::ImageFormat::Png {
        return Ok(image.bytes.clone());
    }
    let format = image::ImageFormat::from_mime_type(image.format.mime_type())
        .ok_or("this kind of image cannot be attached")?;
    let decoded =
        image::load_from_memory_with_format(&image.bytes, format).map_err(|e| e.to_string())?;
    let mut png = Cursor::new(vec![]);
    (decoded.write_to(&mut png, image::ImageFormat::Png)).map_err(|e| e.to_string())?;
    Ok(png.into_inner())
}

/// A pasted image's name, `Pasted image 2026-10-21 at 14.03.12.png` like a macOS screenshot's,
/// numbered from 2 when `taken` holds it already.
pub(crate) fn pasted_image_name(
    at: chrono::DateTime<chrono::Local>,
    taken: impl Fn(&str) -> bool,
) -> String {
    let stem = format!("Pasted image {}", at.format("%Y-%m-%d at %H.%M.%S"));
    std::iter::once(format!("{stem}.png"))
        .chain((2..).map(|n| format!("{stem} {n}.png")))
        .find(|name| !taken(name))
        .expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::{Staging, pasted_image_name};
    use chrono::TimeZone;
    use std::fs;

    #[test]
    fn a_dropped_file_is_copied_and_released_and_leftovers_go_at_start() {
        let home = tempfile::tempdir().unwrap();
        let staging = Staging::open(home.path());
        let source = home.path().join("shot.png");
        fs::write(&source, b"png").unwrap();

        let copy = staging.stage(&source).unwrap();
        assert!(copy.starts_with(home.path().join("staged-attachments")));
        assert_eq!(copy.file_name().unwrap(), "shot.png");
        assert_eq!(fs::read(&copy).unwrap(), b"png");

        // Only staged copies are removed.
        staging.release(&source);
        assert!(source.exists());
        staging.release(&copy);
        assert!(!copy.parent().unwrap().exists());

        let left = staging.stage(&source).unwrap();
        Staging::open(home.path());
        assert!(!left.exists() && !home.path().join("staged-attachments").exists());
    }

    #[test]
    fn a_pasted_image_is_saved_as_png() {
        let home = tempfile::tempdir().unwrap();
        let staging = Staging::open(home.path());
        let mut bmp = std::io::Cursor::new(vec![]);
        (image::RgbImage::new(2, 2).write_to(&mut bmp, image::ImageFormat::Bmp)).unwrap();
        let pasted = gpui::Image::from_bytes(gpui::ImageFormat::Bmp, bmp.into_inner());

        let saved = staging.stage_image("Pasted image.png", &pasted).unwrap();
        assert_eq!(saved.file_name().unwrap(), "Pasted image.png");
        assert!(fs::read(&saved).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn pasted_images_are_named_by_time_and_numbered_apart() {
        let at = chrono::Local
            .with_ymd_and_hms(2026, 10, 21, 14, 3, 12)
            .unwrap();
        let first = "Pasted image 2026-10-21 at 14.03.12.png";
        assert_eq!(pasted_image_name(at, |_| false), first);
        let taken = [first, "Pasted image 2026-10-21 at 14.03.12 2.png"];
        assert_eq!(
            pasted_image_name(at, |name| taken.contains(&name)),
            "Pasted image 2026-10-21 at 14.03.12 3.png"
        );
    }
}
