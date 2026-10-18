//! A merge of a release's stock text into the human's rewrite, as the parts of the merged text.
use crate::MergePassage;
use serde::{Deserialize, Serialize};

/// Where a change in a merged rewrite came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The update's change, where the human had changed nothing.
    Update,
    /// An overlapping passage the model merged.
    Model,
    /// An overlapping passage the human settled by choosing a wording.
    Human,
}
/// A change a merge made to the rewrite: what the rewrite said there, and what it says now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergedChange {
    pub source: Source,
    pub before: String,
    pub after: String,
}
/// The merge of an update into a rewrite, as the parts that make up the merged text in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    parts: Vec<Part>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// What the rewrite says, kept as it is.
    Kept(String),
    /// The update's change where the human changed nothing: `yours` becomes `stock`.
    Update {
        yours: String,
        stock: String,
    },
    Overlap(MergePassage),
}
impl Merge {
    pub fn new(parts: Vec<Part>) -> Self {
        Self { parts }
    }
    /// The merged rewrite and its changes, when no passage overlaps.
    pub fn plain(&self) -> Option<(String, Vec<MergedChange>)> {
        let overlaps = self
            .parts
            .iter()
            .any(|part| matches!(part, Part::Overlap(_)));
        (!overlaps).then(|| self.settled(|_, _| unreachable!("no passage overlaps")))
    }
    /// The merged rewrite and its changes, with the text `settle` gives each overlapping
    /// passage, by its index, and where that text came from. Every other part is the plain
    /// merge's.
    pub fn settled(
        &self,
        mut settle: impl FnMut(usize, &MergePassage) -> (String, Source),
    ) -> (String, Vec<MergedChange>) {
        let mut text = String::new();
        let mut changes = vec![];
        let mut overlaps = 0;
        for part in &self.parts {
            let (source, before, after) = match part {
                Part::Kept(kept) => {
                    text.push_str(kept);
                    continue;
                }
                Part::Update { yours, stock } => (Source::Update, yours, stock.clone()),
                Part::Overlap(passage) => {
                    let (after, source) = settle(overlaps, passage);
                    overlaps += 1;
                    (source, &passage.yours, after)
                }
            };
            text.push_str(&after);
            if *before != after {
                let before = before.clone();
                changes.push(MergedChange {
                    source,
                    before,
                    after,
                });
            }
        }
        (text, changes)
    }
    pub fn passages(&self) -> Vec<MergePassage> {
        let overlap = |part: &Part| match part {
            Part::Overlap(passage) => Some(passage.clone()),
            _ => None,
        };
        self.parts.iter().filter_map(overlap).collect()
    }
}
