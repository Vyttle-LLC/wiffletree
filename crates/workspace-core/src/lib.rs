//! Provider-neutral contracts. No renderer, database or provider runtime dependencies.
pub mod ask;
pub mod backoff;
mod brain;
mod brain_browser;
mod client;
mod context;
pub mod data_dir;
#[cfg(target_os = "macos")]
pub mod device_key;
mod files;
mod host;
pub mod host_copy;
mod index;
mod issues;
mod launch;
mod merging;
mod messages;
mod models;
mod owners;
pub mod pairing;
pub mod pricing;
mod process;
mod progress;
mod protected;
mod publishing;
mod remote;
pub mod remote_client;
pub mod remote_launch;
mod selection;
pub mod skills;
mod space;
mod steps;
mod suggestion;
mod usage;
use anyhow::{Context, Result, bail, ensure};
pub use brain::*;
pub use brain_browser::*;
pub use client::*;
pub use context::*;
pub use files::*;
pub use host::*;
pub use index::WorkspaceIndex;
pub use issues::*;
pub use launch::*;
pub use merging::*;
pub use messages::*;
pub use models::*;
pub use owners::*;
pub use process::*;
pub use progress::*;
pub use protected::*;
pub use publishing::*;
pub use remote::*;
pub use selection::*;
use serde::{Deserialize, Serialize};
pub use space::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
pub use steps::*;
pub use suggestion::*;
pub use usage::*;

pub const MAX_PAGE: usize = 100;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    ProjectOrchestrator,
    /// Legacy repository coordinator. It can no longer be created; it remains only so archived,
    /// migrated sessions decode and display.
    TaskOrchestrator,
    Implementer,
    Tester,
    Reviewer,
    Maintenance,
    /// The PR keeper's model runs. Only the host creates these agents; no coordinator tool can.
    Keeper,
}
impl Role {
    pub const ALL: [Self; 7] = [
        Self::ProjectOrchestrator,
        Self::TaskOrchestrator,
        Self::Implementer,
        Self::Tester,
        Self::Reviewer,
        Self::Maintenance,
        Self::Keeper,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectOrchestrator => "Coordinator",
            Self::TaskOrchestrator => "Repository coordinator",
            Self::Implementer => "Implementer",
            Self::Tester => "Tester",
            Self::Reviewer => "Reviewer",
            Self::Maintenance => "Maintenance",
            Self::Keeper => "PR keeper",
        }
    }
    pub fn is_worker(self) -> bool {
        !matches!(self, Self::ProjectOrchestrator | Self::TaskOrchestrator)
    }
    /// A ticket agent's short name: its role, plus its focus when others share that role.
    pub fn agent_label(self, focus: Option<&str>) -> String {
        match focus {
            Some(focus) => format!("{} · {focus}", self.label()),
            None => self.label().to_owned(),
        }
    }
}

/// A named way of working that Wiffletree runs, with a stock text the human can read and alter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Skill {
    Coordinating,
    Implementing,
    Reviewing,
    KeepingAPr,
}
impl Skill {
    pub const ALL: [Self; 4] = [
        Self::Coordinating,
        Self::Implementing,
        Self::Reviewing,
        Self::KeepingAPr,
    ];
    /// The skill a role's instructions end with. The PR keeper's comment sorting is not a role's
    /// turn and gets none.
    pub fn for_role(role: Role) -> Self {
        match role {
            // Retired repository coordinators never run again; their archived sessions remain.
            Role::ProjectOrchestrator | Role::TaskOrchestrator => Self::Coordinating,
            Role::Implementer | Role::Maintenance => Self::Implementing,
            Role::Tester | Role::Reviewer => Self::Reviewing,
            Role::Keeper => Self::KeepingAPr,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Coordinating => "Coordinating",
            Self::Implementing => "Implementing",
            Self::Reviewing => "Reviewing",
            Self::KeepingAPr => "Keeping a PR",
        }
    }
    pub fn purpose(self) -> &'static str {
        match self {
            Self::Coordinating => {
                "How the coordinator plans, when it creates tickets and starts agents, which model and effort it picks, when it stops chasing findings, and when it asks you."
            }
            Self::Implementing => {
                "How an implementer works on its ticket, runs checks and answers review findings."
            }
            Self::Reviewing => {
                "What testers and reviewers run and read, and what counts as a blocking finding."
            }
            Self::KeepingAPr => {
                "How the PR keeper's model resolves a conflict or fixes failed checks in its one try."
            }
        }
    }
    /// The skill's folder in the skills folder.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Coordinating => "coordinating",
            Self::Implementing => "implementing",
            Self::Reviewing => "reviewing",
            Self::KeepingAPr => "keeping-a-pr",
        }
    }
}
/// The longest text the human may save for a skill, in characters.
pub const MAX_SKILL_TEXT: usize = 32_000;
/// A skill as the human has left it, on the snapshot without its history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillState {
    pub skill: Skill,
    pub stock: String,
    pub stock_version: String,
    /// The human's own rules, sent after the stock text; empty when there are none.
    pub rules: String,
    /// The human's rewrite, sent in place of the stock text with no separate rules.
    pub rewrite: Option<Rewrite>,
    /// Why the skill's files could not be read; until they can, they count as missing.
    pub unreadable: Option<String>,
    /// A stock version newer than this release's, recorded in the history by another machine.
    #[serde(default)]
    pub newer_stock_version: Option<String>,
    /// The note of the merge the skill holds now, unless dismissed on this machine.
    #[serde(default)]
    pub merge_note: Option<MergeNote>,
    /// Why no automatic merge touches the rewrite.
    #[serde(default)]
    pub merge_standing: Option<MergeStanding>,
    /// The model's question about merging this release's stock text into the rewrite as it is.
    #[serde(default)]
    pub question: Option<SkillQuestion>,
}
/// A merge that waits for the human, because the model found that the update and the rewrite
/// want different things in one or more passages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillQuestion {
    pub skill: Skill,
    /// The stock version the merge brings in.
    pub to: String,
    /// How many changes the merge brings in whatever the human chooses.
    pub other_changes: usize,
    pub model: ModelProfile,
    /// In order, the passages in question; an answer chooses a wording for each.
    pub passages: Vec<QuestionedPassage>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionedPassage {
    /// The model's one sentence on what each wording wants.
    pub question: String,
    pub stock: String,
    pub yours: String,
}
/// A "Merged" history entry, as its note and its review name it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeNote {
    /// The entry's name, the time it was written.
    pub entry: String,
    pub from: String,
    pub to: String,
    /// The changes that came from the update as plain text.
    pub from_update: usize,
    /// The overlapping passages the model merged.
    pub by_model: usize,
    pub model: Option<MergeModel>,
    /// The machine that merged, none for this one.
    pub machine: Option<String>,
}
/// The model that merged a rewrite's overlapping passages, and the tokens it used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeModel {
    pub model: ModelProfile,
    pub tokens: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStanding {
    /// Out of date and due a merge, which waits for this launch's first sync pass to end.
    WaitingForSync,
    /// The PR keeper's model is merging the `passages` that overlap.
    Merging { passages: usize },
    /// The latest try changed nothing, for `reason`; the next start tries again.
    Failed { reason: String },
    /// Passages overlap and no model merges them: the PR keeper has none or, when `codex`, a
    /// Codex model. The human chooses a wording for each.
    NeedsModel {
        passages: Vec<MergePassage>,
        codex: bool,
    },
    /// Out of date, and an Undo or a restore from before a merge stops automatic merges into
    /// this release's stock version.
    Undone,
    /// Based on a newer release's stock text, so not out of date; `machine` merged it in when a
    /// merge did.
    NewerBase { machine: Option<String> },
}
/// A run of paragraphs that both the update and the human changed, each in its own way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergePassage {
    pub base: String,
    pub stock: String,
    pub yours: String,
}
/// The wording the human keeps for an overlapping passage: the update's or their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wording {
    Stock,
    Yours,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewrite {
    pub text: String,
    /// The stock text the rewrite started from, or was last marked up to date against or merged
    /// with.
    pub base: String,
    /// The version on the first line of the stock text the rewrite is based on.
    pub base_version: String,
    /// Its base is not this release's stock text, compared as text, and not a newer version.
    pub out_of_date: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Codex,
    Claude,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    Working,
    Blocked,
    Done,
    Paused,
    Disconnected,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Working => "Working",
            Self::Blocked => "Blocked",
            Self::Done => "Done",
            Self::Paused => "Paused",
            Self::Disconnected => "Disconnected",
        }
    }
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Ready => "○",
            Self::Working => "◉",
            Self::Blocked => "⊖",
            Self::Done => "✓",
            Self::Paused => "Ⅱ",
            Self::Disconnected => "◇",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// The Space it belongs to; it logs to that Space's brain. Empty from an older host.
    #[serde(default)]
    pub space: SpaceId,
    /// The workspace repositories this project may use; `None` means all of them.
    #[serde(default)]
    pub repositories: Option<Vec<String>>,
    /// The main coordinator's directory, fixed at creation; its name is the project's slug.
    /// `None` for projects created before workspace folders, which keep their original directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// Names the folders of a project without `home`, fixed when its first ticket needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// The coordinator whose `create_project` call made this project; `None` when the human did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<ProjectCreator>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCreator {
    pub project_id: String,
    pub session_id: String,
}
/// What a coordinator's `create_project` answers: the new project and the repositories of its
/// Space it went without, because they are not cloned on this Mac.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedProject {
    #[serde(flatten)]
    pub project: Project,
    /// As `owner/name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_on_this_mac: Vec<String>,
}
impl Repository {
    /// Its GitHub name, `owner/name`: where GitHub says it is, or else what `origin` names.
    pub fn github_identity(&self) -> Option<String> {
        let github = self.github.as_ref()?;
        Some(match &github.facts {
            Some(facts) => facts.name_with_owner.clone(),
            None => format!("{}/{}", github.owner, github.name),
        })
    }
    /// Whether it is the GitHub repository `identity` names, ignoring case as GitHub does.
    pub fn is_github(&self, identity: &str) -> bool {
        self.github_identity()
            .is_some_and(|own| own.eq_ignore_ascii_case(identity))
    }
}
impl Project {
    pub fn uses(&self, repository_id: &str) -> bool {
        self.repositories
            .as_ref()
            .is_none_or(|ids| ids.iter().any(|id| id == repository_id))
    }
}
/// A repository known to the whole workspace. Projects choose which ones they use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub path: String,
    pub base: String,
    /// The GitHub repository `origin` names, when it names exactly one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GitHubRepository>,
    /// Why `origin` exists and still names no GitHub repository to publish to, such as a second
    /// push URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_problem: Option<String>,
    /// The human's command that runs the repository's checks, for the PR keeper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<RepositoryChecks>,
    /// The human's protected branches: exact names and patterns. Only the human's client
    /// changes them, and they stay on this machine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_branches: Vec<String>,
}
/// A repository's checks: the human's own shell text, and how long it may run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryChecks {
    pub command: String,
    pub minutes: u32,
}
/// The time limits a checks command accepts, in minutes.
pub const CHECKS_MINUTES: std::ops::RangeInclusive<u32> = 1..=120;
impl RepositoryChecks {
    pub const DEFAULT_MINUTES: u32 = 20;
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.command.trim().is_empty(), "Enter the checks command");
        ensure!(
            self.command.len() <= 4096,
            "The checks command is at most 4,096 bytes"
        );
        ensure!(
            CHECKS_MINUTES.contains(&self.minutes),
            "The checks may run for {}–{} minutes",
            CHECKS_MINUTES.start(),
            CHECKS_MINUTES.end()
        );
        Ok(())
    }
}
/// The GitHub repository a workspace repository's `origin` names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubRepository {
    /// As `origin`'s URL names it; GitHub's own answer is in `facts`.
    pub owner: String,
    pub name: String,
    /// `None` until GitHub has answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<GitHubFacts>,
    /// Why GitHub gave no answer for this repository, such as that it could not be found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}
/// What GitHub last said about a repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubFacts {
    /// Where GitHub says the repository is now, as `owner/name`; a push lands there.
    pub name_with_owner: String,
    /// Its owner is an organization rather than a user.
    pub organization: bool,
    pub fork: bool,
    /// What a fork is a fork of, as `owner/name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub public: bool,
    pub checked_at: i64,
    /// Its default branch, merge methods and rulesets; `None` in facts read before they were
    /// asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<GitHubBranches>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositoryCandidate {
    pub name: String,
    pub path: String,
    pub base: String,
    /// Already in the workspace.
    pub added: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepositoryDiscovery {
    pub repositories: Vec<RepositoryCandidate>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepositoryImport {
    pub imported: Vec<Repository>,
    pub failures: Vec<String>,
}

/// New repositories found in the workspace's root folders.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepositoryRefresh {
    pub added: Vec<Repository>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub repository_id: Option<String>,
    pub name: String,
    pub role: Role,
    pub provider: Provider,
    pub status: Status,
    /// Hidden from the working tree and never scheduled; everything it owns is preserved.
    #[serde(default)]
    pub archived: bool,
    /// The Space or brain that owns a session that is not project work; `None` for project
    /// work, which every session is until a role runs outside projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<SessionOwner>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Receipt {
    Queued,
    Delivered,
    Acknowledged,
    Completed,
    Held,
    Cancelled,
}
impl Receipt {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Delivered => "Delivered",
            Self::Acknowledged => "Acknowledged",
            Self::Completed => "Completed",
            Self::Held => "Held",
            Self::Cancelled => "Cancelled",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub sequence: i64,
    pub id: String,
    pub project_id: String,
    pub sender: Option<String>,
    pub recipient: String,
    pub body: String,
    pub receipt: Receipt,
    pub created_at: i64,
    /// Only the human's messages carry attachments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// On a coordinator's reply to a turn the human did not start: how the project's recorded
    /// facts compare with its last reply shown in full. Display only; the body never changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<ReplyUpdate>,
}
impl Message {
    /// Messages without a sender are the human's, Inbox answers included, apart from
    /// Wiffletree's own notices.
    pub fn from_human(&self) -> bool {
        self.sender.is_none() && host_notice(&self.id).is_none()
    }
}
/// What changed in a project's records between a coordinator's last full reply and this one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyUpdate {
    /// Nothing recorded changed, so the reply folds.
    Unchanged,
    /// The differences, one line each, such as "Stage 1 committed 684cd0f".
    Changed { changes: Vec<String> },
}
/// A page of a repository's local history, newest first, read from Git without changing it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitHistory {
    pub repository: Repository,
    pub commits: Vec<Commit>,
    /// Where the next page starts.
    pub next_skip: usize,
    pub observed_at: i64,
    pub source: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub subject: String,
}
/// A file on a message: before sending, the dropped file; after, the stored read-only copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub path: PathBuf,
    pub size: u64,
    pub image: Option<ImageFormat>,
}
impl Attachment {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}
/// Whether two file names would name the same file. macOS volumes ignore case by default,
/// so neither may `Notes.txt` and `notes.txt` share a message.
pub fn same_file_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
/// Image formats both providers accept as images rather than files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}
impl ImageFormat {
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attention {
    pub id: String,
    pub project_id: String,
    pub session_id: String,
    pub host: String,
    pub operation_id: String,
    pub prompt: String,
    /// Suggested answers the human can pick with one click; free text is always allowed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    pub answer: Option<String>,
}
impl Attention {
    /// A parked provider operation waiting for Approve or Deny, rather than a question.
    /// Or a publish step or a merge waiting for the human, approved or denied in the same way.
    pub fn is_permission(&self) -> bool {
        self.operation_id.starts_with(PERMISSION_PREFIX) || self.is_publish() || self.is_merge()
    }
    /// A step of a ticket's publish request waiting for the human's approval.
    pub fn is_publish(&self) -> bool {
        self.operation_id.starts_with(PUBLISH_PREFIX)
    }
    /// A ticket's merge request waiting for the human's approval.
    pub fn is_merge(&self) -> bool {
        self.operation_id.starts_with(MERGE_PREFIX)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    pub sequence: i64,
    pub project_id: String,
    pub session_id: Option<String>,
    pub kind: String,
    pub detail: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModelProfile {
    pub provider: Provider,
    pub model: String,
    pub effort: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Decision,
    Observation,
}
/// What a log entry's author gives (OpenSpec change `brain-log-capture`). The host fills in the
/// rest; nothing here names the entry's id, date, `ref` or author.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogInput {
    pub session_id: String,
    /// A deduplication key: repeating it with the same content returns the first entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    pub kind: LogKind,
    pub title: String,
    /// The coordinator's choice of one of its project's repositories, by id or name. Without
    /// one, its entry is a planning entry in the brain repository's own folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The coordinator's choice of one of its project's tickets: the entry is that ticket's,
    /// for its repository, with its `ref`, and stays in the coordinator session's batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    pub changed: String,
    pub why: String,
    pub state: String,
}
/// The human's fix of a held entry: its whole text, its folder and its `ref`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogFix {
    pub kind: LogKind,
    pub title: String,
    /// The folder under `logs/`.
    pub repository: String,
    pub reference: String,
    pub changed: String,
    pub why: String,
    pub state: String,
}
/// A stored log entry. An entry captured before `brain-log-capture` keeps its old id and has no
/// brain fields while it is in a legacy `logs/<repo>.md` file or waits for a brain; once queued
/// into a batch it gains an entry id, its brain and its path like any other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLog {
    pub sequence: i64,
    /// The entry's lowercase ULID.
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    /// The entry's folder under `logs/`: the GitHub repository's name.
    pub repository: String,
    pub created_at: i64,
    /// The host's local date at capture, `YYYY-MM-DD`.
    #[serde(default)]
    pub date: String,
    pub input: LogInput,
    /// The entry's `ref`, which the host computed.
    #[serde(default)]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_id: Option<SpaceId>,
    /// The brain repository, `owner/name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<String>,
    /// The entry's file in the brain repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    /// The human's GitHub login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    /// Exported to a legacy `logs/<repo>.md` file before per-entry files; the legacy import
    /// brings it over, so it is never written again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub in_legacy_file: bool,
    /// Why an entry captured before per-entry files is held in its batch and not sent: as it
    /// stands it breaks the entry contract or the cap. Its text is never changed; it waits for
    /// the human, who fixes or releases it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
    /// The human released this held entry: it stays here and is never sent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub released: bool,
}
impl WorkLog {
    /// Neither in a batch nor in a legacy file: it waits for its project's Space to have a brain.
    pub fn is_unsent(&self) -> bool {
        self.batch_id.is_none() && !self.in_legacy_file
    }
}
/// The live workspace. Archived sessions, accepted and closed tickets and those sessions'
/// runtimes are left to the `Archive`; `WorkspaceIndex` joins the two for views.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub projects: Vec<Project>,
    /// Every Space, in the order they were created.
    #[serde(default)]
    pub spaces: Vec<Space>,
    /// This Mac's facts about each Space, by id. Left out for a connection without the app's
    /// credential, so an agent's shell cannot read them.
    #[serde(default)]
    pub space_locals: BTreeMap<SpaceId, SpaceLocal>,
    /// Each Space's tracker, in the order of `spaces`. Spaces sharing a storage repository
    /// each have an entry with the same counts.
    #[serde(default)]
    pub issue_trackers: Vec<IssueTracker>,
    /// The Space of the last issue filed on this Mac, where New issue starts without a Space
    /// filter.
    #[serde(default)]
    pub last_filed_space: Option<SpaceId>,
    pub repositories: Vec<Repository>,
    /// The sessions that are not archived and open tickets' archived agents, in the order they
    /// were created, then archived sessions still in a turn.
    pub sessions: Vec<Session>,
    pub attention: Vec<Attention>,
    /// The open tickets, and accepted or closed ones an unarchived agent works on, in the order
    /// they were created.
    #[serde(default)]
    pub tickets: Vec<Ticket>,
    /// The runtimes of `sessions`.
    #[serde(default)]
    pub runtimes: Vec<SessionRuntime>,
    /// Changes whenever what `Command::Archive` answers does, so a client re-reads the archive
    /// only then.
    #[serde(default)]
    pub archive_stamp: u64,
    /// Opaque, durable message revisions for sessions included in this snapshot. Compare for
    /// equality only. A revision changes on any insert, edit, receipt change or deletion in
    /// that session's `Messages` pages; unrelated sessions and no-op writes leave it alone.
    /// Missing entries mean unknown: fetch on first selection or when the entry is absent.
    /// Older hosts omit this field; older clients may ignore it and keep refetching.
    #[serde(default)]
    pub message_revisions: BTreeMap<String, u64>,
    /// How many sessions are archived, the archive's included.
    #[serde(default)]
    pub archived_sessions: u32,
    /// By coordinator, how many tickets it owns, open or not.
    #[serde(default)]
    pub tickets_owned: BTreeMap<String, u32>,
    #[serde(default)]
    pub live_projects: Vec<String>,
    /// Parent folders checked for new repositories.
    #[serde(default)]
    pub repository_roots: Vec<String>,
    /// Workspace repositories whose folder no longer exists.
    #[serde(default)]
    pub missing_repositories: Vec<String>,
    /// Coordinator timers that will still fire.
    #[serde(default)]
    pub schedules: Vec<Schedule>,
    #[serde(default)]
    pub model_selection: ModelSelection,
    /// The shared folder holds a guide, even an empty one, which a first setup keeps.
    #[serde(default)]
    pub guide_saved: bool,
    /// Sessions whose input has not reached a turn yet.
    #[serde(default)]
    pub undelivered: Vec<UndeliveredInput>,
    /// Open tickets whose branch has fallen behind its base or overlaps another's.
    #[serde(default)]
    pub branch_warnings: Vec<BranchWarnings>,
    /// When each watched repository's PRs were last checked, and why a check failed.
    #[serde(default)]
    pub pull_request_checks: Vec<PullRequestCheck>,
    /// Sessions with a `blocked` report their parent has not answered.
    #[serde(default)]
    pub questions_waiting: Vec<QuestionsWaiting>,
    /// The owners, and single forks as `owner/name`, allowed to have PRs opened without asking.
    #[serde(default)]
    pub allowed_owners: Vec<String>,
    /// Why GitHub could not be read for the repositories' fork status and visibility.
    #[serde(default)]
    pub github_error: Option<String>,
    #[serde(default)]
    pub keeper: KeeperSettings,
    /// How many processes each agent is recorded as having left running.
    #[serde(default)]
    pub running_processes: Vec<RunningProcesses>,
    /// By ticket, the PR keeper's part of a kept ticket's PR line.
    #[serde(default)]
    pub keeper_parts: BTreeMap<String, KeeperPart>,
    #[serde(default)]
    pub skills: Vec<SkillState>,
    /// The fixed core instructions of the running host, in the order it sends them.
    #[serde(default)]
    pub core_instructions: Vec<skills::CoreInstructions>,
    /// What this machine still needs to run the shared settings, one line each.
    #[serde(default)]
    pub still_needs: Vec<StillNeed>,
    /// Set while a settings repository is chosen.
    #[serde(default)]
    pub settings_sync: Option<SyncStatus>,
    #[serde(default)]
    pub workspace_notes: Vec<WorkspaceNote>,
    /// Why each keeper trigger the shared settings turn on is off on this machine.
    #[serde(default)]
    pub keeper_held: Vec<String>,
}
/// What a `Snapshot` leaves out: archived sessions other than open tickets' agents, accepted
/// and closed tickets no unarchived agent works on, and those sessions' runtimes. Answers
/// `Command::Archive`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Archive {
    pub sessions: Vec<Session>,
    pub tickets: Vec<Ticket>,
    pub runtimes: Vec<SessionRuntime>,
    /// Every session's id, archived or not, in the order they were created, so that joining a
    /// snapshot keeps each session in its place; likewise every ticket's.
    pub session_order: Vec<String>,
    pub ticket_order: Vec<String>,
    /// The `Snapshot::archive_stamp` this archive was read at.
    pub stamp: u64,
}
/// How many processes the record says an agent started and still runs; between turns the
/// record is exact, during one it can count members that have since exited.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningProcesses {
    pub session_id: String,
    pub count: u32,
}
/// What one agent has running, as the Processes panel lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProcesses {
    pub session_id: String,
    pub name: String,
    pub groups: Vec<ProcessGroup>,
}
/// A process group an agent started that the host believes still runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessGroup {
    /// The record's id, by which `StopProcesses` names the group.
    pub id: String,
    pub group: i64,
    /// A process the host recorded is still alive in the group, so the rest is the agent's
    /// too and Stop may signal it. An unconfirmed group is never signalled.
    pub confirmed: bool,
    /// The earliest start among `processes`, or among the recorded ones when unconfirmed, in
    /// microseconds since the epoch.
    pub started: i64,
    /// The processes in the group now; empty when unconfirmed.
    pub processes: Vec<GroupProcess>,
}
/// One live process of a group, with the CPU time it has used so far.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupProcess {
    pub pid: i32,
    /// Microseconds since the epoch; with `pid`, the process's identity.
    pub started: i64,
    pub program: String,
    /// User and system time, in nanoseconds.
    pub cpu_nanos: u64,
}
/// How many of a session's `blocked` reports wait for its parent's answer to reach it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionsWaiting {
    pub session_id: String,
    pub count: u32,
}
/// A session's input waiting for a turn: held after an interrupted turn, or queued.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndeliveredInput {
    pub session_id: String,
    pub held: u32,
    pub queued: u32,
}
/// What an open ticket's branch risks when it lands: its base moving on, and other open
/// tickets in the same repository changing the same files. Read from committed work only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchWarnings {
    pub ticket_id: String,
    /// The repository's base ref, such as `origin/main`.
    pub base: String,
    /// Commits on the base since the branch's merge-base.
    pub base_ahead: u32,
    /// Files that would conflict when merging the base into the branch.
    pub base_conflicts: Vec<String>,
    pub overlaps: Vec<BranchOverlap>,
    /// Checks that could not run: a failed fetch of the base, or a failed conflict check.
    #[serde(default)]
    pub failures: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchOverlap {
    pub ticket_id: String,
    pub title: String,
    pub project: String,
    /// Files both branches changed since leaving the base.
    pub files: Vec<String>,
    /// Those of them that would conflict when merging the two branches.
    pub conflicts: Vec<String>,
    /// Why the conflict check between the two branches could not run.
    #[serde(default)]
    pub failure: Option<String>,
}
impl BranchWarnings {
    pub fn has_conflicts(&self) -> bool {
        !self.base_conflicts.is_empty() || self.overlaps.iter().any(|o| !o.conflicts.is_empty())
    }
    /// One sentence per warning, such as "origin/main is 3 commits ahead; conflicts in a.rs".
    pub fn lines(&self) -> Vec<String> {
        let conflicts = |files: &[String], failure: Option<&String>| match failure {
            Some(reason) => format!("; conflict check failed: {reason}"),
            None if files.is_empty() => String::new(),
            None => format!("; conflicts in {}", file_list(files)),
        };
        let mut lines = vec![];
        if self.base_ahead > 0 {
            let commits = if self.base_ahead == 1 {
                "commit"
            } else {
                "commits"
            };
            lines.push(format!(
                "{} is {} {commits} ahead{}",
                self.base,
                self.base_ahead,
                conflicts(&self.base_conflicts, None)
            ));
        }
        for overlap in &self.overlaps {
            lines.push(format!(
                "overlaps ticket {} (project {}) in {}{}",
                overlap.title,
                overlap.project,
                file_list(&overlap.files),
                conflicts(&overlap.conflicts, overlap.failure.as_ref())
            ));
        }
        lines.extend(self.failures.iter().cloned());
        lines
    }
}
/// Names the first few files and counts the rest.
fn file_list(files: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut list = files[..files.len().min(SHOWN)].join(", ");
    if files.len() > SHOWN {
        list.push_str(&format!(" and {} more", files.len() - SHOWN));
    }
    list
}
impl Snapshot {
    /// The workspace notes about settings sync paused.
    pub fn paused_settings(&self) -> impl Iterator<Item = &PausedSetting> {
        self.workspace_notes.iter().filter_map(|note| match note {
            WorkspaceNote::Paused(setting) => Some(setting),
            WorkspaceNote::SkillQuestion(_) | WorkspaceNote::Held(_) => None,
        })
    }

    /// The workspace repositories a project uses, in the order they were added.
    pub fn project_repositories(&self, project_id: &str) -> Vec<&Repository> {
        let Some(project) = self.projects.iter().find(|p| p.id == project_id) else {
            return Vec::new();
        };
        self.repositories
            .iter()
            .filter(|r| project.uses(&r.id))
            .collect()
    }
    pub fn questions_waiting(&self, session_id: &str) -> u32 {
        self.questions_waiting
            .iter()
            .find(|q| q.session_id == session_id)
            .map_or(0, |q| q.count)
    }
}

/// A coordinator's durable timer. Fires land at `first_at + k * every_ms`, up to `until`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub project_id: String,
    pub session_id: String,
    pub label: String,
    pub prompt: String,
    pub first_at: i64,
    pub every_ms: Option<i64>,
    pub until: Option<i64>,
    /// `None` once the last slot has fired or the timer was stopped.
    pub next_fire_at: Option<i64>,
    pub created_at: i64,
}
/// A whole-unit duration such as `30m`, `2h` or `1d`.
pub fn duration_label(ms: i64) -> String {
    const MINUTE: i64 = 60_000;
    match ms {
        _ if ms % (24 * 60 * MINUTE) == 0 => format!("{}d", ms / (24 * 60 * MINUTE)),
        _ if ms % (60 * MINUTE) == 0 => format!("{}h", ms / (60 * MINUTE)),
        _ if ms % MINUTE == 0 => format!("{}m", ms / MINUTE),
        _ => format!("{}s", ms / 1000),
    }
}
/// Host-owned settings, kept in the host's `settings.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSettings {
    /// Where new projects and ticket worktrees are created.
    pub workspaces_dir: String,
    #[serde(default)]
    pub verification: VerificationSettings,
    /// Turns a session runs without a message from its parent before the parent gets a check-in.
    #[serde(default = "default_checkin_child_turns")]
    pub checkin_child_turns: u32,
    /// Turns the project coordinator runs without the human before the human gets a check-in.
    #[serde(default = "default_checkin_human_turns")]
    pub checkin_human_turns: u32,
    /// The owners, and single forks as `owner/name`, allowed to have PRs opened without asking.
    /// Kept on this machine only.
    #[serde(default)]
    pub allowed_owners: Vec<String>,
    #[serde(default)]
    pub keeper: KeeperSettings,
    /// The repository the shared folder is a clone of. Kept on this machine only.
    #[serde(default)]
    pub settings_repository: Option<SettingsRepository>,
    /// "Let other devices use this Mac's Wiffletree". Kept on this machine only.
    #[serde(default)]
    pub remote_access: bool,
    /// "Builds at once": how many builds agents may compile or test at a time across this
    /// machine; the rest wait. Kept on this machine only.
    #[serde(default = "default_builds_at_once")]
    pub builds_at_once: usize,
}
/// A private GitHub repository the shared folder syncs with, as this machine recorded it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsRepository {
    /// `owner/name`, as GitHub names it.
    pub name: String,
    /// The only URL sync pushes to.
    pub push_url: String,
    /// The repository's default branch, the only one sync pushes.
    pub branch: String,
    #[serde(default)]
    pub readers: Readers,
}
/// Who can read a private settings repository, as far as GitHub says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readers {
    /// Only its owner and the people given access.
    Invited,
    /// Every member of its organization, whose base permission is Read or more.
    EveryMember,
    /// Every member of its organization's enterprise: the repository is internal.
    EnterpriseMembers,
    /// GitHub did not report the organization's base permission.
    #[default]
    MaybeMembers,
}
/// What checking a candidate settings repository found. Nothing was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsRepositoryCheck {
    pub repository: SettingsRepository,
    pub organization: bool,
    /// Set when the repository already holds shared settings: this machine joins it.
    pub joining: Option<Joining>,
}
/// Sync with the settings repository chosen on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub repository: SettingsRepository,
    /// When a pass last reached GitHub, and how many files it pulled and pushed.
    pub synced_at: Option<i64>,
    pub pulled: usize,
    pub pushed: usize,
    pub next_pull_at: Option<i64>,
    /// Why the latest pass failed, in Git's words; cleared by one that succeeds.
    pub error: Option<String>,
    /// When the latest pass failed, and when the failures since the last success began.
    pub failed_at: Option<i64>,
    pub failing_since: Option<i64>,
    /// Sync pulls and pushes nothing until the repository's format can be read: `error` says why.
    pub stopped: bool,
    /// A pass runs now.
    pub syncing: bool,
    /// The settings saved here since they were last synced, each named once.
    pub waiting: Vec<String>,
    /// This machine's name, as sync's commits give it.
    pub machine: String,
}
/// Something this machine lacks to run the shared settings, and the page that provides it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StillNeed {
    pub text: String,
    pub page: WorkspacePage,
}
/// A page of the main window about the whole workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePage {
    Models,
    Repositories,
}
/// Why sync leaves a shared file alone until the human decides.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Paused {
    /// This machine and `machine` both changed it since it was last synced.
    TwoMachines { machine: String },
    /// Its content here is neither what the host last wrote nor what it last synced.
    OutsideWiffletree,
    /// The version `machine` pushed is one this machine would refuse to save, for `reason`.
    CannotBeRead { machine: String, reason: String },
}
/// A note about the whole workspace, listed for every project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "note", rename_all = "snake_case")]
pub enum WorkspaceNote {
    /// A shared setting sync paused.
    Paused(PausedSetting),
    SkillQuestion(SkillQuestion),
    /// A file held back from a push because a scan failed or could not run.
    Held(HeldFile),
}
/// A file of a storage repository that is not pushed until a scan passes it. Names the check and
/// its rule, never what matched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldFile {
    /// The storage repository, `owner/name`.
    pub repository: String,
    pub path: String,
    /// `sensitive-info` or `secret-scan`.
    pub check: String,
    /// The rule that failed, such as `phone`; `None` when the check could not run.
    pub rule: Option<String>,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PausedSetting {
    /// The paused file, or for a skill's rules, rewrite and base, the skill's folder.
    pub path: String,
    /// As the app names it, such as "Implementing · your rules".
    pub setting: String,
    pub paused: Paused,
    /// What happened and what this machine does meanwhile, starting with `setting`.
    pub text: String,
    /// The storage repository the file is in; `None` for the settings repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}
/// How the human settles a paused file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Settle {
    /// Pushes this machine's version over the other machine's.
    KeepThisMachines,
    /// Writes the other machine's version here.
    UseTheOthers,
    /// Pushes a version changed outside Wiffletree, once the host accepts it.
    SyncThisVersion,
    /// Puts back the version last synced.
    PutBackSynced,
}
/// What a repository holds beside what this machine has, before joining it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Joining {
    pub settings: Vec<SettingComparison>,
    /// The machine the repository's last commit names, and that commit's time.
    pub last_machine: String,
    pub last_changed_at: i64,
}
/// One shared setting, as each side holds it, such as "Implementing": "3 of your rules".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingComparison {
    pub setting: String,
    pub repository: String,
    pub this_machine: String,
}
/// Whose shared settings a joining machine keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinWith {
    /// The repository's, with this machine's skill history added.
    Repository,
    /// This machine's, pushed over the repository's in one commit.
    ThisMachine,
}
fn default_builds_at_once() -> usize {
    DEFAULT_BUILDS_AT_ONCE
}
/// "Builds at once" when it was never set: each build gets half the cores, so one leaves the
/// other half to the human.
pub const DEFAULT_BUILDS_AT_ONCE: usize = 1;
/// The most "Builds at once" the settings accept.
pub const MAX_BUILDS_AT_ONCE: usize = 4;
fn default_checkin_child_turns() -> u32 {
    25
}
fn default_checkin_human_turns() -> u32 {
    100
}
/// The child check-in thresholds the settings accept.
pub const CHECKIN_CHILD_TURNS: std::ops::RangeInclusive<u32> = 5..=500;
/// The human check-in thresholds the settings accept.
pub const CHECKIN_HUMAN_TURNS: std::ops::RangeInclusive<u32> = 10..=1000;
impl HostSettings {
    /// Settings with `workspaces_dir` and every default.
    pub fn new(workspaces_dir: String) -> Self {
        Self {
            workspaces_dir,
            verification: VerificationSettings::default(),
            checkin_child_turns: default_checkin_child_turns(),
            checkin_human_turns: default_checkin_human_turns(),
            allowed_owners: vec![],
            keeper: KeeperSettings::default(),
            settings_repository: None,
            remote_access: false,
            builds_at_once: DEFAULT_BUILDS_AT_ONCE,
        }
    }
    pub fn validate_check_ins(child_turns: u32, human_turns: u32) -> Result<()> {
        ensure!(
            CHECKIN_CHILD_TURNS.contains(&child_turns),
            "Agent check-ins must come every {}–{} turns",
            CHECKIN_CHILD_TURNS.start(),
            CHECKIN_CHILD_TURNS.end()
        );
        ensure!(
            CHECKIN_HUMAN_TURNS.contains(&human_turns),
            "Your check-ins must come every {}–{} coordinator turns",
            CHECKIN_HUMAN_TURNS.start(),
            CHECKIN_HUMAN_TURNS.end()
        );
        Ok(())
    }
}

/// The PR keeper's triggers and quiet period, one value for the whole workspace. Every trigger
/// is off until the human turns it on; only the human's client changes them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeeperSettings {
    /// Rebase a kept ticket's PR when its base branch moves.
    #[serde(default)]
    pub rebase: bool,
    /// How long the base must stay at one commit before a rebase.
    #[serde(default = "default_quiet_minutes")]
    pub quiet_minutes: u32,
    /// Sort new comments on a kept ticket's PR with the keeper's model, every tool off.
    #[serde(default)]
    pub comments: bool,
    /// Check and review a rebase whose patch changed before pushing it.
    #[serde(default)]
    pub conflicts: bool,
    /// Try once to fix a kept ticket's PR whose checks failed, when they fail here too.
    #[serde(default)]
    pub checks: bool,
    /// The re-check level the keeper's reviews run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_level: Option<String>,
}
fn default_quiet_minutes() -> u32 {
    2
}
/// The quiet periods the settings accept, in minutes.
pub const KEEPER_QUIET_MINUTES: std::ops::RangeInclusive<u32> = 1..=60;
impl Default for KeeperSettings {
    fn default() -> Self {
        Self {
            rebase: false,
            quiet_minutes: default_quiet_minutes(),
            comments: false,
            conflicts: false,
            checks: false,
            review_level: None,
        }
    }
}
impl KeeperSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            KEEPER_QUIET_MINUTES.contains(&self.quiet_minutes),
            "The base must be quiet for {}–{} minutes",
            KEEPER_QUIET_MINUTES.start(),
            KEEPER_QUIET_MINUTES.end()
        );
        self.check_rebase()
    }
    /// Refuses a trigger for tries without the rebase trigger, on every machine, since a try
    /// waits for the keeper's own rebase.
    pub fn check_rebase(&self) -> Result<()> {
        for problem in [KeeperProblem::Conflict, KeeperProblem::Checks] {
            ensure!(
                self.rebase || !self.tries(problem),
                "\"{}\" needs \"{KEEPER_REBASE}\"",
                problem.trigger()
            );
        }
        Ok(())
    }
    /// Whether any trigger is on, so repositories are said to be kept or not.
    pub fn is_on(&self) -> bool {
        self.rebase || self.comments || self.conflicts || self.checks
    }
    /// Whether the trigger for tries at `problem` is on.
    pub fn tries(&self, problem: KeeperProblem) -> bool {
        match problem {
            KeeperProblem::Conflict => self.conflicts,
            KeeperProblem::Checks => self.checks,
        }
    }
}
/// Refuses a trigger for tries unless the rebase trigger is on, the table has a PR keeper row
/// and the review level names a configured re-check level: a try waits for the keeper's own
/// rebase, and what the keeper makes is reviewed at that level.
pub fn check_tries(
    keeper: &KeeperSettings,
    model: Option<&JobRow>,
    verification: &VerificationSettings,
) -> Result<()> {
    let rechecks: Vec<&str> = verification
        .levels
        .iter()
        .filter(|l| l.kind == LevelKind::Recheck)
        .map(|l| l.name.as_str())
        .collect();
    keeper.check_rebase()?;
    for problem in [KeeperProblem::Conflict, KeeperProblem::Checks] {
        if !keeper.tries(problem) {
            continue;
        }
        let trigger = format!("\"{}\"", problem.trigger());
        ensure!(
            model.is_some(),
            "{trigger} needs a PR keeper model. Choose a PR keeper model in Models first."
        );
        ensure!(
            !rechecks.is_empty(),
            "{trigger} needs a re-check level to review with: add one in Models → Reviews"
        );
        let named = keeper.review_level.as_deref().unwrap_or_default();
        ensure!(
            rechecks.iter().any(|name| same_name(name, named)),
            "{trigger} reviews with a re-check level: choose {}",
            or_list(&rechecks)
        );
    }
    Ok(())
}
/// Refuses a PR keeper row that cannot sort comments: sorting runs with every tool turned off,
/// which only Claude can be started with, so it needs a Claude choice in the row.
pub fn check_sorting_model(keeper: Option<&JobRow>) -> Result<()> {
    ensure!(
        keeper.is_some(),
        "\"Sort new comments\" needs a PR keeper model with a Claude choice. Choose a PR keeper model in Models first."
    );
    ensure!(
        keeper.is_some_and(|row| row.main.choices().any(|p| p.provider == Provider::Claude)),
        "Sorting comments needs a Claude model in the PR keeper's row: it runs with every tool turned off, and Codex cannot be started that way"
    );
    Ok(())
}

/// The highest verification round cap the settings accept.
pub const MAX_VERIFICATION_ROUNDS: u32 = 5;
/// The highest verification cycle cap the settings accept.
pub const MAX_VERIFICATION_CYCLES: u32 = 5;
/// Who verifies a ticket when `verify_ticket` is called, the levels the coordinator picks from, how
/// many rounds a cycle may take and how many full reviews a ticket may start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedVerification")]
pub struct VerificationSettings {
    pub verifiers: Vec<VerifierConfig>,
    pub levels: Vec<ReviewLevel>,
    pub max_rounds: u32,
    /// Counts full reviews only; re-checks are not counted.
    pub max_cycles: u32,
}
/// The setting as it is stored. Reading it never fails for a setting saved before levels or the
/// cycle cap existed, because an unreadable settings file drops every setting to its default.
#[derive(Deserialize)]
struct SavedVerification {
    verifiers: Vec<VerifierConfig>,
    levels: Option<Vec<ReviewLevel>>,
    max_rounds: u32,
    #[serde(default = "default_max_cycles")]
    max_cycles: u32,
}
impl From<SavedVerification> for VerificationSettings {
    fn from(saved: SavedVerification) -> Self {
        match saved.levels {
            Some(levels) => Self {
                verifiers: saved.verifiers,
                levels,
                max_rounds: saved.max_rounds,
                max_cycles: saved.max_cycles,
            },
            None => Self::one_level(saved.verifiers, saved.max_rounds, saved.max_cycles),
        }
    }
}
fn default_max_cycles() -> u32 {
    2
}
impl Default for VerificationSettings {
    /// A tester and a Claude and a Codex review with their lenses, all run by the one level
    /// "Full"; three rounds and two full reviews.
    fn default() -> Self {
        let verifier = |role, focus: &str, instruction: Option<&str>| VerifierConfig {
            role,
            focus: focus.into(),
            instruction: instruction.map(Into::into),
        };
        Self::one_level(
            vec![
                verifier(Role::Tester, "Tests", None),
                verifier(
                    Role::Reviewer,
                    "Correctness",
                    Some("Correctness against the ticket's acceptance criteria."),
                ),
                verifier(
                    Role::Reviewer,
                    "Regressions",
                    Some("Regressions and test coverage."),
                ),
            ],
            3,
            default_max_cycles(),
        )
    }
}
/// Names and focuses are compared ignoring surrounding spaces and ASCII case.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}
/// Such as "Full, Light or Docs".
pub fn or_list(names: &[&str]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => names.concat(),
    }
}
impl VerificationSettings {
    /// What a setting saved before levels becomes: its verifiers and caps with one level,
    /// "Full", a full review that runs every verifier.
    pub fn one_level(verifiers: Vec<VerifierConfig>, max_rounds: u32, max_cycles: u32) -> Self {
        let full = ReviewLevel {
            name: "Full".into(),
            kind: LevelKind::FullReview,
            verifiers: verifiers.iter().map(|v| v.focus.clone()).collect(),
            use_when: "for every review.".into(),
        };
        Self {
            verifiers,
            levels: vec![full],
            max_rounds,
            max_cycles,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=MAX_VERIFICATION_ROUNDS).contains(&self.max_rounds),
            "Rounds per review must be 1–{MAX_VERIFICATION_ROUNDS}"
        );
        ensure!(
            (1..=MAX_VERIFICATION_CYCLES).contains(&self.max_cycles),
            "Full reviews per ticket must be 1–{MAX_VERIFICATION_CYCLES}"
        );
        ensure!(
            !self.verifiers.is_empty(),
            "Configure at least one verifier"
        );
        let mut focuses = BTreeSet::new();
        for verifier in &self.verifiers {
            ensure!(
                matches!(verifier.role, Role::Tester | Role::Reviewer),
                "Verifiers are testers or reviewers"
            );
            let focus = verifier.focus.trim();
            ensure!(
                !focus.is_empty() && focus.len() <= 60,
                "Give each verifier a focus of 1–60 bytes"
            );
            ensure!(
                focuses.insert(focus.to_lowercase()),
                "Two verifiers share the focus \"{focus}\""
            );
            ensure!(
                verifier
                    .instruction
                    .as_ref()
                    .is_none_or(|i| i.len() <= MAX_TEXT_BYTES),
                "Verifier instruction is too long"
            );
        }
        ensure!(
            self.levels.iter().any(|l| l.kind == LevelKind::FullReview),
            "Keep at least one level marked Full review."
        );
        let mut names = BTreeSet::new();
        for level in &self.levels {
            let name = level.name.trim();
            ensure!(
                !name.is_empty() && name.len() <= 60,
                "Give each level a name of 1–60 bytes"
            );
            ensure!(
                names.insert(name.to_lowercase()),
                "Two levels are named \"{name}\"."
            );
            ensure!(
                !level.verifiers.is_empty(),
                "{name} would run no verifier. Tick another verifier for {name}, or remove {name}."
            );
            let mut run = BTreeSet::new();
            for focus in &level.verifiers {
                let focus = focus.trim();
                ensure!(
                    focuses.contains(&focus.to_lowercase()),
                    "{name} runs \"{focus}\", which is not a configured verifier"
                );
                ensure!(
                    run.insert(focus.to_lowercase()),
                    "{name} names the verifier \"{focus}\" twice"
                );
            }
            ensure!(
                one_line_reason(&level.use_when).is_ok(),
                "Say when to use {name}, in one line of at most {MAX_REASON_CHARS} characters"
            );
        }
        Ok(())
    }
    /// The level named `name` and its verifiers in configured order. The only place a level
    /// becomes verifiers.
    pub fn level(&self, name: &str) -> Result<(&ReviewLevel, Vec<&VerifierConfig>)> {
        let level = self
            .levels
            .iter()
            .find(|l| same_name(&l.name, name))
            .with_context(|| {
                format!(
                    "No review level is named \"{}\". {}",
                    name.trim(),
                    self.name_a_level()
                )
            })?;
        let configured = |focus: &str| self.verifiers.iter().find(|v| same_name(&v.focus, focus));
        if let Some(missing) = level.verifiers.iter().find(|f| configured(f).is_none()) {
            bail!(
                "{} runs \"{missing}\", which is not a configured verifier. Choose its verifiers again in Models → Reviews.",
                level.name
            );
        }
        let verifiers = self
            .verifiers
            .iter()
            .filter(|v| level.runs(&v.focus))
            .collect();
        Ok((level, verifiers))
    }
    /// Such as "Name a review level: Full, Light or Docs."
    pub fn name_a_level(&self) -> String {
        let names: Vec<&str> = self.levels.iter().map(|l| l.name.as_str()).collect();
        format!("Name a review level: {}.", or_list(&names))
    }
}
/// A named set of the configured verifiers. The coordinator names one each time it starts a
/// review, and every verifier in it must pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewLevel {
    pub name: String,
    pub kind: LevelKind,
    /// The focuses of the verifiers it runs.
    pub verifiers: Vec<String>,
    /// One line telling the coordinator when to pick this level.
    pub use_when: String,
}
impl ReviewLevel {
    /// Focuses match as level names do: trimmed and ignoring ASCII case.
    pub fn runs(&self, focus: &str) -> bool {
        self.verifiers.iter().any(|f| same_name(f, focus))
    }
}
/// Whether a level can be a ticket's only review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelKind {
    FullReview,
    /// Starts only on a ticket whose latest review passed.
    Recheck,
}
/// One configured verifier. Its model comes from its row in the Who runs what table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierConfig {
    pub role: Role,
    /// Tells verifiers apart on a ticket, such as "Codex" or "Style".
    pub focus: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

/// Where a ticket is in its life, stored and sent as its snake_case name. Reports move an open
/// ticket to the state their kind names; verification, acceptance and closing move it too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketState {
    Planned,
    Assigned,
    ReadyForTesting,
    Verifying,
    Passed,
    Failed,
    Blocked,
    Completed,
    Accepted,
    Closed,
}
impl TicketState {
    pub const ALL: [Self; 10] = [
        Self::Planned,
        Self::Assigned,
        Self::ReadyForTesting,
        Self::Verifying,
        Self::Passed,
        Self::Failed,
        Self::Blocked,
        Self::Completed,
        Self::Accepted,
        Self::Closed,
    ];
    /// The stored name, such as `ready_for_testing`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Assigned => "assigned",
            Self::ReadyForTesting => "ready_for_testing",
            Self::Verifying => "verifying",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Accepted => "accepted",
            Self::Closed => "closed",
        }
    }
    /// Accepted and closed tickets take no further agents or state changes.
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Accepted | Self::Closed)
    }
    /// The state a stored name, or the report kind of the same name, stands for.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.name() == name)
    }
}
impl std::fmt::Display for TicketState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
/// A ticket is a workspace: one repository, one worktree and branch, and its agents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    /// The project coordinator that owns the ticket.
    pub coordinator_id: String,
    /// Empty only on tickets stored before tickets recorded their repository.
    #[serde(default)]
    pub repository_id: String,
    pub title: String,
    pub brief: String,
    pub worktree: String,
    pub branch: String,
    /// The ticket this one is stacked on, in the same repository: its branch started from the
    /// parent's branch, is measured against it, and its pull request opens into it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacked_on: Option<String>,
    /// The branch on origin, such as `Parental-controls`, that the ticket starts from, is
    /// measured against and whose pull request opens into it, in place of the repository's base.
    /// A stacked ticket carries its parent's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    pub state: TicketState,
    /// The latest verification cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<Verification>,
    /// Earlier cycles, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_cycles: Vec<Verification>,
    /// Every finding verifiers recorded, across all cycles, with the coordinator's decisions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<LedgerEntry>,
    /// Why the coordinator accepted or published the ticket after a blocked cycle, or with
    /// commits no review checked once it had used every full review. A new cycle clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<String>,
    /// The commits no review checked that the waiver named, past the verified commit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waived_commits: Vec<String>,
    /// The ticket branch's GitHub PR, as the PR watcher last read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequest>,
    /// The publish request whose step is running or waiting for the human.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publishing: Option<PublishRequest>,
    /// The coordinator's `merge_ticket` request while it is decided, waits or runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merging: Option<MergeRequest>,
    /// The commit the host last pushed to the ticket's branch on GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed: Option<String>,
    /// What the PR keeper did to the ticket's branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keeper: Option<Keeper>,
}
/// The PR keeper's record of a ticket.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keeper {
    /// The keeper's last action on the branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<KeeperAction>,
    /// The PR head and base commit last rebased; a pair is rebased at most once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebased: Option<KeeperRebase>,
    /// The rebase of `rebased`, waiting for the ticket's worktree to be free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    /// The waiting rebase changed the patch and passed the repository's checks, so the
    /// keeper reviews it before the push.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub waiting_changed: bool,
    /// Commits Wiffletree did not make, taken from GitHub's branch; while set, nothing is
    /// pushed or accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreviewed: Option<Unreviewed>,
    #[serde(default)]
    pub comments: KeeperComments,
    /// Every review the keeper started, oldest first; the last may be running.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviews: Vec<KeeperReview>,
    /// Every try by the keeper's model, oldest first; the last may be running.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tries: Vec<KeeperTry>,
    /// The PR watcher's wakes held back while the keeper may still try their problems.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_wakes: Vec<HeldWake>,
}
/// The Repositories page's names for the PR keeper's rebase trigger and its trigger for tries.
pub const KEEPER_REBASE: &str = "Keep it rebased";
pub const KEEPER_TRIES: &str = "Try once to fix a conflict or failed checks";
/// What a try by the keeper's model fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeeperProblem {
    Conflict,
    Checks,
}
impl KeeperProblem {
    /// The trigger for tries at this problem, as the Repositories page names it: one row for both.
    pub fn trigger(self) -> &'static str {
        KEEPER_TRIES
    }
}
/// One try by the keeper's model at one problem at one PR head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeeperTry {
    pub problem: KeeperProblem,
    pub head: String,
    pub base: String,
    /// The files Git could not merge, or for failed checks the files the PR changes against its
    /// base: the files the try may change.
    pub files: Vec<String>,
    pub started: i64,
    /// Unset while the try runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<TryEnd>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TryEnd {
    Pushed,
    /// Reviewed and on the ticket's branch, but GitHub's branch is unchanged.
    NotPushed,
    ThrownAway(String),
}
/// A watcher's wake for `problem` at `head`, as the watcher would have sent it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldWake {
    pub problem: KeeperProblem,
    pub head: String,
    pub id: String,
    pub body: String,
}
/// A review the keeper started on a commit it moved onto the ticket's branch: everything a
/// withdrawal restores, and once withdrawn, what it took away.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeeperReview {
    pub cycle: u32,
    /// The branch's commit before the keeper moved it, which was the PR's head.
    pub before: String,
    /// The keeper's commit under review.
    pub after: String,
    /// The base commit the keeper's commit sits on.
    pub base: String,
    /// The ticket's state when the review started.
    pub state: TicketState,
    /// The keeper's last action before this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<KeeperAction>,
    /// The problem of the try whose result this reviews; none for a rebase whose patch changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<KeeperProblem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<Withdrawn>,
    /// The push of the review that passed was started; how it ended is publishing's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub push_started: bool,
}
/// A withdrawn keeper review: its cycle and the ledger entries it recorded, with any decision
/// made on them, and whether the branch still waits to be put back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withdrawn {
    pub cycle: Verification,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<LedgerEntry>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub putting_back: bool,
}
/// The comments the keeper sorted on the ticket's PR: ids and classes, never their text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeeperComments {
    /// Each comment read that a read can still return, by id, such as `discussion_r4190331`,
    /// with its class: a comment is new exactly when its id is not here. Unset until the first
    /// read with the comments trigger on, which records the comments already there as earlier.
    pub read: Option<BTreeMap<String, CommentClass>>,
    /// The watcher's newest comments when they were last read in full.
    pub cursors: CommentCursors,
    /// Comments by people who cannot push: counted, never sorted.
    pub outside: u32,
    /// The comments that need a change, and the questions, until the next review starts.
    pub needs_change: u32,
    pub questions: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentClass {
    NeedsChange,
    Question,
    NoAction,
    /// The run gave no class the host accepts, failed or was discarded.
    NotSorted,
    /// By someone who cannot push: counted, never sent to a model.
    Outside,
    /// On the PR before the comments trigger was turned on: never sorted.
    Earlier,
}
/// The head the keeper adopted, how many commits no review has checked, and the number of the
/// ticket's latest cycle then: only a later cycle that ends clears the record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unreviewed {
    pub head: String,
    pub commits: u32,
    pub cycle: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeeperRebase {
    pub head: String,
    pub base: String,
}
/// What the keeper did, when, the branch's commits before and after, and the base commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeeperAction {
    pub kind: KeeperKind,
    pub at: i64,
    pub before: String,
    pub after: String,
    pub base: String,
    /// GitHub's branch is at `after`.
    pub pushed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeeperKind {
    /// A rebase with the same change as the reviewed commit.
    Rebase,
    /// A rebase whose patch changed: checked and reviewed by the keeper before its push.
    ReviewedRebase,
    /// A conflict resolved by the keeper's model: checked and reviewed before its push.
    Resolved,
    /// Failed checks fixed by the keeper's model: checked and reviewed before its push.
    Fixed,
}
/// What the PR keeper is doing for a kept ticket whose branch it may change, as the host found
/// it after the latest pass: the end of the ticket's PR line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeeperPart {
    /// A rebase waits for the base to stay quiet until this time.
    RebaseAt(i64),
    /// A rebase waits for the ticket's worktree to be free.
    Waiting,
    /// The PR's head is the keeper's rebase made at this time, less than an hour ago.
    Rebased(i64),
    /// The PR's head contains the base's head.
    UpToDate,
    /// The keeper's model is trying to resolve the PR's conflict.
    Resolving,
    /// The keeper's model is trying to fix the PR's failed checks.
    Fixing,
    /// The keeper threw its try at the PR's head away and told the coordinator.
    WithCoordinator,
}
/// One accepted `publish_ticket` call: everything the Inbox item shows and the host will send,
/// frozen when the call passed its checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRequest {
    pub id: String,
    /// The session that called `publish_ticket` and gets the result.
    pub caller: String,
    /// GitHub's `owner/name` for the repository.
    pub repository: String,
    pub push_url: String,
    pub branch: String,
    /// The branch a pull request merges into.
    pub base: String,
    pub commit: String,
    /// The verification cycle the request was decided on, and how it ended. A request does not
    /// outlive it: once the ticket has a later cycle, nothing more is sent.
    pub cycle: u32,
    pub outcome: VerificationOutcome,
    /// The commit's size, such as "2 commits · 3 files changed, 41 insertions(+)".
    pub size: String,
    /// Set when the request opens a pull request; either or both when it edits the open one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The open pull request whose title or description the request edits after its push.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edits: Option<u64>,
    /// The step that runs or waits next.
    pub step: PublishStep,
    /// The Inbox item the step waits on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    /// The human approved this commit's steps on one item after a blocked review in an allowed
    /// owner's repository.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked_review_approved: bool,
    /// How many of this request's Inbox items the human approved.
    #[serde(default)]
    pub approvals: u8,
    /// The coordinator's waiver: of a blocked review's open findings, or of commits past the
    /// verified one once the ticket had used every full review. In an allowed owner's
    /// repository such a request runs without asking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waived: Option<CommitWaiver>,
    /// Started by the host for the PR keeper: it never asks, and its result goes to the
    /// keeper's record rather than to an agent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keeper: bool,
}
/// One accepted `merge_ticket` call: the pull request and head the host merges, decided on one
/// review. Only the host changes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRequest {
    pub id: String,
    /// The coordinator that called `merge_ticket` and gets the result.
    pub caller: String,
    /// GitHub's `owner/name` for the repository.
    pub repository: String,
    pub number: u64,
    pub branch: String,
    /// The branch the pull request merges into, which is not protected.
    pub base: String,
    /// The head the request merges. A request that needs no approval follows the host's own
    /// pushes of the same change; one that needs approval ends on any other head.
    pub head: String,
    /// The verification cycle the request was decided on. A later cycle ends it.
    pub cycle: u32,
    /// The acceptance waiver the merge records: the coordinator's, or after a blocked review
    /// without one, the reason the host records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waived: Option<String>,
    /// The review ended blocked, or the head stands on commits no review checked, so the human
    /// approves the merge first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_approval: bool,
    /// The human approved merging this head.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approved: bool,
    /// The method the merge is sent with, recorded once every check held and before it is sent,
    /// and when the human is asked, so the approval card names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<MergeMethod>,
    /// The commits no review checked that the merge would take, as the approval card lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreviewed: Vec<String>,
    /// What GitHub last said about the pull request while the request waits for the human.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<MergeRead>,
    pub stage: MergeStage,
}
/// The pull request's checks and mergeability as the host last read them from GitHub for a
/// merge request, never from the PR watcher's record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRead {
    pub checks: CheckState,
    /// GitHub had worked out that it merges with no conflict.
    pub mergeable: bool,
}
impl MergeRequest {
    /// Whether the host may merge without asking the human.
    pub fn may_merge(&self) -> bool {
        !self.needs_approval || self.approved
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MergeStage {
    /// GitHub is being read for a new or just approved request. This read sends no merge.
    Checking,
    /// Every check held and the method is recorded: `gh pr merge` may have been sent, so after a
    /// restart the request is finished or ended on GitHub's word.
    Merging,
    /// GitHub is being read again for a request that waited for its checks. This read sends no
    /// merge, so after a restart the request waits again unless GitHub reports it merged.
    Rechecking,
    /// It waits for the pull request's checks to finish, or for GitHub to work out whether it
    /// can be merged.
    WaitingForChecks,
    /// It waits for the human's answer to this Inbox item.
    WaitingForHuman { item: String },
}
/// A pull request the host merged with `merge_ticket`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostMerge {
    /// `None` for a merge a restart or a time limit cut short, which GitHub finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<MergeMethod>,
    /// The head it merged.
    pub head: String,
    /// Merged after the human approved it, following a blocked review or a waiver.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approved: bool,
}
/// The coordinator's waiver: its one-line reason and the commits no review checked that it
/// names, none when it waives only a blocked review's open findings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitWaiver {
    pub reason: String,
    pub commits: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishStep {
    Push,
    OpenPullRequest,
    EditPullRequest,
}
impl PublishStep {
    pub fn label(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::OpenPullRequest => "pull request",
            Self::EditPullRequest => "pull request edit",
        }
    }
}
impl PublishRequest {
    /// Whether this request opens a pull request after its push.
    pub fn opens_pull_request(&self) -> bool {
        self.edits.is_none() && self.title.is_some()
    }
    /// The step after the push, if the request has one.
    pub fn after_push(&self) -> Option<PublishStep> {
        match self.edits {
            Some(_) => Some(PublishStep::EditPullRequest),
            None => self.title.is_some().then_some(PublishStep::OpenPullRequest),
        }
    }
    /// What an edit changes, such as "title and description".
    pub fn edited_fields(&self) -> &'static str {
        match (&self.title, &self.body) {
            (Some(_), Some(_)) => "title and description",
            (Some(_), None) => "title",
            _ => "description",
        }
    }
}
impl Ticket {
    /// Accepted and closed tickets take no further agents or state changes.
    pub fn is_open(&self) -> bool {
        self.state.is_open()
    }
    /// The verification cycle in progress, if any.
    pub fn running_cycle(&self) -> Option<&Verification> {
        self.verification
            .as_ref()
            .filter(|v| v.outcome == VerificationOutcome::Running)
    }
    /// The latest cycle when a publish may stand on it without the human: it passed, or it
    /// ended blocked and the coordinator waived it.
    pub fn standing_review(&self) -> Option<&Verification> {
        self.verification.as_ref().filter(|v| match v.outcome {
            VerificationOutcome::Passed => true,
            VerificationOutcome::Blocked => self.waiver.is_some(),
            VerificationOutcome::Running => false,
        })
    }
    /// The commit a publish stands on: the last commit a waiver at the cap covered, past the
    /// verified one, or else the verified commit.
    pub fn standing_commit(&self) -> Option<&str> {
        (self.waived_commits.last().map(String::as_str))
            .or_else(|| self.verification.as_ref()?.verified_commit())
    }
    /// When the running cycle of this open ticket was queued, until the host starts it.
    pub fn queued_since(&self) -> Option<i64> {
        self.running_cycle()
            .filter(|_| self.is_open())
            .and_then(|v| v.queued_at)
    }
    /// The most recent full review that passed, which any later re-check rests on.
    pub fn last_full_review(&self) -> Option<&Verification> {
        self.previous_cycles
            .iter()
            .chain(&self.verification)
            .rev()
            .find(|v| v.is_full_review() && v.outcome == VerificationOutcome::Passed)
    }
    /// Every finding that is neither fixed, noted nor settled as follow-up or won't-fix.
    pub fn open_findings(&self) -> Vec<&LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| {
                !matches!(e.status, EntryStatus::Fixed | EntryStatus::Noted)
                    && !e.status.is_settled()
            })
            .collect()
    }
    /// The ticket's pull request while it is open; a publish then pushes to it and may edit it.
    pub fn open_pull_request(&self) -> Option<&PullRequest> {
        self.pull_request
            .as_ref()
            .filter(|pr| pr.state == PrState::Open)
    }
    /// The ticket's pull request once it was closed or merged; opening another needs the human.
    pub fn ended_pull_request(&self) -> Option<&PullRequest> {
        self.pull_request
            .as_ref()
            .filter(|pr| pr.state != PrState::Open)
    }
    /// The keeper's reviews, oldest first.
    pub fn keeper_reviews(&self) -> &[KeeperReview] {
        self.keeper.as_ref().map_or(&[], |k| &k.reviews)
    }
    /// The next cycle's number: above every cycle, withdrawn ones included, since verifier
    /// messages are keyed by it.
    pub fn next_cycle(&self) -> u32 {
        let withdrawn = self.keeper_reviews().iter().map(|r| r.cycle);
        let latest = self.verification.as_ref().map(|v| v.cycle);
        latest.into_iter().chain(withdrawn).max().unwrap_or(0) + 1
    }
    /// How many ledger entries withdrawn reviews took away. Their ids are never given again.
    pub fn withdrawn_findings(&self) -> usize {
        self.keeper_reviews()
            .iter()
            .filter_map(|r| r.withdrawn.as_ref())
            .map(|w| w.entries.len())
            .sum()
    }
    /// The keeper's try that has not ended.
    pub fn running_try(&self) -> Option<&KeeperTry> {
        let last = self.keeper.as_ref()?.tries.last();
        last.filter(|t| t.ended.is_none())
    }
    /// The keeper's part for its try at the PR's head, which outranks every other part: read
    /// from the ticket, so it shows as soon as a try starts or ends.
    pub fn try_part(&self) -> Option<KeeperPart> {
        let head = &self.open_pull_request()?.head;
        let last = self.keeper.as_ref()?.tries.last()?;
        match &last.ended {
            _ if last.head != *head => None,
            None if last.problem == KeeperProblem::Checks => Some(KeeperPart::Fixing),
            None => Some(KeeperPart::Resolving),
            Some(TryEnd::ThrownAway(_)) => Some(KeeperPart::WithCoordinator),
            Some(_) => None,
        }
    }
    /// Whether the keeper tried `problem` at `head`, which it does once.
    pub fn tried(&self, problem: KeeperProblem, head: &str) -> bool {
        let tries = self.keeper.as_ref().map_or(&[][..], |k| &k.tries);
        tries.iter().any(|t| t.problem == problem && t.head == head)
    }
    /// The commits the PR keeper adopted that no review has checked yet.
    pub fn unreviewed(&self) -> Option<&Unreviewed> {
        self.keeper.as_ref()?.unreviewed.as_ref()
    }
    /// The sorted comments that need a change, and the questions, waiting on the PR.
    pub fn waiting_comments(&self) -> Option<&KeeperComments> {
        self.keeper
            .as_ref()
            .map(|k| &k.comments)
            .filter(|c| c.needs_change + c.questions > 0)
    }
    /// Stops counting waiting comments: a review has started, or the PR has ended.
    pub fn end_waiting_comments(&mut self) {
        if let Some(keeper) = &mut self.keeper {
            keeper.comments.needs_change = 0;
            keeper.comments.questions = 0;
        }
    }
    /// Clears the record of adopted commits once a cycle that started after the adoption has
    /// ended, however it ended; that cycle's outcome then decides.
    pub fn end_unreviewed(&mut self) {
        let ended = self
            .verification
            .as_ref()
            .filter(|v| v.outcome != VerificationOutcome::Running)
            .map(|v| v.cycle);
        if let Some(keeper) = &mut self.keeper
            && keeper
                .unreviewed
                .as_ref()
                .is_some_and(|u| ended > Some(u.cycle))
        {
            keeper.unreviewed = None;
        }
    }
}
/// A ticket branch's GitHub PR, as the PR watcher last read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub state: PrState,
    pub draft: bool,
    /// The branch the PR merges into.
    pub base: String,
    /// The PR's head commit.
    pub head: String,
    pub merge_state: MergeState,
    pub checks: CheckState,
    pub unresolved_threads: u32,
    pub comments: CommentCursors,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit: Option<String>,
    /// Set when the host merged it, which the PR watcher's reading keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_by_host: Option<HostMerge>,
}
impl PullRequest {
    /// What changed from `old`, such as "checks failure, new head 9e03c1b".
    pub fn changes(old: Option<&Self>, new: &Self) -> String {
        let Some(old) = old else {
            return format!("found, {}", serde_name(&new.state));
        };
        let mut changes = vec![];
        if old.number != new.number {
            changes.push(format!("now PR #{}", new.number));
        }
        if old.state != new.state {
            changes.push(serde_name(&new.state));
        }
        if old.draft != new.draft {
            changes.push(if new.draft { "draft" } else { "ready" }.into());
        }
        if old.head != new.head {
            changes.push(format!("new head {}", short_sha(&new.head)));
        }
        if old.merge_state != new.merge_state {
            changes.push(format!("merge state {}", serde_name(&new.merge_state)));
        }
        if old.checks != new.checks {
            changes.push(format!("checks {}", serde_name(&new.checks)));
        }
        if old.unresolved_threads != new.unresolved_threads {
            changes.push(format!("{} unresolved threads", new.unresolved_threads));
        }
        if old.comments != new.comments {
            changes.push("new comments".into());
        }
        if changes.is_empty() {
            changes.push("details".into());
        }
        changes.join(", ")
    }
}
/// A unit enum variant's serialized name, such as `has_hooks`.
fn serde_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
/// The first seven characters of a commit id.
pub fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}
/// `text` with its first letter in upper case, such as "Squash" for "squash".
pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or(String::new(), |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}
/// GitHub's `MergeStateStatus`. GitHub computes it lazily and reports `Unknown` meanwhile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeState {
    Clean,
    Behind,
    Blocked,
    /// Conflicts with the base.
    Dirty,
    Unstable,
    HasHooks,
    #[serde(other)]
    Unknown,
}
/// The head commit's check rollup; `None` when the commit has no checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    None,
    Pending,
    Success,
    Failure,
}
/// The newest comment id of each kind. The kinds' ids are not ordered against each other.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentCursors {
    pub issue: Option<u64>,
    pub review: Option<u64>,
    /// The newest reply inside any review thread.
    pub inline: Option<u64>,
}
/// A repository's latest PR check. Kept in memory only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestCheck {
    pub repository_id: String,
    /// The last successful check.
    pub checked_at: Option<i64>,
    /// Why the last check failed.
    pub error: Option<String>,
    /// False when the base's remote is not on github.com, so nothing is checked.
    pub watched: bool,
    pub next_at: i64,
}
/// Messages the host sends as Wiffletree rather than as the human, and their sender label.
pub fn host_notice(id: &str) -> Option<&'static str> {
    if id.starts_with(PR_NOTICE_PREFIX) {
        Some("Wiffletree PR watcher")
    } else if id.starts_with(KEEPER_NOTICE_PREFIX) {
        Some("Wiffletree PR keeper")
    } else {
        HOST_NOTICE_PREFIXES
            .iter()
            .any(|prefix| id.starts_with(prefix))
            .then_some("Wiffletree")
    }
}
/// The id prefix of the PR watcher's messages.
pub const PR_NOTICE_PREFIX: &str = "pr:";
/// The id prefix of the PR keeper's messages.
pub const KEEPER_NOTICE_PREFIX: &str = "keeper:";
/// The id prefix of a publish request's result message and of its Inbox items. Only the host
/// creates either.
pub const PUBLISH_PREFIX: &str = "publish:";
/// The id prefix of a merge request's result message and of its Inbox item. Only the host
/// creates either.
pub const MERGE_PREFIX: &str = "merge:";
/// The id prefix of the Inbox item for a parked provider permission.
pub const PERMISSION_PREFIX: &str = "permission:";
/// The id prefix of the notices about the processes an agent left running and the ones the
/// host stopped. Only the host creates them.
pub const PROCESS_NOTICE_PREFIX: &str = "processes:";
const VERIFICATION_NOTICE_PREFIX: &str = "verification:";
const WORKTREE_KEPT_PREFIX: &str = "worktree-kept:";
const MIGRATION_PREFIX: &str = "migration:";
/// The id prefix of the brief a coordinator's `create_project` hands the new coordinator. It
/// carries another coordinator's words, never the human's.
pub const PROJECT_BRIEF_PREFIX: &str = "project-brief:";
/// The id prefixes of the host's other messages without a sender: verification results, kept
/// worktrees, migration notices, publish and merge results, process notices and project briefs.
/// Inbox answers (`answer:`) carry the human's words.
const HOST_NOTICE_PREFIXES: [&str; 7] = [
    VERIFICATION_NOTICE_PREFIX,
    WORKTREE_KEPT_PREFIX,
    MIGRATION_PREFIX,
    PUBLISH_PREFIX,
    MERGE_PREFIX,
    PROCESS_NOTICE_PREFIX,
    PROJECT_BRIEF_PREFIX,
];
/// A ticket's latest verification cycle: rounds of concurrent, commit-pinned verifier turns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub cycle: u32,
    /// The level the cycle ran. A cycle recorded before levels existed has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<CycleLevel>,
    pub max_rounds: u32,
    pub outcome: VerificationOutcome,
    pub rounds: Vec<VerificationRound>,
    /// When a host from before builds were gated queued the cycle for a verification slot. Its
    /// first round's verifiers were chosen but sent nothing; the host now starts it at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_at: Option<i64>,
    /// The coordinator asked for small work, so a queued cycle's verifiers resolve their
    /// small-work lines when it starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub small: bool,
    /// Started by the host for the PR keeper.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keeper: bool,
    /// Verifiers the cycle left out as it was created: their row says skip and their first
    /// choice was spent or not signed in. They start no session and never block the cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedVerifier>,
}
/// A verifier a cycle went ahead without, and why, such as "Claude's weekly limit was at 96%;
/// its row says skip".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedVerifier {
    pub role: Role,
    pub focus: String,
    pub reason: String,
}
/// A level's name and kind as they were when a cycle started at it; later edits to the level do
/// not change them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleLevel {
    pub name: String,
    pub kind: LevelKind,
}
impl CycleLevel {
    /// Such as "Full" or "Light re-check".
    pub fn label(&self) -> String {
        match self.kind {
            LevelKind::FullReview => self.name.clone(),
            LevelKind::Recheck => format!("{} re-check", self.name),
        }
    }
}
impl Verification {
    /// A cycle recorded before levels existed ran every configured verifier, so it counts as a
    /// full review.
    pub fn is_full_review(&self) -> bool {
        self.level
            .as_ref()
            .is_none_or(|level| level.kind == LevelKind::FullReview)
    }
    pub fn current_round(&self) -> Option<&VerificationRound> {
        self.rounds.last()
    }
    /// Each verifier's most recent run in the cycle, in configured order.
    pub fn latest_results(&self) -> Vec<&VerifierRun> {
        let mut latest: Vec<&VerifierRun> = Vec::new();
        for run in self.rounds.iter().flat_map(|r| &r.verifiers) {
            match latest.iter_mut().find(|l| l.session_id == run.session_id) {
                Some(slot) => *slot = run,
                None => latest.push(run),
            }
        }
        latest
    }
    /// The commit the last round verified; `accept_ticket` requires HEAD to equal it or to be
    /// patch-equivalent to it.
    pub fn verified_commit(&self) -> Option<&str> {
        self.current_round().map(|r| r.commit.as_str())
    }
    /// The verifiers that did not check: they reported they could not verify, or never reported.
    pub fn unchecked(&self) -> Vec<&str> {
        self.latest_results()
            .into_iter()
            .filter(|run| {
                matches!(
                    run.result,
                    VerifierResult::Blocked | VerifierResult::Pending
                )
            })
            .map(|run| run.focus.as_str())
            .collect()
    }
    /// Such as "Cycle 2 · Light re-check by the PR keeper", or "Cycle 1" for a cycle recorded
    /// without a level.
    pub fn cycle_label(&self, noun: &str) -> String {
        let by = if self.keeper { " by the PR keeper" } else { "" };
        match &self.level {
            Some(level) => format!("{noun} {} · {}{by}", self.cycle, level.label()),
            None => format!("{noun} {}{by}", self.cycle),
        }
    }
    /// The verifiers whose latest result stands, such as "Tests, Correctness".
    fn focuses(&self) -> String {
        let focuses: Vec<&str> = self
            .latest_results()
            .iter()
            .map(|run| run.focus.as_str())
            .collect();
        focuses.join(", ")
    }
    /// The skipped verifiers as a line ends, such as " · Claude skipped", or nothing.
    fn skipped_suffix(&self) -> String {
        let focuses: Vec<&str> = self.skipped.iter().map(|s| s.focus.as_str()).collect();
        match focuses.is_empty() {
            true => String::new(),
            false => format!(" · {} skipped", focuses.join(", ")),
        }
    }
    /// A passed cycle with the commit it verified, such as "Cycle 1 · Full passed at 9e03c1b ·
    /// Tests, Correctness, Regressions", and any verifiers it skipped.
    pub fn passed_line(&self) -> String {
        format!(
            "{} passed at {} · {}{}",
            self.cycle_label("Cycle"),
            short_sha(self.verified_commit().unwrap_or_default()),
            self.focuses(),
            self.skipped_suffix()
        )
    }
    /// How the review ended for `commit`, such as "Cycle 2 · Light re-check passed at this
    /// commit · Correctness" or "Cycle 2 · Full ended blocked at c81f04d, after 3 of 3 rounds ·
    /// Tests passed, Correctness failed".
    pub fn review_line(&self, commit: &str) -> String {
        let verified = self.verified_commit().unwrap_or_default();
        let results = self.latest_results();
        let cycle = self.cycle_label("Cycle");
        match self.outcome {
            VerificationOutcome::Passed => {
                let at = if verified == commit {
                    "at this commit".to_owned()
                } else {
                    format!(
                        "at {}, which has the same changes as this commit",
                        short_sha(verified)
                    )
                };
                format!(
                    "{cycle} passed {at} · {}{}",
                    self.focuses(),
                    self.skipped_suffix()
                )
            }
            _ => {
                let results: Vec<String> = results
                    .iter()
                    .map(|run| match run.result {
                        VerifierResult::Blocked => format!("{} could not check", run.focus),
                        VerifierResult::Pending => format!("{} never reported", run.focus),
                        result => format!("{} {}", run.focus, result.label()),
                    })
                    .collect();
                format!(
                    "{cycle} ended blocked at {}, after {} of {} rounds · {}",
                    short_sha(verified),
                    self.rounds.len(),
                    self.max_rounds,
                    results.join(", ")
                )
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Running,
    Passed,
    Blocked,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRound {
    pub round: u32,
    /// The commit every verifier of the round checks.
    pub commit: String,
    pub verifiers: Vec<VerifierRun>,
}
/// One verifier's part in a round. Its run is the `provider_runs` row for `session_id`
/// whose `messages` contain `message_id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierRun {
    pub session_id: String,
    pub role: Role,
    pub focus: String,
    pub message_id: String,
    pub result: VerifierResult,
    /// The verifier's report message, once it reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    /// Set by the host when it overrides the reported result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierResult {
    Pending,
    Passed,
    Failed,
    Blocked,
}
impl VerifierResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocking,
    NonBlocking,
    /// A bug that predates the change under review.
    PreExisting,
}
impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blocking => "blocking",
            Self::NonBlocking => "non-blocking",
            Self::PreExisting => "pre-existing",
        }
    }
}
/// One verifier finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    /// `path:line`.
    pub location: String,
    pub summary: String,
    /// What concretely makes the defect happen.
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub evidence: String,
}
impl Finding {
    /// Only an evidenced blocking finding can fail a verifier's check: severity `blocking`, a
    /// location ending in `:<line>`, and a trigger and evidence.
    pub fn is_evidenced_blocking(&self) -> bool {
        let located = self.location.rsplit_once(':').is_some_and(|(path, line)| {
            !path.trim().is_empty() && !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit())
        });
        self.severity == Severity::Blocking
            && located
            && !self.trigger.trim().is_empty()
            && !self.evidence.trim().is_empty()
    }
}
/// A finding as a verifier reports it; `id` reports a ledger entry again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedFinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub finding: Finding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// An evidenced blocking finding routed to the implementer.
    Open,
    /// Its verifier's later check no longer found it.
    Fixed,
    /// Waiting for the coordinator's decision.
    Untriaged,
    /// A non-blocking or pre-existing finding that needs no decision.
    Noted,
    FixNow,
    FollowUp,
    WontFix,
}
impl EntryStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Fixed => "fixed",
            Self::Untriaged => "untriaged",
            Self::Noted => "noted",
            Self::FixNow => "fix now",
            Self::FollowUp => "follow-up",
            Self::WontFix => "won't fix",
        }
    }
    /// Decided by the coordinator not to be fixed in this ticket.
    pub fn is_settled(self) -> bool {
        matches!(self, Self::FollowUp | Self::WontFix)
    }
}
/// One finding in a ticket's decision ledger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// `F1`, `F2`, … in the ticket.
    pub id: String,
    #[serde(flatten)]
    pub finding: Finding,
    /// The verifier focus, cycle and round that last reported it.
    pub focus: String,
    pub cycle: u32,
    pub round: u32,
    pub status: EntryStatus,
    /// The coordinator's one-line reason for its decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
impl LedgerEntry {
    /// The finding's id, severity, verifier and location, such as "F4 · blocking · Correctness
    /// · src/sidebar.rs:412". Its summary follows on the card.
    pub fn heading(&self) -> String {
        format!(
            "{} · {} · {} · {}",
            self.id,
            self.finding.severity.label(),
            self.focus,
            self.finding.location
        )
    }
}
/// Why a leftover worktree may or may not be removed; earlier classes take precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftoverClass {
    InTurn,
    Locked,
    Dirty,
    Detached,
    Unpushed,
    UntrackedByWiffletree,
    Clean,
}
impl LeftoverClass {
    /// Only clean and unpushed worktrees may be removed, and only when the human selects them.
    pub fn is_removable(self) -> bool {
        matches!(self, Self::Clean | Self::Unpushed)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::InTurn => "Agent in its turn",
            Self::Locked => "Locked",
            Self::Dirty => "Uncommitted changes",
            Self::Detached => "Not on its branch",
            Self::Unpushed => "Unpushed commits; branch is kept",
            Self::UntrackedByWiffletree => "Not recorded by any ticket",
            Self::Clean => "Clean",
        }
    }
}
/// A worktree left by a ticket finished before worktrees were removed on close and accept, or
/// a Wiffletree-made worktree no ticket records (`ticket_id` is then `None`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftoverWorktree {
    pub project_id: Option<String>,
    pub project: String,
    pub repository: String,
    pub ticket_id: Option<String>,
    pub title: String,
    pub state: String,
    pub worktree: String,
    pub branch: String,
    pub head: String,
    pub class: LeftoverClass,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftoverRemoval {
    pub ticket_id: String,
    pub removed: bool,
    /// Why it was kept, or `None` once removed.
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionRuntime {
    pub session_id: String,
    pub provider_session_id: Option<String>,
    pub profile: Option<ModelProfile>,
    pub ticket_id: Option<String>,
    /// What this agent covers when others share its role on the ticket, such as "Codex correctness".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    pub workdir: Option<String>,
    /// Where the provider last ran this session; its conversation is resumable only from there.
    #[serde(default)]
    pub directory: Option<String>,
    pub last_error: Option<String>,
    /// How `profile` was chosen; `None` for sessions pinned before model selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    /// A turn was held (no model, or waiting for a reset) and its queued input is retained.
    /// Cleared when Skip cancels that input or a turn starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// Pinned while every choice of its job was spent: its turns wait until its provider is
    /// not. Cleared when a turn starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub waiting: bool,
    /// Its last turn failed on its provider's usage limit: the held input is retried by itself
    /// once the limit resets. Cleared by that retry, Retry, Skip or the next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_retry: Option<LimitRetry>,
    /// Who stopped the session, while it stays paused or interrupted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<Stopper>,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    /// The person's request to compact this session's context, until it has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Compaction>,
    /// How the last compaction ended, until it is dismissed or another is requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_compaction: Option<CompactionOutcome>,
}

/// A turn that failed on a usage limit and waits for it to reset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitRetry {
    pub failed_at: i64,
    /// When the provider said the limit resets; `None` waits for its next reading.
    pub resets_at: Option<i64>,
}

/// A request to replace a session's conversation with the provider's summary of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compaction {
    /// What the summary should keep, on one line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub requested_at: i64,
    pub state: CompactionState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionState {
    /// Waiting for the session's turn to end, or for the session to run again.
    Queued,
    Running,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionOutcome {
    pub result: CompactionResult,
    /// The provider's own words for an outcome that changed nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub at: i64,
}
/// The longest reason the host builds from a provider's own text, in characters. The Context
/// card cuts every reason it shows at the same length.
pub const REASON_LIMIT: usize = 200;

/// The first line of `text`, cut at `limit` characters and ended with "…" when it was.
pub fn first_line(text: &str, limit: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > limit {
        format!("{}…", line.chars().take(limit).collect::<String>())
    } else {
        line.to_owned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionResult {
    Compacted,
    NothingToCompact,
    Failed,
    /// The host stopped while it ran; it is never run again.
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stopper {
    /// An emergency stop: the session resumes only when the human says so.
    Human,
    Parent,
}
impl Stopper {
    pub fn label(self) -> &'static str {
        match self {
            Self::Human => "the human",
            Self::Parent => "its parent",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    DiscoverRepositories {
        folders: Vec<String>,
    },
    /// Adds repositories to the workspace; a project with a custom selection also gains them.
    /// `roots` are parent folders to keep checking for new repositories; `dismissed` are
    /// repositories found in them that the user chose not to add.
    ImportRepositories {
        project_id: Option<String>,
        paths: Vec<String>,
        #[serde(default)]
        roots: Vec<String>,
        #[serde(default)]
        dismissed: Vec<String>,
    },
    /// Adds repositories that appeared in the root folders since the last check.
    RefreshRepositories,
    RemoveRepositoryRoot {
        path: String,
    },
    /// `None` lets the project use every workspace repository.
    SetProjectRepositories {
        project_id: String,
        repository_ids: Option<Vec<String>>,
    },
    RemoveRepository {
        repository_id: String,
    },
    Snapshot,
    /// Read-only: the history a `Snapshot` leaves out; answered with `Archive`.
    Archive,
    SetLive {
        project_id: String,
        enabled: bool,
    },
    ConfigureSession {
        session_id: String,
        profile: ModelProfile,
    },
    ReconcileSession {
        session_id: String,
        retry: bool,
    },
    /// Queues a compaction of a Claude session's context; only the human sends it.
    CompactContext {
        session_id: String,
        instructions: Option<String>,
    },
    /// Withdraws a compaction that has not started.
    CancelCompaction {
        session_id: String,
    },
    /// Clears the notice the last compaction left.
    DismissCompaction {
        session_id: String,
    },
    Quotas,
    /// Reads Codex's account limits, records them and answers with every reading. Claude's
    /// arrive with its turns.
    RefreshCodexQuota,
    /// What a provider's CLI says about itself on the host: a `ProviderReport`.
    InspectProvider {
        provider: Provider,
    },
    /// How full a session's context window is, measured with `profile`: a `ContextUsage`.
    MeasureContext {
        session_id: String,
        profile: ModelProfile,
    },
    /// Whether each file can be attached to a message: the `Attachment` it would be, or why not.
    InspectAttachments {
        paths: Vec<PathBuf>,
    },
    Usage {
        days: u16,
        project_id: Option<String>,
        provider: Option<Provider>,
        timezone: String,
    },
    /// `coordinator_id` must be a project coordinator whose project uses `repository_id`.
    CreateTicket {
        coordinator_id: String,
        repository_id: String,
        title: String,
        brief: String,
    },
    /// The human assigns an agent; `profile` is never limited by the Who runs what table.
    AssignTicket {
        ticket_id: String,
        role: Role,
        profile: ModelProfile,
        instruction: String,
        /// Tells apart agents sharing a role on one ticket, such as two reviewers.
        #[serde(default)]
        focus: Option<String>,
    },
    /// Creates a project in `space_id`, or in the Space last used for a new project.
    CreateProject {
        name: String,
        #[serde(default)]
        space_id: Option<SpaceId>,
    },
    RenameProject {
        project_id: String,
        name: String,
    },
    /// Moves the projects, kept in their relative order, to just before `before`, or to the end
    /// of the order.
    MoveProjects {
        project_ids: Vec<String>,
        #[serde(default)]
        before: Option<String>,
    },
    /// Adds one repository with an explicit base, updating the base if it is already known.
    AttachRepository {
        project_id: Option<String>,
        path: String,
        base: String,
    },
    CreateSession {
        project_id: String,
        parent_id: String,
        repository_id: Option<String>,
        name: String,
        role: Role,
        profile: ModelProfile,
    },
    SetStatus {
        session_id: String,
        status: Status,
    },
    /// Archives or restores a session together with the sessions it owns.
    SetArchived {
        session_id: String,
        archived: bool,
    },
    Send {
        id: String,
        sender: Option<String>,
        recipient: String,
        body: String,
        /// Files the human attached; the host stores copies of them with the message.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<PathBuf>,
        /// Uploads a remote device made with `UploadAttachment`, attached as files would be.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        uploads: Vec<String>,
    },
    Messages {
        session_id: String,
        before: Option<i64>,
        limit: usize,
    },
    /// One run's steps changed after `after`: `run_id` names the run, or `None` for the
    /// session's latest. Answered with a `StepPage`.
    Steps {
        session_id: String,
        run_id: Option<String>,
        after: u64,
    },
    /// The files a ticket agent touched, or every agent's on a coordinator's open tickets;
    /// answered with `Vec<TicketFiles>`. `whole_tree` also lists each worktree's files.
    Files {
        session_id: String,
        #[serde(default)]
        whole_tree: bool,
    },
    /// The progress entries changed after `after`; `0` asks for the whole board. Answered
    /// with a `ProgressPage`.
    Progress {
        after: u64,
    },
    /// Duration and step counts for up to a page of the session's runs.
    RunSummaries {
        session_id: String,
        run_ids: Vec<String>,
    },
    /// Saves the Who runs what table, the switch threshold and the guide together; answered with
    /// the saved `ModelSelection` at its next revision. The saved one-time notes are kept.
    SetModelSelection {
        selection: Box<ModelSelection>,
        /// The verifiers to save with it: the first save of a suggested setup sends them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verification: Option<VerificationSettings>,
    },
    /// Clears the Models page's one-time notes; answered with the `ModelSelection`.
    DismissModelNotes,
    ResolveAttention {
        id: String,
        answer: String,
    },
    /// Clears a question from the inbox without answering or waking its asker.
    DismissAttention {
        id: String,
    },
    AppendLog {
        input: LogInput,
    },
    Logs {
        project_id: String,
        before: Option<i64>,
        limit: usize,
    },
    /// Creates a Space with local storage and a local tracker, or, with `storage`, stored in
    /// that private GitHub repository (`owner/name` or its URL), whose tracker it shares; without
    /// `prefix`, a new tracker gets the one proposed from the Space's name.
    CreateSpace {
        name: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        instructions: String,
        #[serde(default)]
        prefix: Option<String>,
        #[serde(default)]
        storage: Option<String>,
    },
    /// Read-only: checks a GitHub repository as a Space's storage and lists the Spaces it holds
    /// and its tracker's prefix; answers with `StorageCheck`. Writes nothing.
    CheckStorageRepository {
        repository: String,
    },
    /// Adds the ticked Spaces of a storage repository on GitHub to this Mac; answers with the
    /// Spaces added, as `SpaceRecord`s.
    JoinSpaces {
        repository: String,
        spaces: Vec<JoinSpace>,
    },
    /// Read-only, the human's: what the Space page shows about the Space's storage repository;
    /// answered with `StorageStatus`.
    SpaceStorage {
        space_id: SpaceId,
    },
    /// From the Space's page: where its GitHub brain is cloned on this Mac, or else its brain as
    /// the local folder `path`, for every project in it.
    SetSpaceBrain {
        space_id: SpaceId,
        path: String,
    },
    /// Changes the prefix of the tracker a Space's storage repository is: a local tracker's at
    /// any time, rewriting its issue IDs, and a GitHub tracker's only while it has no issue.
    SetTrackerPrefix {
        space_id: SpaceId,
        prefix: String,
    },
    /// Adds `wiffletree-issues.yaml` to a Space's GitHub storage repository that has none.
    SetUpIssues {
        space_id: SpaceId,
        prefix: String,
    },
    /// Runs a pass of the Space's tracker now, or of every tracker without `space_id`.
    SyncIssues {
        #[serde(default)]
        space_id: Option<SpaceId>,
    },
    /// Read-only: the issues of the Space's tracker, or of every tracker on this Mac without
    /// `space_id`, as `IssueListing`s ordered by tracker and ID; with `search`, only those
    /// whose ID, title, description or comments hold each of its words.
    Issues {
        #[serde(default)]
        space_id: Option<SpaceId>,
        #[serde(default)]
        search: Option<String>,
    },
    /// Read-only: one issue with its comments and events, as an `IssueRecord`.
    Issue {
        id: IssueId,
    },
    /// Files an issue in the Space's tracker, numbered at once in a local tracker and with a
    /// temporary ID until it is pushed to a GitHub one; answered with the `Issue`.
    CreateIssue {
        space_id: SpaceId,
        issue: NewIssue,
    },
    /// Replaces an issue's fields; its ID, state, author and creation stay as they are, and a
    /// changed priority or assignee is recorded as an event. Answered with the `Issue`.
    EditIssue {
        issue: Box<Issue>,
    },
    /// Adds the human's comment to an issue; answered with the `Comment`.
    Comment {
        id: IssueId,
        body: String,
    },
    /// Moves an issue to `state`, recorded as an event; answered with the `Issue`.
    MoveIssue {
        id: IssueId,
        state: IssueState,
    },
    /// Marks an issue read on this Mac as it is now; nothing is written to its tracker.
    MarkIssueRead {
        id: IssueId,
    },
    /// Read-only: what Start project from issue offers for the issue, starting in `space_id`
    /// when it shares the issue's tracker, else in the one of them last used for a new project;
    /// answered with `IssueProjectDefaults`.
    IssueProjectDefaults {
        id: IssueId,
        #[serde(default)]
        space_id: Option<SpaceId>,
    },
    /// Creates a project and its coordinator in `space_id`, one of the Spaces sharing the
    /// issue's tracker, with the issue's repositories on this Mac, or the Space's when it lists
    /// none; links it to the issue, sends the coordinator the issue as untrusted context, and
    /// records in the tracker that the human started work. Answered with `CreatedProject`.
    StartProjectFromIssue {
        id: IssueId,
        space_id: SpaceId,
    },
    /// Links an issue to a project on this Mac, or to one of its tickets; answered with the
    /// `Issue` as its state now follows the linked work.
    LinkIssue {
        id: IssueId,
        project_id: String,
        #[serde(default)]
        ticket_id: Option<String>,
    },
    /// Removes a link; the issue keeps its state. Answered with the remaining `IssueLink`s.
    UnlinkIssue {
        id: IssueId,
        project_id: String,
        #[serde(default)]
        ticket_id: Option<String>,
    },
    /// Replaces a Space's definition; its id and storage stay as they are.
    UpdateSpace {
        space: Box<Space>,
    },
    /// Removes a Space with no open project, moving its archived projects to Personal.
    RemoveSpace {
        space_id: SpaceId,
    },
    /// Moves a project to another Space: its later entries go to that Space's brain.
    MoveProjectToSpace {
        project_id: String,
        space_id: SpaceId,
    },
    /// The Space New project starts in, `space_id` or else the one last used for a new project,
    /// with its repositories on this Mac; answered with `SpaceRepositories`.
    NewProjectDefaults {
        #[serde(default)]
        space_id: Option<SpaceId>,
    },
    /// Puts the repository named `owner/name` in exactly `space_ids`, editing each Space's list
    /// of repositories; none puts it under No Space.
    SetRepositorySpaces {
        repository: String,
        space_ids: Vec<SpaceId>,
    },
    /// The Space to suggest for a repository being added, from its GitHub owner; answered with
    /// an `Option<SpaceId>`, where `None` suggests No Space.
    SuggestRepositorySpace {
        repository: String,
    },
    /// The upgrade's suggested Spaces for the repositories it found, until the human confirms
    /// them; answered with `Vec<RepositorySuggestion>`.
    RepositorySuggestions,
    /// Applies the human's choices for the upgrade's suggestions and stops offering them.
    ConfirmRepositorySuggestions {
        choices: Vec<RepositorySpaces>,
    },
    /// The brains this Mac's Spaces name and their batches; answered with `BrainsPage`.
    Brains,
    /// The brain to pre-fill for a Space with these code repositories (`owner/name`): the one
    /// another Space names for their GitHub owner, or none. Answered with an `Option<String>`.
    SuggestSpaceBrain {
        repositories: Vec<String>,
    },
    /// Allows the host to push batch branches to the brain `owner/name`, open pull requests from
    /// them into `main` and turn on auto-merge for them.
    ConfirmBrain {
        repository: String,
    },
    /// Forgets a brain no Space names any longer and deletes the host's checkout of it. GitHub
    /// is untouched.
    ForgetBrain {
        repository: String,
    },
    /// Queues the batch that adds `wiffletree-brain.json`, with this host as compiler, and the
    /// gate's caller, config, allow-list and `CODEOWNERS` lines; answered with the `BrainBatch`.
    SetUpBrain {
        repository: String,
    },
    /// Queues a batch that changes only the compiler in `wiffletree-brain.json`, keeping the
    /// keys this host does not know; answered with the `BrainBatch`.
    SetBrainCompiler {
        repository: String,
        compiler: BrainCompiler,
    },
    /// Amends or discards a batch whose pull request is open and unmerged; answered with the
    /// `BrainBatch`.
    AmendBrainBatch {
        batch_id: String,
        amendment: BrainAmendment,
    },
    /// Send now: closes every open log batch of the brain `owner/name`, or of every brain, so
    /// each opens its pull request on the brain's next pass; answered with how many closed.
    SendLogsNow {
        repository: Option<String>,
    },
    /// Import legacy logs: queues the brain's legacy `logs/<repo>.md` entries as entry files in
    /// log-only batches; answered with the `LegacyImport`.
    ImportLegacyLogs {
        repository: String,
    },
    /// Replaces a held entry's text with the human's fix, which must then fit the entry
    /// contract and the cap; answered with the `WorkLog`.
    FixHeldLog {
        log_id: String,
        fix: LogFix,
    },
    /// Releases a held entry: it stays in the project's memory and is never sent; answered with
    /// the `WorkLog`.
    ReleaseHeldLog {
        log_id: String,
    },
    /// A page of the brain `owner/name`'s Logs, newest first, filtered, and in the project's
    /// scope when `project_id` names one; answered with `BrainLogPage`.
    BrainLogs {
        repository: String,
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        filter: BrainLogFilter,
        #[serde(default)]
        from: BrainLogsFrom,
    },
    /// The brain's note index tree, in the project's scope when given; answered with
    /// `BrainNoteTree`.
    BrainNotes {
        repository: String,
        #[serde(default)]
        project_id: Option<String>,
    },
    /// One note with its links, backlinks and sources; answered with `BrainNote`.
    BrainNote {
        repository: String,
        path: String,
    },
    /// The open conflicts of `reconcile-queue.md`, in the project's scope when given; answered
    /// with `Vec<BrainConflict>`.
    BrainConflicts {
        repository: String,
        #[serde(default)]
        project_id: Option<String>,
    },
    /// STATUS.md, the last compile and the pending entries; answered with `BrainStatus`.
    BrainStatus {
        repository: String,
    },
    /// The first page of notes and entries matching `query`, decisions first; answered with
    /// `Vec<BrainHit>`.
    SearchBrain {
        repository: String,
        #[serde(default)]
        project_id: Option<String>,
        query: String,
    },
    Activity {
        project_id: String,
        before: Option<i64>,
        limit: usize,
    },
    GitHistory {
        repository_id: String,
        skip: usize,
        limit: usize,
    },
    /// Answered with `HostSettings`.
    Settings,
    /// Checks the folder can be created and written to, saves it and answers with
    /// `HostSettings`. Only projects and tickets created afterwards use it.
    SetWorkspacesDir {
        path: String,
    },
    /// Validates and saves the verification setting; answers with `HostSettings`.
    SetVerification {
        verification: VerificationSettings,
    },
    /// Validates and saves "Builds at once"; answers with `HostSettings`.
    SetBuildsAtOnce {
        builds: usize,
    },
    /// Validates and saves both check-in thresholds; answers with `HostSettings`.
    SetCheckIns {
        child_turns: u32,
        human_turns: u32,
    },
    /// Allows an owner, or a single fork as `owner/name`, to have PRs opened without asking, or
    /// stops allowing it; answers with `HostSettings`. Only the human's client sends it.
    SetOwnerAllowed {
        entry: String,
        allowed: bool,
    },
    /// Validates and saves the PR keeper's settings; answers with `HostSettings`. Only the
    /// human's client sends it.
    SetKeeper {
        keeper: KeeperSettings,
    },
    /// Saves the human's own rules for a skill, an empty text removing them, and refuses more
    /// than `MAX_SKILL_TEXT` characters; answers with the `SkillState`. Only the human's client
    /// sends it.
    SaveSkillRules {
        skill: Skill,
        rules: String,
    },
    /// Returns a skill to its stock text with no rules of the human's own; answers with the
    /// `SkillState`. Only the human's client sends it.
    ResetSkill {
        skill: Skill,
    },
    /// Makes the state a history entry holds current, as a new entry; answers with the
    /// `SkillState`. Only the human's client sends it.
    RestoreSkill {
        skill: Skill,
        entry: String,
    },
    /// Asks GitHub, read-only, about a repository named as `owner/name` or by its `github.com`
    /// URL, and checks this machine can read it; answers with `SettingsRepositoryCheck`. A
    /// public repository is refused. Writes nothing.
    CheckSettingsRepository {
        repository: String,
    },
    /// Adds this machine's shared settings to a repository that has none, in one pushed commit,
    /// and makes the shared folder its clone; answers with `HostSettings`. Only the human's
    /// client sends it.
    SetUpSettingsRepository {
        repository: String,
    },
    /// Joins a repository that holds shared settings, keeping `with`'s; answers with
    /// `HostSettings`. Only the human's client sends it, once the human confirms.
    JoinSettingsRepository {
        repository: String,
        with: JoinWith,
    },
    /// Runs a sync pass with the settings repository at once; answers with nothing. Only the
    /// human's client sends it.
    SyncNow,
    /// Stops syncing and forgets the settings repository chosen, keeping the shared folder as
    /// it is and changing nothing on GitHub; answers with `HostSettings`. Only the human's client
    /// sends it, once the human confirms.
    StopSettingsSync,
    /// Settles the paused file, or a skill's folder, a workspace note names; answers with
    /// nothing. Only the human's client sends it.
    SettlePausedFile {
        path: String,
        settle: Settle,
        /// The storage repository the note names; `None` for the settings repository.
        #[serde(default)]
        repository: Option<String>,
    },
    /// Read-only: this machine's version of a paused file beside the other one, line by line,
    /// the other being the last synced version for an edit made outside Wiffletree. For a
    /// skill, its rewrite or else its rules. Answers with lines as `SkillStockChanges` does.
    ComparePausedFile {
        path: String,
        #[serde(default)]
        repository: Option<String>,
    },
    /// Read-only: a skill's history, newest first, each entry with why it could not be read
    /// when it cannot.
    SkillHistory {
        skill: Skill,
    },
    /// Replaces the human's rules with a rewrite of the stock text, starting as the stock text
    /// and the rules, based on the stock text; answers with the `SkillState`. Only the human's
    /// client sends it.
    StartSkillRewrite {
        skill: Skill,
    },
    /// Saves a rewrite, an empty one included, and refuses more than `MAX_SKILL_TEXT`
    /// characters; answers with the `SkillState`. Only the human's client sends it.
    SaveSkillRewrite {
        skill: Skill,
        text: String,
    },
    /// Bases a rewrite on this release's stock text, changing nothing else; answers with the
    /// `SkillState`. Only the human's client sends it.
    MarkSkillUpToDate {
        skill: Skill,
    },
    /// Writes back exactly the rewrite and base from before the merge the "Merged" history entry
    /// `entry` names, as a new entry; answers with the `SkillState`. Only the human's client
    /// sends it.
    UndoSkillMerge {
        skill: Skill,
        entry: String,
    },
    /// Merges this release's stock text into an out-of-date rewrite, or tries again after a
    /// failed try; passages that overlap go to the PR keeper's model. Refused while a question
    /// about the rewrite waits. Answers with the `SkillState`. Only the human's client sends it.
    MergeSkillUpdate {
        skill: Skill,
    },
    /// Merges this release's stock text into an out-of-date rewrite with the human's wording
    /// for each passage that overlaps, in order, or with a question waiting, for each passage in
    /// question and the model's text for the rest; answers with the `SkillState`. Only the
    /// human's client sends it.
    AnswerSkillQuestion {
        skill: Skill,
        wordings: Vec<Wording>,
    },
    /// Hides the note of the merge `entry` names on this machine; answers with the `SkillState`.
    /// Only the human's client sends it.
    DismissSkillMerge {
        skill: Skill,
        entry: String,
    },
    /// Read-only: the rewrite before and after the merge `entry` names, line by line, each
    /// change with where it came from.
    SkillMergeReview {
        skill: Skill,
        entry: String,
    },
    /// Read-only: what changed, line by line, from a rewrite's base to this release's stock text.
    SkillStockChanges {
        skill: Skill,
    },
    /// Saves or, with none, removes a repository's checks command; answers with the
    /// `Repository`. Only the human's client sends it.
    SetRepositoryChecks {
        repository_id: String,
        checks: Option<RepositoryChecks>,
    },
    /// Changes a repository's base in place once it resolves to a commit; answers with the
    /// `Repository`. Only the human's client sends it.
    SetRepositoryBase {
        repository_id: String,
        base: String,
    },
    /// Adds an entry to a repository's protected branches, refusing it with the reason, or
    /// removes one; answers with the `Repository`. Only the human's client sends it: a removed
    /// entry is a branch the host may then merge into.
    SetProtectedBranches {
        repository_id: String,
        /// One entry, added to the host's current list or removed from it, so a client whose
        /// list is stale changes nothing else.
        entry: String,
        protected: bool,
    },
    /// Read-only: the branches on a repository's `origin`, as last fetched, for the protected
    /// branches each pattern matches.
    OriginBranches {
        repository_id: String,
    },
    /// Read-only listing of leftover worktrees, answered with `Vec<LeftoverWorktree>`.
    LeftoverWorktrees,
    /// Removes the worktrees of the tickets the human selected from the listing, re-checking
    /// each first; answered with `Vec<LeftoverRemoval>`.
    RemoveLeftoverWorktrees {
        ticket_ids: Vec<String>,
    },
    /// What the agent has left running, or for a coordinator every agent of its project;
    /// answered with `Vec<AgentProcesses>`. Groups that have exited are forgotten.
    Processes {
        session_id: String,
    },
    /// Stops these groups of the Processes panel of `session_id`, or with none named every
    /// group it lists, as the human's stop; answered as `Processes`. The agents keep working.
    StopProcesses {
        session_id: String,
        #[serde(default)]
        groups: Vec<String>,
    },
    /// Socket only: turns the connection into a subscriber of the host's events, after the
    /// events following `after` when it is still in the host's log, or after `resync`
    /// otherwise; answered with `Subscribed`. With no cursor only later events come.
    Subscribe {
        #[serde(default)]
        after: Option<Cursor>,
        /// Only these kinds of event, and `resync`; every kind when left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kinds: Option<Vec<EventKindName>>,
    },
    /// Socket only: answered with nothing by the connection itself, not the host's actor, so a
    /// client can tell a quiet host from one that is gone while the actor is busy.
    Ping,
    /// Which `references` name a file, each as it is or, when relative, under the first of
    /// `bases` that holds it; a `~/` reference is under the host's home folder. Answered with
    /// one `Option<ResolvedPath>` per reference, in order. Reads no file's contents.
    ResolvePaths {
        references: Vec<String>,
        bases: Vec<PathBuf>,
    },
    /// Reads a file under the roots `docs/host-protocol.md` lists, refusing one larger than
    /// `max_bytes` or `MAX_ATTACHMENT_BYTES`; answered with `FileContents`.
    ReadFile {
        path: PathBuf,
        max_bytes: u64,
    },
    /// Names the folders in `path` and which are Git repositories; answered with
    /// `FolderListing`. A leading `~` is the host's home folder, except for a View only device.
    ListFolder {
        path: PathBuf,
    },
    /// Read-only: the fixed core instructions the running host gives agents, each with its
    /// text; answered with `Vec<CoreText>`.
    CoreInstructions,
    /// Stops every running turn as a quit does, then ends the host; answered once the turns are
    /// stopped. Only the human's client sends it.
    StopAgentsAndQuit,
    /// Ends the host so the app can start the newest installed version, at once or once no turn
    /// and no compaction runs; the only command a connection may send after a refused `Hello`.
    RestartHost {
        when: RestartWhen,
    },
    /// This Mac's remote access: the switch, its name and addresses, pairing mode with its code,
    /// and the paired devices; answered with `RemoteAccess`. Only the host itself sends it.
    RemoteAccess,
    /// Turns "Let other devices use this Mac's Wiffletree" on or off; answered with
    /// `RemoteAccess`. Off closes the listener and every remote connection; paired devices stay.
    SetRemoteAccess {
        enabled: bool,
    },
    /// Opens pairing mode with a new 4-digit code for 2 minutes; answered with `RemoteAccess`.
    OpenPairing,
    /// Closes pairing mode; answered with `RemoteAccess`.
    CancelPairing,
    /// Changes what a paired device may do and closes its connections; answered with
    /// `RemoteAccess`.
    SetDeviceLevel {
        device_id: String,
        level: AccessLevel,
    },
    /// Forgets a paired device and closes its connections; answered with `RemoteAccess`.
    RemoveDevice {
        device_id: String,
    },
    /// Stores a piece of a file a remote device attaches, at `offset`, from the base64 `bytes`
    /// of at most `UPLOAD_PIECE_BYTES`. A piece is stored only where the upload ends, so a piece
    /// sent again changes nothing; answered with `Uploaded`, where the next piece starts.
    UploadAttachment {
        upload_id: String,
        /// The file's name, which the stored attachment keeps.
        name: String,
        offset: u64,
        bytes: String,
    },
}
impl Command {
    /// Reads change nothing in the store, so the host signals no change for them and clients
    /// reload nothing after one. Every command is listed, so a new one must be classified.
    pub fn is_read(&self) -> bool {
        match self {
            Self::Snapshot
            | Self::Archive
            | Self::DiscoverRepositories { .. }
            | Self::Quotas
            | Self::InspectProvider { .. }
            | Self::MeasureContext { .. }
            | Self::InspectAttachments { .. }
            | Self::Usage { .. }
            | Self::Messages { .. }
            | Self::Steps { .. }
            | Self::Files { .. }
            | Self::Progress { .. }
            | Self::RunSummaries { .. }
            | Self::Logs { .. }
            | Self::Activity { .. }
            | Self::GitHistory { .. }
            | Self::Settings
            | Self::CheckSettingsRepository { .. }
            | Self::CheckStorageRepository { .. }
            | Self::ComparePausedFile { .. }
            | Self::SkillHistory { .. }
            | Self::SkillMergeReview { .. }
            | Self::SkillStockChanges { .. }
            | Self::LeftoverWorktrees
            | Self::Processes { .. }
            | Self::Subscribe { .. }
            | Self::Ping
            | Self::ResolvePaths { .. }
            | Self::ReadFile { .. }
            | Self::ListFolder { .. }
            | Self::CoreInstructions
            | Self::NewProjectDefaults { .. }
            | Self::SuggestRepositorySpace { .. }
            | Self::RepositorySuggestions
            | Self::SpaceStorage { .. }
            | Self::Brains
            | Self::BrainLogs { .. }
            | Self::BrainNotes { .. }
            | Self::BrainNote { .. }
            | Self::BrainConflicts { .. }
            | Self::BrainStatus { .. }
            | Self::SearchBrain { .. }
            | Self::SuggestSpaceBrain { .. }
            | Self::OriginBranches { .. }
            | Self::Issues { .. }
            | Self::Issue { .. }
            | Self::IssueProjectDefaults { .. }
            | Self::RemoteAccess => true,
            Self::ImportRepositories { .. }
            | Self::RefreshRepositories
            | Self::RemoveRepositoryRoot { .. }
            | Self::SetProjectRepositories { .. }
            | Self::RemoveRepository { .. }
            | Self::SetLive { .. }
            | Self::ConfigureSession { .. }
            | Self::ReconcileSession { .. }
            | Self::CompactContext { .. }
            | Self::CancelCompaction { .. }
            | Self::DismissCompaction { .. }
            | Self::RefreshCodexQuota
            | Self::CreateTicket { .. }
            | Self::AssignTicket { .. }
            | Self::CreateProject { .. }
            | Self::RenameProject { .. }
            | Self::MoveProjects { .. }
            | Self::AttachRepository { .. }
            | Self::CreateSession { .. }
            | Self::SetStatus { .. }
            | Self::SetArchived { .. }
            | Self::Send { .. }
            | Self::SetModelSelection { .. }
            | Self::DismissModelNotes
            | Self::ResolveAttention { .. }
            | Self::DismissAttention { .. }
            | Self::AppendLog { .. }
            | Self::SetSpaceBrain { .. }
            | Self::CreateSpace { .. }
            | Self::UpdateSpace { .. }
            | Self::RemoveSpace { .. }
            | Self::MoveProjectToSpace { .. }
            | Self::SetRepositorySpaces { .. }
            | Self::ConfirmRepositorySuggestions { .. }
            | Self::JoinSpaces { .. }
            | Self::SetTrackerPrefix { .. }
            | Self::SetUpIssues { .. }
            | Self::ConfirmBrain { .. }
            | Self::ForgetBrain { .. }
            | Self::SetUpBrain { .. }
            | Self::SetBrainCompiler { .. }
            | Self::AmendBrainBatch { .. }
            | Self::SendLogsNow { .. }
            | Self::ImportLegacyLogs { .. }
            | Self::FixHeldLog { .. }
            | Self::ReleaseHeldLog { .. }
            | Self::SyncIssues { .. }
            | Self::CreateIssue { .. }
            | Self::EditIssue { .. }
            | Self::Comment { .. }
            | Self::MoveIssue { .. }
            | Self::MarkIssueRead { .. }
            | Self::StartProjectFromIssue { .. }
            | Self::LinkIssue { .. }
            | Self::UnlinkIssue { .. }
            | Self::SetWorkspacesDir { .. }
            | Self::SetVerification { .. }
            | Self::SetBuildsAtOnce { .. }
            | Self::SetCheckIns { .. }
            | Self::SetOwnerAllowed { .. }
            | Self::SetKeeper { .. }
            | Self::SaveSkillRules { .. }
            | Self::ResetSkill { .. }
            | Self::RestoreSkill { .. }
            | Self::SetUpSettingsRepository { .. }
            | Self::JoinSettingsRepository { .. }
            | Self::SyncNow
            | Self::StopSettingsSync
            | Self::SettlePausedFile { .. }
            | Self::StartSkillRewrite { .. }
            | Self::SaveSkillRewrite { .. }
            | Self::MarkSkillUpToDate { .. }
            | Self::UndoSkillMerge { .. }
            | Self::MergeSkillUpdate { .. }
            | Self::AnswerSkillQuestion { .. }
            | Self::DismissSkillMerge { .. }
            | Self::SetRepositoryChecks { .. }
            | Self::SetRepositoryBase { .. }
            | Self::SetProtectedBranches { .. }
            | Self::RemoveLeftoverWorktrees { .. }
            | Self::StopProcesses { .. }
            | Self::StopAgentsAndQuit
            | Self::RestartHost { .. }
            | Self::SetRemoteAccess { .. }
            | Self::OpenPairing
            | Self::CancelPairing
            | Self::SetDeviceLevel { .. }
            | Self::RemoveDevice { .. }
            | Self::UploadAttachment { .. } => false,
        }
    }
    /// Commands only the human should give: on the host socket they need the app's client
    /// credential. They send a message as the human, answer the Inbox, which also approves a
    /// publish, or grant publish or push authority without it: owners whose PRs open without
    /// asking, the PR keeper's rebases and pushes, the protected branches, whose removed entries
    /// the host may merge into, and a settings repository the host pushes to.
    /// Spaces are the human's: creating, editing or removing one, setting its brain, moving a
    /// project into one and choosing which repositories it lists, and reading where its storage
    /// is on this Mac. So are brains: the push grant, forgetting one, Set up, the compiler,
    /// amending a batch, Send now and the legacy import, and fixing or releasing a held entry.
    /// Every write to a tracker is the human's; reading issues is not. Remote access is the
    /// human's too: its state carries the pairing code.
    pub fn is_human_only(&self) -> bool {
        matches!(
            self,
            Self::Send { sender: None, .. }
                | Self::ResolveAttention { .. }
                | Self::DismissAttention { .. }
                | Self::SetOwnerAllowed { .. }
                | Self::SetKeeper { .. }
                | Self::SetProtectedBranches { .. }
                | Self::SetUpSettingsRepository { .. }
                | Self::JoinSettingsRepository { .. }
                | Self::CreateSpace { .. }
                | Self::UpdateSpace { .. }
                | Self::RemoveSpace { .. }
                | Self::MoveProjectToSpace { .. }
                | Self::SetRepositorySpaces { .. }
                | Self::ConfirmRepositorySuggestions { .. }
                | Self::JoinSpaces { .. }
                | Self::SetTrackerPrefix { .. }
                | Self::SetUpIssues { .. }
                | Self::CreateIssue { .. }
                | Self::EditIssue { .. }
                | Self::Comment { .. }
                | Self::MoveIssue { .. }
                | Self::MarkIssueRead { .. }
                | Self::StartProjectFromIssue { .. }
                | Self::LinkIssue { .. }
                | Self::UnlinkIssue { .. }
                | Self::SetSpaceBrain { .. }
                | Self::SpaceStorage { .. }
                | Self::ConfirmBrain { .. }
                | Self::ForgetBrain { .. }
                | Self::SetUpBrain { .. }
                | Self::SetBrainCompiler { .. }
                | Self::AmendBrainBatch { .. }
                | Self::SendLogsNow { .. }
                | Self::ImportLegacyLogs { .. }
                | Self::FixHeldLog { .. }
                | Self::ReleaseHeldLog { .. }
                | Self::RemoteAccess
                | Self::SetRemoteAccess { .. }
                | Self::OpenPairing
                | Self::CancelPairing
                | Self::SetDeviceLevel { .. }
                | Self::RemoveDevice { .. }
        )
    }
    /// Remote access, pairing and devices, which only the host itself may read or change.
    pub fn is_remote_access(&self) -> bool {
        matches!(
            self,
            Self::RemoteAccess
                | Self::SetRemoteAccess { .. }
                | Self::OpenPairing
                | Self::CancelPairing
                | Self::SetDeviceLevel { .. }
                | Self::RemoveDevice { .. }
        )
    }
    /// The capability a host lists when it answers this command.
    pub fn capability(&self) -> Option<&'static str> {
        match self {
            Self::Subscribe { .. } => Some(capability::EVENTS),
            Self::ResolvePaths { .. } | Self::ReadFile { .. } => Some(capability::FILES),
            Self::ListFolder { .. } => Some(capability::FOLDERS),
            Self::RestartHost {
                when: RestartWhen::Idle,
            } => Some(capability::RESTART_WHEN_IDLE),
            Self::UploadAttachment { .. } => Some(capability::UPLOADS),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    /// Milliseconds from the host reading the request to writing this reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ms: Option<u64>,
}
