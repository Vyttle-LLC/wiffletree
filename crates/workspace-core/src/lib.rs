//! Provider-neutral contracts. No renderer, database or provider runtime dependencies.
mod files;
mod owners;
mod progress;
mod publishing;
mod selection;
mod steps;
mod suggestion;
mod usage;
use anyhow::{Context, Result, bail, ensure};
pub use files::*;
pub use owners::*;
pub use progress::*;
pub use publishing::*;
pub use selection::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
pub use steps::*;
pub use suggestion::*;
pub use usage::*;

pub const PROTOCOL_VERSION: u32 = 1;
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
    pub brain: Option<String>,
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
    /// Or a publish step waiting for the human, which is approved or denied in the same way.
    pub fn is_permission(&self) -> bool {
        self.operation_id.starts_with(PERMISSION_PREFIX) || self.is_publish()
    }
    /// A step of a ticket's publish request waiting for the human's approval.
    pub fn is_publish(&self) -> bool {
        self.operation_id.starts_with(PUBLISH_PREFIX)
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelCapability {
    pub provider: Provider,
    pub model: String,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Decision,
    Observation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogInput {
    pub session_id: String,
    pub milestone: String,
    pub kind: LogKind,
    pub title: String,
    pub reference: String,
    pub changed: String,
    pub why: String,
    pub state: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLog {
    pub sequence: i64,
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub repository: String,
    pub created_at: i64,
    pub input: LogInput,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaintenanceRun {
    pub id: String,
    pub project_id: String,
    pub outcome: String,
    pub exported: usize,
    pub created_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub projects: Vec<Project>,
    pub repositories: Vec<Repository>,
    pub sessions: Vec<Session>,
    pub attention: Vec<Attention>,
    #[serde(default)]
    pub tickets: Vec<Ticket>,
    #[serde(default)]
    pub runtimes: Vec<SessionRuntime>,
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
}
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
        Ok(())
    }
    /// Whether any trigger is on, so repositories are said to be kept or not.
    pub fn is_on(&self) -> bool {
        self.rebase || self.comments || self.conflicts
    }
}
/// Refuses the conflicts trigger unless the rebase trigger is on, the PR keeper's model is
/// chosen and the review level names a configured re-check level: a conflict is found by the
/// keeper's own rebase, and what the keeper makes is reviewed at that level.
pub fn check_conflicts(
    keeper: &KeeperSettings,
    model: Option<&ModelProfile>,
    verification: &VerificationSettings,
) -> Result<()> {
    if !keeper.conflicts {
        return Ok(());
    }
    let trigger = "\"Try to resolve conflicts\"";
    ensure!(
        keeper.rebase,
        "{trigger} needs \"Rebase when the base branch moves\""
    );
    ensure!(
        model.is_some(),
        "{trigger} needs a PR keeper model: choose one in Models first"
    );
    let rechecks: Vec<&str> = verification
        .levels
        .iter()
        .filter(|l| l.kind == LevelKind::Recheck)
        .map(|l| l.name.as_str())
        .collect();
    ensure!(
        !rechecks.is_empty(),
        "{trigger} needs a re-check level to review with: add one in Models → Review"
    );
    let named = keeper.review_level.as_deref().unwrap_or_default();
    ensure!(
        rechecks.iter().any(|name| same_name(name, named)),
        "{trigger} reviews with a re-check level: choose {}",
        or_list(&rechecks)
    );
    Ok(())
}
/// Refuses a PR keeper model that cannot sort comments: sorting runs with every tool turned
/// off, which only Claude can be started with.
pub fn check_sorting_model(keeper: Option<&ModelProfile>) -> Result<()> {
    let provider = keeper.map(|k| k.provider);
    ensure!(
        provider.is_some(),
        "Sorting comments needs a PR keeper model: choose a Claude model in Models first"
    );
    ensure!(
        provider == Some(Provider::Claude),
        "Sorting comments needs a Claude model: it runs with every tool turned off, and Codex cannot be started that way"
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
        let verifier = |role, focus: &str, instruction: Option<&str>, provider| VerifierConfig {
            role,
            focus: focus.into(),
            instruction: instruction.map(Into::into),
            provider,
        };
        Self::one_level(
            vec![
                verifier(Role::Tester, "Tests", None, None),
                verifier(
                    Role::Reviewer,
                    "Correctness",
                    Some("Correctness against the ticket's acceptance criteria."),
                    Some(Provider::Claude),
                ),
                verifier(
                    Role::Reviewer,
                    "Regressions",
                    Some("Regressions and test coverage."),
                    Some(Provider::Codex),
                ),
            ],
            3,
            default_max_cycles(),
        )
    }
}
/// Names and focuses are compared ignoring surrounding spaces and ASCII case.
fn same_name(a: &str, b: &str) -> bool {
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
                "{} runs \"{missing}\", which is not a configured verifier. Choose its verifiers again in Models → Review.",
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
/// One configured verifier. The coordinator picks its exact model when it calls
/// `verify_ticket`; a configured `provider` limits that choice to the provider's models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierConfig {
    pub role: Role,
    /// Tells verifiers apart on a ticket, such as "Codex" or "Style".
    pub focus: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
}

/// The coordinator's exact model for one configured verifier, named by its focus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierChoice {
    pub focus: String,
    pub profile: ModelProfile,
    pub reason: String,
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
    pub state: String,
    /// The latest verification cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<Verification>,
    /// Earlier cycles, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_cycles: Vec<Verification>,
    /// Every finding verifiers recorded, across all cycles, with the coordinator's decisions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<LedgerEntry>,
    /// Why the coordinator accepted the ticket after a blocked cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<String>,
    /// The ticket branch's GitHub PR, as the PR watcher last read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequest>,
    /// The publish request whose step is running or waiting for the human.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publishing: Option<PublishRequest>,
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
    /// Commits Wiffletree did not make, taken from GitHub's branch; while set, nothing is
    /// pushed or accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreviewed: Option<Unreviewed>,
    #[serde(default)]
    pub comments: KeeperComments,
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
    /// Set when the request opens a pull request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
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
    /// Started by the host for the PR keeper: it never asks, and its result goes to the
    /// keeper's record rather than to an agent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keeper: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishStep {
    Push,
    OpenPullRequest,
}
impl PublishStep {
    pub fn label(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::OpenPullRequest => "pull request",
        }
    }
}
impl PublishRequest {
    /// Whether this request opens a pull request after its push.
    pub fn opens_pull_request(&self) -> bool {
        self.title.is_some()
    }
}
impl Ticket {
    /// Accepted and closed tickets take no further agents or state changes.
    pub fn is_open(&self) -> bool {
        !matches!(self.state.as_str(), "accepted" | "closed")
    }
    /// The verification cycle in progress, if any.
    pub fn running_cycle(&self) -> Option<&Verification> {
        self.verification
            .as_ref()
            .filter(|v| v.outcome == VerificationOutcome::Running)
    }
    /// When the running cycle of this open ticket was queued, while it waits for a slot.
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
    /// Every finding that is neither fixed nor settled as follow-up or won't-fix.
    pub fn open_findings(&self) -> Vec<&LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| e.status != EntryStatus::Fixed && !e.status.is_settled())
            .collect()
    }
    /// The ticket's pull request while it is open; a publish then only pushes to it.
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
/// The id prefix of the line a compaction leaves in its session's conversation; the rest is
/// the run's id. A session's other messages to itself are replies, under `output:` ids.
pub const COMPACTION_LINE_PREFIX: &str = "compaction:";
/// The id prefix of a publish request's result message and of its Inbox items. Only the host
/// creates either.
pub const PUBLISH_PREFIX: &str = "publish:";
/// The id prefix of the Inbox item for a parked provider permission.
pub const PERMISSION_PREFIX: &str = "permission:";
/// The id prefix of the notices about the processes an agent left running and the ones the
/// host stopped. Only the host creates them.
pub const PROCESS_NOTICE_PREFIX: &str = "processes:";
/// The id prefixes of the host's other messages without a sender: verification results, kept
/// worktrees, migration notices, publish results and process notices. Inbox answers (`answer:`)
/// carry the human's words.
const HOST_NOTICE_PREFIXES: [&str; 5] = [
    "verification:",
    "worktree-kept:",
    "migration:",
    PUBLISH_PREFIX,
    PROCESS_NOTICE_PREFIX,
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
    /// When the cycle was queued for a verification slot. Until the host starts it, its first
    /// round's verifiers have been chosen but sent nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_at: Option<i64>,
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
    /// Such as "Cycle 2 · Light re-check", or "Cycle 1" for a cycle recorded without a level.
    pub fn cycle_label(&self, noun: &str) -> String {
        match &self.level {
            Some(level) => format!("{noun} {} · {}", self.cycle, level.label()),
            None => format!("{noun} {}", self.cycle),
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
    /// A passed cycle with the commit it verified, such as "Cycle 1 · Full passed at 9e03c1b ·
    /// Tests, Correctness, Regressions".
    pub fn passed_line(&self) -> String {
        format!(
            "{} passed at {} · {}",
            self.cycle_label("Cycle"),
            short_sha(self.verified_commit().unwrap_or_default()),
            self.focuses()
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
                format!("{cycle} passed {at} · {}", self.focuses())
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
    /// A turn was held (no model, or its provider disabled) and its queued input is retained.
    /// Cleared when Skip cancels that input or a turn starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
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
    ProviderCheck,
    Quotas,
    RecordQuota {
        reading: QuotaReading,
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
    /// The human assigns an agent; `profile` is never limited by the model selection.
    AssignTicket {
        ticket_id: String,
        role: Role,
        profile: ModelProfile,
        instruction: String,
        /// Tells apart agents sharing a role on one ticket, such as two reviewers.
        #[serde(default)]
        focus: Option<String>,
    },
    CreateProject {
        name: String,
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
    Simulate {
        message_id: String,
    },
    /// Saves providers, role providers, reviews and the guide together; answered with the saved
    /// `ModelSelection` at its next revision.
    SetModelSelection {
        selection: Box<ModelSelection>,
        /// The verifiers to save with it: the first save of a suggested setup sends them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verification: Option<VerificationSettings>,
    },
    RequestAttention {
        session_id: String,
        host: String,
        operation_id: String,
        prompt: String,
    },
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
    AttachBrain {
        project_id: String,
        path: String,
    },
    ExportLogs {
        project_id: String,
    },
    Runs {
        project_id: String,
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
    /// Saves or, with none, removes a repository's checks command; answers with the
    /// `Repository`. Only the human's client sends it.
    SetRepositoryChecks {
        repository_id: String,
        checks: Option<RepositoryChecks>,
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
}
