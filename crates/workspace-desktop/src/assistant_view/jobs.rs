//! Scheduled jobs in the assistant (stage 3, screen 3 of the mockup): the recipe card a thread
//! shows, the Jobs list with "Start Wiffletree at login", a job's page with its runs, and the
//! form that edits a recipe. Only the user's Save makes or changes a job.
use super::*;
use gpui_component::button::ButtonVariant;
use ui::{needs_you_badge, pill};
use workspace_core::job::{
    JobKey, JobRun, JobState, Recipe, RecipePreview, RunOutcome, Runner, Sharing, SourceCheck,
};

/// What the recipe card says a job may do, as the mockup words it.
const ONLY_READ: &str = "Jobs only read. Each run is a fresh read-only thread; it never sends, posts or changes anything in these apps. Runs need Wiffletree running; missed runs come back as one catch-up run.";
const WHILE_RUNNING: &str = "Jobs run only while Wiffletree runs on this Mac, and never wake it. Missed runs come back as one catch-up run when it runs again.";

/// What the assistant shows in place of a thread.
pub(super) enum JobsView {
    List,
    Job(JobKey),
    Edit(Box<JobForm>),
}

/// A recipe while the user edits it, before Save.
pub(super) struct JobForm {
    recipe: Recipe,
    /// The recipe card it was opened from, which saving dismisses; `None` for a saved job.
    card: Option<String>,
    name: Entity<InputState>,
    prompt: Entity<TextareaState>,
    cron: Entity<InputState>,
    timezone: Entity<InputState>,
    busy: bool,
}

/// How many runs count on the assistant's row, and the job of the newest of them.
pub(crate) fn attention(jobs: &[JobState]) -> (usize, Option<JobKey>) {
    let count = jobs.iter().map(|job| job.attention).sum();
    let newest = (jobs.iter())
        .filter(|job| job.attention > 0)
        .max_by_key(|job| job.attention_at);
    (count, newest.map(|job| job.recipe.key()))
}

/// Each run that has run 30 minutes or more: its job's name, its id and its minutes.
pub(crate) fn long_runs(jobs: &[JobState]) -> Vec<(String, String, i64)> {
    (jobs.iter())
        .flat_map(|job| job.runs.iter().map(move |run| (job, run)))
        .filter(|(_, run)| run.outcome == RunOutcome::Running)
        .filter_map(|(job, run)| Some((job.recipe.name.clone(), run.id.clone(), run.long_minutes?)))
        .collect()
}

/// "Morning brief has run 45 min", or "… 1 h 30 min".
pub(crate) fn long_run_text(name: &str, minutes: i64) -> String {
    format!("{name} has run {}", elapsed(minutes))
}

/// "45 min", "2 h" or "1 h 30 min".
fn elapsed(minutes: i64) -> String {
    match minutes {
        0..60 => format!("{minutes} min"),
        _ if minutes % 60 == 0 => format!("{} h", minutes / 60),
        _ => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

/// A check older than this no longer reads as current.
const CHECK_FRESH_MS: i64 = 24 * 60 * 60 * 1000;
const PAST_30_MINUTES: &str = "This run has gone past 30 minutes. While it runs, the job's scheduled times are skipped; Stop ends it.";
const PAUSED: &str = "Scheduled times are skipped until you resume it; Resume starts from the next time. Run now still works.";

/// "HTML", "Markdown", or the type in capitals.
fn file_type_label(file_type: &str) -> String {
    match file_type.to_ascii_lowercase().as_str() {
        "md" | "markdown" => "Markdown".into(),
        other => other.to_uppercase(),
    }
}

/// A source's line: what it is and what the check found.
fn source_line(check: &SourceCheck) -> (bool, String) {
    match &check.problem {
        None => (true, format!("{} · ready", check.source)),
        Some(problem) => (false, format!("{} · {problem}", check.source)),
    }
}

/// A source's line from a check that is no longer current, in the past tense.
fn stale_source_line(check: &SourceCheck) -> String {
    match &check.problem {
        None => format!("{} · was ready", check.source),
        Some(problem) => format!("{} · then: {problem}", check.source),
    }
}

/// "today 07:00 · catch-up for 1 missed run", beside the pill that names its outcome.
fn run_line(run: &JobRun) -> String {
    let mut parts = vec![wording::day_and_time(
        run.scheduled_at.unwrap_or(run.started_at),
    )];
    if let (RunOutcome::Running, Some(minutes)) = (run.outcome, run.long_minutes) {
        parts.push(format!("over {} so far", elapsed(minutes)));
    }
    parts.extend(run.catch_up_label());
    if run.scheduled_at.is_none() {
        parts.push("Run now".into());
    }
    if run.outcome == RunOutcome::Finished && run.outputs.is_empty() {
        parts.push("no output".into());
    }
    parts.join(" · ")
}

/// The Jobs list's "Last run" line: how this Mac's newest run ended, since a running one is in
/// the pills, or, with no run here, when the Mac that runs it last saved an output.
fn last_run_line(job: &JobState) -> Option<String> {
    match job.runs.first() {
        Some(run) if run.outcome == RunOutcome::Running => None,
        Some(run) => Some(format!(
            "Last run: {} · {}",
            run_line(run),
            run.outcome.label()
        )),
        None => (job.last_run_at).map(|at| format!("Last run: {}", wording::day_and_time(at))),
    }
}

/// The model a run started on, only when it is not the one the job's page names.
fn run_model(run: &JobRun, job: &JobState) -> Option<String> {
    let profile = run.profile.as_ref()?;
    (job.profile.as_ref() != Some(profile))
        .then(|| format!("Ran on {} · {}", profile.short(), run.model_reason))
}

/// Which run checked the sources and when, and whether that check still reads as current.
fn checked_note(run: &JobRun, now: i64) -> (String, bool) {
    let when = wording::day_and_time(run.scheduled_at.unwrap_or(run.started_at));
    let ago = wording::age(run.started_at).to_lowercase();
    match now - run.started_at < CHECK_FRESH_MS {
        true => (format!("Checked by the run of {when}, {ago}"), true),
        false => (
            format!("Last checked by the run of {when}, {ago}. The next run checks again."),
            false,
        ),
    }
}

/// The pills by a job's name: Active, on its page only, or Paused; a running run and the last
/// 30-minute mark it passed, since that is when `long_minutes` moves; and the Mac that runs it
/// in this Mac's place.
fn job_pills(job: &JobState, with_active: bool, p: Palette) -> Vec<Div> {
    let state = match job.recipe.enabled {
        true => with_active.then(|| pill("Active", p.green)),
        false => Some(pill("Paused", p.yellow)),
    };
    let running = (job.runs.iter())
        .find(|run| run.outcome == RunOutcome::Running)
        .map(|run| match run.long_minutes {
            Some(minutes) => pill(format!("Running · over {}", elapsed(minutes)), p.accent),
            None => pill("Running", p.accent),
        });
    let elsewhere = (job.not_here.clone()).map(|why| pill(why, p.subtle));
    state.into_iter().chain(running).chain(elsewhere).collect()
}

/// A label and its value, as the recipe card lists them.
fn row(label: &'static str, value: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .gap_3()
        .text_size(px(font::SMALL))
        .child(
            div()
                .flex_none()
                .w(px(76.))
                .text_color(p.subtle)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

impl AssistantPage {
    fn jobs(&self) -> &[JobState] {
        self.shared.snapshot().map_or(&[], |s| &s.jobs)
    }
    fn job(&self, key: &JobKey) -> Option<&JobState> {
        self.jobs().iter().find(|job| &job.recipe.key() == key)
    }
    fn space_named(&self, id: &SpaceId) -> Option<&Space> {
        self.spaces().iter().find(|s| &s.id == id)
    }
    /// Whether a Space is stored on GitHub, where a job's sharing matters.
    fn shared_space(&self, id: &SpaceId) -> bool {
        self.space_named(id).is_some_and(|space| {
            matches!(space.storage.repository, StorageRepository::GitHub { .. })
        })
    }
    /// Opens the Jobs list, or a job's page, whose failed and unstarted runs stop counting.
    pub(crate) fn open_job(
        &mut self,
        job: Option<JobKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hits = None;
        self.jobs_view = Some(match job {
            Some(key) => {
                let seen = ask::see_job_runs(key.clone());
                self.ask(seen, window, cx, |_, _, _, _| {});
                JobsView::Job(key)
            }
            None => JobsView::List,
        });
        if self.login.is_none() {
            self.login = Some(login_item::starts_at_login());
        }
        cx.notify();
    }
    /// Saves `recipe`; from a `card`, as a new job, which the host refuses when its name is
    /// taken, and then the card goes.
    fn save_recipe(
        &mut self,
        recipe: Recipe,
        card: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = recipe.name.clone();
        self.ask(
            ask::save_job(recipe, card.is_some()),
            window,
            cx,
            move |view, saved, window, cx| {
                match saved {
                    Ok(state) => {
                        if let Some(card) = card.clone() {
                            view.dismiss(card, window, cx);
                        }
                        toast(format!("Saved the job {name}"), window, cx);
                        view.open_job(Some(state.recipe.key()), window, cx);
                    }
                    Err(error) => {
                        if let Some(JobsView::Edit(form)) = &mut view.jobs_view {
                            form.busy = false;
                        }
                        toast_error(error, window, cx);
                    }
                }
                cx.notify();
            },
        );
    }
    fn edit_recipe(
        &mut self,
        recipe: Recipe,
        card: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = |text: &str, cx: &mut Context<Self>, window: &mut Window| {
            let text = text.to_owned();
            cx.new(|cx| InputState::new(window, cx).default_value(text))
        };
        let name = input(&recipe.name, cx, window);
        let cron = input(&recipe.schedule.cron, cx, window);
        let timezone = input(&recipe.schedule.timezone, cx, window);
        let text = recipe.prompt.clone();
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 12)
                .default_value(text)
        });
        self.jobs_view = Some(JobsView::Edit(Box::new(JobForm {
            recipe,
            card,
            name,
            prompt,
            cron,
            timezone,
            busy: false,
        })));
        cx.notify();
    }
    fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(JobsView::Edit(form)) = &mut self.jobs_view else {
            return;
        };
        form.busy = true;
        let mut recipe = form.recipe.clone();
        recipe.name = form.name.read(cx).value().trim().to_owned();
        recipe.prompt = form.prompt.read(cx).value().trim().to_owned();
        recipe.schedule.cron = form.cron.read(cx).value().trim().to_owned();
        recipe.schedule.timezone = form.timezone.read(cx).value().trim().to_owned();
        let card = form.card.clone();
        self.save_recipe(recipe, card, window, cx);
    }
    fn set_enabled(
        &mut self,
        recipe: &Recipe,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::set_job_enabled(recipe.space_id.clone(), recipe.id.clone(), enabled);
        self.ask(ask, window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    /// "Runs on Jamie's Mac · last run Mon 08:00", with Take over running when this Mac may,
    /// for a "One runner" job in a Space stored on GitHub.
    fn runner_line(&self, job: &JobState, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let recipe = &job.recipe;
        if !self.shared_space(&recipe.space_id) || recipe.sharing != Sharing::OneRunner {
            return None;
        }
        let runs_on = (job.not_here.clone()).or_else(|| recipe.runner.as_ref().map(Runner::label));
        let last = (job.last_run_at).map(|at| format!("last run {}", wording::day_and_time(at)));
        let line: Vec<String> = runs_on.into_iter().chain(last).collect();
        let taken = recipe.clone();
        Some(
            ui::banner(p.subtle)
                .text_size(px(font::SMALL))
                .child(div().flex_1().child(line.join(" · ")))
                .when(job.can_take_over(), |d| {
                    d.child(
                        button("job-take-over")
                            .xsmall()
                            .label(boards_view::TAKE_OVER)
                            .on_click(cx.listener(move |v, _, w, c| v.take_over(&taken, w, c))),
                    )
                }),
        )
    }
    fn take_over(&mut self, recipe: &Recipe, window: &mut Window, cx: &mut Context<Self>) {
        let ask = ask::take_over_job(recipe.space_id.clone(), recipe.id.clone());
        self.ask(ask, window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    /// Share or Keep private, for a run's output waiting for approval.
    fn answer_output(
        &mut self,
        run: String,
        title: String,
        share: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::approve_job_output(run, title, share);
        self.ask(ask, window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    fn run_now(&mut self, key: JobKey, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(ask::run_job_now(key), window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    pub(crate) fn stop_run(&mut self, run: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(ask::stop_job_run(run), window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }
    fn confirm_delete_job(&mut self, recipe: &Recipe, window: &mut Window, cx: &mut Context<Self>) {
        let (space, id, name) = (
            recipe.space_id.clone(),
            recipe.id.clone(),
            recipe.name.clone(),
        );
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, space, id) = (weak.clone(), space.clone(), id.clone());
            dialog
                .title(format!("Delete the job {name}?"))
                .confirm()
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .child("Its recipe and its runs are removed. Outputs it saved stay in Boards.")
                .on_ok(move |_, window, cx| {
                    let ask = ask::delete_job(space.clone(), id.clone());
                    let _ = weak.update(cx, |view, cx| {
                        view.ask(ask, window, cx, |view, done, window, cx| match done {
                            Ok(()) => view.open_job(None, window, cx),
                            Err(error) => toast_error(error, window, cx),
                        });
                    });
                    true
                })
        });
    }
    fn set_login(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        match login_item::set_starts_at_login(on) {
            Ok(()) => self.login = Some(login_item::starts_at_login()),
            Err(error) => toast_error(format!("{error:#}"), window, cx),
        }
        cx.notify();
    }

    /// The card `propose_job` shows: every field of the recipe, its schedule in words with its
    /// next runs, and each source as checked now. Nothing is saved until Save job.
    pub(super) fn recipe_card(
        &self,
        card_id: &str,
        recipe: &Recipe,
        preview: &RecipePreview,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let spaces = self.spaces().to_vec();
        let space = self.space_named(&recipe.space_id);
        let shared = self.shared_space(&recipe.space_id);
        let space_line = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(space_mark(space_color(&spaces, &recipe.space_id, p)))
            .child(match (space, shared) {
                (Some(space), true) => format!("{} · shared", space.name),
                (Some(space), false) => space.name.clone(),
                (None, _) => String::new(),
            });
        let schedule = div()
            .flex()
            .flex_col()
            .child(preview.schedule.clone())
            .when(!preview.next_runs.is_empty(), |d| {
                d.child(hint(format!("Next: {}", preview.next_runs.join(" · ")), p))
            });
        let mut sources = div().flex().flex_col().gap(px(2.));
        if preview.sources.is_empty() {
            sources = sources.child(hint("None: the run reads only local files and the web.", p));
        }
        for check in &preview.sources {
            let (ok, text) = source_line(check);
            sources = sources.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_color(if ok { p.green } else { p.red })
                            .child("●"),
                    )
                    .child(text),
            );
        }
        let output = recipe
            .output
            .as_ref()
            .map_or("Whatever the run saves".to_owned(), |o| {
                format!("“{}” · {}", o.title, file_type_label(&o.file_type))
            });
        let (card_id, edited, saved) = (card_id.to_owned(), recipe.clone(), recipe.clone());
        let (cancel, from_card) = (card_id.clone(), card_id.clone());
        card(p)
            .gap_2()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("New job · {}", recipe.name)),
            )
            .child(row("Space", space_line, p))
            .child(row("Schedule", schedule, p))
            .child(row("Prompt", recipe.prompt.clone(), p))
            .child(row("Model", preview.model.clone(), p))
            .child(row("Output", output, p))
            .when(shared, |d| {
                d.child(row("Sharing", recipe.sharing.label(), p))
            })
            .child(row("Sources", sources, p))
            .child(hint(ONLY_READ, p))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        button(SharedString::from(format!("edit-recipe-{card_id}")))
                            .ghost()
                            .xsmall()
                            .label("Edit")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.edit_recipe(edited.clone(), Some(from_card.clone()), w, c)
                            })),
                    )
                    .child(
                        button(SharedString::from(format!("cancel-recipe-{card_id}")))
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(
                                cx.listener(move |v, _, w, c| v.dismiss(cancel.clone(), w, c)),
                            ),
                    )
                    .child(
                        button(SharedString::from(format!("save-recipe-{card_id}")))
                            .primary()
                            .xsmall()
                            .label("Save job")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.save_recipe(saved.clone(), Some(card_id.clone()), w, c)
                            })),
                    ),
            )
    }

    pub(super) fn jobs_body(
        &self,
        view: &JobsView,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let content = match view {
            JobsView::List => self.jobs_list(p, cx),
            JobsView::Job(key) => match self.job(key) {
                Some(job) => self.job_page(job, p, cx),
                None => self.jobs_list(p, cx),
            },
            JobsView::Edit(form) => self.job_form(form, p, cx),
        };
        div()
            .id("jobs")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(div().flex().flex_col().gap_3().px_5().py_4().child(content))
    }

    fn jobs_list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let spaces = self.spaces().to_vec();
        let login = self.login.unwrap_or(false);
        let mut list = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(font::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Jobs"),
            )
            .child(hint(WHILE_RUNNING, p))
            .child(
                Checkbox::new("start-at-login")
                    .label("Start Wiffletree at login")
                    .checked(login)
                    .on_click(cx.listener(move |v, _, w, c| v.set_login(!login, w, c))),
            );
        if self.jobs().is_empty() {
            return list.child(hint(
                "No jobs yet. Ask the assistant for recurring work, such as a brief every weekday at 7, and save the recipe it shows.",
                p,
            ));
        }
        for job in self.jobs() {
            let key = job.recipe.key();
            let last = last_run_line(job);
            // Paused and another Mac's runner are in the pills.
            let next = match (&job.not_here, job.recipe.enabled, job.next_runs.first()) {
                (Some(_), _, _) | (None, false, _) => String::new(),
                (None, true, Some(next)) => format!(" · Next {next}"),
                (None, true, None) => " · No next run".into(),
            };
            let attention = job.attention;
            list = list.child(
                card(p)
                    .id(SharedString::from(format!("job-{}", key.as_str())))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.overlay))
                    .on_click(cx.listener(move |v, _, w, c| v.open_job(Some(key.clone()), w, c)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(space_mark(space_color(&spaces, &job.recipe.space_id, p)))
                            .child(
                                div()
                                    .flex_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(job.recipe.name.clone()),
                            )
                            .children(job_pills(job, false, p))
                            .when(attention > 0, |d| d.child(needs_you_badge(attention, p))),
                    )
                    .child(hint(format!("{}{next}", job.schedule), p))
                    .children(last.map(|last| hint(last, p))),
            );
        }
        list
    }

    fn job_page(&self, job: &JobState, p: Palette, cx: &mut Context<Self>) -> Div {
        let recipe = &job.recipe;
        let key = recipe.key();
        let spaces = self.spaces().to_vec();
        let space = self
            .space_named(&recipe.space_id)
            .map_or(String::new(), |s| s.name.clone());
        let running = job.runs.iter().any(|r| r.outcome == RunOutcome::Running);
        let (paused, toggled, edited, deleted) = (
            !recipe.enabled,
            recipe.clone(),
            recipe.clone(),
            recipe.clone(),
        );
        let actions = div()
            .flex()
            .gap_2()
            .child(
                button("jobs-back")
                    .ghost()
                    .xsmall()
                    .label("‹ All jobs")
                    .on_click(cx.listener(|v, _, w, c| v.open_job(None, w, c))),
            )
            .child(div().flex_1())
            .child(
                button("job-pause")
                    .ghost()
                    .xsmall()
                    .label(if paused { "Resume" } else { "Pause" })
                    .on_click(cx.listener(move |v, _, w, c| v.set_enabled(&toggled, paused, w, c))),
            )
            .child(
                button("job-edit").ghost().xsmall().label("Edit").on_click(
                    cx.listener(move |v, _, w, c| v.edit_recipe(edited.clone(), None, w, c)),
                ),
            )
            .child(
                button("job-delete")
                    .danger()
                    .outline()
                    .xsmall()
                    .label("Delete")
                    .on_click(cx.listener(move |v, _, w, c| v.confirm_delete_job(&deleted, w, c))),
            )
            .child(
                button("job-run-now")
                    .xsmall()
                    .label("Run now")
                    .disabled(running || job.not_here.is_some())
                    .on_click(cx.listener(move |v, _, w, c| v.run_now(key.clone(), w, c))),
            );
        let next = match (paused, job.next_runs.is_empty()) {
            (true, _) => "No next run while paused".to_owned(),
            (false, true) => "No next run".into(),
            (false, false) => format!("Next: {}", job.next_runs.join(" · ")),
        };
        let checked = job.runs.iter().find(|r| !r.sources.is_empty());
        let (when_checked, fresh) = match (checked, &job.not_here) {
            (Some(run), _) => checked_note(run, ui::now()),
            (None, Some(_)) => (
                "Not checked on this Mac, which does not run it.".into(),
                false,
            ),
            (None, None) => (
                "Not checked yet: each run checks them before it starts.".into(),
                false,
            ),
        };
        let mut sources = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .when(!recipe.sources.is_empty(), |d| {
                d.child(hint(when_checked, p))
            });
        for source in &recipe.sources {
            let label = workspace_core::job::source_label(source);
            let found = checked.and_then(|r| r.sources.iter().find(|c| c.source == label));
            // A check that is no longer current, or none, is grey, so it never reads as ready now.
            let (color, text) = match found {
                Some(check) if fresh => match source_line(check) {
                    (true, text) => (Some(p.green), text),
                    (false, text) => (Some(p.red), text),
                },
                Some(check) => (None, stale_source_line(check)),
                None => (None, label),
            };
            sources = sources.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .when(color.is_none(), |d| d.text_color(p.subtle))
                    .child(div().text_color(color.unwrap_or(p.subtle)).child("●"))
                    .child(text),
            );
        }
        if recipe.sources.is_empty() {
            sources = sources.child(hint("None", p));
        }
        let mut runs = div().flex().flex_col().gap_2();
        if job.runs.is_empty() {
            runs = runs.child(hint("No runs yet.", p));
        }
        for run in &job.runs {
            let stop = run.id.clone();
            let color = match run.outcome {
                RunOutcome::Finished => p.green,
                RunOutcome::Running => p.accent,
                RunOutcome::Stopped => p.subtle,
                RunOutcome::Failed | RunOutcome::NotStarted => p.red,
            };
            let long = run.outcome == RunOutcome::Running && run.long_minutes.is_some();
            runs = runs.child(
                card(p)
                    .gap_1()
                    .when(long, |d| {
                        d.border_color(p.yellow.opacity(0.55))
                            .bg(p.yellow.opacity(0.07))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(pill(run.outcome.label(), color))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(font::SMALL))
                                    .child(run_line(run)),
                            )
                            .when(run.outcome == RunOutcome::Running, |d| {
                                d.child(
                                    button(SharedString::from(format!("stop-run-{}", run.id)))
                                        .when(!long, |b| b.ghost())
                                        .xsmall()
                                        .label("Stop")
                                        .on_click(cx.listener(move |v, _, w, c| {
                                            v.stop_run(stop.clone(), w, c)
                                        })),
                                )
                            }),
                    )
                    .when(long, |d| {
                        d.child(
                            div()
                                .text_size(px(font::SMALL))
                                .text_color(p.yellow)
                                .child(PAST_30_MINUTES),
                        )
                    })
                    .when(!run.reason.is_empty(), |d| {
                        d.child(hint(run.reason.clone(), p))
                    })
                    .children(run.outputs.iter().map(|output| {
                        let saved = hint(
                            format!(
                                "Saved “{}” · version {} · {}",
                                output.title, output.version, output.kept
                            ),
                            p,
                        );
                        if !output.waiting {
                            return saved;
                        }
                        let answer = |share: bool| {
                            let (run, title) = (run.id.clone(), output.title.clone());
                            cx.listener(move |v: &mut Self, _: &ClickEvent, w, c| {
                                v.answer_output(run.clone(), title.clone(), share, w, c)
                            })
                        };
                        let id = |what: &str| {
                            SharedString::from(format!("{what}-{}-{}", run.id, output.title))
                        };
                        saved
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(pill(boards_view::WAITING, p.accent))
                            .child(
                                button(id("approve"))
                                    .xsmall()
                                    .label("Share")
                                    .on_click(answer(true)),
                            )
                            .child(
                                button(id("keep"))
                                    .ghost()
                                    .xsmall()
                                    .label("Keep private")
                                    .on_click(answer(false)),
                            )
                    }))
                    .children(run_model(run, job).map(|model| hint(model, p))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(actions)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(space_mark(space_color(&spaces, &recipe.space_id, p)))
                    .child(
                        div()
                            .text_size(px(font::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(recipe.name.clone()),
                    )
                    .children(job_pills(job, true, p)),
            )
            .when(paused, |d| {
                d.child(
                    ui::banner(p.yellow).text_size(px(font::SMALL)).child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                div()
                                    .text_color(p.yellow)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Paused."),
                            )
                            .child(PAUSED),
                    ),
                )
            })
            .children(self.runner_line(job, p, cx))
            .child(
                card(p)
                    .gap_2()
                    .child(row("Space", space, p))
                    .child(row(
                        "Schedule",
                        div()
                            .flex()
                            .flex_col()
                            .child(job.schedule.clone())
                            .child(hint(next, p)),
                        p,
                    ))
                    .child(row("Prompt", recipe.prompt.clone(), p))
                    .child(row("Model", job.model.clone(), p))
                    .when(self.shared_space(&recipe.space_id), |d| {
                        d.child(row("Sharing", recipe.sharing.label(), p))
                    })
                    .child(row("Sources", sources, p))
                    .child(hint(
                        "Jobs only read, through the commands and tools listed here.",
                        p,
                    )),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Runs"))
            .child(runs)
    }

    fn job_form(&self, form: &JobForm, p: Palette, cx: &mut Context<Self>) -> Div {
        let key = form.recipe.key();
        let saved = form.card.is_none();
        let field = |label: &'static str, input: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(hint(label, p))
                .child(input)
        };
        card(p)
            .gap_3()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(match saved {
                true => format!("Edit {}", form.recipe.name),
                false => format!("New job · {}", form.recipe.name),
            }))
            .child(field("Name", Input::new(&form.name).small().into_any_element()))
            .child(field("Prompt", Textarea::new(&form.prompt).w_full().into_any_element()))
            .child(field(
                "Schedule: minute, hour, day of month, month and weekday, such as 0 7 * * 1-5",
                Input::new(&form.cron).small().into_any_element(),
            ))
            .child(field(
                "Timezone, such as America/New_York",
                Input::new(&form.timezone).small().into_any_element(),
            ))
            .child(hint(
                "Its sources stay as the recipe listed them. Saving checks the schedule, and the next run follows it.",
                p,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        button("cancel-job-form")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(cx.listener(move |v, _, w, c| {
                                let back = saved.then(|| key.clone());
                                match back {
                                    Some(key) => v.open_job(Some(key), w, c),
                                    None => {
                                        v.jobs_view = None;
                                        c.notify();
                                    }
                                }
                            })),
                    )
                    .child(
                        button("save-job-form")
                            .primary()
                            .xsmall()
                            .label("Save job")
                            .disabled(form.busy)
                            .on_click(cx.listener(|v, _, w, c| v.save_form(w, c))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHECK_FRESH_MS, attention, checked_note, file_type_label, last_run_line, long_run_text,
        long_runs, run_line, run_model, stale_source_line,
    };
    use workspace_core::job::{
        JobKey, JobRun, JobState, Recipe, RunOutcome, Schedule, SourceCheck,
    };
    use workspace_core::{ModelProfile, Provider, SpaceId};

    fn run(id: &str, outcome: RunOutcome, started_at: i64) -> JobRun {
        JobRun {
            id: id.into(),
            job: JobKey::recipe(&SpaceId("sage".into()), "brief"),
            scheduled_at: Some(started_at),
            started_at,
            ended_at: None,
            outcome,
            reason: String::new(),
            profile: None,
            model_reason: String::new(),
            catch_up: 0,
            sources: vec![],
            outputs: vec![],
            thread_id: None,
            long_minutes: None,
            seen: false,
            approves: None,
        }
    }
    fn job(runs: Vec<JobRun>) -> JobState {
        JobState {
            recipe: Recipe {
                id: "brief".into(),
                name: "Morning brief".into(),
                space_id: SpaceId("sage".into()),
                prompt: "Brief me.".into(),
                schedule: Schedule {
                    cron: "0 7 * * 1-5".into(),
                    timezone: "America/New_York".into(),
                },
                model: None,
                sources: vec![],
                output: None,
                sharing: Default::default(),
                runner: None,
                enabled: true,
                extra: Default::default(),
            },
            schedule: "Weekdays at 07:00 (America/New_York)".into(),
            next_runs: vec![],
            model: String::new(),
            profile: None,
            runs,
            attention: 0,
            attention_at: None,
            not_here: None,
            last_run_at: None,
        }
    }

    #[test]
    fn the_row_counts_every_jobs_unseen_runs_and_opens_the_newest() {
        let mut old = job(vec![]);
        (old.attention, old.attention_at) = (21, Some(1));
        let mut newer = job(vec![]);
        newer.recipe.id = "dashboard".into();
        (newer.attention, newer.attention_at) = (1, Some(5));
        let (count, open) = attention(&[old, newer]);
        assert_eq!(count, 22);
        assert_eq!(
            open,
            Some(JobKey::recipe(&SpaceId("sage".into()), "dashboard"))
        );
        assert_eq!(attention(&[job(vec![])]), (0, None));
    }

    #[test]
    fn a_long_run_says_its_job_and_how_long_it_has_run() {
        let mut long = run("r1", RunOutcome::Running, 1);
        long.long_minutes = Some(90);
        let jobs = [job(vec![long, run("r2", RunOutcome::Running, 2)])];
        assert_eq!(
            long_runs(&jobs),
            [("Morning brief".into(), "r1".into(), 90)]
        );
        assert_eq!(
            long_run_text("Morning brief", 30),
            "Morning brief has run 30 min"
        );
        assert_eq!(
            long_run_text("Morning brief", 90),
            "Morning brief has run 1 h 30 min"
        );
        assert_eq!(
            long_run_text("Morning brief", 120),
            "Morning brief has run 2 h"
        );
    }

    #[test]
    fn a_runs_line_names_a_catch_up_and_a_run_with_no_output() {
        let mut caught = run("r1", RunOutcome::Finished, 0);
        caught.catch_up = 3;
        let line = run_line(&caught);
        assert!(
            line.ends_with(" · catch-up for 3 missed runs · no output"),
            "{line}"
        );
        assert!(
            !line.contains("Finished"),
            "the pill names the outcome: {line}"
        );
        let mut long = run("r2", RunOutcome::Running, 0);
        long.long_minutes = Some(30);
        assert!(run_line(&long).ends_with(" · over 30 min so far"));
        // Another Mac's job, with no run here, says when its runner last saved.
        let mut elsewhere = job(vec![]);
        elsewhere.last_run_at = Some(0);
        assert!(last_run_line(&elsewhere).is_some_and(|line| line.starts_with("Last run: ")));
        assert_eq!(last_run_line(&job(vec![long])), None);
        assert_eq!(file_type_label("md"), "Markdown");
        assert_eq!(file_type_label("html"), "HTML");
    }

    fn profile(model: &str) -> ModelProfile {
        ModelProfile {
            provider: Provider::Claude,
            model: model.into(),
            effort: "medium".into(),
        }
    }

    #[test]
    fn a_run_names_its_model_only_when_it_is_not_the_jobs() {
        let mut usual = run("r1", RunOutcome::Finished, 0);
        usual.profile = Some(profile("sonnet"));
        let mut other = run("r2", RunOutcome::Failed, 0);
        other.profile = Some(profile("opus"));
        other.model_reason = "Job row: Second choice".into();
        let mut state = job(vec![]);
        state.profile = Some(profile("sonnet"));
        assert_eq!(run_model(&usual, &state), None);
        let named = run_model(&other, &state).unwrap();
        assert!(named.starts_with("Ran on ") && named.ends_with(" · Job row: Second choice"));
        assert_eq!(
            run_model(&run("r3", RunOutcome::NotStarted, 0), &state),
            None
        );
    }

    #[test]
    fn a_source_check_older_than_a_day_no_longer_reads_as_ready() {
        let now = 10 * CHECK_FRESH_MS;
        let (note, fresh) = checked_note(&run("r1", RunOutcome::Finished, now - 60_000), now);
        assert!(
            fresh && note.starts_with("Checked by the run of "),
            "{note}"
        );
        let (note, fresh) =
            checked_note(&run("r2", RunOutcome::Finished, now - CHECK_FRESH_MS), now);
        assert!(
            !fresh && note.ends_with("The next run checks again."),
            "{note}"
        );
        let ready = SourceCheck {
            source: "gh pr list".into(),
            problem: None,
        };
        assert_eq!(stale_source_line(&ready), "gh pr list · was ready");
    }
}
