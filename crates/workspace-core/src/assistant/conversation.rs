//! The Assistant's one conversation (OpenSpec change `assistant-does-the-work`): the cards its
//! tools put in the chat, and the tag on a human message the Assistant passed to its work.
use crate::SpaceId;
use crate::job::Recipe;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A card in the conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationCard {
    pub id: String,
    pub created_at: i64,
    /// What text written by others the turn had read when it raised the card, such as "a
    /// report from Comment colour" or "Gmail search_threads"; empty in a turn that was the
    /// human's alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<String>,
    #[serde(flatten)]
    pub kind: CardKind,
}
impl ConversationCard {
    /// Whether the card waits for the human: an open decision, a loop change or an action
    /// waiting for Apply or Run. Approvals are counted from the Inbox items they show.
    pub fn needs_human(&self) -> bool {
        match &self.kind {
            CardKind::Decision(decision) => decision.answer.is_none(),
            CardKind::Loop(change) => change.state == LoopCardState::Waiting,
            CardKind::Action(action) => action.state == ActionState::Waiting,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardKind {
    /// `start_work`: the work item, whose stage, pull requests and state are read from its
    /// project.
    Work { project_id: String },
    /// `ask_human`.
    Decision(Decision),
    /// An approval only the human gives for a work item: the Inbox item it settles with.
    Approval {
        project_id: String,
        attention_id: String,
    },
    /// A loop change: made at once with Undo, or waiting for Apply in a held turn.
    Loop(LoopCard),
    /// `act`: waiting for Run, or done at once for a kind marked Do without asking.
    Action(ActionCard),
    /// `file_issue`, `edit_issue` or `comment_issue`: what was written, with its issue.
    IssueWritten {
        space_id: SpaceId,
        issue_id: String,
        /// "Filed", "Edited" or "Commented".
        written: String,
    },
    /// `save_output`.
    SavedOutput {
        space_id: SpaceId,
        title: String,
        file: String,
        version: u32,
    },
    /// `suggest_space`: opens Settings › Spaces filled in.
    SuggestSpace { name: String, description: String },
}

/// A decision only the human can make, with the Assistant's recommendation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// The work item it is about, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub question: String,
    pub recommendation: String,
    #[serde(default)]
    pub options: Vec<String>,
    /// The human's answer, which reached the Assistant as the human's message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopChange {
    Create,
    Edit,
    Pause,
    Resume,
    Delete,
    Run,
}
impl LoopChange {
    pub fn label(self) -> &'static str {
        match self {
            Self::Create => "Loop created",
            Self::Edit => "Loop changed",
            Self::Pause => "Loop paused",
            Self::Resume => "Loop resumed",
            Self::Delete => "Loop deleted",
            Self::Run => "Loop run",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopCardState {
    /// Raised in a held turn: nothing changed until the human presses Apply.
    Waiting,
    Applied,
    /// The human pressed Undo, which restored the loop as it was before.
    Undone,
    /// The human declined a waiting change.
    Declined,
}
/// One loop change: the loop as it was before and as the change leaves it, so Undo can restore
/// the first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopCard {
    pub space_id: SpaceId,
    pub job_id: String,
    pub change: LoopChange,
    /// `None` for a loop the change creates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Box<Recipe>>,
    /// `None` for a loop the change deletes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Box<Recipe>>,
    pub state: LoopCardState,
}

/// What `act` asks a source to do, exactly as it would run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActCall {
    Mcp { tool: String, arguments: Value },
    Cli { command: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ActionState {
    Waiting,
    Running,
    Done { result: String },
    Failed { error: String },
    Declined,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCard {
    pub space_id: SpaceId,
    /// The source's MCP server or CLI command.
    pub source: String,
    pub call: ActCall,
    pub state: ActionState,
    /// It ran at once, as a kind the human marked Do without asking: a done card.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub without_asking: bool,
}

/// The tag on a human message the Assistant passed to its work with `message_work` in the
/// turn that message started: "→ ‹work title› · Change".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub message_id: String,
    /// The work item it went to; `None` after Change chose "Just the Assistant".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}
