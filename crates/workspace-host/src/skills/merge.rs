//! The three-way merge of a release's stock text into the human's rewrite, by paragraph. A
//! paragraph is a run of non-blank lines with the blank lines after it, so a blank line never
//! keeps two changes apart. The title line, with its version, is merged on its own. Changes that
//! don't overlap merge as plain text; the passages that do are handed back, unmerged.
use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffTag};
use std::ops::Range;
use workspace_core::ModelProfile;

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
/// The model that merged a rewrite's overlapping passages, and the tokens it used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeModel {
    pub model: ModelProfile,
    pub tokens: u64,
}
/// A run of paragraphs that both the update and the human changed, each in its own way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passage {
    pub base: String,
    pub stock: String,
    pub yours: String,
}
/// The merge of an update into a rewrite, as the parts that make up the merged text in order.
#[derive(Debug)]
pub struct Merge {
    parts: Vec<Part>,
}
#[derive(Debug)]
enum Part {
    /// What the rewrite says, kept as it is.
    Kept(String),
    /// The update's change where the human changed nothing: `yours` becomes `stock`.
    Update {
        yours: String,
        stock: String,
    },
    Overlap(Passage),
}
impl Merge {
    /// The merged rewrite and its changes, when no passage overlaps.
    pub fn plain(&self) -> Option<(String, Vec<MergedChange>)> {
        let mut text = String::new();
        let mut changes = vec![];
        for part in &self.parts {
            match part {
                Part::Kept(kept) => text.push_str(kept),
                Part::Update { yours, stock } => {
                    text.push_str(stock);
                    changes.push(MergedChange {
                        source: Source::Update,
                        before: yours.clone(),
                        after: stock.clone(),
                    });
                }
                Part::Overlap(_) => return None,
            }
        }
        Some((text, changes))
    }
    pub fn passages(&self) -> Vec<Passage> {
        let overlap = |part: &Part| match part {
            Part::Overlap(passage) => Some(passage.clone()),
            _ => None,
        };
        self.parts.iter().filter_map(overlap).collect()
    }
}
/// Merges the change from `base` to `stock` into `yours`, the rewrite of `base`.
pub fn merge(base: &str, stock: &str, yours: &str) -> Merge {
    let [
        (base_title, base),
        (stock_title, stock),
        (your_title, yours),
    ] = [base, stock, yours].map(split);
    let title = if stock_title != base_title && your_title == base_title {
        Part::Update {
            yours: your_title.into(),
            stock: stock_title.into(),
        }
    } else {
        Part::Kept(your_title.into())
    };
    let mut parts = vec![title];
    // Each side's hunks in the base's order, the update's marked `true`.
    let mut hunks: Vec<(bool, Hunk)> = (hunks(&base, &stock).into_iter().map(|h| (true, h)))
        .chain(hunks(&base, &yours).into_iter().map(|h| (false, h)))
        .collect();
    hunks.sort_by_key(|(_, hunk)| (hunk.base.start, hunk.base.end));
    let mut unchanged_from = 0;
    let mut rest = &hunks[..];
    while let Some((_, first)) = rest.first() {
        // Hunks that share or touch base paragraphs, or insert at one place, are one passage.
        let mut end = first.base.end;
        let together = rest.iter().take_while(|(_, hunk)| {
            let touches = hunk.base.start <= end;
            if touches {
                end = end.max(hunk.base.end);
            }
            touches
        });
        let (passage, after) = rest.split_at(together.count());
        let start = first.base.start;
        parts.push(Part::Kept(base[unchanged_from..start].concat()));
        let version = |of_stock: bool, text: &[&str]| {
            let mut own = passage.iter().filter(|(side, _)| *side == of_stock);
            let (_, first) = own.next()?;
            let last = own.next_back().map_or(first, |(_, last)| last);
            let from = first.new.start - (first.base.start - start);
            let to = last.new.end + (end - last.base.end);
            Some(text[from..to].concat())
        };
        parts.push(match (version(true, &stock), version(false, &yours)) {
            (Some(stock), None) => Part::Update {
                yours: base[start..end].concat(),
                stock,
            },
            (None, Some(yours)) => Part::Kept(yours),
            (Some(stock), Some(yours)) if stock == yours => Part::Kept(yours),
            (stock, yours) => Part::Overlap(Passage {
                base: base[start..end].concat(),
                stock: stock.unwrap_or_default(),
                yours: yours.unwrap_or_default(),
            }),
        });
        unchanged_from = end;
        rest = after;
    }
    parts.push(Part::Kept(base[unchanged_from..].concat()));
    Merge { parts }
}
/// A text's first line, its title, and the paragraphs after it, which join back into the text.
fn split(text: &str) -> (&str, Vec<&str>) {
    let title = text.split_inclusive('\n').next().unwrap_or_default();
    let body = &text[title.len()..];
    let mut paragraphs = vec![];
    let (mut start, mut at, mut after_blank) = (0, 0, false);
    for line in body.split_inclusive('\n') {
        let blank = line.trim().is_empty();
        if after_blank && !blank {
            paragraphs.push(&body[start..at]);
            start = at;
        }
        after_blank = blank;
        at += line.len();
    }
    if start < body.len() {
        paragraphs.push(&body[start..]);
    }
    (title, paragraphs)
}
/// A range of base paragraphs and the paragraphs that replace it on one side.
#[derive(Debug)]
struct Hunk {
    base: Range<usize>,
    new: Range<usize>,
}
/// What changed from `base` to `new`, paragraph by paragraph, each run of changes as one hunk.
fn hunks(base: &[&str], new: &[&str]) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = vec![];
    for op in similar::capture_diff_slices(Algorithm::Myers, base, new) {
        let (tag, base, new) = op.as_tag_tuple();
        match hunks.last_mut() {
            _ if tag == DiffTag::Equal => {}
            Some(last) if last.base.end == base.start && last.new.end == new.start => {
                last.base.end = base.end;
                last.new.end = new.end;
            }
            _ => hunks.push(Hunk { base, new }),
        }
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::stock;
    use workspace_core::Skill;

    fn coordinating() -> &'static str {
        stock(Skill::Coordinating)
    }

    /// `text` with `old` replaced once, which must be in it.
    fn edit(text: &str, old: &str, new: &str) -> String {
        assert!(text.contains(old), "{old}");
        text.replacen(old, new, 1)
    }

    fn v2(text: &str) -> String {
        edit(text, "# Coordinating — v1", "# Coordinating — v2")
    }

    const FIRST: &str = "Start by clarifying and planning the goal;";
    const SECOND: &str = "Break the plan into bounded tickets,";
    const FIFTH: &str = "Ask the human instead when a waiver";

    #[test]
    fn paragraphs_join_back_into_the_exact_text() {
        let texts = [
            "",
            "# Title only",
            "# Title\n\n\nLeading blank lines\n",
            "# T\nOne  \n \nTwo with trailing spaces and no final newline  ",
            "# T\nA\nrun\n\n\nB\n\n",
        ];
        for text in Skill::ALL.map(stock).into_iter().chain(texts) {
            let (title, paragraphs) = split(text);
            assert_eq!(format!("{title}{}", paragraphs.concat()), text);
        }
        let (title, paragraphs) = split(coordinating());
        assert_eq!(title, "# Coordinating — v1\n");
        assert!(paragraphs[0].starts_with(FIRST) && paragraphs[0].ends_with(".\n\n"));
        assert!(paragraphs[4].starts_with(FIFTH));
        // Lines with no blank line between them are one paragraph.
        assert!(paragraphs.last().unwrap().contains("For UI-facing changes"));
        let (_, paragraphs) = split("# T\nA\nrun\n\n\nB\n\n");
        assert_eq!(paragraphs, ["A\nrun\n\n\n", "B\n\n"]);
    }

    #[test]
    fn changes_that_dont_overlap_merge_as_plain_text() {
        let yours = edit(coordinating(), FIRST, "Start by restating the goal;");
        let stock = v2(&edit(coordinating(), FIFTH, "Ask the human when a waiver"));
        let merged = merge(coordinating(), &stock, &yours);
        let (text, changes) = merged.plain().unwrap();
        let expected = v2(&edit(&yours, FIFTH, "Ask the human when a waiver"));
        assert_eq!(text, expected);
        assert!(merged.passages().is_empty());
        let sources: Vec<_> = changes.iter().map(|change| change.source).collect();
        assert_eq!(sources, [Source::Update, Source::Update]);
        assert_eq!(changes[0].after, "# Coordinating — v2\n");
        assert!(changes[1].before.starts_with(FIFTH));
    }

    #[test]
    fn neighbouring_paragraphs_overlap() {
        let yours = edit(coordinating(), FIRST, "Start by restating the goal;");
        let stock = edit(
            coordinating(),
            SECOND,
            "Split the plan into bounded tickets,",
        );
        let merged = merge(coordinating(), &stock, &yours);
        assert_eq!(merged.plain(), None);
        let [passage] = &merged.passages()[..] else {
            panic!("{merged:?}")
        };
        let (_, paragraphs) = split(coordinating());
        assert_eq!(passage.base, paragraphs[..2].concat());
        assert!(passage.yours.starts_with("Start by restating") && passage.yours.contains(SECOND));
        assert!(passage.stock.starts_with(FIRST) && passage.stock.contains("Split the plan"));
    }

    #[test]
    fn a_new_version_line_and_an_edited_first_paragraph_do_not_overlap() {
        let yours = edit(coordinating(), FIRST, "Start by restating the goal;");
        let merged = merge(coordinating(), &v2(coordinating()), &yours);
        assert_eq!(merged.plain().unwrap().0, v2(&yours));
    }

    #[test]
    fn a_change_the_human_already_copied_is_taken_once() {
        let stock = v2(&edit(coordinating(), FIFTH, "Ask the human when a waiver"));
        let yours = edit(coordinating(), FIFTH, "Ask the human when a waiver");
        let (text, changes) = merge(coordinating(), &stock, &yours).plain().unwrap();
        assert_eq!(text, stock);
        assert_eq!(changes.len(), 1, "only the title is the update's");
    }

    #[test]
    fn two_edits_within_a_run_of_lines_overlap() {
        let yours = edit(coordinating(), "For UI-facing changes", "For UI changes");
        let stock = edit(
            coordinating(),
            "After relaying an answer",
            "After passing on an answer",
        );
        let merged = merge(coordinating(), &stock, &yours);
        let [passage] = &merged.passages()[..] else {
            panic!("{merged:?}")
        };
        assert!(passage.base.contains("For UI-facing") && passage.base.contains("After relaying"));
    }

    #[test]
    fn both_sides_inserting_at_one_place_overlap() {
        let after_second = "and give a reason that names the deciding factor.\n\n";
        let stock = edit(
            coordinating(),
            after_second,
            &format!("{after_second}Stock.\n\n"),
        );
        let yours = edit(
            coordinating(),
            after_second,
            &format!("{after_second}Mine.\n\n"),
        );
        let merged = merge(coordinating(), &stock, &yours);
        let [passage] = &merged.passages()[..] else {
            panic!("{merged:?}")
        };
        assert_eq!(
            (
                passage.base.as_str(),
                passage.stock.as_str(),
                passage.yours.as_str()
            ),
            ("", "Stock.\n\n", "Mine.\n\n")
        );
    }

    #[test]
    fn the_human_deleting_a_paragraph_the_update_changed_overlaps() {
        let (_, paragraphs) = split(coordinating());
        let yours = edit(coordinating(), paragraphs[4], "");
        let stock = edit(coordinating(), FIFTH, "Ask the human when a waiver");
        let merged = merge(coordinating(), &stock, &yours);
        let [passage] = &merged.passages()[..] else {
            panic!("{merged:?}")
        };
        assert_eq!(
            (passage.base.as_str(), passage.yours.as_str()),
            (paragraphs[4], "")
        );
    }

    #[test]
    fn both_sides_changing_the_title_keeps_the_human_s() {
        let yours = edit(coordinating(), "# Coordinating — v1", "# How I coordinate");
        let (text, changes) = merge(coordinating(), &v2(coordinating()), &yours)
            .plain()
            .unwrap();
        assert_eq!((text, changes), (yours, vec![]));
    }
}
