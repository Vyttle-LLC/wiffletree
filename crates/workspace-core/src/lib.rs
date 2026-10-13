//! Provider-neutral contracts. No renderer, database or provider runtime dependencies.
mod files;
mod owners;
mod progress;
mod publishing;
mod selection;
mod steps;
mod suggestion;
mod usage;
use anyhow::{Result, ensure};
pub use files::*;
pub use owners::*;
pub use progress::*;
pub use publishing::*;
pub use selection::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
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
}
impl Role {
    pub const ALL: [Self; 6] = [
        Self::ProjectOrchestrator,
        Self::TaskOrchestrator,
        Self::Implementer,
        Self::Tester,
        Self::Reviewer,
        Self::Maintenance,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectOrchestrator => "Coordinator",
            Self::TaskOrchestrator => "Repository coordinator",
            Self::Implementer => "Implementer",
            Self::Tester => "Tester",
            Self::Reviewer => "Reviewer",
            Self::Maintenance => "Maintenance",
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

/// The highest verification round cap the settings accept.
pub const MAX_VERIFICATION_ROUNDS: u32 = 5;
/// The highest verification cycle cap the settings accept.
pub const MAX_VERIFICATION_CYCLES: u32 = 5;
/// Who verifies a ticket when `verify_ticket` is called, how many rounds a cycle may take and
/// how many cycles a ticket may start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationSettings {
    pub verifiers: Vec<VerifierConfig>,
    pub max_rounds: u32,
    /// Settings saved before the cycle cap existed use the default.
    #[serde(default = "default_max_cycles")]
    pub max_cycles: u32,
}
fn default_max_cycles() -> u32 {
    2
}
impl Default for VerificationSettings {
    /// A tester and a Claude and a Codex review with their lenses, which fit the default turn
    /// limit; three rounds and two cycles.
    fn default() -> Self {
        let verifier = |role, focus: &str, instruction: Option<&str>, provider| VerifierConfig {
            role,
            focus: focus.into(),
            instruction: instruction.map(Into::into),
            provider,
        };
        Self {
            verifiers: vec![
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
            max_rounds: 3,
            max_cycles: default_max_cycles(),
        }
    }
}
impl VerificationSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=MAX_VERIFICATION_ROUNDS).contains(&self.max_rounds),
            "Rounds per review must be 1–{MAX_VERIFICATION_ROUNDS}"
        );
        ensure!(
            (1..=MAX_VERIFICATION_CYCLES).contains(&self.max_cycles),
            "Reviews per ticket must be 1–{MAX_VERIFICATION_CYCLES}"
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
        Ok(())
    }
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
    /// Every finding that is neither fixed nor settled as follow-up or won't-fix.
    pub fn open_findings(&self) -> Vec<&LedgerEntry> {
        self.ledger
            .iter()
            .filter(|e| e.status != EntryStatus::Fixed && !e.status.is_settled())
            .collect()
    }
    /// The ticket's pull request once it was closed or merged; opening another needs the human.
    pub fn ended_pull_request(&self) -> Option<&PullRequest> {
        self.pull_request
            .as_ref()
            .filter(|pr| pr.state != PrState::Open)
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
    } else {
        HOST_NOTICE_PREFIXES
            .iter()
            .any(|prefix| id.starts_with(prefix))
            .then_some("Wiffletree")
    }
}
/// The id prefix of the PR watcher's messages.
pub const PR_NOTICE_PREFIX: &str = "pr:";
/// The id prefix of the line a compaction leaves in its session's conversation; the rest is
/// the run's id. A session's other messages to itself are replies, under `output:` ids.
pub const COMPACTION_LINE_PREFIX: &str = "compaction:";
/// The id prefix of a publish request's result message and of its Inbox items. Only the host
/// creates either.
pub const PUBLISH_PREFIX: &str = "publish:";
/// The id prefix of the Inbox item for a parked provider permission.
pub const PERMISSION_PREFIX: &str = "permission:";
/// The id prefixes of the host's other messages without a sender: verification results, kept
/// worktrees, migration notices and publish results. Inbox answers (`answer:`) carry the human's
/// words.
const HOST_NOTICE_PREFIXES: [&str; 4] = [
    "verification:",
    "worktree-kept:",
    "migration:",
    PUBLISH_PREFIX,
];
/// A ticket's latest verification cycle: rounds of concurrent, commit-pinned verifier turns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub cycle: u32,
    pub max_rounds: u32,
    pub outcome: VerificationOutcome,
    pub rounds: Vec<VerificationRound>,
}
impl Verification {
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
    /// How the review ended for `commit`, such as "Cycle 2 passed at this commit · Tests,
    /// Correctness" or "Cycle 2 ended blocked at c81f04d, after 3 of 3 rounds · Tests passed,
    /// Correctness failed".
    pub fn review_line(&self, commit: &str) -> String {
        let verified = self.verified_commit().unwrap_or_default();
        let results = self.latest_results();
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
                let foci: Vec<&str> = results.iter().map(|run| run.focus.as_str()).collect();
                format!("Cycle {} passed {at} · {}", self.cycle, foci.join(", "))
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
                    "Cycle {} ended blocked at {}, after {} of {} rounds · {}",
                    self.cycle,
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
    /// Read-only listing of leftover worktrees, answered with `Vec<LeftoverWorktree>`.
    LeftoverWorktrees,
    /// Removes the worktrees of the tickets the human selected from the listing, re-checking
    /// each first; answered with `Vec<LeftoverRemoval>`.
    RemoveLeftoverWorktrees {
        ticket_ids: Vec<String>,
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
