//! The rail beside the chat (OpenSpec change `assistant-does-the-work`, task 3.3, screen 1 of
//! its mockup): Needs you, Running, Loops and Done recently. Selecting an item scrolls the chat
//! to its card or opens its page.
use super::*;
use work::Need;
use workspace_core::assistant::conversation::{CardKind as Kind, ConversationCard};
use workspace_core::job::{JobState, RunOutcome};

/// The rail's width, as the mockup draws it.
const RAIL_WIDTH: f32 = 260.;

/// How a loop's newest run ended, as its rail entry ends: "last ✓", "last ✗" or "running".
pub(crate) fn last_result(job: &JobState) -> Option<&'static str> {
    Some(match job.runs.first()?.outcome {
        RunOutcome::Running => "running",
        RunOutcome::Finished => "last ✓",
        RunOutcome::Failed | RunOutcome::NotStarted => "last ✗",
        RunOutcome::Stopped => "last stopped",
    })
}
/// A loop's rail line: its schedule, its next run and how its last run ended.
pub(crate) fn loop_line(job: &JobState) -> String {
    let mut parts = vec![job.schedule.clone()];
    match (job.recipe.enabled, job.next_runs.first()) {
        (false, _) => parts.push("paused".into()),
        (true, Some(next)) => parts.push(format!("next {next}")),
        (true, None) => {}
    }
    parts.extend(last_result(job).map(str::to_owned));
    parts.join(" · ")
}

impl AssistantPage {
    /// What a Needs you entry is about, and what waits.
    fn need_lines(&self, need: &Need) -> (String, String) {
        let work_name = |id: Option<&str>| {
            id.and_then(|id| self.work_project(id))
                .map(|project| project.name.clone())
        };
        match need {
            Need::Card(card) => match &card.kind {
                Kind::Decision(decision) => (
                    work_name(decision.project_id.as_deref()).unwrap_or("A decision".into()),
                    decision.question.clone(),
                ),
                Kind::Action(action) => (
                    format!("{} · {}", action.source, self.space_name(&action.space_id)),
                    "Waiting for Run".into(),
                ),
                Kind::Loop(change) => (
                    (change.after.as_deref().or(change.before.as_deref()))
                        .map_or("A loop".into(), |recipe| recipe.name.clone()),
                    format!("{} · waiting for Apply", change.change.label()),
                ),
                _ => ("Waiting".into(), String::new()),
            },
            Need::Approval(card) => {
                let project = match &card.kind {
                    Kind::Approval { project_id, .. } => work_name(Some(project_id)),
                    _ => None,
                };
                (
                    project.unwrap_or("Work".into()),
                    "Needs your approval".into(),
                )
            }
            Need::Loop(job) => (
                job.recipe.name.clone(),
                match job.attention {
                    1 => "1 run failed or did not start".into(),
                    n => format!("{n} runs failed or did not start"),
                },
            ),
        }
    }

    pub(super) fn rail(&self, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut rail = div()
            .id("assistant-rail")
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .bg(p.base)
            .p_3()
            .flex()
            .flex_col()
            .gap_2();
        let (Some(snapshot), Some(index)) = (self.shared.snapshot(), self.shared.index.as_deref())
        else {
            return rail;
        };
        let spaces = snapshot.spaces.clone();

        rail = rail.child(eyebrow("Needs you", p));
        let needs = work::needs_you(snapshot);
        if needs.is_empty() {
            rail = rail.child(hint("Nothing waits for you.", p));
        }
        for (at, need) in needs.iter().enumerate() {
            let (title, line) = self.need_lines(need);
            let entry = rail_entry(SharedString::from(format!("need-{at}")), title, line, p)
                .border_l_3()
                .border_color(p.accent);
            let entry = match need {
                Need::Card(card) | Need::Approval(card) => {
                    let id = card.id.clone();
                    entry.on_click(cx.listener(move |v, _, _, c| v.show_chat(Some(id.clone()), c)))
                }
                Need::Loop(job) => {
                    let key = job.recipe.key();
                    entry.on_click(
                        cx.listener(move |v, _, w, c| v.open_job(Some(key.clone()), w, c)),
                    )
                }
            };
            rail = rail.child(entry);
        }

        let items = work::work_items(index);
        let cards = snapshot.conversation.as_ref().map_or(&[][..], |c| &c.cards);
        rail = rail.child(eyebrow("Running", p));
        let running: Vec<_> = items.iter().filter(|item| item.running()).collect();
        if running.is_empty() {
            rail = rail.child(hint("No work running.", p));
        }
        for item in running {
            let line = format!(
                "{} · {}",
                item.state_line(),
                self.space_name(&item.project.space)
            );
            let entry = rail_entry(
                SharedString::from(format!("running-{}", item.project.id)),
                item.project.name.clone(),
                line,
                p,
            )
            .child(
                div().flex().children(
                    item.working
                        .then(|| ui::Spinner::new().xsmall().color(p.accent)),
                ),
            );
            rail = rail.child(self.open_work(entry, &item.project.id, item.coordinator, cards, cx));
        }

        rail = rail.child(eyebrow("Loops", p));
        if snapshot.jobs.is_empty() {
            rail = rail.child(hint("No loops yet.", p));
        }
        for job in &snapshot.jobs {
            let key = job.recipe.key();
            let space = self.space_name(&job.recipe.space_id);
            let mut entry = rail_entry(
                SharedString::from(format!("loop-{}", key.as_str())),
                job.recipe.name.clone(),
                loop_line(job),
                p,
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(space_mark(space_color(&spaces, &job.recipe.space_id, p)))
                    .child(space),
            );
            for (name, run, minutes) in jobs::long_runs(std::slice::from_ref(job)) {
                entry = entry.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(font::META))
                        .text_color(p.yellow)
                        .child(div().flex_1().child(jobs::long_run_text(&name, minutes)))
                        .child(
                            button(SharedString::from(format!("stop-loop-run-{run}")))
                                .ghost()
                                .xsmall()
                                .label("Stop")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    c.stop_propagation();
                                    v.stop_run(run.clone(), w, c)
                                })),
                        ),
                );
            }
            rail = rail.child(
                entry.on_click(cx.listener(move |v, _, w, c| v.open_job(Some(key.clone()), w, c))),
            );
        }

        rail = rail.child(eyebrow("Done recently", p));
        let done = work::done_recently(&items, ui::now());
        if done.is_empty() {
            rail = rail.child(hint("Nothing finished this week.", p));
        }
        for item in done {
            let entry = rail_entry(
                SharedString::from(format!("done-{}", item.project.id)),
                item.project.name.clone(),
                item.state_line(),
                p,
            );
            rail = rail.child(self.open_work(entry, &item.project.id, item.coordinator, cards, cx));
        }
        rail
    }

    /// Selecting work scrolls the chat to its card, or opens its details when it has none.
    fn open_work(
        &self,
        entry: Stateful<Div>,
        project_id: &str,
        coordinator: Option<&Session>,
        cards: &[ConversationCard],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let card = (cards.iter())
            .find(|card| matches!(&card.kind, Kind::Work { project_id: id } if id == project_id))
            .map(|card| card.id.clone());
        let coordinator = coordinator.map(|c| c.id.clone());
        entry.on_click(cx.listener(move |v, _, _, c| match (&card, &coordinator) {
            (Some(card), _) => v.show_chat(Some(card.clone()), c),
            (None, Some(coordinator)) => c.emit(Intent::Select(coordinator.clone())),
            (None, None) => {}
        }))
    }
}

fn eyebrow(text: &'static str, p: Palette) -> Div {
    ui::eyebrow(text, p).mt_2()
}
/// One entry of the rail: a bold title and a line under it.
fn rail_entry(id: SharedString, title: String, line: String, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(1.))
        .px_2()
        .py(px(6.))
        .rounded_lg()
        .border_1()
        .border_color(p.edge)
        .bg(p.surface)
        .cursor_pointer()
        .hover(|d| d.bg(p.overlay))
        .text_size(px(font::CAPTION))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(title),
        )
        .when(!line.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(line),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::loop_line;
    use workspace_core::job::{JobRun, JobState, RunOutcome};

    fn job(enabled: bool, next: &[&str], outcome: Option<RunOutcome>) -> JobState {
        let mut job: JobState = serde_json::from_value(serde_json::json!({
            "recipe": {
                "id": "brief", "name": "Morning brief", "space_id": "personal", "prompt": "",
                "schedule": {"cron": "0 8 * * 1-5", "timezone": "America/Chicago"},
                "enabled": enabled,
            },
            "schedule": "Weekdays at 08:00 (America/Chicago)",
            "model": "Sonnet",
        }))
        .unwrap();
        job.next_runs = next.iter().map(|n| n.to_string()).collect();
        job.runs = outcome
            .into_iter()
            .map(|outcome| -> JobRun {
                serde_json::from_value(serde_json::json!({
                    "id": "r", "job": "personal/brief", "started_at": 0, "outcome": outcome,
                }))
                .unwrap()
            })
            .collect();
        job
    }

    #[test]
    fn a_loops_rail_line_says_its_schedule_next_run_and_last_result() {
        assert_eq!(
            loop_line(&job(true, &["Tue 08:00"], Some(RunOutcome::Finished))),
            "Weekdays at 08:00 (America/Chicago) · next Tue 08:00 · last ✓"
        );
        assert_eq!(
            loop_line(&job(false, &[], Some(RunOutcome::NotStarted))),
            "Weekdays at 08:00 (America/Chicago) · paused · last ✗"
        );
        assert_eq!(
            loop_line(&job(true, &["Fri 16:00"], None)),
            "Weekdays at 08:00 (America/Chicago) · next Fri 16:00"
        );
    }
}
