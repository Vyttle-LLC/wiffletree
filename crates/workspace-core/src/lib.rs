//! Provider-neutral contracts. No renderer, database or provider runtime dependencies.
mod selection;
mod steps;
mod usage;
use anyhow::{Result, bail, ensure};
pub use selection::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
pub use steps::*;
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
    pub turn_limit: usize,
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
    pub fn is_permission(&self) -> bool {
        self.operation_id.starts_with("permission:")
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

/// Active turns only: idle coordinators and administrative operations take no slot.
pub struct TurnLimits {
    global: usize,
    active: BTreeMap<String, String>,
}
impl TurnLimits {
    pub fn new(global: usize) -> Result<Self> {
        ensure!(global > 0 && global <= 64, "Invalid global turn limit");
        Ok(Self {
            global,
            active: BTreeMap::new(),
        })
    }
    pub fn acquire(&mut self, session: &str, project: &str, project_limit: usize) -> Result<()> {
        ensure!(
            project_limit > 0 && project_limit <= 64,
            "Invalid project turn limit"
        );
        if self.active.contains_key(session) {
            bail!("Session already has an active turn");
        }
        ensure!(self.active.len() < self.global, "Global turn limit reached");
        ensure!(
            self.active
                .values()
                .filter(|p| p.as_str() == project)
                .count()
                < project_limit,
            "Project turn limit reached"
        );
        self.active.insert(session.into(), project.into());
        Ok(())
    }
    pub fn release(&mut self, session: &str) {
        self.active.remove(session);
    }
    pub fn active(&self) -> usize {
        self.active.len()
    }
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
            "The round cap must be 1–{MAX_VERIFICATION_ROUNDS}"
        );
        ensure!(
            (1..=MAX_VERIFICATION_CYCLES).contains(&self.max_cycles),
            "The cycle cap must be 1–{MAX_VERIFICATION_CYCLES}"
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
}
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
    /// The commit the last round verified; `accept_ticket` requires HEAD to equal it.
    pub fn verified_commit(&self) -> Option<&str> {
        self.current_round().map(|r| r.commit.as_str())
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
