//! Spaces: the one home for projects, their brain, storage and tracker (OpenSpec change `spaces`).
//! A `Space` is what its storage repository's `space.yaml` holds; `SpaceLocal` is what stays on
//! this Mac.
use crate::cascade::{Choices, RepositorySettings, ReviewOverrides};
use crate::{Job, JobRow, Line, ModelProfile, Then};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// Added to every role's instructions in this Space, after the Workspace's additions.
    /// Stored as `instructions`, the name older hosts read.
    #[serde(default, rename = "instructions")]
    pub additions: String,
    /// The Behaviors this Space sets for its members' agents.
    #[serde(default, skip_serializing_if = "Choices::is_empty")]
    pub behaviors: Choices,
    /// What the Space sets for a repository it covers, by `owner/name`, for its tickets only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repository_settings: BTreeMap<String, RepositorySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<SpaceBrain>,
    pub storage: SpaceStorage,
    /// The code repositories this Space covers, as GitHub `owner/name`, never by path.
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default)]
    pub models: SpaceModels,
    /// What the Space sets about its tickets' reviews.
    #[serde(default, skip_serializing_if = "ReviewOverrides::is_empty")]
    pub reviews: ReviewOverrides,
    /// The MCP servers and local CLIs this Space's threads may use.
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Space {
    /// Whether this Space lists `repository`, matched by its GitHub name.
    pub fn lists(&self, repository: &crate::Repository) -> bool {
        self.repositories.iter().any(|i| repository.is_github(i))
    }
    /// Whether this Space lists the repository named `owner/name`, ignoring case as GitHub does.
    pub fn lists_named(&self, identity: &str) -> bool {
        (self.repositories.iter()).any(|listed| listed.eq_ignore_ascii_case(identity))
    }
    /// What the Space sets for the repository named `owner/name`, ignoring case as GitHub does.
    pub fn repository_settings(&self, identity: &str) -> Option<&RepositorySettings> {
        (self.repository_settings.iter())
            .find(|(named, _)| named.eq_ignore_ascii_case(identity))
            .map(|(_, settings)| settings)
    }
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
    /// The repository's Wiffletree folder, `DEFAULT_ROOT` by default, empty for a repository not
    /// yet moved into one. Kept on this Mac with the Space, never in `space.yaml`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root: String,
}

/// The Wiffletree folder a repository gets unless it holds another.
pub const DEFAULT_ROOT: &str = "wiffletree";

/// Refuses a Wiffletree folder that is not a relative path of plain names.
pub fn validate_root(root: &str) -> Result<()> {
    ensure!(
        root.split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.contains(['\\', '\0'])),
        "A folder is a relative path of plain names, with no `.`, `..` or hidden part"
    );
    Ok(())
}

/// Whether `path`, relative to a repository, is under a tracker's `issues/`, wherever its folder
/// is. Anything else named `issues` reads the same, which only asks for more care.
pub fn names_issue_file(path: &str) -> bool {
    path.split('/').any(|part| part == "issues")
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

/// The rows of the Who runs what table a Space overrides, as `space.yaml` stores them. Read and
/// write them with `overrides` and `set_overrides`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceModels {
    /// The Assistant row's first choice, kept for hosts older than `rows`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<SpaceModel>,
    /// The Job row's first choice, kept for hosts older than `rows`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<SpaceModel>,
    /// The rows as written, so that keys this release does not know, within a row too, stay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rows: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl SpaceModels {
    /// The rows the Space overrides. `threads` and `jobs` are the first choices of the Assistant
    /// and Job rows: either one that differs from its row, as when a host older than `rows`
    /// changed it, replaces the row's first choice, and one without a row is a row of its own
    /// that waits.
    /// A row this release cannot read, as a newer host's might be, is left out.
    pub fn overrides(&self) -> Vec<JobRow> {
        let mut rows: Vec<JobRow> = (self.rows.iter())
            .filter_map(|row| serde_json::from_value(row.clone()).ok())
            .collect();
        for (model, job) in [(&self.threads, Job::Assistant), (&self.jobs, Job::Job)] {
            let Some(first) = model.as_ref().map(SpaceModel::profile) else {
                continue;
            };
            match rows.iter_mut().find(|row| row.job == job) {
                Some(row) if row.main.first != first => {
                    if row
                        .main
                        .then
                        .model()
                        .is_some_and(|t| t.provider == first.provider)
                    {
                        row.main.then = Then::Wait;
                    }
                    row.main.first = first;
                }
                Some(_) => {}
                None => rows.push(JobRow {
                    job,
                    main: Line {
                        first,
                        then: Then::Wait,
                    },
                    small: None,
                }),
            }
        }
        rows
    }
    /// The rows the Space holds for jobs no Space may set, which are ignored until removed.
    pub fn ignored(&self) -> Vec<JobRow> {
        (self.overrides().into_iter())
            .filter(|row| !row.job.space_may_set())
            .collect()
    }
    /// Saves `rows` as the Space's overrides, with `threads` and `jobs` written to match the
    /// Assistant and Job rows so that older hosts still use them. Each row keeps the keys this
    /// release does not know from the saved row for its job, and a saved row this release
    /// cannot read stays.
    pub fn set_overrides(&mut self, rows: Vec<JobRow>) {
        let first = |job: Job, kept: &Option<SpaceModel>| {
            let row = rows.iter().find(|row| row.job == job)?;
            Some(match kept {
                Some(kept) if kept.profile() == row.main.first => kept.clone(),
                _ => row.main.first.clone().into(),
            })
        };
        self.threads = first(Job::Assistant, &self.threads);
        self.jobs = first(Job::Job, &self.jobs);
        let saved: Vec<(serde_json::Value, Option<JobRow>)> = (std::mem::take(&mut self.rows))
            .into_iter()
            .map(|raw| {
                let row = serde_json::from_value(raw.clone()).ok();
                (raw, row)
            })
            .collect();
        for row in &rows {
            let mut raw = serde_json::to_value(row).expect("a row serializes");
            let before = saved.iter().find_map(|(raw, saved)| {
                saved
                    .as_ref()
                    .filter(|s| s.job.is(&row.job))
                    .map(|s| (raw, s))
            });
            if let Some((before, read)) = before {
                let known = serde_json::to_value(read).expect("a row serializes");
                keep_unknown(&mut raw, before, &known);
            }
            self.rows.push(raw);
        }
        (self.rows).extend(
            saved
                .into_iter()
                .filter(|(_, row)| row.is_none())
                .map(|(raw, _)| raw),
        );
    }
}

/// Copies into `into` each key of `before` that reading it left out of `known`, at any depth.
fn keep_unknown(
    into: &mut serde_json::Value,
    before: &serde_json::Value,
    known: &serde_json::Value,
) {
    let (Some(into), Some(before)) = (into.as_object_mut(), before.as_object()) else {
        return;
    };
    for (key, value) in before {
        match (known.get(key), into.get_mut(key)) {
            (None, None) => {
                into.insert(key.clone(), value.clone());
            }
            (Some(known), Some(into)) => keep_unknown(into, value, known),
            _ => {}
        }
    }
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
    /// Where the brain an earlier release named is on this Mac, which Bring in a brain reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_path: Option<String>,
    /// Retired: the models this person could use in the Space. Kept so a downgrade keeps the
    /// list; nothing reads it, as a model is usable here when its provider is signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<ModelProfile>>,
    /// What the Space's page notes about this Mac, such as a model it falls back from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The local brain folder the `brain-registry-sync` upgrade took from this Space because
    /// its `origin` is not on GitHub, so the Brains page can say why the Space has no brain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_without_origin: Option<String>,
    /// The root folders whose new repositories join this Space. The Space's own list stays the
    /// record of what it covers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub followed_roots: Vec<String>,
}
impl Default for SpaceLocal {
    fn default() -> Self {
        Self {
            enabled: true,
            brain_path: None,
            models: None,
            notes: vec![],
            brain_without_origin: None,
            followed_roots: vec![],
        }
    }
}

/// A Space with this Mac's facts about it, as the host stores and shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRecord {
    pub space: Space,
    pub local: SpaceLocal,
}

/// A Space's repositories on this Mac, as New project shows them.
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
    /// When the folder's wiki compiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile: Option<crate::BrainCompile>,
    /// The domains whose email addresses the folder's scans let through.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_email_domains: Vec<String>,
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
            compile: None,
            allowed_email_domains: vec![],
            extra: Extra::new(),
        }
    }
}
fn all_states() -> Vec<crate::IssueState> {
    crate::IssueState::ALL.to_vec()
}

/// What checking a GitHub repository as a Space's storage found: it is private and its default
/// branch takes a direct push, or the check is refused saying why. Nothing was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCheck {
    pub repository: crate::SettingsRepository,
    pub organization: bool,
    /// How many people GitHub lists as able to read it, when it lets this Mac count them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_count: Option<usize>,
    /// A ruleset or branch protection blocks force-pushes and deleting its default branch.
    #[serde(default)]
    pub history_protected: bool,
    /// The Wiffletree folder it already holds, empty for one at its root; `None` for none, so a
    /// new Space chooses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The Spaces under its `spaces/`, by folder.
    pub spaces: Vec<SpaceListing>,
    /// Its tracker's prefix; `None` when it has no `wiffletree-issues.yaml` yet.
    pub prefix: Option<String>,
    /// When its wiki compiles, as its tracker says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile: Option<crate::BrainCompile>,
    /// The folder's issues, wiki pages and the Macs that compile its wiki, which its Spaces share.
    #[serde(default)]
    pub issues: usize,
    #[serde(default)]
    pub wiki_pages: usize,
    #[serde(default)]
    pub compilers: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceListing {
    pub folder: String,
    pub id: SpaceId,
    pub name: String,
    pub description: String,
    /// Its code repositories, as `owner/name`, which joining asks for on this Mac.
    #[serde(default)]
    pub repositories: Vec<String>,
    /// Already added on this Mac.
    pub added: bool,
}
/// A Space ticked to join, with where its code repositories are on this Mac. A repository with
/// no clone named here stays missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinSpace {
    pub folder: String,
    /// Folders on this Mac, each a clone of one of the Space's code repositories, which join
    /// the workspace's repositories.
    #[serde(default)]
    pub clones: Vec<String>,
}

/// What New Space asks beyond the Space itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceSetup {
    /// The Wiffletree folder of a repository that holds none yet; `DEFAULT_ROOT` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// `gh` creates the GitHub repository, private, before anything else.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub create_repository: bool,
    /// Compile the wiki on this Mac at this time; `None` leaves it to the human or a teammate.
    /// A repository whose tracker already sets a time keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile: Option<crate::BrainCompile>,
}

/// What New Space proposes: the owner of the repository it offers to create, and this Mac's
/// timezone for the compile time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceFormDefaults {
    /// The most common owner among the workspace's GitHub repositories, or the `gh` login.
    pub owner: Option<String>,
    pub timezone: String,
}

/// What Bring in a brain reads from a brain folder on this Mac and what it would bring.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BringInPlan {
    /// The wiki pages under `notes/`, and whether `STATUS.md` comes with them.
    pub pages: usize,
    pub status: bool,
    /// The log entries and how many files hold them.
    pub entries: usize,
    pub entry_files: usize,
    /// Entries shortened to fit the cap, whose full text stays in the original.
    pub shortened: usize,
    /// Entries and pages the Space's folder already holds, which add nothing.
    pub already: usize,
    /// Pages the folder already has with other text, which are left and not brought.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// Files changed or untracked in the original, so not on GitHub, and of those, the ones
    /// never committed.
    pub not_on_github: usize,
    pub never_committed: usize,
    /// Folders beside the logs and wiki that stay behind, such as `archive/`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_behind: Vec<String>,
    /// Entries the original had compiled, which every Mac reads as compiled.
    pub compiled: usize,
}

/// What the Space page's Repository box shows about a Space's repository and folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageStatus {
    /// The log entries in the folder, and how many wait for the next compile.
    #[serde(default)]
    pub entries: usize,
    #[serde(default)]
    pub waiting: usize,
    /// The wiki's pages under `notes/`.
    #[serde(default)]
    pub wiki_pages: usize,
    /// When the wiki last compiled, and on which Mac.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiled: Option<(i64, String)>,
    /// When it compiles, how many Macs compile it, and whether this Mac is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile: Option<crate::BrainCompile>,
    #[serde(default)]
    pub compilers: usize,
    #[serde(default)]
    pub compiles_here: bool,
    /// How many people GitHub lists as able to read it, when it lets this Mac count them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_count: Option<usize>,
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
    /// Why its wiki is falling behind, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wiki_behind: Option<String>,
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
    use serde_json::json;

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
    fn threads_and_jobs_read_as_rows_and_are_written_back_while_they_match() {
        let mut models: SpaceModels = serde_json::from_value(serde_json::json!({
            "threads": {"provider": "codex", "model": "gpt-6.1-sol", "effort": "high", "tier": "x"},
            "jobs": {"provider": "claude", "model": "sonnet", "effort": "medium"},
            "later": "kept",
        }))
        .unwrap();
        let rows = models.overrides();
        let first = |job: Job| {
            let row = rows.iter().find(|row| row.job == job).unwrap();
            (row.main.first.model.clone(), row.main.then.clone())
        };
        assert_eq!(first(Job::Assistant), ("gpt-6.1-sol".into(), Then::Wait));
        assert_eq!(first(Job::Job), ("sonnet".into(), Then::Wait));

        // The Job row's first choice changes and the Assistant row's stays.
        let mut rows = rows;
        rows.iter_mut()
            .find(|row| row.job == Job::Job)
            .unwrap()
            .main
            .first
            .model = "opus".into();
        models.set_overrides(rows.clone());
        let saved = serde_json::to_value(&models).unwrap();
        let read: SpaceModels = serde_json::from_value(saved).unwrap();
        assert_eq!(read.overrides(), rows);
        let threads = read.threads.clone().unwrap();
        assert_eq!(threads.extra["tier"], "x");
        assert_eq!(read.jobs.as_ref().unwrap().model, "opus");
        assert_eq!(read.extra["later"], "kept");

        // A host older than rows changes threads: the Assistant row takes it.
        let mut older = read.clone();
        older.threads = Some(
            ModelProfile {
                provider: crate::Provider::Claude,
                model: "haiku".into(),
                effort: "low".into(),
            }
            .into(),
        );
        let assistant = older
            .overrides()
            .into_iter()
            .find(|row| row.job == Job::Assistant);
        assert_eq!(assistant.unwrap().main.first.model, "haiku");

        // Without the rows, neither old key is written.
        models.set_overrides(vec![]);
        assert!(models.threads.is_none() && models.jobs.is_none());
    }

    #[test]
    fn a_newer_hosts_keys_within_a_row_stay_through_a_save() {
        let profile = |model: &str| json!({"provider": "claude", "model": model, "effort": "high"});
        let mut first = profile("opus");
        first["future_profile"] = json!(1);
        let mut models: SpaceModels = serde_json::from_value(json!({"rows": [
            {"job": {"kind": "implementer"}, "future_row": 2,
             "main": {"first": first, "future_line": 3}},
            {"job": {"kind": "future_job"}, "main": {"first": profile("sonnet")}},
        ]}))
        .unwrap();
        let mut rows = models.overrides();
        assert_eq!(rows.len(), 1, "the unreadable row is left out");
        rows[0].main.then = Then::Model(ModelProfile {
            provider: crate::Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "high".into(),
        });
        models.set_overrides(rows.clone());

        let saved = serde_json::to_value(&models).unwrap();
        let row = &saved["rows"][0];
        assert_eq!(row["future_row"], json!(2));
        assert_eq!(row["main"]["future_line"], json!(3));
        assert_eq!(row["main"]["first"]["future_profile"], json!(1));
        assert_eq!(saved["rows"][1]["job"], json!({"kind": "future_job"}));
        let read: SpaceModels = serde_json::from_value(saved).unwrap();
        assert_eq!(read.overrides(), rows);
    }

    #[test]
    fn only_project_work_coordinates() {
        let space = SessionOwner::Space(SpaceId("s".into()));
        let brain = SessionOwner::Brain("Sage-Social/sagehaven-docs".into());
        assert!(SessionOwner::Project("p".into()).offers_coordination_tools());
        assert!(!space.offers_coordination_tools() && !brain.offers_coordination_tools());
    }
}
