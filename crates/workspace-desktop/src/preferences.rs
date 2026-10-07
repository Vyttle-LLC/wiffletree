//! View choices that belong to this client rather than the host; a remote client keeps its own.
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Preferences {
    /// Show a working agent's activity as one line instead of the full card.
    #[serde(default)]
    pub minimize_activity: bool,
    #[serde(default, deserialize_with = "appearance_or_system")]
    pub appearance: Appearance,
    #[serde(skip)]
    path: PathBuf,
}
impl gpui::Global for Preferences {}

/// A mode this version does not know, perhaps saved by a newer one, reads as System
/// rather than discarding the rest of the file.
fn appearance_or_system<'de, D: Deserializer<'de>>(input: D) -> Result<Appearance, D::Error> {
    Ok(Appearance::deserialize(serde_json::Value::deserialize(input)?).unwrap_or_default())
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

    #[test]
    fn appearance_survives_a_relaunch() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            Preferences::load(home.path()).appearance,
            Appearance::System
        );
        for appearance in [Appearance::Dark, Appearance::Light, Appearance::System] {
            let mut preferences = Preferences::load(home.path());
            preferences.appearance = appearance;
            preferences.save().unwrap();
            assert_eq!(Preferences::load(home.path()).appearance, appearance);
        }
    }

    #[test]
    fn a_file_from_before_appearance_keeps_its_choices() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("desktop-preferences.json"),
            r#"{ "minimize_activity": true }"#,
        )
        .unwrap();
        let preferences = Preferences::load(home.path());
        assert!(preferences.minimize_activity);
        assert_eq!(preferences.appearance, Appearance::System);
    }

    #[test]
    fn an_unknown_appearance_reads_as_system_and_keeps_the_rest() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("desktop-preferences.json"),
            r#"{ "minimize_activity": true, "appearance": "sepia" }"#,
        )
        .unwrap();
        let preferences = Preferences::load(home.path());
        assert!(preferences.minimize_activity);
        assert_eq!(preferences.appearance, Appearance::System);
    }
}
