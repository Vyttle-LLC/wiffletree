//! Provider-neutral contracts. No renderer, database or provider runtime dependencies.
mod usage;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
            Self::ProjectOrchestrator => "Main coordinator",
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Fixed,
    Automatic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    Small,
    Standard,
    Complex,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RolePolicy {
    pub role: Role,
    pub mode: RoutingMode,
    pub default: ModelProfile,
    pub allowed: Vec<ModelProfile>,
    pub small: ModelProfile,
    pub standard: ModelProfile,
    pub complex: ModelProfile,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_profiles: Vec<ProviderProfiles>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProfiles {
    pub provider: Provider,
    pub big: ModelProfile,
    pub small: ModelProfile,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDefault {
    pub role: Role,
    pub default_provider: Provider,
    pub profiles: Vec<ProviderProfiles>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelCapability {
    pub provider: Provider,
    pub model: String,
    pub efforts: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    pub profile: ModelProfile,
    pub reason: String,
    pub catalog_verified: bool,
}
impl RolePolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.allowed.is_empty() && self.allowed.len() <= 32,
            "Allowlist must contain 1–32 profiles"
        );
        let unique: BTreeSet<_> = self.allowed.iter().collect();
        ensure!(
            unique.len() == self.allowed.len(),
            "Duplicate allowed profile"
        );
        for profile in [&self.default, &self.small, &self.standard, &self.complex] {
            ensure!(
                self.allowed.contains(profile),
                "Every routing tier and default must be allowed"
            );
        }
        for profile in &self.allowed {
            ensure!(
                !profile.model.trim().is_empty() && profile.model.len() <= 128,
                "Invalid model ID"
            );
            ensure!(
                !profile.effort.trim().is_empty() && profile.effort.len() <= 32,
                "Invalid effort"
            );
        }
        if !self.provider_profiles.is_empty() {
            ensure!(
                self.provider_profiles.len() == 2,
                "Configure Claude and Codex profiles"
            );
            let mut approved = BTreeSet::new();
            for provider in [Provider::Claude, Provider::Codex] {
                let entries: Vec<_> = self
                    .provider_profiles
                    .iter()
                    .filter(|p| p.provider == provider)
                    .collect();
                ensure!(entries.len() == 1, "Configure each provider exactly once");
                for profile in [&entries[0].big, &entries[0].small] {
                    ensure!(profile.provider == provider, "Profile provider mismatch");
                    approved.insert(profile);
                }
            }
            ensure!(
                approved == unique,
                "Only configured Big/Small profiles may be allowed"
            );
        }
        Ok(())
    }
    pub fn select(
        &self,
        complexity: Complexity,
        proposal: Option<&ModelProfile>,
        session_provider: Option<Provider>,
        catalog: Option<&[ModelCapability]>,
    ) -> Result<Route> {
        self.validate()?;
        let (profile, reason) = match self.mode {
            RoutingMode::Fixed => (&self.default, "Fixed role default".to_owned()),
            RoutingMode::Automatic => {
                if let Some(proposed) = proposal {
                    ensure!(
                        self.allowed.contains(proposed),
                        "Orchestrator selection is outside the role allowlist"
                    );
                    (
                        proposed,
                        "Orchestrator selection within role allowlist".to_owned(),
                    )
                } else if !self.provider_profiles.is_empty() {
                    let provider = session_provider.unwrap_or(self.default.provider);
                    let profiles = self
                        .provider_profiles
                        .iter()
                        .find(|p| p.provider == provider)
                        .context("Provider has no configured profiles")?;
                    let small = complexity == Complexity::Small;
                    (
                        if small {
                            &profiles.small
                        } else {
                            &profiles.big
                        },
                        format!(
                            "Configured {provider:?} {} profile",
                            if small { "Small" } else { "Big" }
                        ),
                    )
                } else {
                    (
                        match complexity {
                            Complexity::Small => &self.small,
                            Complexity::Standard => &self.standard,
                            Complexity::Complex => &self.complex,
                        },
                        format!("Configured {complexity:?} work tier"),
                    )
                }
            }
        };
        if let Some(provider) = session_provider {
            ensure!(
                profile.provider == provider,
                "Provider change requires a replacement session"
            );
        }
        if let Some(catalog) = catalog {
            ensure!(
                catalog.iter().any(|m| m.provider == profile.provider
                    && m.model == profile.model
                    && m.efforts.contains(&profile.effort)),
                "Runtime catalog does not support selected model/effort"
            );
        }
        Ok(Route {
            profile: profile.clone(),
            reason,
            catalog_verified: catalog.is_some(),
        })
    }
}

pub fn default_policies() -> Vec<RolePolicy> {
    let sol = ModelProfile {
        provider: Provider::Codex,
        model: "gpt-6.1-sol".into(),
        effort: "medium".into(),
    };
    let small = ModelProfile {
        effort: "low".into(),
        ..sol.clone()
    };
    let astra = ModelProfile {
        provider: Provider::Codex,
        model: "gpt-6-astra".into(),
        effort: "high".into(),
    };
    Role::ALL
        .into_iter()
        .map(|role| {
            let mut policy = RolePolicy {
                role,
                mode: RoutingMode::Fixed,
                default: sol.clone(),
                allowed: vec![small.clone(), sol.clone(), astra.clone()],
                small: small.clone(),
                standard: sol.clone(),
                complex: astra.clone(),
                provider_profiles: Vec::new(),
            };
            if matches!(
                role,
                Role::ProjectOrchestrator | Role::TaskOrchestrator | Role::Implementer
            ) {
                let opus = ModelProfile {
                    provider: Provider::Claude,
                    model: "opus".into(),
                    effort: "medium".into(),
                };
                policy.default = ModelProfile {
                    effort: if role == Role::Implementer {
                        "medium"
                    } else {
                        "high"
                    }
                    .into(),
                    ..opus.clone()
                };
                policy.small = ModelProfile {
                    effort: "low".into(),
                    ..opus.clone()
                };
                policy.standard = opus.clone();
                policy.complex = ModelProfile {
                    effort: "high".into(),
                    ..opus
                };
                policy.allowed.extend([
                    policy.small.clone(),
                    policy.standard.clone(),
                    policy.complex.clone(),
                ]);
            }
            policy
        })
        .collect()
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
    pub policies: Vec<RolePolicy>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub coordinator_id: String,
    pub title: String,
    pub brief: String,
    pub worktree: String,
    pub branch: String,
    pub state: String,
}
impl Ticket {
    /// Accepted and closed tickets take no further agents or state changes.
    pub fn is_open(&self) -> bool {
        !matches!(self.state.as_str(), "accepted" | "closed")
    }
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
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
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
    CreateTicket {
        coordinator_id: String,
        title: String,
        brief: String,
    },
    AssignTicket {
        ticket_id: String,
        role: Role,
        provider: Provider,
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
        provider: Provider,
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
    },
    Messages {
        session_id: String,
        before: Option<i64>,
        limit: usize,
    },
    Simulate {
        message_id: String,
    },
    SetRoleDefaults {
        defaults: Vec<RoleDefault>,
    },
    SetPolicy {
        policy: Box<RolePolicy>,
    },
    Route {
        role: Role,
        complexity: Complexity,
        proposal: Option<ModelProfile>,
        session_provider: Option<Provider>,
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
