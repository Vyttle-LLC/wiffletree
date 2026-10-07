//! One turn's full history in a sheet over the right side of the window: each narration
//! paragraph followed by the steps that came after it, updating live while the turn runs.
use super::*;
use activity::{
    RunActivity, StepFetch, clock, duration, state_glyph, step_count, step_page, step_row,
};
use conversation::markdown;
use gpui_component::ActiveTheme;
use ui::icon;

pub(super) struct HistorySheet {
    bridge: Bridge,
    session_id: String,
    run_id: String,
    run: RunActivity,
    fetch: StepFetch,
    list: ListState,
    expanded: BTreeSet<String>,
}

impl HistorySheet {
    pub fn new(bridge: Bridge, session_id: &str, run_id: String) -> Self {
        Self {
            bridge,
            session_id: session_id.into(),
            run_id,
            run: RunActivity::new(session_id),
            fetch: StepFetch::default(),
            list: ListState::new(0, ListAlignment::Bottom, px(200.)),
            expanded: BTreeSet::new(),
        }
    }

    /// Loads the turn's steps changed since the last page; a finished turn loads once.
    pub fn fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let finished = self.run.run_id.is_some() && !self.run.running;
        if finished {
            return;
        }
        let Some(wait) = self.fetch.begin() else {
            return;
        };
        let command = Command::Steps {
            session_id: self.session_id.clone(),
            run_id: Some(self.run_id.clone()),
            after: self.run.revision,
        };
        let page = step_page(
            self.bridge.clone(),
            command,
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
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
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
        format!("{time} · {}{failed}", step_count(actions))
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
                .child(markdown(
                    &format!("{}-{}", step.run_id, step.id),
                    step.detail.as_deref().unwrap_or(&step.title),
                ))
                .into_any_element(),
            StepKind::Thinking => div()
                .w_full()
                .pt_2()
                .text_size(px(12.))
                .italic()
                .text_color(p.subtle)
                .child(step.title.clone())
                .into_any_element(),
            _ => {
                let open = self.expanded.contains(&step.id);
                let chevron = div()
                    .flex_none()
                    .w(px(14.))
                    .when(step.detail.is_some(), |d| {
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
                    .when(step.detail.is_some(), |d| {
                        d.cursor_pointer().on_click(move |_, _, cx| {
                            let _ = view.update(cx, |sheet, cx| sheet.toggle(&id, cx));
                        })
                    })
                    .when(open, |d| d.child(detail(step, p)))
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
            d.child(div().text_size(px(10.5)).text_color(p.subtle).child(note))
        })
        .child(
            div()
                .font_family(".SystemMonoFont")
                .text_size(px(11.))
                .text_color(p.subtle)
                .child(step.detail.clone().unwrap_or_default()),
        )
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
                    .text_size(px(12.))
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

impl Workspace {
    /// Opens a turn's history over the conversation; the draft and selection stay as they are.
    pub(super) fn open_history(
        &mut self,
        run_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let sheet = cx.new(|_| HistorySheet::new(self.bridge.clone(), &session.id, run_id));
        sheet.update(cx, |sheet, cx| sheet.fetch(window, cx));
        self.history = Some(sheet.downgrade());
        let title = format!("{} · this turn", session.name);
        window.open_sheet(cx, move |panel, _, _| {
            panel
                .size(px(560.))
                .title(title.clone())
                .child(sheet.clone())
        });
    }
}
