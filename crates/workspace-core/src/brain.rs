//! Brains as Spaces name them (OpenSpec change `brain-registry-sync`): the registry on this Mac,
//! each brain's `wiffletree-brain.json`, and the batches that reach `main` through pull requests
//! that auto-merge. Nothing here holds a brain file's content or a check's output text.
use crate::{Extra, SpaceId};
use serde::{Deserialize, Serialize};

/// A brain this Mac's Spaces name: a GitHub repository with the host's own checkout of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brain {
    /// `owner/name`, as the Spaces name it.
    pub repository: String,
    /// The Spaces that name it; none once the human removed it from every Space, until they
    /// confirm forgetting it.
    pub spaces: Vec<SpaceId>,
    /// The human confirmed that the host may push its batch branches here, open pull requests
    /// into `main` and turn on auto-merge for them.
    pub granted: bool,
    /// The host's own clone, never a folder the human chose.
    pub checkout: String,
    /// The URL the host reaches it by, once found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_url: Option<String>,
    /// `main` as the host last fetched it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<i64>,
    /// Why the last pass could not reach or read it; `None` once one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Writing is held while something other than the host changed its checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<BrainHold>,
    pub config: BrainConfigReading,
    /// The gate's checks `main` does not require, as the last pass that could write read them;
    /// auto-merge waits while any is missing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_checks: Vec<String>,
    /// The checks `main` requires, as the last pass read them; only these decide a batch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_checks: Vec<String>,
    /// The legacy pointer `~/.config/claude-second-brain` names the human's clone of this brain,
    /// whose safety-net must open pull requests instead of pushing to `main`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_pointer: bool,
    /// The most recent batch PR here, however long ago it was opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pull_request: Option<BrainLastPullRequest>,
}

/// A brain's most recent batch pull request and the stage its batch reached.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainLastPullRequest {
    pub number: u64,
    pub kind: BrainBatchKind,
    pub stage: BrainStage,
}

/// Why the host stopped writing to a brain. It never resets, stashes or overwrites; the human
/// settles it and syncs again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrainHold {
    /// Files in the checkout differ from what the host last fetched.
    ChangedFiles { files: Vec<String> },
    /// The checkout's `main` has commits GitHub's has not, so it cannot fast-forward.
    NotFastForward,
}

/// What the host read of `wiffletree-brain.json` on the brain's `main`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrainConfigReading {
    /// Not fetched yet.
    #[default]
    Unread,
    /// No file: batches still open pull requests, there is no compiler, and Set up is offered.
    Missing,
    /// A file the host refuses, and why.
    Invalid {
        reason: String,
    },
    /// A version newer than this host understands: logs are written, nothing is compiled.
    Newer {
        version: u64,
    },
    Read {
        config: Box<BrainConfig>,
    },
}
impl BrainConfigReading {
    /// The config this host may act on, when it read one.
    pub fn config(&self) -> Option<&BrainConfig> {
        match self {
            Self::Read { config } => Some(config),
            _ => None,
        }
    }
}

/// `wiffletree-brain.json`, version 1. Keys this release does not know are kept in `extra`, so a
/// change the host makes puts them back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainConfig {
    pub version: u64,
    #[serde(default = "BrainConfig::default_dir")]
    pub brain_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler: Option<BrainCompiler>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile: Option<BrainCompile>,
    /// The gate's settings, which `brain-gate` defines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl BrainConfig {
    pub const FILE: &str = "wiffletree-brain.json";
    pub const VERSION: u64 = 1;
    fn default_dir() -> String {
        "second-brain".into()
    }
}

/// The host that compiles a brain, by its stable host id, with a name to show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainCompiler {
    pub host_id: String,
    pub name: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainCompile {
    /// `HH:MM`.
    pub at: String,
    pub timezone: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One branch, one pull request and one squash merge of changes to a brain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainBatch {
    /// A lowercase ULID.
    pub id: String,
    /// The brain, `owner/name`.
    pub repository: String,
    pub kind: BrainBatchKind,
    pub source: BrainSource,
    /// The pull request's title, such as "Log batch: Call relay retry (3 entries)".
    pub title: String,
    /// The files it adds or changes, relative to the repository.
    pub files: Vec<String>,
    /// `wiffletree/brain/<login>/<id>`, once pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<BrainPullRequest>,
    pub stage: BrainStage,
    /// What went wrong, while something did; a project's batch raises it as its one Inbox item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<BrainProblem>,
    /// The checks GitHub ran on the pull request's head: names, conclusions, files and lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<BrainCheck>,
    pub created_at: i64,
    pub updated_at: i64,
}
impl BrainBatch {
    /// Merged or discarded: nothing more happens to it, and nothing is offered for it.
    pub fn is_finished(&self) -> bool {
        matches!(self.stage, BrainStage::Merged | BrainStage::Discarded)
    }
    /// Its pull request is open and unmerged, so the human may amend or discard it.
    pub fn is_amendable(&self) -> bool {
        self.pull_request.is_some() && !self.is_finished()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainBatchKind {
    Log,
    Compile,
    /// A change to `wiffletree-brain.json` alone, such as the compiler.
    Config,
    /// Adds `wiffletree-brain.json` with the gate's caller, config and `CODEOWNERS` lines.
    SetUp,
}

/// Who wrote a batch: the project whose Inbox gets its item, or no project, whose problem shows
/// on the Brains page only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum BrainSource {
    Project(String),
    Space(SpaceId),
    /// The Brains page: Set up, a compiler change or an import.
    BrainsPage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainPullRequest {
    pub number: u64,
    pub url: String,
    /// GitHub accepted auto-merge with squash for it.
    #[serde(default)]
    pub auto_merge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainStage {
    /// Committed here and waiting to be pushed.
    Queued,
    PrOpen,
    Checks,
    AutoMerging,
    Merged,
    Failed,
    Discarded,
}

/// What stops a batch, which its Inbox item names. It holds no check output and no matched text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrainProblem {
    /// Required checks failed, by name, with the files and lines their annotations reported.
    ChecksFailed { checks: Vec<String> },
    /// GitHub refused the push, with Git's reason.
    PushRefused { reason: String },
    /// The pull request conflicts with `main`.
    Conflict,
    /// GitHub refused auto-merge: it is off for the repository, or `main` requires no checks.
    AutoMergeRefused,
    /// `main` does not require these gate checks, so the host does not turn on auto-merge.
    GateNotRequired { missing: Vec<String> },
    /// A Set up pull request: `CODEOWNERS` covers its files, so it waits for the code owner's
    /// review instead of auto-merging. Set up comes from the Brains page, which alone shows it.
    AwaitingReview,
    /// Something other than the host changed the batch's worktree, so it is not pushed.
    ChangedOutside,
    /// Git or `gh` failed on this batch, with its reason; the brain's other batches went on.
    Error { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainCheck {
    pub name: String,
    pub conclusion: BrainCheckConclusion,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<BrainAnnotation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainCheckConclusion {
    Pending,
    Success,
    Failure,
    Neutral,
    Skipped,
}

/// Where a check reported something: the file and line only, never its message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainAnnotation {
    pub path: String,
    pub line: u64,
}

/// The human's change to an unmerged batch, from their client only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrainAmendment {
    /// Replaces the value of the reported field line with `[removed: <rule>]`.
    Redact {
        path: String,
        line: u64,
        rule: String,
    },
    /// Appends the gate's inline marker `<!-- gate:allow <rule> <reason> -->` to the line.
    NotSensitive {
        path: String,
        line: u64,
        rule: String,
        reason: String,
    },
    /// Closes the pull request and deletes the branch the host pushed.
    Discard,
}

/// The rules of the gate's `sensitive-info` check, which its inline marker and an amendment name.
pub const GATE_RULES: [&str; 10] = [
    "phone",
    "email",
    "street-address",
    "gps",
    "aws-access-key",
    "aws-secret-key",
    "private-key",
    "token",
    "url-credentials",
    "secret-assignment",
];

/// What the Brains command answers: every registered brain and its batches that are not merged,
/// or merged in the last day.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainsPage {
    pub brains: Vec<Brain>,
    pub batches: Vec<BrainBatch>,
    /// This host as a compiler, which the page offers as the one to compile on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub this_host: Option<BrainCompiler>,
}
