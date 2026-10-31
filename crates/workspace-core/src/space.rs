//! Spaces: the one home for projects, their brain, storage and tracker (OpenSpec change `spaces`).
//! A `Space` is what its storage repository's `space.yaml` holds; `SpaceLocal` is what stays on
//! this Mac.
use crate::ModelProfile;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Empty for a project from a host older than Spaces, which shows as "No Space".
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpaceId(pub String);
impl SpaceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for SpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of the Space the host creates on a fresh install and for projects without a brain.
pub const PERSONAL: &str = "Personal";

/// The keys of a Space's files this release does not know, kept where they were read so that a
/// write puts them back: in the Space, its brain, models, sources and tracker config.
pub type Extra = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    pub id: SpaceId,
    pub name: String,
    /// What belongs in this Space.
    #[serde(default)]
    pub description: String,
    /// Added to every thread and job run in this Space.
    #[serde(default)]
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<SpaceBrain>,
    pub storage: SpaceStorage,
    /// The code repositories this Space covers, as GitHub `owner/name`, never by path.
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default)]
    pub models: SpaceModels,
    /// The MCP servers and local CLIs this Space's threads may use.
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A Space's brain: a Git repository and the folder in it that entries are written to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceBrain {
    pub repository: BrainRepository,
    pub folder: String,
    #[serde(flatten)]
    pub extra: Extra,
}
impl SpaceBrain {
    /// The folder exports have always written to, under the attached directory.
    pub const DEFAULT_FOLDER: &str = "second-brain";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrainRepository {
    GitHub {
        owner: String,
        name: String,
        #[serde(flatten)]
        extra: Extra,
    },
    /// A folder on this Mac; its path is a machine-local fact (`SpaceLocal::brain_path`).
    Local {
        #[serde(flatten)]
        extra: Extra,
    },
}
impl BrainRepository {
    /// A local brain, as this release writes it.
    pub fn local() -> Self {
        Self::Local {
            extra: Extra::new(),
        }
    }
}

/// Where a Space's definition lives: its folder under `spaces/` in a storage repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceStorage {
    pub repository: StorageRepository,
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StorageRepository {
    /// A repository with no remote at `<data folder>/repos/local-<id>/`.
    Local {
        id: String,
    },
    GitHub {
        owner: String,
        name: String,
    },
}
impl StorageRepository {
    /// Names the repository uniquely on this Mac, as its folder under `repos/` will.
    pub fn key(&self) -> String {
        match self {
            Self::Local { id } => format!("local-{id}"),
            Self::GitHub { owner, name } => format!("{owner}/{name}"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceModels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<SpaceModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<SpaceModel>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A Space's default model, written as a `ModelProfile` is, with the keys this release does not
/// know.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceModel {
    pub provider: crate::Provider,
    pub model: String,
    pub effort: String,
    #[serde(flatten)]
    pub extra: Extra,
}
impl SpaceModel {
    pub fn profile(&self) -> ModelProfile {
        ModelProfile {
            provider: self.provider,
            model: self.model.clone(),
            effort: self.effort.clone(),
        }
    }
}
impl From<ModelProfile> for SpaceModel {
    fn from(profile: ModelProfile) -> Self {
        Self {
            provider: profile.provider,
            model: profile.model,
            effort: profile.effort,
            extra: Extra::new(),
        }
    }
}

/// A source a Space's threads may use, in the format job recipes use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Mcp {
        server: String,
        tools: Vec<String>,
        #[serde(flatten)]
        extra: Extra,
    },
    Cli {
        command: String,
        prefixes: Vec<String>,
        /// How to check that the CLI is signed in, such as `gh auth status`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        check: Option<String>,
        #[serde(flatten)]
        extra: Extra,
    },
}

/// What this Mac keeps about a Space and never writes to its storage repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceLocal {
    /// Whether the Space is on here.
    pub enabled: bool,
    /// Where the brain is on this Mac: a local brain's folder or a GitHub brain's clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_path: Option<String>,
    /// The models this person may use in the Space; `None` allows every model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<ModelProfile>>,
    /// What the Space's page notes about this Mac, such as a model it falls back from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The local brain folder the `brain-registry-sync` upgrade took from this Space because
    /// its `origin` is not on GitHub, so the Brains page can say why the Space has no brain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_without_origin: Option<String>,
}
impl Default for SpaceLocal {
    fn default() -> Self {
        Self {
            enabled: true,
            brain_path: None,
            models: None,
            notes: vec![],
            brain_without_origin: None,
        }
    }
}

/// A Space with this Mac's facts about it, as the host stores and shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRecord {
    pub space: Space,
    pub local: SpaceLocal,
}

/// A Space's repositories on this Mac, as New project ticks them and `create_project` uses them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRepositories {
    pub space_id: SpaceId,
    /// The workspace repositories whose GitHub name the Space lists, in workspace order.
    pub repository_ids: Vec<String>,
    /// The repositories the Space lists that are not cloned on this Mac, as `owner/name`.
    pub not_cloned: Vec<String>,
}

/// The Spaces a repository, named `owner/name`, is in; none puts it under No Space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySpaces {
    pub repository: String,
    pub space_ids: Vec<SpaceId>,
    /// Also lists it in Personal, which is created as on a fresh install when it is missing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub new_personal: bool,
}

/// The upgrade's suggested Space for a workspace repository; `None` suggests No Space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySuggestion {
    pub repository_id: String,
    /// Its GitHub name, `owner/name`.
    pub repository: String,
    pub space: Option<SuggestedSpace>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SuggestedSpace {
    Space(SpaceId),
    /// Personal, which does not exist yet; confirming it creates it.
    NewPersonal,
}

/// The owner of a repository named `owner/name`.
pub fn repository_owner(identity: &str) -> &str {
    identity
        .split_once('/')
        .map_or(identity, |(owner, _)| owner)
}

/// A storage repository's `wiffletree-issues.yaml`: it makes the repository its Spaces' tracker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub format: u32,
    pub prefix: String,
    /// Listed so readers without Wiffletree see them; a tracker from before they were written
    /// has the fixed ones.
    #[serde(default = "all_states")]
    pub states: Vec<crate::IssueState>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Keys this release does not know.
    #[serde(flatten)]
    pub extra: Extra,
}
impl TrackerConfig {
    pub const FORMAT: u32 = 1;
    pub fn new(prefix: String) -> Self {
        Self {
            format: Self::FORMAT,
            prefix,
            states: all_states(),
            labels: vec![],
            extra: Extra::new(),
        }
    }
}
fn all_states() -> Vec<crate::IssueState> {
    crate::IssueState::ALL.to_vec()
}

/// What checking a GitHub repository as a Space's storage found. Nothing was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCheck {
    pub repository: crate::SettingsRepository,
    pub organization: bool,
    /// The Spaces under its `spaces/`, by folder.
    pub spaces: Vec<SpaceListing>,
    /// Its tracker's prefix; `None` when it has no `wiffletree-issues.yaml` yet.
    pub prefix: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceListing {
    pub folder: String,
    pub id: SpaceId,
    pub name: String,
    pub description: String,
    /// Its brain, which joining asks for on this Mac.
    #[serde(default)]
    pub brain: Option<SpaceBrain>,
    /// Its code repositories, as `owner/name`, which joining asks for on this Mac.
    #[serde(default)]
    pub repositories: Vec<String>,
    /// Already added on this Mac.
    pub added: bool,
}
/// A Space ticked to join, with where its brain and code repositories are on this Mac. A
/// repository with no clone named here stays missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinSpace {
    pub folder: String,
    #[serde(default)]
    pub brain_path: Option<String>,
    /// Folders on this Mac, each a clone of one of the Space's code repositories, which join
    /// the workspace's repositories.
    #[serde(default)]
    pub clones: Vec<String>,
}

/// What the Space page's Storage and issues box shows about a Space's storage repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageStatus {
    /// Its tracker's prefix; `None` for a GitHub repository joined without a tracker, which
    /// offers Set up issues.
    pub prefix: Option<String>,
    pub issues: usize,
    /// Who GitHub says can read it; `None` for local storage, which only this Mac reads.
    pub readers: Option<crate::Readers>,
    /// It is also this Mac's settings repository.
    pub settings_repository: bool,
    /// Its clone or local repository on this Mac.
    pub path: String,
    /// When a pass last reached GitHub; `None` for local storage, which never syncs.
    pub synced_at: Option<i64>,
    /// Why the latest pass failed.
    pub error: Option<String>,
}

/// The owner of a session: a project, or, for sessions that are not project work, a Space or a
/// brain. No role runs outside projects yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SessionOwner {
    Project(String),
    Space(SpaceId),
    /// A brain, by its identity, such as `owner/name`.
    Brain(String),
}
impl SessionOwner {
    /// Only project work coordinates: a Space's or a brain's session gets none of
    /// `create_ticket`, `assign_ticket`, `send_message`, `report`, `schedule` and the others.
    pub fn offers_coordination_tools(&self) -> bool {
        matches!(self, Self::Project(_))
    }
}

/// A tracker's prefix: 2 to 5 capital letters.
pub fn validate_prefix(prefix: &str) -> Result<()> {
    ensure!(
        (2..=5).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_uppercase()),
        "A prefix is 2 to 5 capital letters, such as SAGE"
    );
    Ok(())
}

/// The prefix the Space form proposes for `name`: the initials of a name of several words, or
/// the first four letters of one word, changed only as far as needed to avoid `taken`.
pub fn propose_prefix(name: &str, taken: &[String]) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();
    let mut base: String = match words.as_slice() {
        [word] => word.chars().take(4).collect(),
        words => words
            .iter()
            .filter_map(|w| w.chars().next())
            .take(5)
            .collect(),
    };
    if base.len() < 2 {
        base = "SP".into();
    }
    let free = |candidate: &String| !taken.contains(candidate);
    let stem: String = base.chars().take(4).collect();
    let letters = || ('A'..='Z').map(String::from);
    std::iter::once(base)
        .chain(letters().map(|letter| format!("{stem}{letter}")))
        .chain(letters().flat_map(|a| letters().map(move |b| format!("{a}{b}"))))
        .find(free)
        .unwrap_or_else(|| "SP".into())
}

/// A repository named by identity, `owner/name`.
pub fn validate_repository_identity(identity: &str) -> Result<()> {
    let valid = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    ensure!(
        identity
            .split_once('/')
            .is_some_and(|(owner, name)| valid(owner) && valid(name)),
        "Name a repository as owner/name, such as Sage-Social/api"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_comes_from_the_name_and_avoids_those_taken() {
        assert_eq!(propose_prefix("Sage", &[]), "SAGE");
        assert_eq!(propose_prefix("Personal", &[]), "PERS");
        assert_eq!(propose_prefix("team-brain", &[]), "TB");
        assert_eq!(propose_prefix("sage/brain", &[]), "SB");
        assert_eq!(propose_prefix("42", &[]), "SP");
        assert_eq!(propose_prefix("Sage", &["SAGE".into()]), "SAGEA");
        assert_eq!(propose_prefix("tb", &["TB".into(), "TBA".into()]), "TBB");
        for name in ["Sage", "x", "a b c d e f g", "Wood Working"] {
            validate_prefix(&propose_prefix(name, &[])).unwrap();
        }
    }

    #[test]
    fn a_prefix_is_two_to_five_capitals() {
        for good in ["ME", "SAGE", "ABCDE"] {
            validate_prefix(good).unwrap();
        }
        for bad in ["M", "ABCDEF", "Sage", "S1", ""] {
            assert!(validate_prefix(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_project_work_coordinates() {
        let space = SessionOwner::Space(SpaceId("s".into()));
        let brain = SessionOwner::Brain("Sage-Social/sagehaven-docs".into());
        assert!(SessionOwner::Project("p".into()).offers_coordination_tools());
        assert!(!space.offers_coordination_tools() && !brain.offers_coordination_tools());
    }
}
