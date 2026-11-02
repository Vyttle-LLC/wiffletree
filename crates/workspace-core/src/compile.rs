//! Brain compile (OpenSpec change `brain-compile`): its runs as the Brains page shows them, the
//! conflicts it queues as Inbox cards, and the human's rulings on them.
use crate::{BrainConflict, ModelProfile};
use serde::{Deserialize, Serialize};

/// The usage report's id of a brain's compile line, before its repository.
pub const COMPILE_LINE: &str = "brain-compile:";

/// Why a compile run started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileTrigger {
    Scheduled,
    /// The one run for due times missed while the host was stopped or the Mac slept.
    CatchUp,
    RunNow,
    /// A fetched `main` added a ruling entry, in an earlier release.
    Ruling,
}
impl CompileTrigger {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scheduled => "Scheduled",
            Self::CatchUp => "Catch-up",
            Self::RunNow => "Run now",
            Self::Ruling => "Ruling",
        }
    }
}

/// How a compile run ended, or that it still runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileOutcome {
    Running,
    /// Nothing was pending: no model call and no write.
    Skipped,
    /// It did not start: no Brain compiler model is chosen, or the row waits.
    Waiting,
    Failed,
    TimedOut,
    /// `main` already held a cursor at or past the run's, so it landed nothing.
    CompiledElsewhere,
    /// Its commit is on `main`. A run of an earlier release that opened its pull request reads
    /// as landed too.
    #[serde(alias = "pr_opened")]
    Landed,
}
impl CompileOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Skipped => "Skipped",
            Self::Waiting => "Waiting",
            Self::Failed => "Failed",
            Self::TimedOut => "Timed out",
            Self::CompiledElsewhere => "Compiled elsewhere",
            Self::Landed => "Landed",
        }
    }
}

/// One compile run's record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompileRun {
    pub id: String,
    /// The repository of its wiki.
    pub repository: String,
    pub trigger: CompileTrigger,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    /// Entries pending when it started, and how many of them it compiled.
    pub pending: usize,
    pub processed: usize,
    /// Model sessions it started: 0 or 1.
    pub model_calls: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ModelProfile>,
    /// The bundled compile procedure's version it ran under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedure: Option<u32>,
    pub outcome: CompileOutcome,
    /// What it did or why it did not, in a sentence for the Brains page.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// Conflicts it queued.
    #[serde(default)]
    pub conflicts: usize,
    /// The commit it landed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Its model session's tokens and estimated cost, from the usage ledger.
    #[serde(default)]
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

/// One page of a brain's compile runs, newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompileRunPage {
    pub runs: Vec<CompileRun>,
    /// Every run recorded for the brain.
    pub total: usize,
}

/// A wiki's compile as the Brains page shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrainCompileState {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<CompileRun>,
    /// Entries on `main` added since the brain's last compile.
    pub pending: usize,
    /// Why Run now waits, when it does: a run in progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_now_waits: Option<String>,
    /// No Brain compiler row is chosen in Models, so a run with entries pending waits for one.
    #[serde(default)]
    pub needs_model: bool,
}

/// The human's ruling on a conflict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ruling", rename_all = "snake_case")]
pub enum Ruling {
    UpholdExisting,
    KeepNew,
    /// One line, the human's, that scopes both.
    KeepBoth {
        scope: String,
    },
    /// No entry; the card hides until the next compile.
    Defer,
}
impl Ruling {
    /// The four rulings' names, as cards, the compile tools and the conflict queue write them.
    pub const NAMES: [&str; 4] = ["Uphold existing", "Keep new", "Keep both", "Defer"];
    pub fn label(&self) -> &'static str {
        match self {
            Self::UpholdExisting => Self::NAMES[0],
            Self::KeepNew => Self::NAMES[1],
            Self::KeepBoth { .. } => Self::NAMES[2],
            Self::Defer => Self::NAMES[3],
        }
    }
}

/// Where a conflict's card stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConflictState {
    Open,
    /// Ruled by `by`: here, or on another install, whose ruling entry is on `main`. The card
    /// leaves when a compile clears it.
    Ruled {
        ruling: String,
        by: String,
        /// The ruling entry is on `main`, so the next compile picks it up.
        landed: bool,
    },
}

/// What one side of a conflict says: an entry's or a note's text, shown as plain text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictSide {
    /// Such as "The decision note says" or "Priya's entry says · 2026-10-28".
    pub label: String,
    pub text: String,
    /// Where it is and how it stands, such as "notes/Call Relay.md · kind: decision" or
    /// "app · claimed".
    pub detail: String,
}

/// One open conflict of a brain, for the Inbox of each project in `projects`, or the Brains
/// page when it is empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictCard {
    /// The repository of its brain.
    pub repository: String,
    pub conflict: BrainConflict,
    /// What the established note or entry says, and what the new entry says, when the index
    /// holds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<ConflictSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<ConflictSide>,
    pub projects: Vec<String>,
    pub state: ConflictState,
    /// The ruling the conflict recommends, as one of the four names, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,
}
impl ConflictCard {
    /// The recommended ruling's name in `recommend`, such as "Uphold existing" in
    /// "Uphold existing: PR #92 is still open".
    pub fn recommended_in(recommend: &str) -> Option<String> {
        Ruling::NAMES
            .into_iter()
            .find(|name| {
                recommend
                    .trim_start()
                    .to_lowercase()
                    .starts_with(&name.to_lowercase())
            })
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recommended_ruling_is_the_one_its_line_starts_with() {
        assert_eq!(
            ConflictCard::recommended_in("Uphold existing: PR #92 is still open").as_deref(),
            Some("Uphold existing")
        );
        assert_eq!(
            ConflictCard::recommended_in("keep both — two checks").as_deref(),
            Some("Keep both")
        );
        assert_eq!(ConflictCard::recommended_in("Unsure"), None);
    }
}
