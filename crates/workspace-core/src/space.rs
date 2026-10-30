//! Spaces: the one home for projects, their brain, storage and tracker (OpenSpec change `spaces`).
//! A `Space` is what its storage repository's `space.yaml` holds; `SpaceLocal` is what stays on
//! this Mac.
use crate::ModelProfile;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
}

/// A Space's brain: a Git repository and the folder in it that entries are written to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceBrain {
    pub repository: BrainRepository,
    pub folder: String,
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
    },
    /// A folder on this Mac; its path is a machine-local fact (`SpaceLocal::brain_path`).
    Local,
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
    pub threads: Option<ModelProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<ModelProfile>,
}

/// A source a Space's threads may use, in the format job recipes use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Mcp {
        server: String,
        tools: Vec<String>,
    },
    Cli {
        command: String,
        prefixes: Vec<String>,
        /// How to check that the CLI is signed in, such as `gh auth status`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        check: Option<String>,
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
}
impl Default for SpaceLocal {
    fn default() -> Self {
        Self {
            enabled: true,
            brain_path: None,
            models: None,
        }
    }
}

/// A Space with this Mac's facts about it, as the host stores and shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRecord {
    pub space: Space,
    pub local: SpaceLocal,
}

/// A storage repository's `wiffletree-issues.yaml`: it makes the repository its Spaces' tracker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub format: u32,
    pub prefix: String,
}
impl TrackerConfig {
    pub const FORMAT: u32 = 1;
    pub fn new(prefix: String) -> Result<Self> {
        validate_prefix(&prefix)?;
        Ok(Self {
            format: Self::FORMAT,
            prefix,
        })
    }
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
