//! One UI evaluator run's page (screen 5 of `ui-evaluators`' study): its pinned model, mode,
//! folder, who started it and what it has, its recent steps with their screenshots, and Stop.
//! The steps come from the run's stored transcript and show only here: the coordinator never
//! sees them.
use super::*;
use gpui_component::button::ButtonVariants;
use std::path::Path;
use ui::{hint, icon, mono, page_header};

/// How many of the latest steps the page lists.
const RECENT_STEPS: usize = 12;

pub(super) struct RunPage {
    bridge: Bridge,
    shared: Shared,
    run_id: Option<String>,
    /// The latest turn's steps of the run's session, as last read.
    steps: Option<(String, StepPage)>,
}
impl EventEmitter<Intent> for RunPage {}
impl Requests for RunPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        _: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = reply {
            toast_error(error, window, cx);
        }
    }
}
impl RunPage {
    pub(super) fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            shared: Shared::default(),
            run_id: None,
            steps: None,
        }
    }
    pub(super) fn open(&mut self, run_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.run_id.as_ref() != Some(&run_id) {
            self.steps = None;
        }
        self.run_id = Some(run_id);
        self.fetch_steps(window, cx);
    }
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.run().cloned();
        self.shared = shared;
        if self.shared.page == Page::Evaluation && self.run() != before.as_ref() {
            self.fetch_steps(window, cx);
        }
        cx.notify();
    }
    fn run(&self) -> Option<&Evaluation> {
        self.shared.evaluation(self.run_id.as_deref()?)
    }
    /// The run's session, whose steps the page lists.
    pub(super) fn session_id(&self) -> Option<&str> {
        self.run()?.session_id.as_deref()
    }
    /// Reads the run's latest steps again.
    pub(super) fn fetch_steps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session_id().map(str::to_owned) else {
            return;
        };
        let ask = ask::steps(session.clone(), None, 0);
        self.ask(ask, window, cx, move |view, page, _, cx| {
            if let Ok(page) = page {
                view.steps = Some((session, page));
                cx.notify();
            }
        });
    }
    fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(run_id) = self.run_id.clone() {
            self.request(Command::StopEvaluation { run_id }, window, cx);
        }
    }

    /// One label and its value, as the study's key-value grid shows them.
    fn row(label: &'static str, value: impl IntoElement, p: Palette) -> Div {
        div()
            .flex()
            .gap_4()
            .text_size(px(font::MESSAGE))
            .child(
                div()
                    .w(px(110.))
                    .flex_none()
                    .text_color(p.subtle)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(value))
    }

    fn details(&self, run: &Evaluation, p: Palette) -> Div {
        let catalog = &self.shared.catalog;
        let model = div()
            .flex()
            .gap_1()
            .child(div().text_color(p.magenta).child("Pinned"))
            .child(format!(
                "{} · no fallback",
                evaluations::pinned_model(catalog, run)
            ));
        let mode = match (&run.target, run.mode) {
            (Some(target), _) => {
                let app = Path::new(&target.executable)
                    .file_name()
                    .map_or(target.executable.clone(), |name| {
                        name.to_string_lossy().into_owned()
                    });
                let data = target.data_folder().unwrap_or_default();
                div().child(format!(
                    "{} · test copy of {app} on {}",
                    run.mode.label(),
                    evaluations::shown_folder(data)
                ))
            }
            (None, mode) => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(mode.label())
                .children(run.apps.iter().map(|app| evaluations::app_line(app, p))),
        };
        let folder = run.folder.clone();
        let folder_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(mono(evaluations::shown_folder(&run.folder), p).text_color(p.text))
            .when(self.shared.host.remote.is_none(), |d| {
                d.child(
                    div()
                        .id("show-run-folder")
                        .flex_none()
                        .text_color(p.accent)
                        .cursor_pointer()
                        .child("Show in Finder")
                        .on_click(move |_, _, cx| cx.reveal_path(Path::new(&folder))),
                )
            });
        let project = (self.shared.index.as_ref())
            .and_then(|index| index.project(&run.project_id))
            .map_or(String::new(), |project| format!("{} · ", project.name));
        let started = (wording::local(run.created_at)).map_or(String::new(), |at| {
            format!(" · {}", wording::time_of_day(at))
        });
        let started_by = format!("{project}Coordinator{started}");
        let has = match run.mode {
            EvaluationMode::App => {
                "Its prompt, the app driver and its folder. No shell, repository or your settings."
            }
            EvaluationMode::Desktop => {
                "Its prompt, the desktop driver for the apps above and its folder. No shell, repository or your settings."
            }
        };
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(Self::row("Model", model, p))
            .child(Self::row("Mode", mode, p))
            .child(Self::row("Folder", folder_row, p))
            .child(Self::row("Started by", div().child(started_by), p))
            .child(Self::row("It has", div().child(has), p))
            .children((run.reason.clone()).map(|reason| Self::row("Why", div().child(reason), p)))
    }

    /// The latest steps, newest first, each with its time and a screenshot's thumbnail.
    fn recent_steps(&self, run: &Evaluation, p: Palette) -> Div {
        let shown = (self.steps.as_ref())
            .filter(|(session, _)| run.session_id.as_ref() == Some(session))
            .map(|(_, page)| recent(&page.steps));
        let section = div()
            .flex()
            .flex_col()
            .child(ui::section("RECENT STEPS", p));
        let Some(steps) = shown.filter(|steps| !steps.is_empty()) else {
            let none = match run.session_id {
                None => "None yet. The run starts once it is allowed and has its turn.",
                Some(_) => "None yet.",
            };
            return section.child(hint(none, p));
        };
        let local = self.shared.host.remote.is_none();
        section.children(steps.into_iter().map(|step| {
            let at = (wording::local(step.started_at))
                .map_or(String::new(), |at| at.format("%H:%M:%S").to_string());
            let shot = (step.paths.first())
                .filter(|_| local)
                .map(|path| Path::new(&run.folder).join(path));
            div()
                .flex()
                .items_center()
                .gap_3()
                .py_1()
                .border_b_1()
                .border_color(p.edge.opacity(edge::HAIRLINE))
                .text_size(px(font::SMALL))
                .child(mono(at, p).w(px(64.)).flex_none())
                .children(shot.map(|path| {
                    img(path)
                        .flex_none()
                        .w(px(58.))
                        .h(px(38.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(p.edge)
                        .object_fit(ObjectFit::Cover)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(match step.state {
                            StepState::Failed => p.red,
                            _ => p.text,
                        })
                        .child(step.title.clone()),
                )
        }))
    }
}

/// The steps the page lists: the latest actions, newest first. The evaluator's own words are
/// left to the transcript; a step is what it did.
fn recent(steps: &[Step]) -> Vec<&Step> {
    let mut actions: Vec<&Step> = (steps.iter())
        .filter(|step| step.kind.is_action())
        .collect();
    actions.sort_by_key(|step| std::cmp::Reverse(step.seq));
    actions.truncate(RECENT_STEPS);
    actions
}

impl Render for RunPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let Some(run) = self.run().cloned() else {
            return div()
                .size_full()
                .bg(p.base)
                .child(ui::empty_state(
                    "agent",
                    "This run is no longer listed",
                    "A project's Agents panel lists its latest runs.",
                    p,
                ))
                .into_any_element();
        };
        let title = div()
            .flex()
            .items_center()
            .gap_2()
            .child(run.id.clone())
            .child(evaluations::state_pill(run.state, p));
        let header = page_header(div().child("UI evaluator run"), title, p)
            .px_6()
            .gap_3()
            .when(!run.state.ended(), |d| {
                d.child(
                    button("stop-run")
                        .outline()
                        .text_color(p.red)
                        .border_color(p.red)
                        .label("Stop")
                        .map(|b| ui::gated(b, &self.shared.host))
                        .on_click(cx.listener(|view, _, window, cx| view.stop(window, cx))),
                )
            })
            .child(
                button("close-run")
                    .ghost()
                    .icon(icon("close"))
                    .tooltip("Back to the conversation")
                    .on_click(
                        cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Conversation))),
                    ),
            );
        let body = div()
            .w_full()
            .max_w(px(ui::COLUMN))
            .mx_auto()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.details(&run, p))
            .child(self.recent_steps(&run, p))
            .child(hint(
                "Only you see these steps. The coordinator hears only the run's state, model, folder and time, so it can't steer the evaluator or learn its reasoning early.",
                p,
            ));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.base)
            .child(header)
            .child(
                div()
                    .id("run-page-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_6()
                    .py_5()
                    .child(body),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{RECENT_STEPS, recent};
    use workspace_core::{Step, StepKind, StepState};

    fn step(seq: u32, kind: StepKind) -> Step {
        Step {
            run_id: "r".into(),
            id: format!("s{seq}"),
            parent_id: None,
            kind,
            state: StepState::Succeeded,
            title: format!("step {seq}"),
            note: None,
            detail: None,
            paths: vec![],
            plan: None,
            diffs: vec![],
            omitted: 0,
            seq,
            revision: 0,
            started_at: 0,
            finished_at: None,
        }
    }

    #[test]
    fn the_page_lists_the_latest_actions_newest_first_without_the_evaluators_words() {
        let mut steps = vec![step(0, StepKind::Narration), step(1, StepKind::Thinking)];
        steps.extend((2..20).map(|seq| step(seq, StepKind::Tool)));
        let listed: Vec<u32> = recent(&steps).iter().map(|step| step.seq).collect();
        assert_eq!(listed.len(), RECENT_STEPS);
        assert_eq!(listed[0], 19);
        assert!(listed.windows(2).all(|pair| pair[0] > pair[1]));
        assert!(!listed.contains(&0) && !listed.contains(&1));
    }
}
