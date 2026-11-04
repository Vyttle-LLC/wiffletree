//! UI evaluator runs (OpenSpec change `ui-evaluators`): blind agents on one exact model that a
//! coordinator starts with a prompt and a run folder, and the human allows per project.
use crate::ModelProfile;
use serde::{Deserialize, Serialize};

/// How a run drives the screen: only the app under test, or the whole desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationMode {
    App,
    Desktop,
}
impl EvaluationMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::App => "App only",
            Self::Desktop => "Full desktop",
        }
    }
}

/// What the human allows evaluators to do in a project. Only the human sets it; a project
/// with no choice yet is off, and its first run asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorPermission {
    Off,
    App,
    Desktop,
}
impl EvaluatorPermission {
    /// The Inbox card's answers, as the human reads them.
    pub const CHOICES: [(&'static str, Self); 3] = [
        ("App only", Self::App),
        ("Full desktop", Self::Desktop),
        ("Not in this project", Self::Off),
    ];
    /// Full desktop allows app-only runs too.
    pub fn allows(self, mode: EvaluationMode) -> bool {
        match self {
            Self::Off => false,
            Self::App => mode == EvaluationMode::App,
            Self::Desktop => true,
        }
    }
    pub fn from_answer(answer: &str) -> Option<Self> {
        (Self::CHOICES.iter())
            .find(|(label, _)| *label == answer)
            .map(|(_, permission)| *permission)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationState {
    /// Waiting for the screen. Full desktop only.
    Queued,
    /// Waiting for the human's choice, or for its pinned model's limit to reset.
    Waiting,
    Running,
    /// It stopped on its own. Whether its report is valid is the coordinator's call.
    Finished,
    Failed,
    /// By the human, the coordinator, or the project closing.
    Stopped,
}
impl EvaluationState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
    pub fn ended(self) -> bool {
        matches!(self, Self::Finished | Self::Failed | Self::Stopped)
    }
}

/// One evaluator run. Its transcript is its session's, which only the human reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    pub id: String,
    pub project_id: String,
    /// The coordinator that started it, which hears when it ends.
    pub coordinator_id: String,
    /// The run's own session, from when it is admitted; outside every agent tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Exactly what it was started with; nothing replaces it.
    pub profile: ModelProfile,
    /// The model the provider said it runs: Claude's at its start, Codex's in its session
    /// record. For an alias such as `opus`, the model it stood for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_model: Option<String>,
    pub mode: EvaluationMode,
    /// Absolute and canonical; it holds `prompt.md`.
    pub folder: String,
    pub state: EvaluationState,
    /// Why it waits, failed or stopped, for the human. A failure's reason never reaches the
    /// coordinator: a provider's error can quote the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When a run waiting for its provider's limit is due to go on, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
}
