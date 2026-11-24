//! UI evaluator runs as the human sees them (OpenSpec change `ui-evaluators`, study
//! `ui/mockups.html`): how a run's state, model and time read, the Agents panel's Evaluations
//! group, the sidebar's "Evaluating" line, the bands over the main window and the Inbox cards
//! that allow evaluators in a project and start a full-desktop run. The snapshot lists the runs;
//! only the human reads them.
use super::*;
use std::path::Path;
use ui::{hint, mono, pill};

/// The human's name for a state, as the mockup's pills say it.
pub(crate) fn state_label(state: EvaluationState) -> &'static str {
    match state {
        EvaluationState::Queued => "Queued",
        EvaluationState::Waiting => "Waiting",
        EvaluationState::Running => "Running",
        EvaluationState::Finished => "Finished",
        EvaluationState::Failed => "Failed",
        EvaluationState::Stopped => "Stopped",
    }
}

pub(crate) fn state_color(state: EvaluationState, p: Palette) -> Hsla {
    match state {
        EvaluationState::Queued => p.subtle,
        EvaluationState::Waiting => p.yellow,
        EvaluationState::Running => p.focus,
        EvaluationState::Finished => p.green,
        EvaluationState::Failed | EvaluationState::Stopped => p.red,
    }
}

/// The state's pill. A queued run's is quiet, since it only waits its turn.
pub(crate) fn state_pill(state: EvaluationState, p: Palette) -> Div {
    let color = state_color(state, p);
    match state {
        EvaluationState::Queued => div()
            .flex_none()
            .px(px(7.))
            .rounded_full()
            .border_1()
            .border_dashed()
            .border_color(p.edge)
            .text_size(px(font::CAPTION))
            .text_color(color)
            .child(state_label(state)),
        _ => pill(state_label(state), color).text_size(px(font::CAPTION)),
    }
}

/// "Claude Opus 5.5": the model the provider reported. Until it reports one, the name asked for
/// and what was asked, "Claude Opus (opus)", since an alias such as `opus` names no one model.
pub(crate) fn model_name(catalog: &[ModelOption], run: &Evaluation) -> String {
    let provider = run.profile.provider;
    // A pinned run is one model, so a picker's "Opus · latest" or "Opus 5.5 · pinned" reads
    // as its name alone.
    let name = |model: &str| -> String {
        let label = models::model_label(catalog, provider, model);
        label.split(" · ").next().unwrap_or_default().to_owned()
    };
    match &run.reported_model {
        Some(reported) => format!("{} {}", provider.label(), name(reported)),
        None => format!(
            "{} {} ({})",
            provider.label(),
            name(&run.profile.model),
            run.profile.model
        ),
    }
}

/// "Claude Opus 5.5 · high".
pub(crate) fn pinned_model(catalog: &[ModelOption], run: &Evaluation) -> String {
    format!("{} · {}", model_name(catalog, run), run.profile.effort)
}

/// How long a run ran, "41 min" or "under 1 min", as the coordinator's wake-up says it.
pub(crate) fn run_minutes(run: &Evaluation, now: i64) -> String {
    let from = run.started_at.unwrap_or(run.created_at);
    match (run.ended_at.unwrap_or(now) - from).max(0) / 60_000 {
        0 => "under 1 min".into(),
        minutes => format!("{minutes} min"),
    }
}

/// "2nd", "3rd".
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A queued full-desktop run's place in the one queue every project shares: the run holding
/// the screen is first, then the queued ones, oldest first.
pub(crate) fn screen_place(run: &Evaluation, runs: &[Evaluation]) -> usize {
    let held = runs
        .iter()
        .any(|other| other.holds_screen && !other.state.ended());
    let ahead = (runs.iter())
        .filter(|other| other.mode == EvaluationMode::Desktop)
        .filter(|other| other.state == EvaluationState::Queued)
        .filter(|other| (other.created_at, &other.id) < (run.created_at, &run.id))
        .count();
    usize::from(held) + ahead + 1
}

/// Why a waiting run waits, in a few words: the human's choice or Start, or its pinned
/// model's reset.
pub(crate) fn waiting_for(run: &Evaluation) -> String {
    match (&run.session_id, run.resets_at) {
        (None, _) if run.holds_screen => "Needs your Start".into(),
        (None, _) => "Needs your choice".into(),
        (Some(_), Some(at)) => format!(
            "{} resets {}",
            run.profile.provider.label(),
            wording::local(at).map_or_else(String::new, wording::time_of_day)
        ),
        (Some(_), None) => format!("{} at its limit", run.profile.provider.label()),
    }
}

/// A row's last line: how long it has run, why it waits, its place for the screen, or when it
/// ended, with how long a finished run took or why one ended otherwise.
pub(crate) fn when_line(run: &Evaluation, runs: &[Evaluation], now: i64) -> String {
    let ended =
        || (run.ended_at.and_then(wording::local)).map_or_else(String::new, wording::time_of_day);
    match run.state {
        EvaluationState::Running => run_minutes(run, now),
        EvaluationState::Waiting => waiting_for(run),
        EvaluationState::Queued if run.mode == EvaluationMode::Desktop => {
            format!("{} for the screen", ordinal(screen_place(run, runs)))
        }
        EvaluationState::Queued => "Starting".into(),
        EvaluationState::Finished => format!("{} · {}", ended(), run_minutes(run, now)),
        EvaluationState::Failed | EvaluationState::Stopped => match &run.reason {
            Some(reason) => format!("{} · {}", ended(), reason.trim_end_matches('.')),
            None => ended(),
        },
    }
}

/// The sidebar's line under a project with runs that have not ended: "Evaluating · 1 running",
/// or how many wait when none runs.
pub(crate) fn sidebar_line(runs: &[Evaluation], project_id: &str) -> Option<String> {
    let unended: Vec<_> = (runs.iter())
        .filter(|run| run.project_id == project_id && !run.state.ended())
        .collect();
    let running = (unended.iter())
        .filter(|run| run.state == EvaluationState::Running)
        .count();
    match (unended.len(), running) {
        (0, _) => None,
        (waiting, 0) => Some(format!("Evaluating · {waiting} waiting")),
        (_, running) => Some(format!("Evaluating · {running} running")),
    }
}

/// A band over the main window while a run drives something: red while a full-desktop run has
/// the screen, quiet for each app-only run in its test copy.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Band {
    pub(crate) run_id: String,
    pub(crate) desktop: bool,
    /// "Claude Opus 5.5 · run 1 of 2 · 12 min".
    pub(crate) detail: String,
}

/// The bands to show: the full-desktop run first, then each running app-only run.
pub(crate) fn bands(catalog: &[ModelOption], runs: &[Evaluation], now: i64) -> Vec<Band> {
    let queued = (runs.iter())
        .filter(|run| run.mode == EvaluationMode::Desktop && run.state == EvaluationState::Queued)
        .count();
    let mut running: Vec<&Evaluation> = (runs.iter())
        .filter(|run| run.state == EvaluationState::Running)
        .collect();
    running.sort_by_key(|run| run.mode != EvaluationMode::Desktop);
    (running.into_iter())
        .map(|run| {
            let desktop = run.mode == EvaluationMode::Desktop;
            let place = (desktop && queued > 0).then(|| format!("run 1 of {}", queued + 1));
            let parts = [
                Some(model_name(catalog, run)),
                place,
                Some(run_minutes(run, now)),
            ];
            Band {
                run_id: run.id.clone(),
                desktop,
                detail: parts.into_iter().flatten().collect::<Vec<_>>().join(" · "),
            }
        })
        .collect()
}

/// Which of the host's evaluator cards an Inbox item is, with its run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EvaluatorCard {
    /// "Allow UI evaluators in this project?", raised by the project's first run.
    Allow { run_id: String },
    /// "Ready to use your screen", raised when a full-desktop run takes the screen.
    Start { run_id: String },
}
impl EvaluatorCard {
    pub(crate) fn of(item: &Attention) -> Option<Self> {
        let operation = item.operation_id.strip_prefix(EVALUATION_PREFIX)?;
        if let Some(run) = operation.strip_prefix("start:") {
            return Some(Self::Start {
                run_id: run.to_owned(),
            });
        }
        let (_, run) = operation.strip_prefix("allow:")?.rsplit_once(':')?;
        Some(Self::Allow {
            run_id: run.to_owned(),
        })
    }
    pub(crate) fn title(&self) -> &'static str {
        match self {
            Self::Allow { .. } => "Allow UI evaluators in this project?",
            Self::Start { .. } => "Ready to use your screen",
        }
    }
}

/// The Allow card's line about what the coordinator asks: the models of the runs held for the
/// human's choice and where their folders are.
pub(crate) fn allow_summary(
    catalog: &[ModelOption],
    runs: &[Evaluation],
    project_id: &str,
) -> String {
    let held: Vec<&Evaluation> = (runs.iter())
        .rev()
        .filter(|run| run.project_id == project_id)
        .filter(|run| run.state == EvaluationState::Waiting && run.session_id.is_none())
        .filter(|run| !run.holds_screen)
        .collect();
    let models: Vec<String> = held.iter().map(|run| model_name(catalog, run)).collect();
    let wants = match models.as_slice() {
        [] => "The coordinator wants to start an evaluator.".to_owned(),
        [one] => format!("The coordinator wants to start an evaluator on {one}."),
        [rest @ .., last] => format!(
            "The coordinator wants to start {} evaluators: {} and {last}.",
            models.len(),
            rest.join(", ")
        ),
    };
    let parents: BTreeSet<String> = (held.iter())
        .filter_map(|run| Path::new(&run.folder).parent())
        .map(|parent| shown_folder(&parent.to_string_lossy()))
        .collect();
    let folders = match parents.len() {
        1 => format!(
            " Each gets only its prompt and its own folder under {}.",
            parents.first().map_or("", String::as_str)
        ),
        _ => " Each gets only its prompt and its own folder.".into(),
    };
    format!("{wants}{folders}")
}

/// A path under the home folder as `~/…`.
pub(crate) fn shown_folder(path: &str) -> String {
    repository_tree::shown_path(path, spaces_view::home().as_deref())
}

/// What each choice on the Allow card means, as the mockup words it.
pub(crate) fn permission_detail(permission: EvaluatorPermission) -> &'static str {
    match permission {
        EvaluatorPermission::App => {
            "Drives only the app under test, a separate copy on a test profile. It can't see or click your other apps, and you can keep working."
        }
        EvaluatorPermission::Desktop => {
            "Can see and click only the apps a run names, on your screen. Each run waits for you to press Start and step away."
        }
        EvaluatorPermission::Off => "The coordinator is told evaluators are off.",
    }
}

/// The Allow card's primary button for the chosen permission.
pub(crate) fn allow_button(permission: EvaluatorPermission) -> &'static str {
    match permission {
        EvaluatorPermission::App => "Allow app only",
        EvaluatorPermission::Desktop => "Allow full desktop",
        EvaluatorPermission::Off => "Turn evaluators off",
    }
}

/// The host's words for a permission, which answer its card.
pub(crate) fn permission_answer(permission: EvaluatorPermission) -> &'static str {
    (EvaluatorPermission::CHOICES.iter())
        .find(|(_, choice)| *choice == permission)
        .map_or("", |(label, _)| label)
}

/// How an app a run may use reads: its name where this Mac has it installed, then its bundle
/// id, which is all there is for an app that is not.
pub(crate) fn app_label(
    bundle_id: &str,
    installed_name: Option<String>,
) -> (String, Option<String>) {
    match installed_name {
        Some(name) => (name, Some(bundle_id.to_owned())),
        None => (bundle_id.to_owned(), None),
    }
}

/// One app a full-desktop run may use: its name, its bundle id smaller and muted, and why it
/// deserves a second look.
pub(crate) fn app_line(app: &str, p: Palette) -> Div {
    let (name, id) = app_label(app, app_names::installed_app_name(app));
    div()
        .flex()
        .items_baseline()
        .gap_2()
        .child(div().flex_none().text_size(px(font::TEXT)).child(name))
        .children(id.map(|id| mono(id, p).flex_none()))
        .children(sensitive_app(app).map(|why| hint(why, p).text_color(p.yellow)))
}

/// The Start card's line about the run: its model and how many full-desktop runs wait after it.
pub(crate) fn start_summary(
    catalog: &[ModelOption],
    run: &Evaluation,
    runs: &[Evaluation],
) -> String {
    let after = (runs.iter())
        .filter(|other| other.mode == EvaluationMode::Desktop)
        .filter(|other| other.state == EvaluationState::Queued)
        .count();
    let next = match after {
        0 => String::new(),
        1 => " Another full-desktop run waits after it.".into(),
        n => format!(" {n} more full-desktop runs wait after it."),
    };
    format!(
        "Run {} on {} is next for the screen. It may see and use only these apps; every other app stays out of its screenshots and its reach.{next}",
        run.id,
        pinned_model(catalog, run)
    )
}

/// The band over the main window for `band`, with Open run and Stop.
pub(crate) fn band_view<V: 'static>(
    band: &Band,
    open: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    stop: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    p: Palette,
    cx: &mut Context<V>,
) -> Stateful<Div> {
    let color = if band.desktop { p.red } else { p.focus };
    let (title, controls) = match band.desktop {
        true => (
            "An evaluator is using this Mac.",
            div()
                .flex_none()
                .px(px(5.))
                .rounded(px(4.))
                .border_1()
                .border_color(p.edge)
                .font_family(mono_font())
                .text_size(px(font::CAPTION))
                .child("Esc")
                .into_any_element(),
        ),
        false => (
            "Evaluator running in a test copy.",
            button(SharedString::from(format!("open-run-{}", band.run_id)))
                .xsmall()
                .label("Open run")
                .on_click(cx.listener(move |view, _, window, cx| open(view, window, cx)))
                .into_any_element(),
        ),
    };
    div()
        .id(SharedString::from(format!("band-{}", band.run_id)))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py(px(6.))
        .bg(color.opacity(0.12))
        .border_b_1()
        .border_color(color.opacity(0.4))
        .text_size(px(font::SMALL))
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(title),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(band.detail.clone()),
        )
        .child(controls)
        .child(
            button(SharedString::from(format!("stop-band-{}", band.run_id)))
                .xsmall()
                .outline()
                .text_color(p.red)
                .border_color(p.red)
                .label("Stop")
                .on_click(cx.listener(move |view, _, window, cx| stop(view, window, cx))),
        )
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        EvaluatorCard, allow_summary, app_label, bands, model_name, ordinal, pinned_model,
        run_minutes, sidebar_line, start_summary, state_label, waiting_for, when_line,
    };
    use crate::wording;
    use workspace_core::{
        Attention, Evaluation, EvaluationMode, EvaluationState, ModelOption, ModelProfile, Provider,
    };

    fn run(id: &str, state: EvaluationState) -> Evaluation {
        Evaluation {
            id: id.into(),
            project_id: "p".into(),
            coordinator_id: "c".into(),
            session_id: Some(format!("s-{id}")),
            profile: ModelProfile {
                provider: Provider::Claude,
                model: "opus".into(),
                effort: "high".into(),
            },
            reported_model: Some("claude-opus-5-5".into()),
            mode: EvaluationMode::App,
            folder: "/runs/ux/run-a".into(),
            target: None,
            apps: vec![],
            holds_screen: false,
            state,
            reason: None,
            resets_at: None,
            created_at: 0,
            started_at: Some(0),
            ended_at: None,
        }
    }
    /// As the providers' CLIs list them, with the pickers' "latest" and "pinned".
    fn catalog() -> Vec<ModelOption> {
        let option = |provider, model: &str, label: &str| ModelOption {
            provider,
            model: model.into(),
            label: label.into(),
            efforts: vec!["high".into()],
        };
        vec![
            option(Provider::Claude, "opus", "Opus · latest"),
            option(Provider::Claude, "claude-opus-5-5", "Opus 5.5 · pinned"),
            option(Provider::Codex, "gpt-6.1-sol", "GPT 6.1 Sol"),
        ]
    }
    const MINUTE: i64 = 60_000;

    #[test]
    fn a_running_runs_minutes_turn_over_at_each_whole_minute() {
        let running = run("a", EvaluationState::Running);
        let at = |seconds: i64| run_minutes(&running, seconds * 1000);
        assert_eq!(at(0), "under 1 min");
        assert_eq!(at(59), "under 1 min");
        assert_eq!(at(60), "1 min");
        assert_eq!(at(119), "1 min");
        assert_eq!(at(120), "2 min");
        // An ended run keeps its length, however late it is read.
        let mut ended = run("b", EvaluationState::Finished);
        ended.ended_at = Some(41 * MINUTE + 59_000);
        assert_eq!(run_minutes(&ended, 500 * MINUTE), "41 min");
    }

    #[test]
    fn each_state_reads_as_the_mockup_says_it() {
        let names: Vec<_> = [
            EvaluationState::Queued,
            EvaluationState::Waiting,
            EvaluationState::Running,
            EvaluationState::Finished,
            EvaluationState::Failed,
            EvaluationState::Stopped,
        ]
        .map(state_label)
        .into();
        assert_eq!(
            names,
            [
                "Queued", "Waiting", "Running", "Finished", "Failed", "Stopped"
            ]
        );
    }

    #[test]
    fn a_rows_lines_give_its_pinned_model_and_its_time_or_reason() {
        let catalog = catalog();
        let running = run("a", EvaluationState::Running);
        assert_eq!(when_line(&running, &[], 12 * MINUTE), "12 min");
        assert_eq!(pinned_model(&catalog, &running), "Claude Opus 5.5 · high");
        let ended_at =
            |run: &Evaluation| wording::time_of_day(wording::local(run.ended_at.unwrap()).unwrap());
        let mut finished = run("b", EvaluationState::Finished);
        finished.ended_at = Some(41 * MINUTE);
        assert_eq!(
            when_line(&finished, &[], 99 * MINUTE),
            format!("{} · 41 min", ended_at(&finished))
        );
        let mut stopped = run("c", EvaluationState::Stopped);
        stopped.ended_at = Some(MINUTE);
        stopped.reason = Some("Stopped by you.".into());
        assert_eq!(
            when_line(&stopped, &[], 0),
            format!("{} · Stopped by you", ended_at(&stopped))
        );
    }

    #[test]
    fn a_run_is_named_by_the_model_its_provider_reported_and_never_as_latest() {
        let catalog = catalog();
        // Reported: the one model it runs, without the picker's "pinned".
        let reported = run("a", EvaluationState::Running);
        assert_eq!(pinned_model(&catalog, &reported), "Claude Opus 5.5 · high");
        // Not reported yet: the name asked for, without "latest", and the alias itself.
        let mut asked = run("b", EvaluationState::Waiting);
        asked.reported_model = None;
        assert_eq!(model_name(&catalog, &asked), "Claude Opus (opus)");
        asked.profile = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "high".into(),
        };
        assert_eq!(
            model_name(&catalog, &asked),
            "Codex GPT 6.1 Sol (gpt-6.1-sol)"
        );
        // A model no list names reads as its id.
        let mut unknown = run("c", EvaluationState::Running);
        unknown.reported_model = Some("claude-opus-6-0".into());
        assert_eq!(model_name(&catalog, &unknown), "Claude claude-opus-6-0");
    }

    #[test]
    fn an_installed_app_reads_by_its_name_with_its_bundle_id_after() {
        assert_eq!(
            app_label("com.apple.calculator", Some("Calculator".into())),
            ("Calculator".into(), Some("com.apple.calculator".into()))
        );
        // Not on this Mac: only the id is known.
        assert_eq!(
            app_label("com.example.missing", None),
            ("com.example.missing".into(), None)
        );
    }

    #[test]
    fn a_waiting_run_says_what_it_waits_for() {
        let mut asked = run("a", EvaluationState::Waiting);
        asked.session_id = None;
        assert_eq!(waiting_for(&asked), "Needs your choice");
        asked.holds_screen = true;
        assert_eq!(waiting_for(&asked), "Needs your Start");
        let mut limited = run("b", EvaluationState::Waiting);
        assert_eq!(waiting_for(&limited), "Claude at its limit");
        limited.resets_at = Some(0);
        assert!(waiting_for(&limited).starts_with("Claude resets "));
    }

    #[test]
    fn queued_desktop_runs_count_their_place_behind_the_screen_holder() {
        let desktop = |id: &str, at: i64, state| Evaluation {
            mode: EvaluationMode::Desktop,
            created_at: at,
            session_id: None,
            ..run(id, state)
        };
        let mut holder = desktop("a", 0, EvaluationState::Waiting);
        holder.holds_screen = true;
        let second = desktop("b", 1, EvaluationState::Queued);
        let third = desktop("c", 2, EvaluationState::Queued);
        let runs = [holder.clone(), third.clone(), second.clone()];
        assert_eq!(when_line(&second, &runs, 0), "2nd for the screen");
        assert_eq!(when_line(&third, &runs, 0), "3rd for the screen");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(21), "21st");
    }

    #[test]
    fn the_sidebar_says_how_many_run_or_wait() {
        let mut other = run("x", EvaluationState::Running);
        other.project_id = "q".into();
        let runs = [
            run("a", EvaluationState::Running),
            run("b", EvaluationState::Waiting),
            run("c", EvaluationState::Finished),
            other,
        ];
        assert_eq!(
            sidebar_line(&runs, "p").as_deref(),
            Some("Evaluating · 1 running")
        );
        assert_eq!(
            sidebar_line(&runs[1..], "p").as_deref(),
            Some("Evaluating · 1 waiting")
        );
        assert_eq!(sidebar_line(&runs[2..3], "p"), None);
    }

    #[test]
    fn the_full_desktop_band_comes_first_and_counts_the_runs_waiting_for_the_screen() {
        let catalog = catalog();
        let app = run("a", EvaluationState::Running);
        let mut desktop = run("d", EvaluationState::Running);
        desktop.mode = EvaluationMode::Desktop;
        let mut queued = run("q", EvaluationState::Queued);
        queued.mode = EvaluationMode::Desktop;
        let bands = bands(&catalog, &[app, desktop, queued], 12 * MINUTE);
        assert_eq!(bands.len(), 2);
        assert!(bands[0].desktop);
        assert_eq!(bands[0].detail, "Claude Opus 5.5 · run 1 of 2 · 12 min");
        assert!(!bands[1].desktop);
        assert_eq!(bands[1].detail, "Claude Opus 5.5 · 12 min");
        // Nothing running, nothing shown.
        assert!(super::bands(&catalog, &[run("w", EvaluationState::Waiting)], 0).is_empty());
    }

    #[test]
    fn the_hosts_evaluator_cards_are_known_by_their_operation() {
        let item = |operation: &str| Attention {
            id: "i".into(),
            project_id: "project-1".into(),
            session_id: "c".into(),
            host: "local".into(),
            operation_id: operation.into(),
            prompt: String::new(),
            options: vec![],
            answer: None,
            repository_request: None,
        };
        let allow = EvaluatorCard::of(&item("evaluation:allow:project-1:run-a")).unwrap();
        assert_eq!(
            allow,
            EvaluatorCard::Allow {
                run_id: "run-a".into()
            }
        );
        let start = EvaluatorCard::of(&item("evaluation:start:run-b")).unwrap();
        assert_eq!(start.title(), "Ready to use your screen");
        assert_eq!(EvaluatorCard::of(&item("publish:t1")), None);
    }

    #[test]
    fn the_allow_card_names_the_held_runs_models_and_their_folder() {
        let catalog = catalog();
        let held = |id: &str, provider, model: &str| Evaluation {
            session_id: None,
            reported_model: None,
            folder: format!("/runs/ux/{id}"),
            profile: ModelProfile {
                provider,
                model: model.into(),
                effort: "high".into(),
            },
            ..run(id, EvaluationState::Waiting)
        };
        let runs = [
            held("b", Provider::Codex, "gpt-6.1-sol"),
            held("a", Provider::Claude, "opus"),
        ];
        assert_eq!(
            allow_summary(&catalog, &runs, "p"),
            "The coordinator wants to start 2 evaluators: Claude Opus (opus) and Codex GPT 6.1 Sol (gpt-6.1-sol). Each gets only its prompt and its own folder under /runs/ux."
        );
        assert_eq!(
            allow_summary(&catalog, &runs[1..], "p"),
            "The coordinator wants to start an evaluator on Claude Opus (opus). Each gets only its prompt and its own folder under /runs/ux."
        );
    }

    #[test]
    fn the_start_card_names_the_model_and_the_runs_after_it() {
        let catalog = catalog();
        let mut first = run("run-a", EvaluationState::Waiting);
        first.mode = EvaluationMode::Desktop;
        // Before its Start it has no session, so no reported model yet.
        first.reported_model = None;
        let mut next = first.clone();
        next.state = EvaluationState::Queued;
        let summary = start_summary(&catalog, &first, &[first.clone(), next]);
        assert!(
            summary.starts_with("Run run-a on Claude Opus (opus) · high is next"),
            "{summary}"
        );
        assert!(
            summary.ends_with("Another full-desktop run waits after it."),
            "{summary}"
        );
        assert!(!start_summary(&catalog, &first, &[]).contains("after it"));
    }
}
