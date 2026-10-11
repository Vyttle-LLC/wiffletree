//! An agent's in-progress work, kept apart from the durable conversation.
use serde::{Deserialize, Serialize};

/// Steps kept per run; later ones are counted in `StepPage::omitted_steps` but not stored.
pub const MAX_RUN_STEPS: usize = 2_000;
/// Longest single-line step title, in characters.
pub const MAX_STEP_TITLE: usize = 200;
/// Longest short note shown beside a step, such as "+12 −4" or "exit 1", in characters.
pub const MAX_STEP_NOTE: usize = 40;
/// Most items in a checklist recorded with `set_plan`.
pub const MAX_PLAN_ITEMS: usize = 50;
/// Largest step detail, in bytes. Output keeps its tail; prose keeps its head.
pub const MAX_STEP_DETAIL: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Narration,
    Thinking,
    Command,
    FileEdit,
    Read,
    Search,
    Plan,
    Tool,
}
impl StepKind {
    /// Narration and thinking are the agent talking; every other kind is an action.
    pub fn is_action(self) -> bool {
        !matches!(self, Self::Narration | Self::Thinking)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

/// A plan step's checklist, as numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCounts {
    pub done: u32,
    pub total: u32,
    /// The first unfinished item, bounded like a title; `None` when every item is done.
    pub current: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub run_id: String,
    /// The provider's item or tool-use id, unique within the run.
    pub id: String,
    /// The subagent call this step ran under, when a provider nests work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub kind: StepKind,
    pub state: StepState,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Files a read or edit step touched, relative to the working directory when inside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// A plan step's counts; `None` for other kinds and for plan steps stored before counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanCounts>,
    /// Bytes dropped from `detail` to respect `MAX_STEP_DETAIL`.
    #[serde(default)]
    pub omitted: usize,
    /// Order of first appearance within the run.
    pub seq: u32,
    /// Per-session counter; a client asks for steps changed after the last revision it saw.
    pub revision: u64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

/// How long a run took and how much it did, for the line above its reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// Steps other than narration and thinking.
    pub actions: usize,
    pub failed: usize,
}

/// A session's steps for one run, changed after the requested revision.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepPage {
    /// `None` when the session has never run.
    pub run_id: Option<String>,
    pub running: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// The highest revision included; pass it back as `after` to receive only newer changes.
    pub revision: u64,
    pub steps: Vec<Step>,
    pub omitted_steps: usize,
}
