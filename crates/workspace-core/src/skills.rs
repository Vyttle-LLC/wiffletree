//! A skill's history and merges as the host records them and the Skills page shows them.
pub mod merge;
use crate::{MergeModel, MergeNote};
use serde::{Deserialize, Serialize};

/// The version on a bundled text's first line, as "v1" in "# Implementing — v1".
pub fn version(text: &str) -> &str {
    let first = text.lines().next().unwrap_or_default();
    first.rsplit_once(" — ").map_or("", |(_, version)| version)
}
/// One part of the fixed core instructions agents get, which no skill text changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInstructions {
    /// As its first line names it, such as "Implementer core".
    pub name: String,
    /// Who gets it, such as "implementers".
    pub audience: String,
    pub version: String,
}
impl CoreInstructions {
    /// From a bundled text whose first line is "# <name> — <version>".
    pub fn new(audience: &str, text: &str) -> Self {
        let first = text
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("# ");
        let name = first.rsplit_once(" — ").map_or(first, |(name, _)| name);
        Self {
            name: name.into(),
            audience: audience.into(),
            version: version(text).into(),
        }
    }
}
/// What the human has written for a skill: rules in Additions mode, or a rewrite.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edits {
    pub rules: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<Rewritten>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewritten {
    pub text: String,
    /// The stock text the rewrite started from, or was last marked up to date against or merged
    /// with.
    pub base: String,
}
/// What a history entry records: the skill's whole state after a change, and the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub change: Change,
    pub stock_version: String,
    #[serde(flatten)]
    pub edits: Edits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    SavedRules,
    StartedRewrite,
    SavedRewrite,
    MarkedUpToDate,
    ResetToStock,
    /// `entry` names the entry whose state was restored.
    Restored {
        entry: String,
    },
    /// A release brought a stock version other than the one `previous` entry recorded.
    NewStockVersion {
        previous: String,
    },
    /// Stock version `to` merged into a rewrite based on `from`, by `machine`.
    Merged {
        from: String,
        to: String,
        /// The rewrite and base exactly as they were, which Undo writes back.
        before: Rewritten,
        changes: Vec<merge::MergedChange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<MergeModel>,
        machine: String,
        /// The host merged on its own, not at the human's Merge v2 in.
        #[serde(default)]
        automatic: bool,
    },
    /// The merge `entry` names, which brought in stock version `to`, was undone.
    UndidMerge {
        entry: String,
        to: String,
    },
    /// The model found that stock version `to` and the rewrite based on `from` want different
    /// things in a passage, so the human is asked; the skill is unchanged. Holds the merge so
    /// far, so an answer needs no model: the plain merge, and an answer for each passage that
    /// overlaps. `stock` is the stock text it merges into, which only a machine with that text
    /// shows and settles.
    Asked {
        from: String,
        to: String,
        stock: String,
        merge: merge::Merge,
        answers: Vec<PassageAnswer>,
        model: MergeModel,
        machine: String,
    },
}
/// A history entry by its name, the time it was written, or why it could not be read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    pub name: String,
    pub entry: Result<Entry, String>,
}
/// A line of what changed from one text to another, such as a rewrite's base to the stock text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum Line {
    Unchanged(String),
    Removed(String),
    Added(String),
}
/// A passage's answer, once checked: the model's merged text, or its one-sentence question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassageAnswer {
    Merged(String),
    Question(String),
}
/// A skill's history, newest first, and the name of the machine that keeps it, which a merge
/// note leaves out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub entries: Vec<Recorded>,
    pub machine: String,
}
/// The note of the "Merged" entry `name`; it names the machine that merged unless that is
/// `this_machine`.
pub fn merge_note(name: &str, entry: &Entry, this_machine: &str) -> Option<MergeNote> {
    let Change::Merged {
        from,
        to,
        changes,
        model,
        machine,
        ..
    } = &entry.change
    else {
        return None;
    };
    let count = |source| changes.iter().filter(|c| c.source == source).count();
    Some(MergeNote {
        entry: name.into(),
        from: from.clone(),
        to: to.clone(),
        from_update: count(merge::Source::Update),
        by_model: count(merge::Source::Model),
        model: model.clone(),
        machine: (machine != this_machine).then(|| machine.clone()),
    })
}
/// A merge's note, and the rewrite before and after it, line by line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeReview {
    pub note: MergeNote,
    /// The skill still holds what the merge made, with every file read.
    pub live: bool,
    pub lines: Vec<Reviewed>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewed {
    pub line: Line,
    /// Where a removed or added line's change came from.
    pub source: Option<merge::Source>,
}
