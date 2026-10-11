//! Which files agents read and change during a ticket, derived from their work steps.
use crate::Role;
use serde::{Deserialize, Serialize};

/// One file an agent's read and edit steps touched, over all of its runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileActivity {
    /// Relative to the ticket worktree; a path outside it is kept as the provider gave it.
    pub path: String,
    /// Finished reads and edits; failed and interrupted steps count nothing.
    pub reads: u32,
    pub edits: u32,
    /// A read or write of the file is running now.
    pub reading: bool,
    pub writing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFiles {
    pub session_id: String,
    /// The agent's short name on its ticket, such as "Implementer".
    pub name: String,
    pub role: Role,
    pub working: bool,
    pub files: Vec<FileActivity>,
}

/// Lines a file gained and lost against the ticket base, uncommitted changes included.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineCount {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// A ticket's agents and the files each touched, with the worktree's changes from Git.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketFiles {
    pub ticket_id: String,
    pub title: String,
    pub repository_id: String,
    pub repository: String,
    pub agents: Vec<AgentFiles>,
    /// Empty when the worktree is missing or Git could not compare it.
    #[serde(default)]
    pub changes: Vec<LineCount>,
    /// The worktree's tracked and untracked-but-not-ignored files, when asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<Vec<String>>,
}
