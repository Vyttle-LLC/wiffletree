//! Boards (OpenSpec change `assistant-spaces-jobs-boards`, stage 4): every output a thread or a
//! job saved, on this Mac or pulled from a Space's storage. Each output is a folder holding its
//! one file, with its metadata beside the folder, and each version of it is a Git commit.
use crate::SpaceId;
use crate::job::{JobKey, Runner};
use serde::{Deserialize, Serialize};

/// One output as Boards lists it, at its latest version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardOutput {
    /// Names the output on this Mac, and in its web view's `wiffletree-board://` URLs.
    pub id: String,
    pub title: String,
    pub space_id: SpaceId,
    /// The job that wrote it; `None` for an output saved from a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobKey>,
    /// The job's name, when this Mac knows the job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_name: Option<String>,
    /// The file's name in its folder.
    pub file: String,
    /// The output's folder on this Mac, which holds only its file.
    pub folder: String,
    pub updated_at: i64,
    pub keeping: Keeping,
    /// The Mac that runs the shared job that writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<Runner>,
    /// Two scheduled runs of its job passed without a new version.
    #[serde(default)]
    pub stale: bool,
    /// The first lines of a Markdown or text output, or an HTML output's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}
impl BoardOutput {
    pub fn kind(&self) -> FileKind {
        FileKind::of(&self.file)
    }
    /// The output's file on this Mac.
    pub fn path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.folder).join(&self.file)
    }
}

/// Where an output is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keeping {
    /// This Mac's private board store: only on this Mac.
    Private,
    /// The Space's storage on this Mac only, a repository with no remote.
    LocalStorage,
    /// The Space's storage repository on GitHub, which its members read.
    SharedStorage,
}

/// How Wiffletree opens an output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// Rendered natively.
    Markdown,
    /// In the embedded web view.
    Html,
    /// In Quick Look.
    Image,
    Pdf,
    /// Offered to its default app.
    Other,
}
impl FileKind {
    pub fn of(file: &str) -> Self {
        let extension = std::path::Path::new(file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "md" | "markdown" => Self::Markdown,
            "html" | "htm" => Self::Html,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "tiff" | "svg" => Self::Image,
            "pdf" => Self::Pdf,
            _ => Self::Other,
        }
    }
}

/// One version of an output: a commit that saved it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardVersion {
    pub commit: String,
    pub saved_at: i64,
    /// The job run that wrote it, or `None` for a thread's save or a share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// How many missed runs that run caught up on.
    #[serde(default)]
    pub catch_up: u32,
    /// Copied in by Share to Space….
    #[serde(default)]
    pub shared: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_output_opens_by_its_files_type() {
        for (file, kind) in [
            ("brief.md", FileKind::Markdown),
            ("dashboard.HTML", FileKind::Html),
            ("chart.png", FileKind::Image),
            ("report.pdf", FileKind::Pdf),
            ("costs.xlsx", FileKind::Other),
            ("notes", FileKind::Other),
        ] {
            assert_eq!(FileKind::of(file), kind, "{file}");
        }
    }
}
