//! What a paired device may ask, command by command: the access table of design.md 1.
use workspace_core::{AccessLevel, Command};

/// One row of the access table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// Reads, which every device may make.
    Read,
    /// Reads that name a host path, served within the roots; `ListFolder` also under the home
    /// folder for Control.
    HostPath,
    /// Scanning folders the device names: Control only.
    Scan,
    /// Reading host paths the device names as attachments: never from a device, which uploads.
    HostAttachments,
    /// Every other workspace command, settings and uploads included: Control only.
    Change,
    /// Remote access, pairing, devices and levels: the host itself only.
    RemoteAccess,
    /// Stopping or restarting the host process: the host itself only.
    HostProcess,
}

/// The row each command belongs to. Every command is named, so a new one must be classified.
pub(crate) fn row(command: &Command) -> Row {
    match command {
        Command::Snapshot
        | Command::Archive
        | Command::Quotas
        | Command::InspectProvider { .. }
        | Command::MeasureContext { .. }
        | Command::Usage { .. }
        | Command::Messages { .. }
        | Command::Steps { .. }
        | Command::Files { .. }
        | Command::Progress { .. }
        | Command::RunSummaries { .. }
        | Command::Logs { .. }
        | Command::Runs { .. }
        | Command::Activity { .. }
        | Command::GitHistory { .. }
        | Command::Settings
        | Command::CheckSettingsRepository { .. }
        | Command::CheckStorageRepository { .. }
        | Command::ComparePausedFile { .. }
        | Command::SkillHistory { .. }
        | Command::SkillMergeReview { .. }
        | Command::SkillStockChanges { .. }
        | Command::LeftoverWorktrees
        | Command::Processes { .. }
        | Command::Subscribe { .. }
        | Command::Ping
        | Command::CoreInstructions
        | Command::NewProjectDefaults { .. }
        | Command::SuggestRepositorySpace { .. }
        | Command::RepositorySuggestions
        | Command::SpaceStorage { .. }
        | Command::Brains
        | Command::SuggestSpaceBrain { .. } => Row::Read,
        Command::ResolvePaths { .. } | Command::ReadFile { .. } | Command::ListFolder { .. } => {
            Row::HostPath
        }
        Command::DiscoverRepositories { .. } => Row::Scan,
        Command::InspectAttachments { .. } => Row::HostAttachments,
        Command::ImportRepositories { .. }
        | Command::RefreshRepositories
        | Command::RemoveRepositoryRoot { .. }
        | Command::SetProjectRepositories { .. }
        | Command::RemoveRepository { .. }
        | Command::SetLive { .. }
        | Command::ConfigureSession { .. }
        | Command::ReconcileSession { .. }
        | Command::CompactContext { .. }
        | Command::CancelCompaction { .. }
        | Command::DismissCompaction { .. }
        | Command::RefreshCodexQuota
        | Command::CreateTicket { .. }
        | Command::AssignTicket { .. }
        | Command::CreateProject { .. }
        | Command::RenameProject { .. }
        | Command::MoveProjects { .. }
        | Command::AttachRepository { .. }
        | Command::CreateSession { .. }
        | Command::SetStatus { .. }
        | Command::SetArchived { .. }
        | Command::Send { .. }
        | Command::SetModelSelection { .. }
        | Command::DismissModelNotes
        | Command::ResolveAttention { .. }
        | Command::DismissAttention { .. }
        | Command::AppendLog { .. }
        | Command::AttachBrain { .. }
        | Command::SetSpaceBrain { .. }
        | Command::CreateSpace { .. }
        | Command::UpdateSpace { .. }
        | Command::RemoveSpace { .. }
        | Command::MoveProjectToSpace { .. }
        | Command::SetRepositorySpaces { .. }
        | Command::ConfirmRepositorySuggestions { .. }
        | Command::JoinSpaces { .. }
        | Command::SetTrackerPrefix { .. }
        | Command::SetUpIssues { .. }
        | Command::ConfirmBrain { .. }
        | Command::ForgetBrain { .. }
        | Command::SetUpBrain { .. }
        | Command::SetBrainCompiler { .. }
        | Command::AmendBrainBatch { .. }
        | Command::ExportLogs { .. }
        | Command::SetWorkspacesDir { .. }
        | Command::SetVerification { .. }
        | Command::SetCheckIns { .. }
        | Command::SetOwnerAllowed { .. }
        | Command::SetKeeper { .. }
        | Command::SaveSkillRules { .. }
        | Command::ResetSkill { .. }
        | Command::RestoreSkill { .. }
        | Command::SetUpSettingsRepository { .. }
        | Command::JoinSettingsRepository { .. }
        | Command::SyncNow
        | Command::StopSettingsSync
        | Command::SettlePausedFile { .. }
        | Command::StartSkillRewrite { .. }
        | Command::SaveSkillRewrite { .. }
        | Command::MarkSkillUpToDate { .. }
        | Command::UndoSkillMerge { .. }
        | Command::MergeSkillUpdate { .. }
        | Command::AnswerSkillQuestion { .. }
        | Command::DismissSkillMerge { .. }
        | Command::SetRepositoryChecks { .. }
        | Command::SetRepositoryBase { .. }
        | Command::RemoveLeftoverWorktrees { .. }
        | Command::StopProcesses { .. }
        | Command::UploadAttachment { .. } => Row::Change,
        Command::RemoteAccess
        | Command::SetRemoteAccess { .. }
        | Command::OpenPairing
        | Command::CancelPairing
        | Command::SetDeviceLevel { .. }
        | Command::RemoveDevice { .. } => Row::RemoteAccess,
        Command::StopAgentsAndQuit | Command::RestartHost { .. } => Row::HostProcess,
    }
}

/// Whether a device at `level` may give `command` on the host named `host`, or the refusal it
/// gets. Paths are checked again when the command runs.
pub(crate) fn check(command: &Command, level: AccessLevel, host: &str) -> Result<(), String> {
    let view_only = || format!("View only: {host} refuses changes from this device");
    match (row(command), level) {
        (Row::Read | Row::HostPath, _) => Ok(()),
        (Row::Scan | Row::Change, AccessLevel::ViewOnly) => Err(view_only()),
        (Row::Change, AccessLevel::Control) => match command {
            Command::Send { attachments, .. } if !attachments.is_empty() => {
                Err("Remote devices attach files by uploading them".into())
            }
            _ => Ok(()),
        },
        (Row::Scan, AccessLevel::Control) => Ok(()),
        (Row::HostAttachments, _) => Err("Remote devices attach files by uploading them".into()),
        (Row::RemoteAccess, _) => Err(format!("Only {host} itself can change remote access")),
        (Row::HostProcess, _) => Err(format!(
            "Only {host} itself can stop or restart Wiffletree's host"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The variants of `Command`, read from its source.
    fn commands() -> Vec<String> {
        let source = include_str!("../../../workspace-core/src/lib.rs");
        let start = source.find("pub enum Command {").unwrap();
        let end = start + source[start..].find("\nimpl Command {").unwrap();
        source[start..end]
            .lines()
            .filter(|line| line.starts_with("    ") && !line.starts_with("     "))
            .map(str::trim)
            .filter(|line| line.starts_with(|c: char| c.is_ascii_uppercase()))
            .map(|line| line.trim_end_matches([',', '{', ' ']).to_string())
            .collect()
    }

    #[test]
    fn every_command_is_in_exactly_one_row_of_the_access_table() {
        let source = include_str!("access.rs");
        let table = &source[source.find("pub(crate) fn row(").unwrap()
            ..source.find("pub(crate) fn check(").unwrap()];
        let commands = commands();
        assert!(commands.len() > 80, "the enum was read: {}", commands.len());
        for command in commands {
            let named = table
                .match_indices(&format!("Command::{command}"))
                .filter(|(at, name)| {
                    !table[at + name.len()..].starts_with(|c: char| c.is_alphanumeric())
                })
                .count();
            assert_eq!(named, 1, "{command} must be in exactly one row");
        }
    }

    #[test]
    fn view_only_reads_and_control_changes_but_neither_grants_access() {
        let send = Command::Send {
            id: "m".into(),
            sender: None,
            recipient: "c".into(),
            body: "hi".into(),
            attachments: vec![],
            uploads: vec![],
        };
        let host = "Studio Mac";
        assert!(check(&Command::Snapshot, AccessLevel::ViewOnly, host).is_ok());
        assert_eq!(
            check(&send, AccessLevel::ViewOnly, host).unwrap_err(),
            "View only: Studio Mac refuses changes from this device"
        );
        assert!(check(&send, AccessLevel::Control, host).is_ok());
        for command in [
            Command::OpenPairing,
            Command::SetDeviceLevel {
                device_id: "d".into(),
                level: AccessLevel::Control,
            },
        ] {
            assert_eq!(
                check(&command, AccessLevel::Control, host).unwrap_err(),
                "Only Studio Mac itself can change remote access"
            );
        }
        for command in [
            Command::StopAgentsAndQuit,
            Command::RestartHost {
                when: workspace_core::RestartWhen::Idle,
            },
        ] {
            assert!(check(&command, AccessLevel::Control, host).is_err());
        }
        let scan = Command::DiscoverRepositories { folders: vec![] };
        assert!(check(&scan, AccessLevel::ViewOnly, host).is_err());
        assert!(check(&scan, AccessLevel::Control, host).is_ok());
        let inspect = Command::InspectAttachments { paths: vec![] };
        assert!(check(&inspect, AccessLevel::Control, host).is_err());
        let attach = Command::Send {
            id: "m".into(),
            sender: None,
            recipient: "c".into(),
            body: "hi".into(),
            attachments: vec!["/Users/me/.ssh/id_ed25519".into()],
            uploads: vec![],
        };
        assert!(check(&attach, AccessLevel::Control, host).is_err());
    }
}
