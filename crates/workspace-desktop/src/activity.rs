//! A working agent's live activity: the steps of its current run, fetched as they change and
//! shown as a fixed-height card after the transcript.
use super::*;
use gpui_component::{button::ButtonVariants, spinner::Spinner};
use std::time::{Duration, Instant};
use ui::{icon, mono, now};

/// Step fetches start no closer together than this, about 30 a second.
const STEP_PACE: Duration = Duration::from_millis(33);
/// The card and its minimized line keep fixed heights, so streaming never moves the transcript.
const CARD_HEIGHT: f32 = 128.;
const LINE_HEIGHT: f32 = 40.;
const STEP_ROW: f32 = 22.;
/// Recent actions shown under the headline.
const RECENT: usize = 3;

/// One session's latest run, as this client has seen it.
#[derive(Clone, Debug, Default)]
pub(super) struct RunActivity {
    pub session_id: String,
    pub run_id: Option<String>,
    pub running: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub revision: u64,
    pub steps: Vec<Step>,
}
impl RunActivity {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.into(),
            ..Default::default()
        }
    }
    /// Applies a page of changes; a different run replaces the steps shown.
    pub fn merge(&mut self, page: StepPage) {
        if page.run_id != self.run_id {
            self.steps.clear();
            self.run_id = page.run_id;
        }
        self.running = page.running;
        self.started_at = page.started_at;
        self.finished_at = page.finished_at;
        self.revision = self.revision.max(page.revision);
        for step in page.steps {
            match self.steps.iter_mut().find(|s| s.id == step.id) {
                Some(shown) => *shown = step,
                None => self.steps.push(step),
            }
        }
        self.steps.sort_by_key(|s| s.seq);
    }
    /// What the card shows; nothing from a previous run while the next one starts.
    pub fn card(&self) -> Card {
        if !self.running {
            return Card::default();
        }
        let actions: Vec<&Step> = self.steps.iter().filter(|s| s.kind.is_action()).collect();
        Card {
            run_id: self.run_id.clone(),
            started_at: self.started_at,
            actions: actions.len(),
            headline: self
                .steps
                .iter()
                .rev()
                .find(|s| s.kind == StepKind::Narration)
                .map(|s| s.title.clone()),
            recent: actions[actions.len().saturating_sub(RECENT)..]
                .iter()
                .map(|s| (*s).clone())
                .collect(),
            current: self.steps.last().cloned(),
        }
    }
}

/// The few values the activity card draws, copied out once per frame.
#[derive(Clone, Debug, Default)]
pub(super) struct Card {
    run_id: Option<String>,
    started_at: Option<i64>,
    actions: usize,
    headline: Option<String>,
    recent: Vec<Step>,
    current: Option<Step>,
}

/// At most one step request is in flight; changes during it trigger one more.
#[derive(Default)]
pub(super) struct StepFetch {
    in_flight: bool,
    dirty: bool,
    started: Option<Instant>,
}
impl StepFetch {
    /// Claims the next request and says how long to wait to keep the pace, or records that
    /// one more is owed when a request is already out.
    pub fn begin(&mut self) -> Option<Duration> {
        if self.in_flight {
            self.dirty = true;
            return None;
        }
        let wait = self.started.map_or(Duration::ZERO, |started| {
            STEP_PACE.saturating_sub(started.elapsed())
        });
        self.in_flight = true;
        self.started = Some(Instant::now() + wait);
        Some(wait)
    }
    /// Ends the request; true when changes arrived meanwhile and another is owed.
    pub fn finish(&mut self) -> bool {
        self.in_flight = false;
        std::mem::take(&mut self.dirty)
    }
}

/// Waits out the pace, then asks the host for a page of steps.
pub(super) async fn step_page(
    bridge: Bridge,
    command: Command,
    wait: Duration,
    executor: BackgroundExecutor,
) -> Result<StepPage, String> {
    if !wait.is_zero() {
        executor.timer(wait).await;
    }
    let value = bridge
        .request(command)?
        .recv()
        .await
        .map_err(|_| "Host disconnected".to_string())??;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

impl Workspace {
    pub(super) fn watch_steps(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(changes) = self.bridge.step_changes() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            while changes.recv().await.is_ok() {
                let fetched = cx
                    .update(|window, cx| this.update(cx, |view, cx| view.fetch_steps(window, cx)));
                if !matches!(fetched, Ok(Ok(()))) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Asks for the selected session's steps changed since the last page.
    pub(super) fn fetch_steps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected.clone() else {
            return;
        };
        let Some(wait) = self.step_fetch.begin() else {
            return;
        };
        let after = self
            .activity
            .as_ref()
            .filter(|a| a.session_id == session_id)
            .map_or(0, |a| a.revision);
        let command = Command::Steps {
            session_id: session_id.clone(),
            run_id: None,
            after,
        };
        let page = step_page(
            self.bridge.clone(),
            command,
            wait,
            cx.background_executor().clone(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = page.await;
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.accept_steps(&session_id, result, window, cx)
                })
            });
        })
        .detach();
        // The open history sheet follows the same changes.
        if let Some(sheet) = self.history.as_ref().and_then(WeakEntity::upgrade) {
            sheet.update(cx, |sheet, cx| sheet.fetch(window, cx));
        }
    }

    fn accept_steps(
        &mut self,
        session_id: &str,
        result: Result<StepPage, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owed = self.step_fetch.finish();
        match result {
            Ok(page) if self.selected.as_deref() == Some(session_id) => {
                let activity = match &mut self.activity {
                    Some(activity) if activity.session_id == session_id => activity,
                    other => other.insert(RunActivity::new(session_id)),
                };
                activity.merge(page);
                cx.notify();
            }
            Ok(_) => {}
            Err(error) => eprintln!("Work steps: {error}"),
        }
        if owed {
            self.fetch_steps(window, cx);
        }
    }

    /// Ticks the elapsed time once a second, only while the selected session works.
    pub(super) fn sync_activity_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_clock || !self.selected_working() {
            return;
        }
        self.activity_clock = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let ticking = this.update(cx, |view, cx| {
                    view.activity_clock = view.selected_working();
                    if view.activity_clock {
                        cx.notify();
                        if let Some(sheet) = view.history.as_ref().and_then(WeakEntity::upgrade) {
                            sheet.update(cx, |_, cx| cx.notify());
                        }
                    }
                    view.activity_clock
                });
                if !matches!(ticking, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn selected_working(&self) -> bool {
        self.page.is_none()
            && self
                .selected_session()
                .is_some_and(|s| s.status == Status::Working)
    }

    pub(super) fn activity_card(&self) -> Card {
        self.activity
            .as_ref()
            .filter(|a| self.selected.as_ref() == Some(&a.session_id))
            .map(RunActivity::card)
            .unwrap_or_default()
    }

    /// Asks for the summaries of finished turns on the loaded page that are not known yet.
    pub(super) fn load_run_summaries(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected.clone() else {
            return;
        };
        let run_ids: Vec<String> = self
            .messages
            .iter()
            .filter_map(|m| m.id.strip_prefix("output:"))
            .filter(|run| !self.run_summaries.contains_key(*run))
            .map(str::to_owned)
            .collect();
        if !run_ids.is_empty() {
            self.request(
                Command::RunSummaries {
                    session_id,
                    run_ids,
                },
                window,
                cx,
            );
        }
    }

    pub(super) fn accept_run_summaries(&mut self, summaries: Vec<RunSummary>) {
        // Finished runs never change, so the cache only needs a bound.
        if self.run_summaries.len() > 1_000 {
            self.run_summaries.clear();
        }
        for summary in summaries {
            let reply = format!("output:{}", summary.run_id);
            self.run_summaries.insert(summary.run_id.clone(), summary);
            // The reply's row gains a summary line; the list measures it again.
            if let Some(row) = self.messages.iter().position(|m| m.id == reply)
                && row < self.list.item_count()
            {
                self.list.splice(row..row + 1, 1);
            }
        }
    }

    fn toggle_activity(&mut self, cx: &mut Context<Self>) {
        self.preferences.minimize_activity = !self.preferences.minimize_activity;
        if let Err(error) = self.preferences.save() {
            eprintln!("Saving preferences: {error}");
        }
        // The trailing row changed height; the list measures it again.
        let shown = self.messages.len();
        if self.list.item_count() > shown {
            self.list.splice(shown..shown + 1, 1);
        }
        cx.notify();
    }
}

pub(super) fn kind_icon(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Command => "terminal",
        StepKind::FileEdit => "pencil",
        StepKind::Read => "file-text",
        StepKind::Search => "search",
        StepKind::Plan => "list-checks",
        StepKind::Tool => "wrench",
        StepKind::Narration | StepKind::Thinking => "ellipsis",
    }
}

pub(super) fn state_glyph(state: StepState, p: Palette) -> AnyElement {
    let glyph = |name: &str, color: Hsla| icon(name).size(px(13.)).text_color(color);
    match state {
        StepState::Running => Spinner::new().xsmall().color(p.accent).into_any_element(),
        StepState::Succeeded => glyph("check", p.green).into_any_element(),
        StepState::Failed => glyph("close", p.red).into_any_element(),
        StepState::Interrupted => glyph("stop", p.subtle).into_any_element(),
    }
}

/// A one-line step: state, kind, title and its note, or "running".
pub(super) fn step_row(step: &Step, p: Palette) -> Div {
    let note = if step.state == StepState::Running {
        Some("running".to_owned())
    } else {
        step.note.clone()
    };
    div()
        .h(px(STEP_ROW))
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(px(14.))
                .flex()
                .justify_center()
                .child(state_glyph(step.state, p)),
        )
        .child(
            icon(kind_icon(step.kind))
                .size(px(13.))
                .text_color(p.subtle)
                .flex_none(),
        )
        .child(
            mono(step.title.clone(), p)
                .flex_1()
                .text_color(p.text)
                .whitespace_nowrap(),
        )
        .when_some(note, |d, note| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(10.5))
                    .text_color(p.subtle)
                    .child(note),
            )
        })
}

/// Elapsed time of a running turn, "4:18".
pub(super) fn clock(started_at: Option<i64>) -> String {
    let seconds = started_at.map_or(0, |at| (now() - at).max(0) / 1000);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// How long a finished turn took, "42s", "9m 12s" or "1h 04m".
pub(super) fn duration(milliseconds: i64) -> String {
    let seconds = milliseconds.max(0) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds / 60 % 60),
    }
}

/// "23 steps", "1 step".
pub(super) fn step_count(actions: usize) -> String {
    format!("{actions} step{}", if actions == 1 { "" } else { "s" })
}

/// "Worked 9m 12s · 23 steps", above a finished turn's reply; it opens that turn's history.
pub(super) fn turn_summary(
    summary: &RunSummary,
    p: Palette,
    view: WeakEntity<Workspace>,
) -> Stateful<Div> {
    let took = summary.finished_at.map_or(String::new(), |finished| {
        format!("Worked {} · ", duration(finished - summary.started_at))
    });
    let run_id = summary.run_id.clone();
    div()
        .id(SharedString::from(format!(
            "turn-summary-{}",
            summary.run_id
        )))
        .ml(px(38.))
        .mb_1()
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(11.5))
        .text_color(p.subtle)
        .cursor_pointer()
        .hover(|style| style.text_color(p.text))
        .child(icon("chevron-right").size(px(12.)))
        .child(format!("{took}{}", step_count(summary.actions)))
        .when(summary.failed > 0, |d| {
            d.child(
                div()
                    .text_color(p.red)
                    .child(format!("· {} failed", summary.failed)),
            )
        })
        .on_click(move |_, window, cx| {
            let _ = view.update(cx, |view, cx| view.open_history(run_id.clone(), window, cx));
        })
}

/// The row after the transcript while the selected agent works.
pub(super) fn activity_row(
    card: &Card,
    minimized: bool,
    p: Palette,
    view: WeakEntity<Workspace>,
) -> Stateful<Div> {
    let stats = format!(
        "· {} · {}",
        clock(card.started_at),
        step_count(card.actions)
    );
    let current = card
        .current
        .as_ref()
        .map_or("Starting…".to_owned(), |s| s.title.clone());
    let toggle = {
        let view = view.clone();
        button("activity-toggle")
            .ghost()
            .xsmall()
            .icon(
                icon(if minimized {
                    "chevron-down"
                } else {
                    "chevron-up"
                })
                .size(px(14.)),
            )
            .tooltip(if minimized {
                "Show live activity"
            } else {
                "Minimize live activity"
            })
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let _ = view.update(cx, |view, cx| view.toggle_activity(cx));
            })
    };
    let header = div()
        .h(px(24.))
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(Spinner::new().xsmall().color(p.accent))
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Working"),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(p.subtle)
                .child(stats),
        )
        .when(minimized, |d| {
            d.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(format!("· {current}")),
            )
        })
        .when(!minimized, |d| {
            d.child(div().flex_1()).child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .text_size(px(11.5))
                    .text_color(p.subtle)
                    .child("View all")
                    .child(icon("chevron-right").size(px(12.))),
            )
        })
        .child(toggle);
    let body = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(22.))
                .mt_1()
                .text_size(px(13.))
                .whitespace_nowrap()
                .text_ellipsis()
                .child(card.headline.clone().unwrap_or_else(|| "Starting…".into())),
        )
        .children(card.recent.iter().map(|step| step_row(step, p)));
    let run_id = card.run_id.clone();
    div()
        .id("activity-card")
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            if let Some(run_id) = run_id.clone() {
                let _ = view.update(cx, |view, cx| view.open_history(run_id, window, cx));
            }
        })
        .w_full()
        .h(px(if minimized { LINE_HEIGHT } else { CARD_HEIGHT }))
        .overflow_hidden()
        .px_3()
        .py(px(7.))
        .rounded_xl()
        .border_1()
        .border_color(p.edge.opacity(0.45))
        .bg(p.surface)
        .flex()
        .flex_col()
        .child(header)
        .when(!minimized, |d| d.child(body))
}

#[cfg(test)]
mod tests {
    use super::RunActivity;
    use workspace_core::{Step, StepKind, StepPage, StepState};

    fn step(id: &str, seq: u32, kind: StepKind, revision: u64) -> Step {
        Step {
            run_id: "run".into(),
            id: id.into(),
            parent_id: None,
            kind,
            state: StepState::Succeeded,
            title: id.into(),
            note: None,
            detail: None,
            omitted: 0,
            seq,
            revision,
            started_at: 0,
            finished_at: None,
        }
    }

    fn page(run: &str, steps: Vec<Step>) -> StepPage {
        StepPage {
            run_id: Some(run.into()),
            running: true,
            revision: steps.iter().map(|s| s.revision).max().unwrap_or_default(),
            steps,
            ..Default::default()
        }
    }

    #[test]
    fn pages_update_steps_in_place_and_a_new_run_starts_clean() {
        let mut activity = RunActivity::new("session");
        activity.merge(page(
            "a",
            vec![
                step("say", 0, StepKind::Narration, 1),
                step("ls", 1, StepKind::Command, 2),
            ],
        ));
        let mut changed = step("say", 0, StepKind::Narration, 3);
        changed.title = "Checking tests".into();
        activity.merge(page("a", vec![changed]));
        assert_eq!(
            activity
                .steps
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Checking tests", "ls"]
        );
        assert_eq!(activity.revision, 3);
        activity.merge(page("b", vec![step("cat", 0, StepKind::Command, 4)]));
        assert_eq!(activity.steps.len(), 1);
    }

    #[test]
    fn the_card_shows_the_latest_narration_and_three_latest_actions() {
        let mut activity = RunActivity::new("session");
        let mut steps = vec![step("first", 0, StepKind::Narration, 1)];
        steps.extend((1..=5).map(|n| step(&format!("cmd{n}"), n, StepKind::Command, n as u64 + 1)));
        steps.push(step("latest", 6, StepKind::Narration, 7));
        activity.merge(page("a", steps));
        let card = activity.card();
        assert_eq!(card.headline.as_deref(), Some("latest"));
        assert_eq!(card.actions, 5);
        assert_eq!(
            card.recent
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["cmd3", "cmd4", "cmd5"]
        );
        activity.running = false;
        assert_eq!(activity.card().actions, 0);
    }
}
