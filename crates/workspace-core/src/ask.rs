//! Commands paired with the type the host answers each with, so a client decodes a reply as
//! what it is and a mismatch fails to compile. Only this module pairs them.
use crate::{skills::Line, *};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{marker::PhantomData, path::PathBuf};

/// A command whose reply decodes as `R`.
pub struct Ask<R> {
    command: Command,
    reply: PhantomData<fn() -> R>,
}
impl<R> Ask<R> {
    fn new(command: Command) -> Self {
        Self {
            command,
            reply: PhantomData,
        }
    }
    pub fn command(&self) -> &Command {
        &self.command
    }
}
impl<R: DeserializeOwned> Ask<R> {
    /// The host's reply to this command, as what it is.
    pub fn decode(&self, reply: Value) -> Result<R, String> {
        serde_json::from_value(reply).map_err(|e| e.to_string())
    }
}

pub fn snapshot() -> Ask<Snapshot> {
    Ask::new(Command::Snapshot)
}
/// Answered with nothing once the host has stopped its turns.
pub fn stop_agents_and_quit() -> Ask<()> {
    Ask::new(Command::StopAgentsAndQuit)
}
pub fn archive() -> Ask<Archive> {
    Ask::new(Command::Archive)
}
pub fn quotas() -> Ask<Vec<QuotaReading>> {
    Ask::new(Command::Quotas)
}
pub fn refresh_codex_quota() -> Ask<Vec<QuotaReading>> {
    Ask::new(Command::RefreshCodexQuota)
}
pub fn refresh_repositories() -> Ask<RepositoryRefresh> {
    Ask::new(Command::RefreshRepositories)
}
pub fn discover_repositories(folders: Vec<String>) -> Ask<RepositoryDiscovery> {
    Ask::new(Command::DiscoverRepositories { folders })
}
pub fn inspect_provider(provider: Provider) -> Ask<ProviderReport> {
    Ask::new(Command::InspectProvider { provider })
}
pub fn measure_context(session_id: String, profile: ModelProfile) -> Ask<ContextUsage> {
    Ask::new(Command::MeasureContext {
        session_id,
        profile,
    })
}
pub fn inspect_attachments(paths: Vec<PathBuf>) -> Ask<Vec<Result<Attachment, String>>> {
    Ask::new(Command::InspectAttachments { paths })
}
pub fn set_archived(session_id: String, archived: bool) -> Ask<Vec<Session>> {
    Ask::new(Command::SetArchived {
        session_id,
        archived,
    })
}
pub fn messages(session_id: String, before: Option<i64>, limit: usize) -> Ask<Vec<Message>> {
    Ask::new(Command::Messages {
        session_id,
        before,
        limit,
    })
}
pub fn run_summaries(session_id: String, run_ids: Vec<String>) -> Ask<Vec<RunSummary>> {
    Ask::new(Command::RunSummaries {
        session_id,
        run_ids,
    })
}
pub fn steps(session_id: String, run_id: Option<String>, after: u64) -> Ask<StepPage> {
    Ask::new(Command::Steps {
        session_id,
        run_id,
        after,
    })
}
pub fn compare_paused_file(path: String) -> Ask<Vec<Line>> {
    Ask::new(Command::ComparePausedFile { path })
}
pub fn set_model_selection(
    selection: ModelSelection,
    verification: Option<VerificationSettings>,
) -> Ask<ModelSelection> {
    Ask::new(Command::SetModelSelection {
        selection: Box::new(selection),
        verification,
    })
}
pub fn dismiss_model_notes() -> Ask<ModelSelection> {
    Ask::new(Command::DismissModelNotes)
}
pub fn skill_history(skill: Skill) -> Ask<skills::History> {
    Ask::new(Command::SkillHistory { skill })
}
pub fn skill_merge_review(skill: Skill, entry: String) -> Ask<skills::MergeReview> {
    Ask::new(Command::SkillMergeReview { skill, entry })
}
pub fn skill_stock_changes(skill: Skill) -> Ask<Vec<Line>> {
    Ask::new(Command::SkillStockChanges { skill })
}
pub fn files(session_id: String, whole_tree: bool) -> Ask<Vec<TicketFiles>> {
    Ask::new(Command::Files {
        session_id,
        whole_tree,
    })
}
pub fn processes(session_id: String) -> Ask<Vec<AgentProcesses>> {
    Ask::new(Command::Processes { session_id })
}
pub fn stop_processes(session_id: String, groups: Vec<String>) -> Ask<Vec<AgentProcesses>> {
    Ask::new(Command::StopProcesses { session_id, groups })
}
pub fn progress(after: u64) -> Ask<ProgressPage> {
    Ask::new(Command::Progress { after })
}
pub fn logs(project_id: String, before: Option<i64>, limit: usize) -> Ask<Vec<WorkLog>> {
    Ask::new(Command::Logs {
        project_id,
        before,
        limit,
    })
}
pub fn activity(project_id: String, before: Option<i64>, limit: usize) -> Ask<Vec<Activity>> {
    Ask::new(Command::Activity {
        project_id,
        before,
        limit,
    })
}
pub fn git_history(repository_id: String, skip: usize, limit: usize) -> Ask<GitHistory> {
    Ask::new(Command::GitHistory {
        repository_id,
        skip,
        limit,
    })
}
pub fn create_project(name: String) -> Ask<Project> {
    Ask::new(Command::CreateProject { name })
}
pub fn rename_project(project_id: String, name: String) -> Ask<Project> {
    Ask::new(Command::RenameProject { project_id, name })
}
pub fn move_projects(project_ids: Vec<String>, before: Option<String>) -> Ask<Vec<Project>> {
    Ask::new(Command::MoveProjects {
        project_ids,
        before,
    })
}
pub fn settings() -> Ask<HostSettings> {
    Ask::new(Command::Settings)
}
pub fn set_workspaces_dir(path: String) -> Ask<HostSettings> {
    Ask::new(Command::SetWorkspacesDir { path })
}
pub fn remote_access() -> Ask<RemoteAccess> {
    Ask::new(Command::RemoteAccess)
}
pub fn set_remote_access(enabled: bool) -> Ask<RemoteAccess> {
    Ask::new(Command::SetRemoteAccess { enabled })
}
pub fn open_pairing() -> Ask<RemoteAccess> {
    Ask::new(Command::OpenPairing)
}
pub fn cancel_pairing() -> Ask<RemoteAccess> {
    Ask::new(Command::CancelPairing)
}
pub fn set_device_level(device_id: String, level: AccessLevel) -> Ask<RemoteAccess> {
    Ask::new(Command::SetDeviceLevel { device_id, level })
}
pub fn remove_device(device_id: String) -> Ask<RemoteAccess> {
    Ask::new(Command::RemoveDevice { device_id })
}
pub fn upload_attachment(
    upload_id: String,
    name: String,
    offset: u64,
    bytes: String,
) -> Ask<Uploaded> {
    Ask::new(Command::UploadAttachment {
        upload_id,
        name,
        offset,
        bytes,
    })
}
pub fn set_check_ins(child_turns: u32, human_turns: u32) -> Ask<HostSettings> {
    Ask::new(Command::SetCheckIns {
        child_turns,
        human_turns,
    })
}
pub fn set_verification(verification: VerificationSettings) -> Ask<HostSettings> {
    Ask::new(Command::SetVerification { verification })
}
pub fn leftover_worktrees() -> Ask<Vec<LeftoverWorktree>> {
    Ask::new(Command::LeftoverWorktrees)
}
pub fn remove_leftover_worktrees(ticket_ids: Vec<String>) -> Ask<Vec<LeftoverRemoval>> {
    Ask::new(Command::RemoveLeftoverWorktrees { ticket_ids })
}
pub fn usage(
    days: u16,
    project_id: Option<String>,
    provider: Option<Provider>,
    timezone: String,
) -> Ask<UsageReport> {
    Ask::new(Command::Usage {
        days,
        project_id,
        provider,
        timezone,
    })
}
pub fn attach_repository(
    project_id: Option<String>,
    path: String,
    base: String,
) -> Ask<Repository> {
    Ask::new(Command::AttachRepository {
        project_id,
        path,
        base,
    })
}
pub fn resolve_paths(
    references: Vec<String>,
    bases: Vec<PathBuf>,
) -> Ask<Vec<Option<ResolvedPath>>> {
    Ask::new(Command::ResolvePaths { references, bases })
}
pub fn read_file(path: PathBuf, max_bytes: u64) -> Ask<FileContents> {
    Ask::new(Command::ReadFile { path, max_bytes })
}
pub fn core_instructions() -> Ask<Vec<crate::skills::CoreText>> {
    Ask::new(Command::CoreInstructions)
}
