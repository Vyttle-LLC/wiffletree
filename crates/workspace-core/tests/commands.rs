use workspace_core::*;

fn text() -> String {
    "x".into()
}

fn profile() -> ModelProfile {
    ModelProfile {
        provider: Provider::Claude,
        model: "opus".into(),
        effort: "high".into(),
    }
}

fn space() -> Space {
    Space {
        id: SpaceId(text()),
        name: text(),
        description: text(),
        instructions: text(),
        brain: None,
        storage: SpaceStorage {
            repository: StorageRepository::Local { id: text() },
            folder: text(),
        },
        repositories: vec![],
        models: SpaceModels::default(),
        sources: vec![],
        extra: Extra::new(),
    }
}

#[test]
fn reads_are_exactly_the_commands_that_change_nothing() {
    let reads = [
        Command::Snapshot,
        Command::Archive,
        Command::DiscoverRepositories { folders: vec![] },
        Command::Quotas,
        Command::InspectProvider {
            provider: Provider::Codex,
        },
        Command::MeasureContext {
            session_id: text(),
            profile: ModelProfile {
                provider: Provider::Claude,
                model: text(),
                effort: text(),
            },
        },
        Command::InspectAttachments { paths: vec![] },
        Command::Usage {
            days: 7,
            project_id: None,
            provider: None,
            timezone: text(),
        },
        Command::Messages {
            session_id: text(),
            before: None,
            limit: 1,
        },
        Command::Steps {
            session_id: text(),
            run_id: None,
            after: 0,
        },
        Command::Files {
            session_id: text(),
            whole_tree: false,
        },
        Command::Progress { after: 0 },
        Command::RunSummaries {
            session_id: text(),
            run_ids: vec![],
        },
        Command::Logs {
            project_id: text(),
            before: None,
            limit: 1,
        },
        Command::Runs { project_id: text() },
        Command::Activity {
            project_id: text(),
            before: None,
            limit: 1,
        },
        Command::GitHistory {
            repository_id: text(),
            skip: 0,
            limit: 1,
        },
        Command::Settings,
        Command::CheckSettingsRepository { repository: text() },
        Command::CheckStorageRepository { repository: text() },
        Command::ComparePausedFile {
            path: text(),
            repository: None,
        },
        Command::SkillHistory {
            skill: Skill::Coordinating,
        },
        Command::SkillMergeReview {
            skill: Skill::Coordinating,
            entry: text(),
        },
        Command::SkillStockChanges {
            skill: Skill::Coordinating,
        },
        Command::LeftoverWorktrees,
        Command::Processes { session_id: text() },
    ];
    let writes = [
        Command::ImportRepositories {
            project_id: None,
            paths: vec![],
            roots: vec![],
            dismissed: vec![],
        },
        Command::RefreshRepositories,
        Command::RemoveRepositoryRoot { path: text() },
        Command::SetProjectRepositories {
            project_id: text(),
            repository_ids: None,
        },
        Command::RemoveRepository {
            repository_id: text(),
        },
        Command::SetLive {
            project_id: text(),
            enabled: true,
        },
        Command::ConfigureSession {
            session_id: text(),
            profile: profile(),
        },
        Command::ReconcileSession {
            session_id: text(),
            retry: true,
        },
        Command::CompactContext {
            session_id: text(),
            instructions: None,
        },
        Command::CancelCompaction { session_id: text() },
        Command::DismissCompaction { session_id: text() },
        Command::RefreshCodexQuota,
        Command::CreateTicket {
            coordinator_id: text(),
            repository_id: text(),
            title: text(),
            brief: text(),
        },
        Command::AssignTicket {
            ticket_id: text(),
            role: Role::Implementer,
            profile: profile(),
            instruction: text(),
            focus: None,
        },
        Command::CreateProject {
            name: text(),
            space_id: None,
        },
        Command::RenameProject {
            project_id: text(),
            name: text(),
        },
        Command::MoveProjects {
            project_ids: vec![],
            before: None,
        },
        Command::AttachRepository {
            project_id: None,
            path: text(),
            base: text(),
        },
        Command::CreateSession {
            project_id: text(),
            parent_id: text(),
            repository_id: None,
            name: text(),
            role: Role::Implementer,
            profile: profile(),
        },
        Command::SetStatus {
            session_id: text(),
            status: Status::Paused,
        },
        Command::SetArchived {
            session_id: text(),
            archived: true,
        },
        Command::Send {
            id: text(),
            sender: None,
            recipient: text(),
            body: text(),
            attachments: vec![],
            uploads: vec![],
        },
        Command::SetModelSelection {
            selection: Box::default(),
            verification: None,
        },
        Command::ResolveAttention {
            id: text(),
            answer: text(),
        },
        Command::DismissAttention { id: text() },
        Command::AppendLog {
            input: LogInput {
                session_id: text(),
                milestone: text(),
                kind: LogKind::Decision,
                title: text(),
                reference: text(),
                changed: text(),
                why: text(),
                state: text(),
            },
        },
        Command::AttachBrain {
            project_id: text(),
            path: text(),
        },
        Command::CreateSpace {
            name: text(),
            description: text(),
            instructions: text(),
            prefix: None,
            storage: None,
        },
        Command::UpdateSpace {
            space: Box::new(space()),
        },
        Command::RemoveSpace {
            space_id: space().id,
        },
        Command::MoveProjectToSpace {
            project_id: text(),
            space_id: space().id,
        },
        Command::ExportLogs { project_id: text() },
        Command::SetWorkspacesDir { path: text() },
        Command::SetVerification {
            verification: VerificationSettings::default(),
        },
        Command::SetCheckIns {
            child_turns: 1,
            human_turns: 1,
        },
        Command::SetOwnerAllowed {
            entry: text(),
            allowed: true,
        },
        Command::SetKeeper {
            keeper: KeeperSettings::default(),
        },
        Command::SaveSkillRules {
            skill: Skill::Coordinating,
            rules: text(),
        },
        Command::ResetSkill {
            skill: Skill::Coordinating,
        },
        Command::RestoreSkill {
            skill: Skill::Coordinating,
            entry: text(),
        },
        Command::SetUpSettingsRepository { repository: text() },
        Command::JoinSettingsRepository {
            repository: text(),
            with: JoinWith::Repository,
        },
        Command::SyncNow,
        Command::StopSettingsSync,
        Command::SettlePausedFile {
            path: text(),
            settle: Settle::UseTheOthers,
            repository: Some(text()),
        },
        Command::StartSkillRewrite {
            skill: Skill::Coordinating,
        },
        Command::SaveSkillRewrite {
            skill: Skill::Coordinating,
            text: text(),
        },
        Command::MarkSkillUpToDate {
            skill: Skill::Coordinating,
        },
        Command::UndoSkillMerge {
            skill: Skill::Coordinating,
            entry: text(),
        },
        Command::MergeSkillUpdate {
            skill: Skill::Coordinating,
        },
        Command::AnswerSkillQuestion {
            skill: Skill::Coordinating,
            wordings: vec![Wording::Stock],
        },
        Command::DismissSkillMerge {
            skill: Skill::Coordinating,
            entry: text(),
        },
        Command::SetRepositoryChecks {
            repository_id: text(),
            checks: None,
        },
        Command::SetRepositoryBase {
            repository_id: text(),
            base: text(),
        },
        Command::RemoveLeftoverWorktrees { ticket_ids: vec![] },
        Command::StopProcesses {
            session_id: text(),
            groups: vec![],
        },
    ];
    for command in &reads {
        assert!(command.is_read(), "{command:?} is a read");
    }
    for command in &writes {
        assert!(!command.is_read(), "{command:?} changes the store");
    }
}

#[test]
fn space_commands_are_the_humans() {
    let human = [
        Command::CreateSpace {
            name: text(),
            description: text(),
            instructions: text(),
            prefix: None,
            storage: Some(text()),
        },
        Command::UpdateSpace {
            space: Box::new(space()),
        },
        Command::RemoveSpace {
            space_id: space().id,
        },
        Command::MoveProjectToSpace {
            project_id: text(),
            space_id: space().id,
        },
        Command::AttachBrain {
            project_id: text(),
            path: text(),
        },
        Command::JoinSpaces {
            repository: text(),
            spaces: vec![JoinSpace {
                folder: text(),
                brain_path: None,
                clones: vec![],
            }],
        },
        Command::SetTrackerPrefix {
            space_id: space().id,
            prefix: text(),
        },
        Command::SetUpIssues {
            space_id: space().id,
            prefix: text(),
        },
        Command::SetSpaceBrain {
            space_id: space().id,
            path: text(),
        },
    ];
    for command in human {
        assert!(command.is_human_only() && !command.is_read(), "{command:?}");
    }
    let storage = Command::SpaceStorage {
        space_id: space().id,
    };
    assert!(
        storage.is_human_only() && storage.is_read(),
        "it reads where the storage is on this Mac"
    );
    assert!(!Command::Snapshot.is_human_only());
}

#[test]
fn a_snapshot_from_a_host_older_than_spaces_still_reads() {
    let old = serde_json::json!({
        "projects": [{"id": "p", "name": "Calls", "brain": "/Users/m/dev/brain"}],
        "repositories": [],
        "sessions": [],
        "attention": [],
    });
    let snapshot: Snapshot = serde_json::from_value(old).unwrap();
    assert_eq!(snapshot.projects[0].space, SpaceId::default());
    assert!(snapshot.spaces.is_empty() && snapshot.space_locals.is_empty());
}
