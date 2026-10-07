//! View choices that belong to this client rather than the host; a remote client keeps its own.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Preferences {
    /// Show a working agent's activity as one line instead of the full card.
    #[serde(default)]
    pub minimize_activity: bool,
    #[serde(skip)]
    path: PathBuf,
}
impl Preferences {
    /// A missing or unreadable file means the defaults.
    pub fn load(home: &Path) -> Self {
        let path = home.join("desktop-preferences.json");
        let saved = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok());
        Self {
            path,
            ..saved.unwrap_or_default()
        }
    }
    /// Replaces the file in one step, so a crash never leaves half a file behind.
    pub fn save(&self) -> std::io::Result<()> {
        let staged = self.path.with_extension("json.tmp");
        std::fs::write(&staged, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(staged, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_survive_a_relaunch_and_bad_files_fall_back() {
        let home = tempfile::tempdir().unwrap();
        assert!(!Preferences::load(home.path()).minimize_activity);
        let mut preferences = Preferences::load(home.path());
        preferences.minimize_activity = true;
        preferences.save().unwrap();
        assert!(Preferences::load(home.path()).minimize_activity);
        std::fs::write(home.path().join("desktop-preferences.json"), "not json").unwrap();
        assert!(!Preferences::load(home.path()).minimize_activity);
    }
}
