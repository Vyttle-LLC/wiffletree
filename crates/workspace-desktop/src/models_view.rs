//! The Models page: the subscriptions on this machine, when a subscription counts as spent,
//! who runs each job, the reviews and the notes coordinators read.
use super::*;
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use models::{Cell, Now};
use ui::{card, hint, icon, pill};

pub(super) fn card_title(title: &'static str, note: &'static str, p: Palette) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap_2()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(hint(note, p))
}

/// Where Wiffletree may move on, in percent, as the threshold menu offers it.
const SWITCH_CHOICES: [u32; 6] = [75, 80, 85, 90, 95, 99];

fn provider_color(provider: Provider, p: Palette) -> Hsla {
    match provider {
        Provider::Claude => p.accent,
        Provider::Codex => p.blue,
    }
}
fn dot(provider: Provider, p: Palette) -> Div {
    div()
        .flex_none()
        .size(px(8.))
        .rounded_full()
        .bg(provider_color(provider, p))
}

/// The Models page. It also finds the CLIs on this machine and lists their models, which the
/// welcome screen and every model picker read.
pub(super) struct ModelsPage {
    bridge: Bridge,
    shared: Shared,
    /// The page's draft and the host's saved copy.
    model_selection: ModelSelection,
    pub(super) saved_model_selection: ModelSelection,
    guide_input: Entity<TextareaState>,
    verifiers: Entity<verifiers::VerifierEditor>,
    /// What the last refresh found for each provider's CLI: installed, signed in and its plan.
    pub(super) detection: Detection,
    /// The setup last suggested from `detection`, while the store has no saved one.
    suggested: Option<Setup>,
    catalog: Rc<Vec<ModelOption>>,
    pub(super) catalog_pending: usize,
    catalog_loaded: bool,
    catalog_notice: String,
}
impl EventEmitter<Intent> for ModelsPage {}
impl ModelsPage {
    /// The review settings as the Reviews card shows them, for the pickers' pre-fill.
    pub(super) fn verification(&self, cx: &App) -> VerificationSettings {
        (self.verifiers.read(cx).verification().cloned()).unwrap_or_default()
    }
    pub(super) fn new(
        bridge: Bridge,
        verifiers: Entity<verifiers::VerifierEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let guide_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 24)
                .placeholder("e.g. Ask for small work only for contained changes with tests.")
        });
        cx.subscribe(&guide_input, |view: &mut Self, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                view.model_selection.guide = input.read(cx).value().to_string();
                cx.notify();
            }
        })
        .detach();
        // The Save bar follows the verifiers while they are part of an unsaved setup.
        cx.observe(&verifiers, |_, _, cx| cx.notify()).detach();
        Self {
            bridge,
            shared: Shared::default(),
            model_selection: ModelSelection::default(),
            saved_model_selection: ModelSelection::default(),
            guide_input,
            verifiers,
            detection: Detection::new(),
            suggested: None,
            catalog: Rc::new(fallback_catalog()),
            catalog_pending: 0,
            catalog_loaded: false,
            catalog_notice: String::new(),
        }
    }
    /// Follows the saved setup in each snapshot. The CLIs are checked once the first snapshot
    /// arrives, on every store, so the welcome, this page, the pickers and the host share one
    /// result.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let opened = shared.page == Page::Models && self.shared.page != Page::Models;
        self.shared = shared;
        if moved && let Some(snapshot) = self.shared.snapshot() {
            let saved = snapshot.model_selection.clone();
            self.sync_model_selection(&saved, window, cx);
            if saved.is_saved() {
                self.verifiers
                    .update(cx, |editor, cx| editor.setup_saved(cx));
            }
            if !self.catalog_loaded {
                self.refresh_models(window, cx);
            }
            self.offer_suggestion(window, cx);
            cx.notify();
        }
        if opened {
            if !self.catalog_loaded {
                self.refresh_models(window, cx);
            }
            cx.notify();
        }
    }
    pub(super) fn refresh_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.catalog_pending > 0 {
            return;
        }
        self.catalog_pending = 2;
        self.catalog_loaded = true;
        self.catalog_notice.clear();
        for provider in PROVIDERS {
            let ask = ask::inspect_provider(provider);
            self.ask(ask, window, cx, move |view, report, window, cx| {
                view.inspected(provider, report, window, cx)
            });
        }
        cx.notify();
    }
    fn inspected(
        &mut self,
        provider: Provider,
        report: Result<ProviderReport, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.catalog_pending -= 1;
        // A check that failed counts as not usable, so its pill reads "Cannot start".
        let (status, models) = match report {
            Ok(report) => (report.status, report.models),
            Err(_) => (ProviderStatus::default(), vec![]),
        };
        self.detection.insert(provider, status);
        cx.emit(Intent::Detected(Rc::new(self.detection.clone())));
        if models.is_empty() {
            if !self.catalog_notice.is_empty() {
                self.catalog_notice.push(' ');
            }
            self.catalog_notice.push_str(&format!(
                "{} list unavailable; showing built-in and saved models.",
                provider.label()
            ));
        } else {
            let catalog = Rc::make_mut(&mut self.catalog);
            catalog.retain(|model| model.provider != provider);
            catalog.extend(models);
            cx.emit(Intent::Catalog(self.catalog.clone()));
        }
        self.offer_suggestion(window, cx);
    }
    fn save_model_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A first save also sends the verifiers shown; afterwards they save on their own.
        let verification = self
            .suggested
            .as_ref()
            .and_then(|_| self.verifiers.read(cx).verification().cloned());
        let selection = self.model_selection.clone();
        let ask = ask::set_model_selection(selection.clone(), verification);
        self.ask(
            ask,
            window,
            cx,
            move |view, saved, window, cx| match saved {
                Ok(saved) => view.saved(&selection, saved, window, cx),
                Err(error) => toast_error(error, window, cx),
            },
        );
    }
    /// The host answers with the saved selection at its new revision, so the store reads as
    /// saved before the next snapshot arrives.
    fn saved(
        &mut self,
        selection: &ModelSelection,
        saved: ModelSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_selection = models::after_save(&self.model_selection, selection, &saved);
        self.saved_model_selection = saved;
        self.suggested = None;
        self.verifiers
            .update(cx, |editor, cx| editor.setup_saved(cx));
        toast("Models saved", window, cx);
    }
    fn dismiss_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(
            ask::dismiss_model_notes(),
            window,
            cx,
            |view, saved, window, cx| match saved {
                Ok(saved) => {
                    view.model_selection.notes = saved.notes.clone();
                    view.saved_model_selection.notes = saved.notes;
                    cx.notify();
                }
                Err(error) => toast_error(error, window, cx),
            },
        );
    }
    /// A fresh store waits for its first suggestion before anything can change.
    fn models_editable(&self) -> bool {
        models::editable(&self.saved_model_selection, self.suggested.as_ref())
    }

    /// Edited, or holding rows copied for verifiers that have none yet, which a save stores.
    fn model_selection_dirty(&self) -> bool {
        self.model_selection != self.saved_model_selection
            || !self.model_selection.copied.is_empty()
    }

    /// Unsaved edits survive refreshes; an untouched editor follows the saved selection.
    pub(super) fn sync_model_selection(
        &mut self,
        saved: &ModelSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_selection == self.saved_model_selection {
            self.model_selection = saved.clone();
            self.guide_input.update(cx, |input, cx| {
                input.set_value(saved.guide.clone(), window, cx)
            });
        }
        self.saved_model_selection = saved.clone();
    }

    fn suggestion_untouched(&self, suggested: &Setup, cx: &App) -> bool {
        models::untouched(
            suggested,
            &self.model_selection,
            self.verifiers.read(cx).verification(),
        )
    }

    fn show_suggestion(&mut self, setup: &Setup, window: &mut Window, cx: &mut Context<Self>) {
        self.model_selection = setup.selection.clone();
        self.guide_input.update(cx, |input, cx| {
            input.set_value(setup.selection.guide.clone(), window, cx)
        });
        self.verifiers.update(cx, |editor, cx| {
            editor.hold_draft(setup.verification.clone(), cx)
        });
    }

    /// On a store with no saved setup, suggests one from the last detection. An untouched
    /// suggestion follows a new detection; one the human edited is kept.
    pub(super) fn offer_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shared.index.is_none() || self.catalog_pending > 0 {
            return;
        }
        let Some(offer) = models::offer(
            &self.saved_model_selection,
            self.shared.snapshot().is_some_and(|s| s.guide_saved),
            self.suggested.as_ref(),
            (
                &self.model_selection,
                self.verifiers.read(cx).verification(),
            ),
            &self.detection,
            &self.catalog,
        ) else {
            return;
        };
        if offer.replaces_draft {
            self.show_suggestion(&offer.suggested, window, cx);
        }
        self.suggested = Some(offer.suggested);
    }

    /// The first-run notice: what was suggested, or why nothing could be.
    fn suggestion_banner(&self, suggested: &Setup, p: Palette) -> Div {
        let found = !suggested.selection.jobs.is_empty();
        let (color, title, note) = if found {
            (
                p.accent,
                "Suggested setup · not saved yet",
                "We filled this in from the CLIs signed in on this machine. Change anything you like, then Save. Agents cannot start until a setup is saved.",
            )
        } else {
            (
                p.yellow,
                "No signed-in CLI found",
                "Install Claude Code or Codex and sign in, then press Refresh. Wiffletree uses your existing CLI sign-in and needs at least one provider.",
            )
        };
        div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(color)
            .bg(color.opacity(0.08))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(hint(note, p))
    }

    /// The one-time notes, such as the upgrade's, and a note for each row copied for a verifier
    /// that has none on this machine yet.
    fn notes(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let saved = &self.saved_model_selection.notes;
        let copied = &self.model_selection.copied;
        if saved.is_empty() && copied.is_empty() {
            return None;
        }
        let mut lines = div().flex().flex_col().gap_1();
        if let Some((first, rest)) = saved.split_first() {
            lines = lines
                .child(div().font_weight(FontWeight::SEMIBOLD).child(first.clone()))
                .children(rest.iter().map(|line| hint(line.clone(), p)));
        }
        for focus in copied {
            lines = lines.child(hint(
                format!(
                    "{focus} has no row on this Mac yet, so it uses a copy of another row. Check it, then Save."
                ),
                p,
            ));
        }
        Some(
            ui::banner(p.focus)
                .items_start()
                .child(lines.flex_1().min_w_0())
                .when(!saved.is_empty(), |d| {
                    d.child(
                        button("dismiss-model-notes")
                            .ghost()
                            .small()
                            .label("Dismiss")
                            .on_click(cx.listener(|v, _, w, c| v.dismiss_notes(w, c))),
                    )
                }),
        )
    }

    fn subscriptions_card(&self, p: Palette) -> Div {
        let switch_at = self.model_selection.switch_at;
        let mut row = div().flex().flex_wrap().gap_3();
        for provider in PROVIDERS {
            let status = match self.detection.get(&provider) {
                Some(found) => {
                    let color = match (found.installed, found.signed_in) {
                        (true, true) => p.green,
                        (true, false) => p.yellow,
                        (false, _) => p.subtle,
                    };
                    hint(models::status_label(found), p).text_color(color)
                }
                None => hint("Checking CLI…", p),
            };
            let fix = (self.suggested.as_ref())
                .and(self.detection.get(&provider))
                .and_then(|found| models::fix_hint(provider, found));
            let reading = self.shared.quotas.iter().find(|r| r.provider == provider);
            let mut windows: Vec<&QuotaWindow> =
                reading.map_or(vec![], |r| r.windows.iter().collect());
            windows.sort_by_key(|w| w.duration_mins.unwrap_or(u64::MAX));
            let mut meters = div().flex().flex_col().gap_1();
            for window in &windows {
                meters = meters.child(meter(window, switch_at, p));
            }
            let measured = windows.iter().any(|w| w.used_percent.is_some());
            let footer = match reading {
                Some(reading) if measured => {
                    let resets = (windows.iter())
                        .filter_map(|w| Some((w, w.resets_at?)))
                        .max_by_key(|(_, at)| *at)
                        .map(|(w, at)| format!("{} resets {} · ", w.label, models::reset_time(at)))
                        .unwrap_or_default();
                    format!(
                        "{resets}read {}",
                        quotas::reading_age(reading.observed_at).to_lowercase()
                    )
                }
                Some(reading) => reading
                    .error
                    .clone()
                    .unwrap_or_else(|| "Unavailable: no percentage reported yet.".into()),
                None => "Unavailable".into(),
            };
            row = row.child(
                div()
                    .flex_1()
                    .min_w(px(280.))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(p.edge.opacity(edge::CARD))
                    .bg(p.surface)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(dot(provider, p))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(provider.label()),
                            )
                            .child(status),
                    )
                    .children(fix.map(|fix| hint(fix, p)))
                    .when(!measured, |d| {
                        d.child(hint("Unavailable", p).text_color(p.text))
                    })
                    .child(meters)
                    .child(hint(footer, p)),
            );
        }
        card(p)
            .gap_3()
            .child(card_title(
                "Subscriptions",
                "the CLIs on this Mac and how much of each limit is used",
                p,
            ))
            .when(!self.catalog_notice.is_empty(), |d| {
                d.child(hint(self.catalog_notice.clone(), p).text_color(p.yellow))
            })
            .child(row)
            .child(hint(
                format!("The tick on each bar is where Wiffletree moves on ({switch_at}%). Limits come only from the providers' own telemetry; Claude reports them during its turns."),
                p,
            ))
    }

    fn threshold_card(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let current = self.model_selection.switch_at;
        let weak = cx.weak_entity();
        let picker = button("switch-at")
            .small()
            .label(format!("{current}%"))
            .dropdown_caret(true)
            .disabled(!self.models_editable())
            .dropdown_menu(move |mut menu, _, _| {
                let mut choices = SWITCH_CHOICES.to_vec();
                if !choices.contains(&current) {
                    choices.push(current);
                    choices.sort_unstable();
                }
                for at in choices {
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("{at}%"))
                            .checked(at == current)
                            .on_click(move |_, _, c| {
                                let _ = weak.update(c, |v, c| {
                                    v.model_selection.switch_at = at;
                                    c.notify();
                                });
                            }),
                    );
                }
                menu
            });
        card(p)
            .gap_3()
            .child(card_title("When a subscription runs low", "one rule for every job", p))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child("Move to the next choice once any limit (5-hour, weekly or another) passes")
                    .child(picker)
                    .child(". When every choice is past it, wait for the earliest reset, or skip the review where its row says skip."),
            )
            .child(hint(
                "Applies when an agent starts. An agent that is already running keeps its model. Fresh reviewers start each review, so they pick up a switch on the next review. Shared with your other Macs.",
                p,
            ))
    }

    /// A dropdown for one cell of `job`'s row, which holds `current`, or for a then choice skip
    /// when `skips`; `first` is the provider of the cell's line's first choice, which a then
    /// choice may not use. Only a verifier's then choice offers skip.
    fn cell_menu(
        &self,
        job: &Job,
        which: Cell,
        current: Option<ModelProfile>,
        skips: bool,
        first: Option<Provider>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let catalog = self.catalog.clone();
        let may_skip = which.then && matches!(job, Job::Verifier { .. });
        let label = match (&current, which) {
            (Some(profile), _) => models::cell_label(&catalog, profile),
            (None, Cell { then: true, .. }) if skips => "skip".to_owned(),
            (None, Cell { then: true, .. }) => "wait".to_owned(),
            (None, Cell { small: true, .. }) => "same".to_owned(),
            (None, _) => "No model".to_owned(),
        };
        // A then choice is on the other subscription.
        let providers: Vec<Provider> = PROVIDERS
            .into_iter()
            .filter(|p| !which.then || Some(*p) != first)
            .collect();
        let groups: Vec<(Provider, Vec<ModelProfile>)> = (providers.into_iter())
            .map(|provider| {
                let profiles = models::profile_choices(&catalog, &self.model_selection, provider);
                (provider, profiles)
            })
            .collect();
        let none = match which {
            Cell { then: true, .. } => Some("Wait for the reset"),
            Cell { small: true, .. } => Some("Same as the first line"),
            _ if *job == Job::Keeper => Some("No model"),
            _ => None,
        };
        let id = SharedString::from(format!(
            "cell-{}-{}-{}",
            job_key(job),
            which.small,
            which.then
        ));
        let weak = cx.weak_entity();
        let job = job.clone();
        button(id)
            .small()
            .when(current.is_none(), |b| b.ghost())
            .label(label)
            .dropdown_caret(true)
            .disabled(!self.models_editable())
            .dropdown_menu(move |mut menu: PopupMenu, _, _| {
                let set = |profile: Option<ModelProfile>| {
                    let (weak, job) = (weak.clone(), job.clone());
                    move |_: &ClickEvent, _: &mut Window, c: &mut App| {
                        let _ = weak.update(c, |v, c| {
                            models::set_cell(&mut v.model_selection, &job, which, profile.clone());
                            c.notify();
                        });
                    }
                };
                if may_skip {
                    let resets = first.map_or("its subscription", Provider::label);
                    let skip = {
                        let (weak, job) = (weak.clone(), job.clone());
                        move |_: &ClickEvent, _: &mut Window, c: &mut App| {
                            let _ = weak.update(c, |v, c| {
                                if let Some(row) = v.model_selection.row_mut(&job) {
                                    models::set_then(row, which.small, Then::Skip);
                                }
                                c.notify();
                            });
                        }
                    };
                    menu = menu
                        .item(
                            titled(
                                "Wait for the reset",
                                format!("The review runs once {resets} resets"),
                            )
                            .checked(current.is_none() && !skips)
                            .on_click(set(None)),
                        )
                        .item(
                            titled("Skip this review", "The others run without it".into())
                                .checked(skips)
                                .on_click(skip),
                        );
                } else if let Some(none) = none {
                    menu = menu.item(
                        PopupMenuItem::new(none)
                            .checked(current.is_none())
                            .on_click(set(None)),
                    );
                }
                for (provider, profiles) in &groups {
                    menu = menu.label(provider.label());
                    for profile in profiles {
                        menu = menu.item(
                            PopupMenuItem::new(models::cell_label(&catalog, profile))
                                .checked(current.as_ref() == Some(profile))
                                .on_click(set(Some(profile.clone()))),
                        );
                    }
                }
                menu.scrollable(true).max_h(px(360.))
            })
    }

    fn right_now(&self, row: &JobRow, small: bool, p: Palette) -> Stateful<Div> {
        let pick = models::right_now(
            &self.model_selection,
            row,
            small,
            &self.shared.quotas,
            &self.detection,
            ui::now(),
        );
        let reason = match &pick {
            Ok(pick) if pick.skips => {
                let again = (pick.until.map(models::reset_time)).map_or(String::new(), |at| {
                    format!(" Runs again after the reset, {at}.")
                });
                format!("Skipped: {}.{again}", pick.reason)
            }
            Ok(pick) => pick.reason.clone(),
            Err(why) => why.clone(),
        };
        let shown = match models::now_of(row, small, &pick, &self.detection) {
            Now::First(provider) => pill(provider.label(), p.green),
            Now::Moved(provider) => pill(provider.label(), p.blue),
            Now::Waits(Some(at)) => pill(format!("Waits · {at}"), p.yellow),
            Now::Waits(None) => pill("Waits", p.yellow),
            Now::Skips(Some(at)) => pill(format!("Skips · {at}"), p.subtle),
            Now::Skips(None) => pill("Skips", p.subtle),
            Now::Cannot => pill("Cannot start", p.red),
            Now::Checking => pill("Checking…", p.subtle),
        };
        div()
            .id(SharedString::from(format!(
                "now-{}-{small}",
                job_key(&row.job)
            )))
            .child(shown)
            .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
    }

    fn menu_for(&self, row: &JobRow, which: Cell, cx: &mut Context<Self>) -> impl IntoElement {
        let line = if which.small {
            row.small.as_ref()
        } else {
            Some(&row.main)
        };
        let current = models::cell(row, which).cloned();
        let skips = which.then && line.is_some_and(|line| line.then == Then::Skip);
        let first = line.map(|line| line.first.provider);
        self.cell_menu(&row.job, which, current, skips, first, cx)
    }
    fn job_row(
        &self,
        row: &JobRow,
        verifiers: &[VerifierConfig],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (name, note) = job_label(&row.job, verifiers);
        let main = |then| Cell { small: false, then };
        let small = |then| Cell { small: true, then };
        let arrow = || div().text_color(p.subtle).child("→");
        let line = |which: fn(bool) -> Cell, cx: &mut Context<Self>| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(self.menu_for(row, which(false), cx))
                .child(arrow())
                .child(self.menu_for(row, which(true), cx))
        };
        // Small work sits under the first line: a menu that adds one, or its own line.
        let small_work = match row.small {
            Some(_) => line(small, cx),
            None => div().child(self.menu_for(row, small(false), cx)),
        };
        let mut now = div()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .child(self.right_now(row, false, p));
        if row.small.is_some() {
            now = now.child(self.right_now(row, true, p));
        }
        div()
            .py_2()
            .flex()
            .items_start()
            .gap_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::CARD))
            .child(
                div()
                    .w(px(170.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_ellipsis()
                            .child(name),
                    )
                    .child(hint(note, p).text_ellipsis()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(70.))
                                    .flex_none()
                                    .child(dot(row.main.first.provider, p)),
                            )
                            .child(line(main, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(hint("Small work", p).w(px(70.)).flex_none())
                            .child(small_work),
                    ),
            )
            .child(now.w(px(120.)).flex_none())
    }
    fn table_card(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let heading = |text: &'static str| {
            div()
                .text_size(px(font::TINY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.subtle)
                .child(text)
        };
        let mut table = div().flex().flex_col().child(
            div()
                .pb_1()
                .flex()
                .gap_2()
                .border_b_1()
                .border_color(p.edge.opacity(edge::STRONG))
                .child(heading("JOB").w(px(170.)).flex_none())
                .child(heading("FIRST CHOICE → THEN, AND FOR SMALL WORK").flex_1())
                .child(heading("RIGHT NOW").w(px(120.)).flex_none()),
        );
        if !self.models_editable() {
            return card(p)
                .gap_3()
                .child(card_title("Who runs what", "each job's model", p))
                .child(hint(
                    "Available once the CLIs on this machine are checked.",
                    p,
                ));
        }
        let verifiers =
            (self.verifiers.read(cx).verification()).map_or(vec![], |v| v.verifiers.clone());
        for row in &self.model_selection.jobs {
            table = table.child(self.job_row(row, &verifiers, p, cx));
        }
        if self.model_selection.keeper().is_none() {
            table = table.child(self.keeper_placeholder(p, cx));
        }
        let error = self.model_selection.validate().err();
        card(p)
            .gap_3()
            .child(card_title("Who runs what", "each job's model on this Mac", p))
            .child(hint(
                "First choice is used until its subscription passes the threshold. \"Small work\" is the lighter line the coordinator may ask for; leave it as same to always use the first line.",
                p,
            ))
            .child(table)
            .children(
                error
                    .filter(|_| !self.model_selection.jobs.is_empty())
                    .map(|error| hint(error.to_string(), p).text_color(p.red)),
            )
            .child(hint(
                "The coordinator never picks exact models: it asks for a job and, optionally, small work, and Wiffletree picks the line from this table. Sorting comments and merging skill updates use only the PR keeper's Claude choice. You can still change any one agent from its chat.",
                p,
            ))
    }
    /// The PR keeper's row while it has no model: one menu to choose one.
    fn keeper_placeholder(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let first = Cell {
            small: false,
            then: false,
        };
        div()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(170.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("PR keeper"))
                    .child(hint("Fixes conflicts, sorts comments", p)),
            )
            .child(
                div()
                    .flex_1()
                    .child(self.cell_menu(&Job::Keeper, first, None, false, None, cx)),
            )
            .child(hint("No PR keeper model: its model triggers stay off.", p).flex_1())
    }

    fn review_card(&self, p: Palette) -> Div {
        card(p)
            .gap_3()
            .child(card_title(
                "Reviews",
                "which reviewers run, and how many tries a ticket gets",
                p,
            ))
            .child(if !self.models_editable() {
                hint("Available once the CLIs on this machine are checked.", p).into_any_element()
            } else if self.suggested.is_some() && self.model_selection.jobs.is_empty() {
                hint("Available once a CLI is signed in.", p).into_any_element()
            } else {
                self.verifiers.clone().into_any_element()
            })
    }

    fn guide_card(&self, p: Palette) -> Div {
        let bytes = self.model_selection.guide.len();
        card(p)
            .gap_3()
            .child(card_title(
                "Notes",
                "optional notes coordinators read with the table",
                p,
            ))
            .child(
                Textarea::new(&self.guide_input)
                    .w_full()
                    .disabled(!self.models_editable()),
            )
            .child(
                hint(
                    format!(
                        "{:.1} / {} KB · shared with your other Macs",
                        bytes as f32 / 1024.,
                        MAX_GUIDE_BYTES / 1024
                    ),
                    p,
                )
                .text_right()
                .when(bytes > MAX_GUIDE_BYTES, |d| d.text_color(p.red)),
            )
    }

    fn models_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .children(
                self.suggested
                    .as_ref()
                    .map(|suggested| self.suggestion_banner(suggested, p)),
            )
            .children(self.notes(p, cx))
            .child(hint(
                "Who runs each job, and what happens when a subscription runs low. The table is saved for this Mac; the threshold, the reviews and the notes are shared with your other Macs. Your own choices are never limited.",
                p,
            ))
            .child(self.subscriptions_card(p))
            .child(self.threshold_card(p, cx))
            .child(self.table_card(p, cx))
            .child(self.review_card(p))
            .child(self.guide_card(p))
    }

    fn models_footer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        // A suggested setup is unsaved from the start: Save needs no edit first.
        let untouched = self
            .suggested
            .as_ref()
            .map(|suggested| self.suggestion_untouched(suggested, cx));
        let dirty = untouched.is_some() || self.model_selection_dirty();
        let invalid = self.model_selection.validate().err();
        let loading = self.catalog_pending > 0;
        let empty = self.model_selection.jobs.is_empty();
        let can_save = models::can_save(
            self.models_editable(),
            untouched.is_some(),
            self.model_selection_dirty(),
            &self.model_selection,
        );
        let state = match (&invalid, untouched) {
            (Some(_), Some(_)) if empty => hint("Sign in to a CLI to save", p).text_color(p.red),
            (Some(error), _) if dirty => hint(error.to_string(), p).text_color(p.red),
            (None, Some(true)) => hint("Suggested setup · not saved yet", p),
            _ if dirty => hint("Unsaved changes", p),
            _ if loading && !self.saved_model_selection.is_saved() => {
                hint("Checking the CLIs on this machine…", p)
            }
            _ => hint("Saved", p),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                ui::icon_or_spinner(
                    button("refresh-models").ghost(),
                    icon("refresh"),
                    None,
                    loading,
                    gpui_component::Size::Small,
                )
                .disabled(loading)
                .tooltip("Check the CLIs again and refresh their model lists")
                .on_click(cx.listener(|v, _, w, c| v.refresh_models(w, c))),
            )
            .child(state.flex_1().min_w_0())
            .when(dirty && untouched != Some(true), |d| {
                d.child(
                    button("discard-models")
                        .ghost()
                        .label("Discard")
                        .on_click(cx.listener(|v, _, w, c| {
                            // Before the first save there is only the suggestion to go back to.
                            if let Some(suggested) = v.suggested.clone() {
                                v.show_suggestion(&suggested, w, c);
                            } else {
                                let saved = v.saved_model_selection.clone();
                                v.model_selection = v.saved_model_selection.clone();
                                v.sync_model_selection(&saved, w, c);
                            }
                            c.notify();
                        })),
                )
            })
            .child(
                button("save-models")
                    .primary()
                    .label("Save")
                    .disabled(!can_save)
                    .on_click(cx.listener(|v, _, w, c| v.save_model_selection(w, c))),
            )
    }
}

/// A job's name and what it does, as the table's first column shows them.
fn job_label(job: &Job, verifiers: &[VerifierConfig]) -> (String, String) {
    match job {
        Job::Coordinator => ("Coordinator".into(), "Plans, talks to you".into()),
        Job::Implementer => ("Implementer".into(), "Writes the change".into()),
        Job::Tester => ("Tester".into(), "Runs build, lint, tests".into()),
        Job::Keeper => ("PR keeper".into(), "Fixes conflicts, sorts comments".into()),
        Job::Verifier { focus } => {
            let verifier = (verifiers.iter()).find(|v| v.focus.eq_ignore_ascii_case(focus));
            let role = verifier.map_or(Role::Reviewer, |v| v.role);
            let note = (verifier.and_then(|v| v.instruction.clone()))
                .unwrap_or_else(|| "A verifier in Reviews".into());
            (role.agent_label(Some(focus)), note)
        }
    }
}

/// A short, stable key for a job, for element ids.
fn job_key(job: &Job) -> String {
    match job {
        Job::Verifier { focus } => format!("verifier-{}", focus.to_lowercase()),
        other => other.name().to_lowercase(),
    }
}

/// One window's meter: its label, a bar filled to the share used with a tick at the threshold,
/// and the percentage, or "Unavailable" with no percentage.
fn meter(window: &QuotaWindow, switch_at: u32, p: Palette) -> Div {
    let used = window.used_percent;
    let tick = div()
        .absolute()
        .top(px(-2.))
        .bottom(px(-2.))
        .left(relative(switch_at as f32 / 100.))
        .w(px(2.))
        .bg(p.text.opacity(0.55));
    let fill = used.map(|used| {
        let color = if used >= f64::from(switch_at) {
            p.red
        } else {
            quotas::quota_color(used, p)
        };
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(relative((used.clamp(0., 100.) / 100.) as f32))
            .rounded_full()
            .bg(color)
    });
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(font::SMALL))
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .text_ellipsis()
                .text_color(p.subtle)
                .child(window.label.clone()),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .h(px(7.))
                .rounded_full()
                .bg(p.overlay)
                .children(fill)
                .child(tick),
        )
        .child(
            div()
                .w(px(70.))
                .flex_none()
                .text_right()
                .child(used.map_or("Unavailable".into(), |u| format!("{u:.0}%"))),
        )
}

impl Requests for ModelsPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}
impl Render for ModelsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Models");
        let p = palette(is_dark(cx));
        ui::page(
            "Models",
            "Models on this machine",
            self.models_panel(p, cx),
            Some(self.models_footer(p, cx)),
            p,
            cx,
        )
    }
}

/// A menu item with a title and a line under it.
fn titled(title: &'static str, detail: String) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let p = palette(cx.theme().is_dark());
        div()
            .flex()
            .flex_col()
            .child(title)
            .child(hint(detail.clone(), p))
    })
}
