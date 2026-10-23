//! One turn's full history in a sheet over the right side of the window: each narration
//! paragraph followed by the steps that came after it, updating live while the turn runs.
use super::*;
use activity::{
    RunActivity, STEP_PACE, StepFetch, line_counts, paced_reply, state_glyph, step_row,
};
use file_links::Linked;
use gpui_component::ActiveTheme;
use std::{collections::HashMap, sync::Arc};
use ui::icon;
use wording::{clock, counted, duration};

pub(super) struct HistorySheet {
    bridge: Bridge,
    session_id: String,
    run_id: String,
    run: RunActivity,
    fetch: StepFetch,
    list: ListState,
    /// Steps the reader opened or closed; an edit's diff starts open, other details closed.
    toggled: BTreeSet<String>,
    /// Where the session's relative file references resolve.
    link_bases: Arc<[PathBuf]>,
    /// Each narration step's text with its file references linked, by step id.
    narration: HashMap<String, Linked>,
}

impl HistorySheet {
    pub(crate) fn new(
        bridge: Bridge,
        session_id: &str,
        run_id: String,
        link_bases: Arc<[PathBuf]>,
    ) -> Self {
        Self {
            bridge,
            session_id: session_id.into(),
            run_id,
            run: RunActivity::new(session_id),
            fetch: StepFetch::default(),
            list: ListState::new(0, ListAlignment::Bottom, px(200.)),
            toggled: BTreeSet::new(),
            link_bases,
            narration: HashMap::new(),
        }
    }

    /// Loads the turn's steps changed since the last page; a finished turn loads once.
    pub(crate) fn fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let finished = self.run.run_id.is_some() && !self.run.running;
        if finished {
            return;
        }
        let Some(wait) = self.fetch.begin(STEP_PACE) else {
            return;
        };
        let ask = ask::steps(
            self.session_id.clone(),
            Some(self.run_id.clone()),
            self.run.revision,
        );
        let page = paced_reply(
            self.bridge.clone(),
            ask,
            wait,
            cx.background_executor().clone(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = page.await;
            let _ = cx
                .update(|window, cx| this.update(cx, |sheet, cx| sheet.accept(result, window, cx)));
        })
        .detach();
    }

    fn accept(
        &mut self,
        result: Result<StepPage, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owed = self.fetch.finish();
        match result {
            Ok(page) => {
                let before: Vec<_> = self
                    .run
                    .steps
                    .iter()
                    .map(|s| (s.id.clone(), s.revision))
                    .collect();
                self.run.merge(page);
                // Rows before the first change keep their measured heights and the reader's place.
                let unchanged = before
                    .iter()
                    .zip(&self.run.steps)
                    .take_while(|((id, revision), step)| {
                        *id == step.id && *revision == step.revision
                    })
                    .count();
                // Narration is linked here, as it arrives, so drawing it asks the host nothing.
                for step in &self.run.steps[unchanged..] {
                    if step.kind == StepKind::Narration {
                        let id = format!("{}-{}", step.run_id, step.id);
                        let text = step.detail.as_deref().unwrap_or(&step.title);
                        let linked = Linked::new(&id, text, self.link_bases.clone(), &self.bridge);
                        self.narration.insert(step.id.clone(), linked);
                    }
                }
                file_links::link_when_resolved(self.narration.values(), &self.bridge, cx);
                let (shown, rows) = (self.list.item_count(), self.run.steps.len());
                if unchanged < rows || shown != rows {
                    self.list.splice(unchanged..shown, rows - unchanged);
                }
                cx.notify();
            }
            Err(error) => eprintln!("Turn history: {error}"),
        }
        if owed {
            self.fetch(window, cx);
        }
    }

    fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.toggled.remove(id) {
            self.toggled.insert(id.to_owned());
        }
        if let Some(row) = self.run.steps.iter().position(|s| s.id == id) {
            self.list.splice(row..row + 1, 1);
        }
        cx.notify();
    }

    fn summary(&self) -> String {
        let actions = self.run.steps.iter().filter(|s| s.kind.is_action()).count();
        let failed = self
            .run
            .steps
            .iter()
            .filter(|s| s.state == StepState::Failed)
            .count();
        let time = match (self.run.running, self.run.started_at, self.run.finished_at) {
            (true, started, _) => format!("Working · {}", clock(started)),
            (false, Some(started), Some(finished)) => {
                format!("Worked {}", duration(finished - started))
            }
            _ => "Loading…".into(),
        };
        let failed = if failed > 0 {
            format!(" · {failed} failed")
        } else {
            String::new()
        };
        format!("{time} · {}{failed}", counted(actions, "step"))
    }

    fn row(&self, index: usize, p: Palette, view: WeakEntity<Self>) -> AnyElement {
        let Some(step) = self.run.steps.get(index) else {
            return div().into_any_element();
        };
        match step.kind {
            StepKind::Narration => div()
                .w_full()
                .pt_3()
                .pb_1()
                .children(self.narration.get(&step.id).map(Linked::view))
                .into_any_element(),
            StepKind::Thinking => div()
                .w_full()
                .pt_2()
                .text_size(px(font::SMALL))
                .italic()
                .text_color(p.subtle)
                .child(step.title.clone())
                .into_any_element(),
            _ => {
                let has_diff = !step.diffs.is_empty();
                let openable = has_diff || step.detail.is_some();
                let open = openable && has_diff != self.toggled.contains(&step.id);
                let chevron = div().flex_none().w(px(14.)).when(openable, |d| {
                    d.child(
                        icon(if open {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        })
                        .size(px(12.))
                        .text_color(p.subtle),
                    )
                });
                let id = step.id.clone();
                div()
                    .id(SharedString::from(format!("history-step-{}", step.id)))
                    .w_full()
                    // Subagent work sits under the call that started it.
                    .pl(px(if step.parent_id.is_some() { 26. } else { 8. }))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(chevron)
                            .child(step_row(step, p).flex_1()),
                    )
                    .when(openable, |d| {
                        d.cursor_pointer().on_click(move |_, _, cx| {
                            let _ = view.update(cx, |sheet, cx| sheet.toggle(&id, cx));
                        })
                    })
                    .when(open, |d| {
                        d.child(if has_diff {
                            diffs(step, p)
                        } else {
                            detail(step, p)
                        })
                    })
                    .into_any_element()
            }
        }
    }
}

/// A step's bounded output, with a note when part of it was dropped.
fn detail(step: &Step, p: Palette) -> Div {
    let omitted = (step.omitted > 0).then(|| {
        if step.kind.is_action() {
            format!("Earlier output omitted ({} bytes)", step.omitted)
        } else {
            format!("{} more bytes not kept", step.omitted)
        }
    });
    div()
        .ml(px(38.))
        .mt_1()
        .mb_2()
        .px_2()
        .py(px(6.))
        .rounded_md()
        .bg(p.overlay)
        .flex()
        .flex_col()
        .gap_1()
        .when_some(omitted, |d, note| {
            d.child(
                div()
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(note),
            )
        })
        .child(
            div()
                .font_family(".SystemMonoFont")
                .text_size(px(font::CAPTION))
                .text_color(p.subtle)
                .child(step.detail.clone().unwrap_or_default()),
        )
}

/// An edit's files: each one's hunks with line numbers, and a note for the lines not kept.
/// A single file is already named by the step's row.
fn diffs(step: &Step, p: Palette) -> Div {
    let named = step.diffs.len() > 1;
    div()
        .ml(px(38.))
        .mt_1()
        .mb_2()
        .flex()
        .flex_col()
        .gap_2()
        .children(step.diffs.iter().map(|diff| file_diff(diff, named, p)))
}

fn file_diff(diff: &FileDiff, named: bool, p: Palette) -> Div {
    let header = |text: String, first: bool| {
        div()
            .px_2()
            .py_1()
            .text_color(p.subtle)
            .when(!first, |d| {
                d.border_t_1().border_color(p.edge.opacity(edge::SEPARATOR))
            })
            .child(text)
    };
    let more = (diff.omitted_lines > 0).then(|| counted(diff.omitted_lines as usize, "more line"));
    div()
        .rounded_md()
        .overflow_hidden()
        .bg(p.overlay)
        .flex()
        .flex_col()
        .font_family(".SystemMonoFont")
        .text_size(px(font::CAPTION))
        .when(named, |d| {
            d.child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .gap_2()
                    .text_color(p.text)
                    .child(diff.path.clone())
                    .child(line_counts((diff.added, diff.removed), p)),
            )
        })
        .children(diff.hunks.iter().enumerate().flat_map(|(index, hunk)| {
            let title = if diff.created {
                "new file".to_owned()
            } else {
                format!("@@ line {}", hunk.new_start)
            };
            std::iter::once(header(title, index == 0 && !named)).chain(hunk_lines(hunk, p))
        }))
        .when_some(more, |d, more| {
            d.child(
                div()
                    .px_2()
                    .py_1()
                    .font_family(".SystemUIFont")
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(more),
            )
        })
}

/// Each line's number in the file, its sign and its text: the old number for a removal, the
/// new one otherwise.
fn numbered(hunk: &DiffHunk) -> impl Iterator<Item = (u32, &str, &str)> {
    let (mut old, mut new) = (hunk.old_start, hunk.new_start);
    hunk.lines.iter().map(move |line| {
        let (sign, text) = line.split_at(line.chars().next().map_or(0, char::len_utf8));
        let number = if sign == "-" { old } else { new };
        match sign {
            "+" => new += 1,
            "-" => old += 1,
            _ => (old, new) = (old + 1, new + 1),
        }
        (number, sign, text)
    })
}

fn hunk_lines(hunk: &DiffHunk, p: Palette) -> impl Iterator<Item = Div> + '_ {
    numbered(hunk).map(move |(number, sign, text)| {
        let (color, fill) = match sign {
            "+" => (p.green, Some(p.green.opacity(0.14))),
            "-" => (p.red, Some(p.red.opacity(0.14))),
            _ => (p.subtle, None),
        };
        div()
            .flex()
            .whitespace_nowrap()
            .when_some(fill, |d, fill| d.bg(fill))
            .child(
                div()
                    .flex_none()
                    .w(px(38.))
                    .pr_2()
                    .flex()
                    .justify_end()
                    .text_color(p.subtle)
                    .child(number.to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(14.))
                    .text_color(color)
                    .child(sign.to_owned()),
            )
            .child(
                div()
                    .min_w_0()
                    .text_color(p.text)
                    .child(text.replace('\t', "    ")),
            )
    })
}

impl Render for HistorySheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx.theme().is_dark());
        let view = cx.entity().downgrade();
        let running = self.run.running;
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .pb_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(font::SMALL))
                    .text_color(p.subtle)
                    .child(if running {
                        state_glyph(StepState::Running, p)
                    } else {
                        state_glyph(StepState::Succeeded, p)
                    })
                    .child(self.summary()),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        list(self.list.clone(), move |index, _, cx| {
                            let Some(sheet) = view.upgrade() else {
                                return div().into_any_element();
                            };
                            sheet.read(cx).row(index, p, view.clone())
                        })
                        .size_full(),
                    )
                    .vertical_scrollbar(&self.list),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::numbered;
    use workspace_core::DiffHunk;

    #[test]
    fn a_removal_shows_its_old_number_and_every_other_line_its_new_one() {
        let hunk = |old_start, new_start, lines: &[&str]| DiffHunk {
            old_start,
            new_start,
            lines: lines.iter().map(|l| l.to_string()).collect(),
        };
        let numbers = |hunk: &DiffHunk| -> Vec<(u32, String)> {
            numbered(hunk)
                .map(|(n, sign, _)| (n, sign.into()))
                .collect()
        };
        let expected = |list: &[(u32, &str)]| -> Vec<(u32, String)> {
            list.iter().map(|&(n, sign)| (n, sign.into())).collect()
        };
        // Each hunk counts from its own header, not from where the one before ended.
        let first = hunk(40, 40, &[" a", "-b", "-c", "+b", "+c", "+d", " e"]);
        let second = hunk(90, 91, &["-x", "+y", " z"]);
        assert_eq!(
            numbers(&first),
            expected(&[
                (40, " "),
                (41, "-"),
                (42, "-"),
                (41, "+"),
                (42, "+"),
                (43, "+"),
                (44, " ")
            ])
        );
        assert_eq!(
            numbers(&second),
            expected(&[(90, "-"), (91, "+"), (92, " ")])
        );
        assert_eq!(numbered(&first).nth(1).unwrap().2, "b");
    }
}
