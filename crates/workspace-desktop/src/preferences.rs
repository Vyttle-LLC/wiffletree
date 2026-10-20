//! View choices that belong to this client rather than the host; a remote client keeps its own.
use crate::hosts::PairedHost;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Preferences {
    /// Show a working agent's activity as one line instead of the full card.
    #[serde(default)]
    pub minimize_activity: bool,
    #[serde(default, deserialize_with = "appearance_or_system")]
    pub appearance: Appearance,
    /// Show projects whose names share a first word under one group row.
    #[serde(default = "on")]
    pub group_projects: bool,
    /// The hosts this device is paired with; they stay on this device.
    #[serde(default)]
    pub paired_hosts: Vec<PairedHost>,
    /// Look for Wiffletree on other Macs on this network; off, macOS never asks about it.
    #[serde(default)]
    pub find_on_network: bool,
    #[serde(skip)]
    path: PathBuf,
}
impl gpui::Global for Preferences {}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            minimize_activity: false,
            appearance: Appearance::default(),
            group_projects: true,
            paired_hosts: vec![],
            find_on_network: false,
            path: PathBuf::new(),
        }
    }
}

fn on() -> bool {
    true
}

/// A mode this version does not know, perhaps saved by a newer one, reads as System
/// rather than discarding the rest of the file.
fn appearance_or_system<'de, D: Deserializer<'de>>(input: D) -> Result<Appearance, D::Error> {
    Ok(Appearance::deserialize(serde_json::Value::deserialize(input)?).unwrap_or_default())
}

impl Preferences {
    /// A missing or unreadable file means the defaults.
    pub(crate) fn load(home: &Path) -> Self {
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
    pub(crate) fn save(&self) -> std::io::Result<()> {
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
    fn grouping_starts_on_and_the_choice_survives_a_relaunch() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("desktop-preferences.json");
        assert!(Preferences::load(home.path()).group_projects);
        // A file saved before the choice existed, and one that cannot be read.
        std::fs::write(
            &file,
            r#"{ "minimize_activity": true, "appearance": "dark" }"#,
        )
        .unwrap();
        let older = Preferences::load(home.path());
        assert!(older.group_projects && older.minimize_activity);
        assert_eq!(older.appearance, Appearance::Dark);
        std::fs::write(&file, "not json").unwrap();
        assert!(Preferences::load(home.path()).group_projects);

        let mut preferences = Preferences::load(home.path());
        preferences.group_projects = false;
        preferences.save().unwrap();
        assert!(!Preferences::load(home.path()).group_projects);
        preferences.group_projects = true;
        preferences.save().unwrap();
        assert!(Preferences::load(home.path()).group_projects);
    }

    #[test]
    fn paired_hosts_survive_a_relaunch_and_an_older_file_has_none() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("desktop-preferences.json");
        std::fs::write(&file, r#"{ "appearance": "dark" }"#).unwrap();
        assert!(Preferences::load(home.path()).paired_hosts.is_empty());

        let studio = PairedHost {
            id: "ab12".into(),
            public_key: "BAE=".into(),
            name: "Studio Mac".into(),
            addresses: vec!["studio.local:47821".into()],
            last_seen: Some(5),
        };
        let mut preferences = Preferences::load(home.path());
        preferences.paired_hosts.push(studio.clone());
        preferences.save().unwrap();
        let loaded = Preferences::load(home.path());
        assert_eq!(loaded.paired_hosts, [studio]);
        assert_eq!(loaded.appearance, Appearance::Dark);
    }

    #[test]
    fn finding_other_macs_starts_off_even_in_an_older_file_and_survives_a_relaunch() {
        let home = tempfile::tempdir().unwrap();
        assert!(!Preferences::load(home.path()).find_on_network);
        let file = home.path().join("desktop-preferences.json");
        std::fs::write(&file, r#"{ "appearance": "dark", "paired_hosts": [] }"#).unwrap();
        assert!(!Preferences::load(home.path()).find_on_network);

        let mut preferences = Preferences::load(home.path());
        preferences.find_on_network = true;
        preferences.save().unwrap();
        assert!(Preferences::load(home.path()).find_on_network);
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
    fn missing_or_unknown_appearance_preserves_other_choices() {
        for saved in [
            r#"{ "minimize_activity": true }"#,
            r#"{ "minimize_activity": true, "appearance": "sepia" }"#,
        ] {
            let home = tempfile::tempdir().unwrap();
            std::fs::write(home.path().join("desktop-preferences.json"), saved).unwrap();
            let preferences = Preferences::load(home.path());
            assert!(preferences.minimize_activity, "{saved}");
            assert_eq!(preferences.appearance, Appearance::System, "{saved}");
        }
    }
}
