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
pub fn compare_paused_file(path: String, repository: Option<String>) -> Ask<Vec<Line>> {
    Ask::new(Command::ComparePausedFile { path, repository })
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
pub fn behavior_catalog() -> Ask<Vec<cascade::Behavior>> {
    Ask::new(Command::BehaviorCatalog)
}
pub fn layer_values(scope: cascade::Scope) -> Ask<cascade::LayerValues> {
    Ask::new(Command::LayerValues { scope })
}
pub fn set_layer_values(
    scope: cascade::Scope,
    values: cascade::LayerValues,
) -> Ask<cascade::LayerValues> {
    Ask::new(Command::SetLayerValues { scope, values })
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
/// A project in `space_id`, or else in the Space last used for a new project.
pub fn create_project(name: String, space_id: Option<SpaceId>) -> Ask<Project> {
    Ask::new(Command::CreateProject { name, space_id })
}
pub fn create_space(
    name: String,
    description: String,
    additions: String,
    prefix: Option<String>,
    storage: Option<String>,
    setup: SpaceSetup,
) -> Ask<SpaceRecord> {
    Ask::new(Command::CreateSpace {
        name,
        description,
        additions,
        prefix,
        storage,
        setup,
    })
}
pub fn space_form_defaults() -> Ask<SpaceFormDefaults> {
    Ask::new(Command::SpaceFormDefaults)
}
pub fn protect_history(repository: String) -> Ask<()> {
    Ask::new(Command::ProtectHistory { repository })
}
pub fn preview_bring_in(space_id: SpaceId, from: String, folder: String) -> Ask<BringInPlan> {
    Ask::new(Command::PreviewBringIn {
        space_id,
        from,
        folder,
    })
}
pub fn bring_in(space_id: SpaceId, from: String, folder: String) -> Ask<BringInPlan> {
    Ask::new(Command::BringIn {
        space_id,
        from,
        folder,
    })
}
pub fn update_space(space: Space) -> Ask<SpaceRecord> {
    Ask::new(Command::UpdateSpace {
        space: Box::new(space),
    })
}
pub fn set_followed_roots(space_id: SpaceId, roots: Vec<String>) -> Ask<SpaceRecord> {
    Ask::new(Command::SetFollowedRoots { space_id, roots })
}
pub fn check_storage_repository(repository: String) -> Ask<StorageCheck> {
    Ask::new(Command::CheckStorageRepository { repository })
}
pub fn join_spaces(
    repository: String,
    spaces: Vec<JoinSpace>,
    compile_here: bool,
) -> Ask<Vec<SpaceRecord>> {
    Ask::new(Command::JoinSpaces {
        repository,
        spaces,
        compile_here,
    })
}
pub fn space_storage(space_id: SpaceId) -> Ask<StorageStatus> {
    Ask::new(Command::SpaceStorage { space_id })
}
pub fn set_space_models(
    space_id: SpaceId,
    settings: cascade::SpaceModelSettings,
) -> Ask<cascade::SpaceModelSettings> {
    Ask::new(Command::SetSpaceModels { space_id, settings })
}
pub fn effective_settings(
    project_id: String,
    ticket_id: Option<String>,
) -> Ask<cascade::EffectiveSettings> {
    Ask::new(Command::EffectiveSettings {
        project_id,
        ticket_id,
    })
}
pub fn brains() -> Ask<BrainsPage> {
    Ask::new(Command::Brains)
}
pub fn brain_logs(
    repository: String,
    project_id: Option<String>,
    filter: BrainLogFilter,
    from: BrainLogsFrom,
) -> Ask<BrainLogPage> {
    Ask::new(Command::BrainLogs {
        repository,
        project_id,
        filter,
        from,
    })
}
pub fn brain_notes(repository: String, project_id: Option<String>) -> Ask<BrainNoteTree> {
    Ask::new(Command::BrainNotes {
        repository,
        project_id,
    })
}
pub fn brain_note(repository: String, path: String) -> Ask<BrainNote> {
    Ask::new(Command::BrainNote { repository, path })
}
pub fn brain_conflicts(repository: String, project_id: Option<String>) -> Ask<Vec<BrainConflict>> {
    Ask::new(Command::BrainConflicts {
        repository,
        project_id,
    })
}
pub fn run_brain_compile(repository: String) -> Ask<CompileRun> {
    Ask::new(Command::RunBrainCompile { repository })
}
pub fn held_entry_text(repository: String, path: String) -> Ask<String> {
    Ask::new(Command::HeldEntryText { repository, path })
}
pub fn brain_compile_runs(repository: String, offset: usize, limit: usize) -> Ask<CompileRunPage> {
    Ask::new(Command::BrainCompileRuns {
        repository,
        offset,
        limit,
    })
}
pub fn rule_conflict(
    repository: String,
    conflict_id: String,
    ruling: Ruling,
) -> Ask<Option<ConflictCard>> {
    Ask::new(Command::RuleConflict {
        repository,
        conflict_id,
        ruling,
    })
}
pub fn brain_status(repository: String) -> Ask<BrainStatus> {
    Ask::new(Command::BrainStatus { repository })
}
pub fn search_brain(
    repository: String,
    project_id: Option<String>,
    query: String,
) -> Ask<Vec<BrainHit>> {
    Ask::new(Command::SearchBrain {
        repository,
        project_id,
        query,
    })
}
pub fn set_tracker_prefix(space_id: SpaceId, prefix: String) -> Ask<TrackerConfig> {
    Ask::new(Command::SetTrackerPrefix { space_id, prefix })
}
pub fn set_up_issues(space_id: SpaceId, prefix: String) -> Ask<TrackerConfig> {
    Ask::new(Command::SetUpIssues { space_id, prefix })
}
pub fn move_into_folder(space_id: SpaceId) -> Ask<()> {
    Ask::new(Command::MoveIntoFolder { space_id })
}
pub fn sync_issues(space_id: Option<SpaceId>) -> Ask<()> {
    Ask::new(Command::SyncIssues { space_id })
}
pub fn issues(space_id: Option<SpaceId>, search: Option<String>) -> Ask<Vec<IssueListing>> {
    Ask::new(Command::Issues { space_id, search })
}
pub fn issue(id: IssueId) -> Ask<IssueRecord> {
    Ask::new(Command::Issue { id })
}
pub fn create_issue(space_id: SpaceId, issue: NewIssue) -> Ask<Issue> {
    Ask::new(Command::CreateIssue { space_id, issue })
}
pub fn edit_issue(issue: Issue) -> Ask<Issue> {
    Ask::new(Command::EditIssue {
        issue: Box::new(issue),
    })
}
pub fn comment_on_issue(id: IssueId, body: String) -> Ask<Comment> {
    Ask::new(Command::Comment { id, body })
}
pub fn move_issue(id: IssueId, state: IssueState) -> Ask<Issue> {
    Ask::new(Command::MoveIssue { id, state })
}
pub fn mark_issue_read(id: IssueId) -> Ask<()> {
    Ask::new(Command::MarkIssueRead { id })
}
pub fn follow_issue(id: IssueId, follow: bool) -> Ask<()> {
    Ask::new(Command::FollowIssue { id, follow })
}
pub fn issue_people(space_id: SpaceId) -> Ask<Vec<IssuePerson>> {
    Ask::new(Command::IssuePeople { space_id })
}
pub fn linear_teams(space_id: SpaceId, key: LinearKey) -> Ask<Vec<LinearTeam>> {
    Ask::new(Command::LinearTeams { space_id, key })
}
pub fn preview_linear_import(space_id: SpaceId, team_id: String) -> Ask<LinearImportPlan> {
    Ask::new(Command::PreviewLinearImport { space_id, team_id })
}
pub fn import_from_linear(space_id: SpaceId, logins: BTreeMap<String, String>) -> Ask<()> {
    Ask::new(Command::ImportFromLinear { space_id, logins })
}
pub fn cancel_linear_import(space_id: SpaceId) -> Ask<()> {
    Ask::new(Command::CancelLinearImport { space_id })
}
pub fn issue_project_defaults(id: IssueId, space_id: Option<SpaceId>) -> Ask<IssueProjectDefaults> {
    Ask::new(Command::IssueProjectDefaults { id, space_id })
}
pub fn new_project_defaults(space_id: Option<SpaceId>) -> Ask<SpaceRepositories> {
    Ask::new(Command::NewProjectDefaults { space_id })
}
/// `None` suggests No Space.
pub fn suggest_repository_space(repository: String) -> Ask<Option<SpaceId>> {
    Ask::new(Command::SuggestRepositorySpace { repository })
}
pub fn repository_suggestions() -> Ask<Vec<RepositorySuggestion>> {
    Ask::new(Command::RepositorySuggestions)
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
pub fn set_builds_at_once(builds: usize) -> Ask<HostSettings> {
    Ask::new(Command::SetBuildsAtOnce { builds })
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
pub fn list_folder(path: PathBuf) -> Ask<FolderListing> {
    Ask::new(Command::ListFolder { path })
}
pub fn core_instructions() -> Ask<Vec<crate::skills::CoreText>> {
    Ask::new(Command::CoreInstructions)
}
pub fn start_thread(
    space_id: SpaceId,
    body: String,
    profile: Option<ModelProfile>,
) -> Ask<assistant::Thread> {
    Ask::new(Command::StartThread {
        id: new_id(),
        space_id,
        body,
        attachments: vec![],
        profile,
    })
}
pub fn rename_thread(thread_id: String, title: String) -> Ask<assistant::Thread> {
    Ask::new(Command::RenameThread { thread_id, title })
}
pub fn set_thread_open(thread_id: String, open: bool) -> Ask<assistant::Thread> {
    Ask::new(Command::SetThreadOpen { thread_id, open })
}
pub fn order_threads(thread_ids: Vec<String>) -> Ask<()> {
    Ask::new(Command::OrderThreads { thread_ids })
}
pub fn delete_thread(thread_id: String) -> Ask<()> {
    Ask::new(Command::DeleteThread { thread_id })
}
pub fn search_threads(query: String, space_id: Option<SpaceId>) -> Ask<Vec<assistant::ThreadHit>> {
    Ask::new(Command::SearchThreads { query, space_id })
}
pub fn draft_project_brief(thread_id: String) -> Ask<Message> {
    Ask::new(Command::DraftProjectBrief { thread_id })
}
pub fn make_project(
    thread_id: String,
    name: String,
    brief: String,
    files: Vec<String>,
) -> Ask<Project> {
    Ask::new(Command::MakeProject {
        thread_id,
        name,
        brief,
        files,
    })
}
pub fn scratch_files(thread_id: String) -> Ask<Vec<String>> {
    Ask::new(Command::ScratchFiles { thread_id })
}
pub fn dismiss_thread_card(thread_id: String, card_id: String) -> Ask<assistant::Thread> {
    Ask::new(Command::DismissThreadCard { thread_id, card_id })
}
pub fn open_issue_thread(
    attached: assistant::proposal::Attached,
    space_id: Option<SpaceId>,
) -> Ask<assistant::Thread> {
    Ask::new(Command::OpenIssueThread { attached, space_id })
}
pub fn proposals(thread_id: String) -> Ask<Vec<assistant::proposal::ProposalView>> {
    Ask::new(Command::Proposals { thread_id })
}
pub fn apply_proposal(
    thread_id: String,
    proposal_id: String,
    edited: Option<assistant::proposal::ProposalChange>,
) -> Ask<assistant::Thread> {
    Ask::new(Command::ApplyProposal {
        thread_id,
        proposal_id,
        edited: edited.map(Box::new),
    })
}
pub fn discard_proposal(thread_id: String, proposal_id: String) -> Ask<assistant::Thread> {
    Ask::new(Command::DiscardProposal {
        thread_id,
        proposal_id,
    })
}
/// Answered with the name as saved.
pub fn set_assistant_name(name: String) -> Ask<String> {
    Ask::new(Command::SetAssistantName { name })
}
pub fn set_router_threshold(threshold: f64) -> Ask<HostSettings> {
    Ask::new(Command::SetRouterThreshold { threshold })
}
pub fn route_message(id: String, thread_id: Option<String>, body: String) -> Ask<()> {
    Ask::new(Command::RouteMessage {
        id,
        thread_id,
        body,
    })
}
pub fn answer_held(id: String, destination: assistant::router::Destination) -> Ask<()> {
    Ask::new(Command::AnswerHeld { id, destination })
}
pub fn change_route(
    thread_id: String,
    message_id: String,
    destination: assistant::router::Destination,
) -> Ask<()> {
    Ask::new(Command::ChangeRoute {
        thread_id,
        message_id,
        destination,
    })
}
/// With `new`, from a recipe card, refused when a job of that name exists.
pub fn save_job(recipe: job::Recipe, new: bool) -> Ask<job::JobState> {
    Ask::new(Command::SaveJob {
        recipe: Box::new(recipe),
        new,
    })
}
pub fn preview_job(recipe: job::Recipe) -> Ask<job::RecipePreview> {
    Ask::new(Command::PreviewJob {
        recipe: Box::new(recipe),
    })
}
pub fn set_job_enabled(space_id: SpaceId, job_id: String, enabled: bool) -> Ask<job::JobState> {
    Ask::new(Command::SetJobEnabled {
        space_id,
        job_id,
        enabled,
    })
}
pub fn run_job_now(job: job::JobKey) -> Ask<job::JobRun> {
    Ask::new(Command::RunJobNow { job })
}
pub fn delete_job(space_id: SpaceId, job_id: String) -> Ask<()> {
    Ask::new(Command::DeleteJob { space_id, job_id })
}
pub fn stop_job_run(run_id: String) -> Ask<Value> {
    Ask::new(Command::StopJobRun { run_id })
}
pub fn see_job_runs(job: job::JobKey) -> Ask<()> {
    Ask::new(Command::SeeJobRuns { job })
}
pub fn take_over_job(space_id: SpaceId, job_id: String) -> Ask<job::JobState> {
    Ask::new(Command::TakeOverJob { space_id, job_id })
}
/// Share with `share`, else Keep private.
pub fn approve_job_output(run_id: String, title: String, share: bool) -> Ask<()> {
    Ask::new(Command::ApproveJobOutput {
        run_id,
        title,
        share,
    })
}
pub fn boards() -> Ask<Vec<board::BoardOutput>> {
    Ask::new(Command::Boards)
}
pub fn board_versions(board_id: String) -> Ask<Vec<board::BoardVersion>> {
    Ask::new(Command::BoardVersions { board_id })
}
pub fn board_version_folder(board_id: String, commit: String) -> Ask<String> {
    Ask::new(Command::BoardVersionFolder { board_id, commit })
}
pub fn share_board(board_id: String, space_id: SpaceId) -> Ask<board::BoardOutput> {
    Ask::new(Command::ShareBoard { board_id, space_id })
}
