//! A working agent's live turn, drawn into its conversation after the last message: narration,
//! edits with their diffs open and commands as they happen, and each unbroken run of quiet
//! steps folded into one line that opens in place.
use super::*;
use activity::{LinkedNarration, live_step_row};
use history_sheet::{clipped, detail, file_diff};
use std::ops::Range;
use ui::icon;
use wording::counted;

/// Diff lines an edit shows before its "N more lines".
const DIFF_PREVIEW: usize = 10;

/// One row of the live feed: a step shown on its own, or a run of quiet steps folded into one
/// line. Rows compare unequal once any of their steps changed, so only those are measured again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FeedRow {
    pub steps: Range<usize>,
    pub folded: bool,
    /// The newest revision among its steps.
    revision: u64,
}

impl FeedRow {
    /// Whether `other` is this row at a later moment: the same first step, folded or not as
    /// before, though its run may have grown or its steps changed.
    pub(super) fn same_place(&self, other: &Self) -> bool {
        (self.steps.start, self.folded) == (other.steps.start, other.folded)
    }
}

/// Reads, searches, thinking, plan updates and other tool calls fold; a failed one stays visible.
fn quiet(step: &Step) -> bool {
    !matches!(
        step.kind,
        StepKind::Narration | StepKind::FileEdit | StepKind::Command
    ) && step.state != StepState::Failed
}

/// The feed's rows, in turn order.
pub(super) fn feed(steps: &[Step]) -> Vec<FeedRow> {
    let mut rows: Vec<FeedRow> = vec![];
    for (index, step) in steps.iter().enumerate() {
        match rows.last_mut() {
            Some(run) if run.folded && quiet(step) => {
                run.steps.end = index + 1;
                run.revision = run.revision.max(step.revision);
            }
            _ => rows.push(FeedRow {
                steps: index..index + 1,
                folded: quiet(step),
                revision: step.revision,
            }),
        }
    }
    rows
}

/// "Read 4 files · searched 2 times · 1 tool call", each kind in the order it first appears; a
/// run of one step is named by what it did, such as "Read src/lib.rs".
pub(super) fn folded_label(run: &[Step]) -> String {
    if let [step] = run {
        return match step.kind {
            StepKind::Read => format!("Read {}", step.title),
            StepKind::Search => format!("Searched “{}”", step.title),
            _ => step.title.clone(),
        };
    }
    let mut kinds: Vec<(StepKind, usize)> = vec![];
    for step in run {
        match kinds.iter_mut().find(|(kind, _)| *kind == step.kind) {
            Some((_, count)) => *count += 1,
            None => kinds.push((step.kind, 1)),
        }
    }
    let label = (kinds.iter())
        .map(|&(kind, count)| match kind {
            StepKind::Read => format!("read {}", counted(count, "file")),
            StepKind::Search => format!("searched {}", counted(count, "time")),
            StepKind::Thinking => format!("thought {}", counted(count, "time")),
            StepKind::Plan => counted(count, "plan update"),
            StepKind::Tool => counted(count, "tool call"),
            _ => counted(count, "step"),
        })
        .collect::<Vec<_>>()
        .join(" · ");
    let mut chars = label.chars();
    chars.next().map_or(String::new(), |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// An edit's kept diff lines its preview leaves out.
fn hidden_lines(step: &Step) -> usize {
    (step.diffs.iter())
        .map(|diff| clipped(&diff.hunks, DIFF_PREVIEW).1)
        .sum()
}

/// The pieces of the transcript a feed row draws with.
pub(super) struct FeedContext<'a> {
    pub steps: &'a [Step],
    pub narration: &'a LinkedNarration,
    /// Folded runs opened, edits showing every kept line and steps showing their output, by
    /// the id of their first step.
    pub open: &'a BTreeSet<String>,
    pub view: WeakEntity<transcript::Transcript>,
}

pub(super) fn feed_row(row: &FeedRow, feed: &FeedContext, p: Palette) -> AnyElement {
    let run = &feed.steps[row.steps.clone()];
    let step = &run[0];
    let open = feed.open.contains(&step.id);
    let toggle = {
        let (view, id) = (feed.view.clone(), step.id.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.toggle_live(&id, cx));
        }
    };
    if row.folded {
        return folded(run, open, p).on_click(toggle).into_any_element();
    }
    match step.kind {
        StepKind::Narration => div()
            .w_full()
            .py_1()
            .children((feed.narration.view(&step.id)).map(|text| text.text_size(px(font::MESSAGE))))
            .into_any_element(),
        StepKind::FileEdit if !step.diffs.is_empty() => edit(step, open, p, toggle),
        _ => {
            let openable = step.detail.is_some();
            div()
                .id(SharedString::from(format!("live-step-{}", step.id)))
                .w_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(chevron(openable.then_some(open), p))
                        .child(live_step_row(step, p).flex_1()),
                )
                .when(openable, |d| d.cursor_pointer().on_click(toggle))
                .when(open && openable, |d| d.child(detail(step, p)))
                .into_any_element()
        }
    }
}

/// An open or closed chevron, or the room for one when there is nothing to open.
fn chevron(open: Option<bool>, p: Palette) -> Div {
    div().flex_none().w(px(14.)).when_some(open, |d, open| {
        d.child(
            icon(if open {
                "chevron-down"
            } else {
                "chevron-right"
            })
            .size(px(12.))
            .text_color(p.subtle),
        )
    })
}

/// A quiet run's one muted line and, opened, its steps.
fn folded(run: &[Step], open: bool, p: Palette) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("live-run-{}", run[0].id)))
        .w_full()
        .cursor_pointer()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(22.))
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(font::DETAIL))
                .text_color(p.subtle)
                .hover(|style| style.text_color(p.text))
                .child(chevron(Some(open), p))
                .child(
                    div()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(folded_label(run)),
                ),
        )
        .when(open, |d| {
            d.child(
                div()
                    .pl(px(18.))
                    .flex()
                    .flex_col()
                    .children(run.iter().map(|step| live_step_row(step, p))),
            )
        })
}

/// An edit with its diff open: the first lines of each file, then a toggle for the rest kept.
fn edit(
    step: &Step,
    open: bool,
    p: Palette,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let hidden = hidden_lines(step);
    let limit = (!open).then_some(DIFF_PREVIEW);
    let named = step.diffs.len() > 1;
    div()
        .w_full()
        .my_1()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(p.edge.opacity(edge::SOFT))
        .bg(p.surface)
        .flex()
        .flex_col()
        .child(div().px_2().child(live_step_row(step, p)))
        .children(
            (step.diffs.iter())
                .map(|diff| div().px_1().pb_1().child(file_diff(diff, named, limit, p))),
        )
        .when(hidden > 0, |d| {
            d.child(
                div()
                    .id(SharedString::from(format!("live-more-{}", step.id)))
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .border_t_1()
                    .border_color(p.edge.opacity(edge::SEPARATOR))
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .hover(|style| style.text_color(p.text))
                    .child(icon(if open { "chevron-up" } else { "chevron-down" }).size(px(12.)))
                    .child(if open {
                        "Fewer lines".to_owned()
                    } else {
                        counted(hidden, "more line")
                    })
                    .on_click(toggle),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{FeedRow, feed, folded_label, hidden_lines};
    use crate::history_sheet::clipped;
    use workspace_core::{DiffHunk, FileDiff, Step, StepKind, StepState};

    fn step(kind: StepKind, seq: u32) -> Step {
        Step {
            run_id: "run".into(),
            id: format!("s{seq}"),
            parent_id: None,
            kind,
            state: StepState::Succeeded,
            title: format!("s{seq}"),
            note: None,
            detail: None,
            paths: vec![],
            plan: None,
            diffs: vec![],
            omitted: 0,
            seq,
            revision: seq as u64 + 1,
            started_at: 0,
            finished_at: None,
        }
    }

    fn steps(kinds: &[StepKind]) -> Vec<Step> {
        (kinds.iter().enumerate())
            .map(|(seq, kind)| step(*kind, seq as u32))
            .collect()
    }

    /// Each row as its step range and whether it folds.
    fn shape(rows: &[FeedRow]) -> Vec<(std::ops::Range<usize>, bool)> {
        rows.iter().map(|r| (r.steps.clone(), r.folded)).collect()
    }

    #[test]
    fn quiet_steps_fold_per_run_between_narration_edits_and_commands() {
        use StepKind::*;
        let run = steps(&[
            Read, Search, Tool, Narration, Read, FileEdit, Command, Thinking, Plan, Narration,
        ]);
        assert_eq!(
            shape(&feed(&run)),
            [
                (0..3, true),
                (3..4, false),
                (4..5, true),
                (5..6, false),
                (6..7, false),
                (7..9, true),
                (9..10, false),
            ]
        );
    }

    #[test]
    fn a_failed_quiet_step_breaks_out_of_its_run_but_a_failed_command_stays_one_row() {
        use StepKind::*;
        let mut run = steps(&[Read, Search, Read, Command]);
        run[1].state = StepState::Failed;
        run[3].state = StepState::Failed;
        assert_eq!(
            shape(&feed(&run)),
            [(0..1, true), (1..2, false), (2..3, true), (3..4, false)]
        );
    }

    #[test]
    fn a_run_that_grows_or_changes_compares_unequal_and_earlier_rows_stay_equal() {
        use StepKind::*;
        let mut run = steps(&[Narration, Read, Read]);
        let before = feed(&run);
        run.push(step(Search, 3));
        let grown = feed(&run);
        assert_eq!(before[0], grown[0]);
        assert_ne!(before[1], grown[1]);
        assert!(
            before[1].same_place(&grown[1]),
            "a grown run keeps its place"
        );
        // A failed read splits the run: the rows after it are new.
        run.push(step(Read, 4));
        run[3].state = StepState::Failed;
        let split = feed(&run);
        assert!(split[1].same_place(&grown[1]));
        assert_eq!(split.len(), 4);
        run[1].revision = 10;
        assert_ne!(feed(&run)[1], grown[1]);
    }

    #[test]
    fn a_folded_line_counts_each_kind_in_order_and_one_step_says_what_it_did() {
        use StepKind::*;
        let run = steps(&[Read, Search, Read, Tool, Read, Search]);
        assert_eq!(
            folded_label(&run),
            "Read 3 files · searched 2 times · 1 tool call"
        );
        assert_eq!(
            folded_label(&steps(&[Thinking, Plan, Plan])),
            "Thought 1 time · 2 plan updates"
        );
        let mut one = step(Read, 0);
        one.title = "src/protected.rs".into();
        assert_eq!(folded_label(&[one]), "Read src/protected.rs");
        let mut tool = step(Tool, 0);
        tool.title = "workspace_context".into();
        assert_eq!(folded_label(&[tool]), "workspace_context");
    }

    #[test]
    fn a_diff_preview_takes_its_first_lines_across_hunks() {
        let hunk = |lines: usize| DiffHunk {
            old_start: 1,
            new_start: 1,
            lines: vec![" x".into(); lines],
        };
        let hunks = [hunk(6), hunk(7), hunk(3)];
        assert_eq!(clipped(&hunks, 10), (vec![6, 4, 0], 6));
        assert_eq!(clipped(&hunks, 40), (vec![6, 7, 3], 0));
        let mut edit = step(StepKind::FileEdit, 0);
        let file = |hunks: Vec<DiffHunk>| FileDiff {
            path: "a.rs".into(),
            created: false,
            added: 0,
            removed: 0,
            hunks,
            omitted_lines: 12,
        };
        edit.diffs = vec![file(vec![hunk(14)]), file(vec![hunk(4)])];
        // Lines never kept are not counted as hidden; they stay the diff's own note.
        assert_eq!(hidden_lines(&edit), 4);
    }
}
