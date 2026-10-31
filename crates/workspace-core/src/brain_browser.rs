//! What the brain browser reads (OpenSpec change `brain-browser`): pages of log entries, the note
//! tree, one note with its sources and backlinks, the conflicts, STATUS and search results, all
//! from the host's index of one brain's `main`. Paths are relative to the brain folder.
use crate::BrainStage;
use serde::{Deserialize, Serialize};

/// A note's `kind` or an entry's `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainKind {
    Decision,
    Observation,
}
impl BrainKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Observation => "observation",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "decision" => Some(Self::Decision),
            "observation" => Some(Self::Observation),
            _ => None,
        }
    }
}

/// An entry's `ref` checked on this machine, as `brain-compile` defines the states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefState {
    /// A named commit is on the repository's default branch.
    Verified,
    /// A named commit exists here but is not on the default branch.
    Claimed,
    /// No anchor, the anchor is not found, or the repository is not on this machine.
    Unverified,
}
impl RefState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Claimed => "claimed",
            Self::Unverified => "unverified",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "verified" => Some(Self::Verified),
            "claimed" => Some(Self::Claimed),
            "unverified" => Some(Self::Unverified),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefVerification {
    pub state: RefState,
    /// Why it is not verified, such as "not on this machine".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What an entry's `ref` names: its branch, commits and pull request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefAnchors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<u64>,
}

/// The pull request that added an entry to `main`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryPullRequest {
    /// This machine's entry: its batch's PR, when opened, and the batch's live stage.
    Batch {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number: Option<u64>,
        stage: BrainStage,
    },
    /// The PR of the squash commit that added the file.
    Merged { number: u64 },
    /// Added by a direct push.
    None,
}

/// Where this machine wrote an entry; other entries were logged outside this install.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryOrigin {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    pub session_id: String,
}

/// One log entry as Logs shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainEntry {
    pub id: String,
    pub path: String,
    pub repository: String,
    pub author: String,
    /// `wiffletree` or `skill`.
    pub source: String,
    pub date: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: BrainKind,
    #[serde(rename = "ref")]
    pub reference: String,
    pub changed: String,
    pub why: String,
    pub state: String,
    pub anchors: RefAnchors,
    pub verification: RefVerification,
    pub pull_request: EntryPullRequest,
    /// Set for entries this machine wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<EntryOrigin>,
    /// This machine's entry is not on `main` yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainLogFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<BrainKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<RefState>,
}
impl BrainLogFilter {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Which page of Logs, by entry id: ids sort by time, so the keyset is the id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum BrainLogsFrom {
    #[default]
    Newest,
    /// The page of entries older than this id.
    Before(String),
    /// The page of entries newer than this id.
    After(String),
    /// The page that starts with this entry, for opening one from elsewhere.
    At(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainLogPage {
    /// Newest first, at most `BRAIN_LOGS_PAGE`.
    pub entries: Vec<BrainEntry>,
    /// This machine's entries not yet on `main`, on the newest page only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<BrainEntry>,
    /// The id to page older from, when there are older entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older: Option<String>,
    /// The id to page newer from, when there are newer entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer: Option<String>,
}
pub const BRAIN_LOGS_PAGE: usize = 50;

/// A note as the index tree and backlinks list it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainNoteRow {
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BrainKind>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainNoteGroup {
    /// An INDEX heading.
    pub heading: String,
    pub notes: Vec<BrainNoteRow>,
}

/// The note index tree: `notes/INDEX.md`'s headings as groups, then the notes INDEX misses.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainNoteTree {
    pub groups: Vec<BrainNoteGroup>,
    /// Shown under "Not in the index".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_in_index: Vec<BrainNoteRow>,
    /// Notes outside the project's scope, which the tree leaves out.
    #[serde(default)]
    pub hidden: usize,
}

/// A `[[wikilink]]` in a note or STATUS, and the note it names, or none when broken.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainLink {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One source of a claim: a log entry, or a footnote link whose entry is missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaimSource {
    Entry {
        id: String,
        path: String,
        date: String,
        author: String,
        repository: String,
        anchors: RefAnchors,
        verification: RefVerification,
    },
    Broken {
        target: String,
    },
}

/// One footnote: one claim and the entries it cites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainClaim {
    /// The footnote's label, as in `[^label]`.
    pub label: String,
    pub sources: Vec<ClaimSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainNote {
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BrainKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    /// The Markdown after the frontmatter, for the client to render.
    pub body: String,
    pub links: Vec<BrainLink>,
    pub backlinks: Vec<BrainNoteRow>,
    pub claims: Vec<BrainClaim>,
    /// `false` when a footnote names no source the host can read, so the client shows "sources
    /// not readable" for it instead of failing.
    pub sources_readable: bool,
}

/// One open conflict from `reconcile-queue.md`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainConflict {
    pub id: String,
    pub title: String,
    /// `decision-contradiction`, `agent-disagreement` or `destructive`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub claim: String,
    pub conflicts_with: String,
    pub evidence: String,
    pub recommend: String,
    /// The claim's entry, when the block links one, and its repository folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

/// The Status view: STATUS.md and how fresh the brain is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainStatus {
    /// STATUS.md's Markdown; `None` until a compile wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<BrainLink>,
    /// When a commit on `main` last changed the wiki; `None` is "No compile yet".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_compile: Option<i64>,
    /// Entries on `main` added since then.
    pub pending_entries: usize,
    /// The commit of `main` the index holds; `None` before the first index build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed: Option<String>,
}

/// Part of a search excerpt; matched parts hold the query's terms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcerptPart {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub matched: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainHitKind {
    Note,
    Entry,
}

/// One search result: a note or a log entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainHit {
    pub hit: BrainHitKind,
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<BrainKind>,
    /// An entry's repository folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// An entry's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    /// A note's `updated` or an entry's date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub excerpt: Vec<ExcerptPart>,
}
pub const BRAIN_SEARCH_PAGE: usize = 20;
