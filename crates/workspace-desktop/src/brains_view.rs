//! The Brains page (OpenSpec change `one-space-repository`, screen 7 of its mockup): one group per
//! wiki on this Mac, named after its Spaces, with Run now, its compile time and Change, its last
//! and next run, each Mac that compiles it with this Mac's tick, and its runs; then the pull
//! requests an earlier release left open, the alerts from pull scans, and the held entries and
//! conflicts no project's Inbox shows. Every action here is the human's, and nothing on the page
//! shows a matched value. Only a conflict card quotes brain text, as plain text.
use super::*;
use brain_browser_view::{BrainOpening, BrainTarget, BrainView};
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, InputState},
};
use held_entry::{HeldAction, HeldForm};
use scan_alert::{AlertAction, AlertForm};
use std::collections::HashMap;
use ui::{banner, card, empty_state, hint, icon, pill, section};
use wording::age;
use workspace_core::BrainsPage as Listing;

const SUBTITLE: &str =
    "Your Spaces' logs and wikis · every change lands straight on main, scanned on this Mac";
const NO_BRAINS: &str = "Every Space has a brain. Create a Space and it starts here.";
/// How long a reading stands before a change to the workspace asks again.
const FRESH_MS: i64 = 10_000;
/// After Sync now, how long the passes get before the page reads again.
const AFTER_SYNC: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) struct BrainsPage {
    bridge: Bridge,
    shared: Shared,
    read: Option<Result<Listing, String>>,
    asked_at: i64,
    /// Why the host refused the last action.
    error: Option<String>,
    /// Each wiki's page of compile runs, and where that page starts.
    runs: HashMap<String, CompileRunPage>,
    run_offsets: HashMap<String, usize>,
    /// The conflict with no project whose Keep both line is open, by its card's key, and that
    /// line.
    keeping_both: Option<String>,
    scope: Entity<InputState>,
    /// The form of the held entry being edited or marked.
    held: HeldForm,
    /// The reason line of the alert being marked.
    alert: AlertForm,
    /// The wiki whose compile time Change is editing, with its time and timezone.
    changing_time: Option<String>,
    time: Entity<InputState>,
    timezone: Entity<InputState>,
}

/// Compile runs shown per page.
const RUNS_PAGE: usize = 6;
impl EventEmitter<Intent> for BrainsPage {}
impl BrainsPage {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            bridge,
            shared: Shared::default(),
            read: None,
            asked_at: 0,
            error: None,
            runs: HashMap::new(),
            run_offsets: HashMap::new(),
            keeping_both: None,
            scope: cx
                .new(|cx| InputState::new(window, cx).placeholder("One line that scopes both")),
            held: HeldForm::new(window, cx),
            alert: AlertForm::new(window, cx),
            changing_time: None,
            time: cx.new(|cx| InputState::new(window, cx).placeholder("06:00")),
            timezone: cx.new(|cx| InputState::new(window, cx).placeholder("America/Chicago")),
        }
    }

    /// Opens Change on a wiki's compile time, filled in as it is.
    fn change_time(
        &mut self,
        compile: &BrainCompileState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (at, timezone) = (compile.compile.as_ref())
            .map_or(("06:00".to_owned(), compile.this_timezone.clone()), |c| {
                (c.at.clone(), c.timezone.clone())
            });
        self.time
            .update(cx, |input, cx| input.set_value(at, window, cx));
        self.timezone
            .update(cx, |input, cx| input.set_value(timezone, window, cx));
        self.changing_time = Some(compile.repository.clone());
        cx.notify();
    }

    /// Saves Change: the host refuses a time or timezone the schedule could not read.
    fn save_time(&mut self, space_id: SpaceId, window: &mut Window, cx: &mut Context<Self>) {
        let compile = BrainCompile {
            at: self.time.read(cx).value().trim().to_owned(),
            timezone: self.timezone.read(cx).value().trim().to_owned(),
            extra: Extra::new(),
        };
        self.changing_time = None;
        let command = Command::SetCompileTime {
            space_id,
            compile: Some(compile),
        };
        self.request(command, window, cx);
    }

    /// Reads the brains when the page opens, and again after a change to the workspace once the
    /// reading is old.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let opened = shared.page == Page::Brains && self.shared.page != Page::Brains;
        self.shared = shared;
        if self.shared.page != Page::Brains {
            return;
        }
        if opened || (moved && ui::now() - self.asked_at > FRESH_MS) {
            self.read_brains(window, cx);
        }
        if opened || moved {
            cx.notify();
        }
    }

    fn read_brains(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asked_at = ui::now();
        self.ask(ask::brains(), window, cx, |view, read, window, cx| {
            let compiled: Vec<String> = (read.iter())
                .flat_map(|listing| listing.compiles.iter().map(|c| c.repository.clone()))
                .collect();
            view.read = Some(read);
            view.runs
                .retain(|repository, _| compiled.contains(repository));
            for repository in compiled {
                view.read_runs(repository, window, cx);
            }
            cx.notify();
        });
    }

    /// Reads the brain's page of compile runs that starts where the human paged to.
    fn read_runs(&mut self, repository: String, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.run_offsets.get(&repository).copied().unwrap_or(0);
        let ask = ask::brain_compile_runs(repository.clone(), offset, RUNS_PAGE);
        self.ask(ask, window, cx, move |view, page, _, cx| {
            if let Ok(page) = page {
                view.runs.insert(repository, page);
                cx.notify();
            }
        });
    }

    /// Pages a brain's compile runs `by` pages, newer for a negative one.
    fn page_runs(
        &mut self,
        repository: String,
        by: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.run_offsets.entry(repository.clone()).or_default();
        *offset = offset.saturating_add_signed(by * RUNS_PAGE as isize);
        self.read_runs(repository, window, cx);
    }

    /// Asks the host to sync every brain now, and reads the page again once the passes had time.
    fn sync_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(Command::SyncNow, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(AFTER_SYNC).await;
            let _ =
                cx.update(|window, cx| this.update(cx, |view, cx| view.read_brains(window, cx)));
        })
        .detach();
    }

    fn space_name(&self, id: &SpaceId) -> String {
        (self.shared.snapshot().into_iter())
            .flat_map(|s| s.spaces.iter())
            .find(|s| &s.id == id)
            .map_or_else(|| id.0.clone(), |s| s.name.clone())
    }

    fn body(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut body = div().flex().flex_col().gap_3();
        let listing = match &self.read {
            None => return body.child(hint("Reading the brains…", p)),
            Some(Err(error)) => {
                return body.child(hint(format!("Could not read the brains: {error}"), p));
            }
            Some(Ok(listing)) => listing,
        };
        if let Some(error) = &self.error {
            body = body.child(banner(p.red).child(div().flex_1().min_w_0().child(error.clone())));
        }
        body = body.child(
            section(format!("BRAINS · {}", listing.brains.len()), p).child(
                ui::gated(
                    button("sync-brains")
                        .ghost()
                        .icon(icon("refresh"))
                        .label("Sync now"),
                    &self.shared.host,
                )
                .on_click(cx.listener(|v, _, w, c| v.sync_now(w, c))),
            ),
        );
        if listing.brains.is_empty() {
            return body.child(
                empty_state("brain", "No brains yet", NO_BRAINS, p).child(
                    button("open-spaces")
                        .outline()
                        .label("Open Spaces")
                        .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Spaces)))),
                ),
            );
        }
        for brain in &listing.brains {
            body = body.child(self.brain_card(brain, listing, p, cx));
        }
        if !listing.left_pull_requests.is_empty() {
            body = body
                .child(section("PULL REQUESTS LEFT OPEN", p))
                .child(hint(left_line(), p));
            for left in &listing.left_pull_requests {
                let line = format!("{} #{}", left.repository, left.number);
                body = body.child(card(p).child(line));
            }
        }
        if !listing.alerts.is_empty() {
            body = body.child(section(
                format!("FAILED A SCAN · {}", listing.alerts.len()),
                p,
            ));
            for alert in &listing.alerts {
                body = body.child(self.alert_card(alert, p, cx));
            }
        }
        if !listing.held.is_empty() {
            body = body
                .child(section(
                    format!("HELD ON THIS MAC · {}", listing.held.len()),
                    p,
                ))
                .child(hint("No project wrote these, so no Inbox shows them.", p));
            for entry in &listing.held {
                body = body.child(self.held_card(entry, p, cx));
            }
        }
        if !listing.conflicts.is_empty() {
            body = body
                .child(section(
                    format!("CONFLICTS WITH NO PROJECT · {}", listing.conflicts.len()),
                    p,
                ))
                .child(hint(
                    "No project includes these claims' repositories, so no Inbox shows them.",
                    p,
                ));
            for conflict in &listing.conflicts {
                body = body.child(self.unplaced_conflict(conflict, p, cx));
            }
        }
        body
    }

    fn alert_card(&self, alert: &ScanAlert, p: Palette, cx: &mut Context<Self>) -> Div {
        let (view, kept) = (cx.entity().downgrade(), alert.clone());
        let on: scan_alert::OnAlert = Rc::new(move |action: AlertAction, window, cx| {
            let alert = kept.clone();
            let _ = view.update(cx, |view: &mut Self, cx| {
                scan_alert::act(view, |v| &mut v.alert, alert, action, window, cx)
            });
        });
        scan_alert::alert_card(alert, &self.alert, on, p)
    }

    fn held_card(&self, entry: &HeldEntry, p: Palette, cx: &mut Context<Self>) -> Div {
        let (view, kept) = (cx.entity().downgrade(), entry.clone());
        let on: held_entry::OnHeld = Rc::new(move |action: HeldAction, window, cx| {
            let entry = kept.clone();
            let _ = view.update(cx, |view: &mut Self, cx| {
                held_entry::act(view, |v| &mut v.held, entry, action, window, cx)
            });
        });
        held_entry::held_entry_card(entry, &self.held, on, p)
    }

    fn brain_card(
        &self,
        brain: &Brain,
        listing: &Listing,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let names: Vec<String> = brain.spaces.iter().map(|id| self.space_name(id)).collect();
        let (sync, tone) = sync_state(brain);
        let compile = (listing.compiles.iter()).find(|c| c.repository == brain.repository);
        let opening = BrainOpening {
            repository: brain.repository.clone(),
            project: None,
            target: BrainTarget::View(BrainView::Status),
        };
        let mut head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(icon("brain").text_color(p.subtle))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(wiki_name(&names)),
            )
            .child(pill(sync, tone.color(p)));
        if let Some(compile) = compile {
            head = head.child(self.run_now(compile, cx));
        }
        let mut card = card(p).child(head).child(hint(brain.repository.clone(), p));
        let Some(compile) = compile else {
            return card.child(ui::property("Last run", last_compile(None), p));
        };
        let today = chrono::Local::now().date_naive();
        card = card
            .child(fact("Compile time", self.compile_time(compile, p, cx), p))
            .child(fact("Last run", last_run_line(compile, today), p))
            .children(next_run_line(compile, today).map(|line| fact("Next run", line, p)));
        if !compile.local {
            card = card.child(self.macs(compile, p, cx));
        }
        if let Some(behind) = &compile.behind {
            card = card.child(self.behind_banner(compile, behind, p, cx));
        }
        card = card.child(self.compile_runs(compile, p, cx));
        card.child(
            button(SharedString::from(format!(
                "open-brain-{}",
                brain.repository
            )))
            .ghost()
            .label("Open brain browser")
            .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::OpenBrain(opening.clone())))),
        )
    }

    /// Run now, or why it waits.
    fn run_now(&self, compile: &BrainCompileState, cx: &mut Context<Self>) -> Button {
        let repository = compile.repository.clone();
        let waits = compile.run_now_waits.clone().or_else(|| {
            compile
                .needs_model
                .then(|| "Choose a Brain compiler model first.".to_owned())
        });
        let off =
            (!compile.compiles_here).then(|| "Turn compiling on for this Mac first.".to_owned());
        let waits = waits.or(off);
        let r = repository.clone();
        let mut run_now = button(SharedString::from(format!("run-compile-{repository}")))
            .outline()
            .small()
            .icon(icon("play"))
            .label("Run now")
            .disabled(waits.is_some());
        if let Some(why) = waits {
            run_now = run_now.tooltip(why);
        }
        ui::gated(run_now, &self.shared.host).on_click(cx.listener(move |v, _, w, c| {
            let repository = r.clone();
            v.request(Command::RunBrainCompile { repository }, w, c)
        }))
    }

    /// Daily at 06:00 America/Chicago · Change, or its editor.
    fn compile_time(&self, compile: &BrainCompileState, p: Palette, cx: &mut Context<Self>) -> Div {
        let editing = self.changing_time.as_deref() == Some(compile.repository.as_str());
        let Some(space_id) = compile.space_id.clone() else {
            return div().child(compile_time_line(compile));
        };
        if !editing {
            let shown = compile.clone();
            return div()
                .flex()
                .items_center()
                .gap_1()
                .child(compile_time_line(compile))
                .child(hint("·", p))
                .child(
                    ui::gated(
                        button(SharedString::from(format!(
                            "change-time-{}",
                            compile.repository
                        )))
                        .link()
                        .xsmall()
                        .label("Change"),
                        &self.shared.host,
                    )
                    .on_click(
                        cx.listener(move |view, _, window, cx| {
                            view.change_time(&shown, window, cx)
                        }),
                    ),
                );
        }
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(hint("Daily at", p))
            .child(div().w(px(70.)).child(Input::new(&self.time).small()))
            .child(div().w(px(180.)).child(Input::new(&self.timezone).small()))
            .child(
                button("save-compile-time")
                    .xsmall()
                    .primary()
                    .label("Save")
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.save_time(space_id.clone(), window, cx)
                    })),
            )
            .child(
                button("cancel-compile-time")
                    .xsmall()
                    .ghost()
                    .label("Cancel")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.changing_time = None;
                        cx.notify();
                    })),
            )
    }

    /// Each Mac that compiles the wiki, this Mac first with its tick, another with when it last
    /// ran.
    fn macs(&self, compile: &BrainCompileState, p: Palette, cx: &mut Context<Self>) -> Div {
        let today = chrono::Local::now().date_naive();
        let mut macs = div().flex().flex_col().gap_1();
        let space_id = compile.space_id.clone();
        let tick = Checkbox::new(SharedString::from(format!(
            "compile-here-{}",
            compile.repository
        )))
        .checked(compile.compiles_here)
        .on_click(cx.listener(move |view, checked: &bool, window, cx| {
            let Some(space_id) = space_id.clone() else {
                return;
            };
            let command = Command::SetCompileHere {
                space_id,
                compiles: *checked,
            };
            view.request(command, window, cx);
        }));
        macs = macs.child(mac_row(
            format!("{} (this Mac)", compile.this_mac),
            compile.compiles_here,
            div().child(tick),
            p,
        ));
        for other in (compile.compilers.iter()).filter(|c| c.host != compile.this_host) {
            let last = (other.last_ran)
                .and_then(wording::local)
                .map(|at| format!("Last ran {}", wording::day(at, today)));
            macs = macs.child(mac_row(
                other.machine.clone(),
                true,
                hint(last.unwrap_or_default(), p),
                p,
            ));
        }
        macs
    }

    /// No Mac compiles the wiki and entries wait, with Compile on this Mac.
    fn behind_banner(
        &self,
        compile: &BrainCompileState,
        behind: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let offer = (compile.compilers.is_empty() && !compile.local)
            .then(|| compile.space_id.clone())
            .flatten()
            .map(|space_id| {
                ui::gated(
                    button(SharedString::from(format!(
                        "compile-on-{}",
                        compile.repository
                    )))
                    .xsmall()
                    .label("Compile on this Mac"),
                    &self.shared.host,
                )
                .on_click(cx.listener(move |view, _, window, cx| {
                    let command = Command::SetCompileHere {
                        space_id: space_id.clone(),
                        compiles: true,
                    };
                    view.request(command, window, cx);
                }))
            });
        banner(p.yellow)
            .child(div().flex_1().min_w_0().child(behind.to_owned()))
            .children(offer)
    }

    fn compile_runs(&self, compile: &BrainCompileState, p: Palette, cx: &mut Context<Self>) -> Div {
        let repository = compile.repository.clone();
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("COMPILE RUNS", p));
        let Some(page) = self.runs.get(&repository) else {
            return body.child(hint("Reading the runs…", p));
        };
        if page.runs.is_empty() {
            return body.child(hint("No compile run yet.", p));
        }
        for run in &page.runs {
            body = body.child(run_row(run, p));
        }
        let offset = self.run_offsets.get(&repository).copied().unwrap_or(0);
        let shown = format!(
            "{}–{} of {}",
            offset + 1,
            offset + page.runs.len(),
            page.total
        );
        let (newer, older) = (repository.clone(), repository.clone());
        body.child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(hint(shown, p))
                .child(
                    button(SharedString::from(format!("runs-newer-{repository}")))
                        .ghost()
                        .xsmall()
                        .label("Newer")
                        .disabled(offset == 0)
                        .on_click(
                            cx.listener(move |v, _, w, c| v.page_runs(newer.clone(), -1, w, c)),
                        ),
                )
                .child(
                    button(SharedString::from(format!("runs-older-{repository}")))
                        .ghost()
                        .xsmall()
                        .label("Older")
                        .disabled(offset + page.runs.len() >= page.total)
                        .on_click(
                            cx.listener(move |v, _, w, c| v.page_runs(older.clone(), 1, w, c)),
                        ),
                ),
        )
    }

    /// A conflict no project's Inbox shows, with its rulings.
    fn unplaced_conflict(&self, card: &ConflictCard, p: Palette, cx: &mut Context<Self>) -> Div {
        let view = cx.entity().downgrade();
        let (repository, conflict) = (card.repository.clone(), card.conflict.id.clone());
        let key = conflict_card::key(card);
        let open = self.keeping_both.as_deref() == Some(key.as_str());
        let on: conflict_card::OnConflict = Rc::new(move |action, window, cx| {
            let _ = view.update(cx, |view, cx| match action {
                conflict_card::ConflictAction::Evidence => {
                    cx.emit(Intent::OpenBrain(BrainOpening {
                        repository: repository.clone(),
                        project: None,
                        target: BrainTarget::View(BrainView::Conflicts),
                    }))
                }
                conflict_card::ConflictAction::KeepBoth(open) => {
                    view.keeping_both = open.then(|| key.clone());
                    if open {
                        view.scope.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                            input.focus(window, cx);
                        });
                    }
                    cx.notify();
                }
                conflict_card::ConflictAction::Rule(ruling) => {
                    let command = Command::RuleConflict {
                        repository: repository.clone(),
                        conflict_id: conflict.clone(),
                        ruling,
                    };
                    view.request(command, window, cx)
                }
            });
        });
        conflict_card::conflict_card(card, open, &self.scope, on, p)
    }
}

impl Requests for BrainsPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Err(error) => self.error = Some(error),
            Ok(_) => {
                self.error = None;
                if held_entry::settles(&command) {
                    self.held.close();
                }
                if scan_alert::settles(&command) {
                    self.alert.close();
                }
            }
        }
        self.read_brains(window, cx);
    }
}

impl Render for BrainsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Brains");
        let p = palette(is_dark(cx));
        ui::page(
            "Brains",
            SUBTITLE,
            self.body(p, cx),
            None,
            &self.shared.host,
            p,
            cx,
        )
    }
}

/// How a line on the page reads: fine, worth a look, or wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    Good,
    Quiet,
    Warn,
    Bad,
}
impl Tone {
    pub(super) fn color(self, p: Palette) -> Hsla {
        match self {
            Tone::Good => p.green,
            Tone::Quiet => p.subtle,
            Tone::Warn => p.yellow,
            Tone::Bad => p.red,
        }
    }
}

/// How the brain's repository syncs: its problem, when it last reached GitHub, or that a local
/// one has no remote.
pub(super) fn sync_state(brain: &Brain) -> (String, Tone) {
    match (&brain.problem, brain.synced_at) {
        (Some(_), _) => ("Not syncing".into(), Tone::Bad),
        (None, Some(at)) => (format!("Synced {}", age(at).to_lowercase()), Tone::Good),
        (None, None) if brain.repository.contains('/') => ("Not synced yet".into(), Tone::Quiet),
        (None, None) => ("On this Mac".into(), Tone::Quiet),
    }
}

/// What the pull requests an earlier release left open are.
/// "Vyttle's wiki", or "Sage and Sage Watch's wiki" for Spaces that share one.
fn wiki_name(names: &[String]) -> String {
    let named = match names {
        [] => "This".to_owned(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    format!("{named}’s wiki")
}

/// One line of a wiki's group.
fn fact(label: &'static str, value: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .text_size(px(font::SMALL))
        .child(
            div()
                .w(px(110.))
                .flex_none()
                .text_color(p.subtle)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// A Mac in a wiki's group: whether it compiles, its name, and its tick or last run.
fn mac_row(name: String, on: bool, right: Div, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(font::SMALL))
        .child(div().size(px(7.)).flex_none().rounded_full().bg(if on {
            p.green
        } else {
            p.subtle
        }))
        .child(div().flex_1().min_w_0().child(name))
        .child(right)
}

/// "Daily at 06:00 America/Chicago", or that it runs only on Run now.
fn compile_time_line(compile: &BrainCompileState) -> String {
    match &compile.compile {
        Some(at) => format!("Daily at {} {}", at.at, at.timezone),
        None => "Only on Run now".to_owned(),
    }
}

/// "Today at 06:04 on Michael's Mac · 9 entries · 1 conflict", or how the last run ended.
fn last_run_line(compile: &BrainCompileState, today: chrono::NaiveDate) -> String {
    let Some(run) = &compile.last_run else {
        return last_compile(None);
    };
    if run.outcome != CompileOutcome::Landed {
        return last_compile(Some(run));
    }
    let when =
        wording::local(run.started_at).map_or(String::new(), |at| wording::day_at(at, today));
    format!(
        "{when} on {} · {} · {}",
        compile.this_mac,
        wording::counted_as(run.processed, "entry", "entries"),
        wording::counted(run.conflicts, "conflict")
    )
}

/// "Tomorrow at 06:00, on the first of these Macs awake", or `None` with no time or no Mac.
fn next_run_line(compile: &BrainCompileState, today: chrono::NaiveDate) -> Option<String> {
    let at = wording::local(compile.next_run?)?;
    let who = match (compile.local, compile.compilers.len()) {
        (true, _) => "on this Mac",
        (false, 0) => return None,
        (false, 1) => "on that Mac",
        (false, _) => "on the first of these Macs awake",
    };
    Some(format!("{}, {who}", wording::day_at(at, today)))
}

fn left_line() -> &'static str {
    "An earlier release left these on the old brains. They stay on GitHub for you to merge or close, and nothing watches them."
}

/// The wiki's last run, in a line.
fn last_compile(run: Option<&CompileRun>) -> String {
    let Some(run) = run else {
        return "None yet".into();
    };
    let mut line = format!(
        "{} · {}",
        wording::day_and_time(run.started_at),
        run.outcome.label()
    );
    if !run.reason.is_empty() {
        line.push_str(&format!(" · {}", run.reason));
    }
    line
}

/// One compile run: when and why it started, its counts and model, how it ended, then its
/// reason, commit, usage and procedure.
fn run_row(run: &CompileRun, p: Palette) -> Div {
    let tone = match run.outcome {
        CompileOutcome::Running => Tone::Warn,
        CompileOutcome::Skipped | CompileOutcome::Lost => Tone::Quiet,
        CompileOutcome::Waiting => Tone::Warn,
        CompileOutcome::Failed | CompileOutcome::TimedOut => Tone::Bad,
        CompileOutcome::CompiledElsewhere => Tone::Quiet,
        CompileOutcome::Landed => Tone::Good,
    };
    let model = (run.profile.as_ref())
        .map(|profile| format!(" · {}", profile.short()))
        .unwrap_or_default();
    let counts = match run.model_calls {
        0 if run.outcome == CompileOutcome::Skipped => {
            "No pending entries · no model call".to_owned()
        }
        _ => format!(
            "{} pending · {} processed{model}",
            run.pending, run.processed
        ),
    };
    let mut detail = vec![];
    if !run.reason.is_empty() && run.outcome != CompileOutcome::Skipped {
        detail.push(run.reason.clone());
    }
    if run.unasked > 0 {
        detail.push(format!("GitHub could not be asked about {}", run.unasked));
    }
    if let Some(commit) = &run.commit {
        detail.push(commit.chars().take(7).collect());
    }
    if run.tokens > 0 {
        let cost = run.cost.map(|c| format!(" · ${c:.2}")).unwrap_or_default();
        detail.push(format!("{} tokens{cost}", run.tokens));
    }
    if let Some(version) = run.procedure {
        detail.push(format!("procedure v{version}"));
    }
    div()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(match tone {
            Tone::Bad => p.red.opacity(0.4),
            _ => p.edge,
        })
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(font::SMALL))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(120.))
                        .flex_none()
                        .child(wording::day_and_time(run.started_at)),
                )
                .child(pill(run.trigger.label(), p.subtle))
                .child(div().flex_1().min_w_0().text_ellipsis().child(counts))
                .child(pill(run.outcome.label(), tone.color(p))),
        )
        .when(!detail.is_empty(), |row| {
            row.child(hint(detail.join(" · "), p))
        })
}

#[cfg(test)]
mod tests {
    use super::{Tone, last_compile, sync_state, ui};
    use workspace_core::{Brain, CompileOutcome, CompileRun, CompileTrigger};

    fn brain(repository: &str) -> Brain {
        Brain {
            repository: repository.into(),
            spaces: vec![],
            synced_at: None,
            problem: None,
        }
    }

    #[test]
    fn a_problem_outranks_a_sync_and_a_local_brain_says_it_stays_here() {
        assert_eq!(sync_state(&brain("local-7k2q")).0, "On this Mac");
        assert_eq!(sync_state(&brain("Sage/x")).0, "Not synced yet");
        let synced = Brain {
            synced_at: Some(ui::now()),
            ..brain("Sage/x")
        };
        assert_eq!(sync_state(&synced).1, Tone::Good);
        let failing = Brain {
            problem: Some("offline".into()),
            ..synced
        };
        assert_eq!(sync_state(&failing), ("Not syncing".into(), Tone::Bad));
    }

    #[test]
    fn the_last_run_says_how_it_ended_and_why() {
        assert_eq!(last_compile(None), "None yet");
        let run = CompileRun {
            id: "01".into(),
            repository: "Sage/x".into(),
            trigger: CompileTrigger::RunNow,
            started_at: ui::now(),
            ended_at: None,
            pending: 0,
            processed: 0,
            unasked: 0,
            model_calls: 0,
            profile: None,
            procedure: None,
            outcome: CompileOutcome::Failed,
            reason: "notes/A.md failed the scan".into(),
            conflicts: 0,
            commit: None,
            tokens: 0,
            cost: None,
        };
        let line = last_compile(Some(&run));
        assert!(
            line.contains("Failed") && line.contains("failed the scan"),
            "{line}"
        );
    }
}
