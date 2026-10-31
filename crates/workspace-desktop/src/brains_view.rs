//! The Brains page (OpenSpec change `brain-registry-sync`, `design/mockups/current/brains.html`
//! without its repository mapping): each brain this Mac's Spaces name, with the Spaces that name
//! it, its grant, sync and compiler, then the PR gate's batches with their checks. Brains are
//! added and removed in a Space's settings. Every action here is the human's, and nothing on the
//! page shows a brain file's content or a check's output.
use super::*;
use brain_browser_view::{BrainOpening, BrainTarget, BrainView};
use gpui_component::{
    Sizable,
    button::ButtonVariants,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
};
use ui::{banner, card, empty_state, hint, icon, mono, pill, section};
use wording::{age, plural};
use workspace_core::BrainsPage as Listing;

const SUBTITLE: &str = "Git-backed Markdown memory shared with your team · every change reaches main through a pull request";
const IN_SPACES: &str =
    "A Space names its brain in its settings. Brains are added and removed there.";
const GATE_HINT: &str =
    "Log batches, compile runs and config changes on their way to main · select one for its checks";
/// How long a reading stands before a change to the workspace asks again.
const FRESH_MS: i64 = 10_000;
/// After Sync now, how long the passes get before the page reads again.
const AFTER_SYNC: std::time::Duration = std::time::Duration::from_secs(5);

/// What the human is in the middle of on the page.
#[derive(Clone, PartialEq, Eq)]
enum Editing {
    Grant(String),
    /// Confirming forgetting a brain no Space names any longer.
    Forget(String),
    /// Naming another host, by id, as a brain's compiler.
    Compiler(String),
    Discard(String),
    /// Giving the reason a reported line is not sensitive.
    NotSensitive {
        batch: String,
        path: String,
        line: u64,
        rule: String,
    },
}

pub(super) struct BrainsPage {
    bridge: Bridge,
    shared: Shared,
    read: Option<Result<Listing, String>>,
    asked_at: i64,
    open_batch: Option<String>,
    editing: Option<Editing>,
    host_id: Entity<InputState>,
    host_name: Entity<InputState>,
    reason: Entity<InputState>,
    /// Why the host refused the last action.
    error: Option<String>,
}
impl EventEmitter<Intent> for BrainsPage {}
impl BrainsPage {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        Self {
            bridge,
            shared: Shared::default(),
            read: None,
            asked_at: 0,
            open_batch: None,
            editing: None,
            host_id: input("The other host's id", window, cx),
            host_name: input("Its name, such as Michael’s Mac", window, cx),
            reason: input(
                "Why it is not sensitive, such as fixture call ID",
                window,
                cx,
            ),
            error: None,
        }
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
        self.ask(ask::brains(), window, cx, |view, read, _, cx| {
            view.read = Some(read);
            cx.notify();
        });
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

    /// Shows a batch's checks, as the brain browser's Status links to it.
    pub(super) fn open_batch(&mut self, id: String, cx: &mut Context<Self>) {
        self.open_batch = Some(id);
        cx.notify();
    }

    fn edit(&mut self, editing: Option<Editing>, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.host_id, &self.host_name, &self.reason] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.editing = editing;
        self.error = None;
        cx.notify();
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
            body = body.child(banner(p.red).child(error.clone()));
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
        if let Some(snapshot) = self.shared.snapshot() {
            for (id, name, folder) in without_origin(&snapshot.spaces, &snapshot.space_locals) {
                body = body.child(
                    banner(p.yellow)
                        .child(div().flex_1().child(without_origin_line(&name, &folder)))
                        .child(
                            button(SharedString::from(format!("without-origin-{}", id.0)))
                                .link()
                                .xsmall()
                                .label("Space settings")
                                .on_click(cx.listener(move |_, _, _, c| {
                                    c.emit(Intent::OpenSpace(id.clone()))
                                })),
                        ),
                );
            }
        }
        if listing.brains.is_empty() {
            return body.child(
                empty_state("brain", "No brains yet", IN_SPACES, p).child(
                    button("open-spaces")
                        .outline()
                        .label("Open Spaces")
                        .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Spaces)))),
                ),
            );
        }
        body = body.child(hint(IN_SPACES, p));
        for brain in &listing.brains {
            body = body.child(self.brain_card(brain, listing, p, cx));
        }
        body = body.child(section("PR GATE", p)).child(hint(GATE_HINT, p));
        if listing.batches.is_empty() {
            return body.child(hint("No batch is on its way to main.", p));
        }
        for batch in &listing.batches {
            let required = (listing.brains.iter())
                .find(|b| b.repository.eq_ignore_ascii_case(&batch.repository))
                .map_or(&[][..], |b| &b.required_checks);
            body = body.child(self.gate_row(batch, required, p, cx));
        }
        body
    }

    fn brain_card(
        &self,
        brain: &Brain,
        listing: &Listing,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let repository = brain.repository.clone();
        let (sync, tone) = sync_state(brain);
        let batches: Vec<&BrainBatch> = (listing.batches.iter())
            .filter(|b| b.repository.eq_ignore_ascii_case(&brain.repository))
            .collect();
        let fact = |label: &'static str, value: AnyElement| {
            div()
                .flex()
                .gap_3()
                .text_size(px(font::SMALL))
                .child(
                    div()
                        .w(px(110.))
                        .flex_none()
                        .text_color(p.subtle)
                        .child(label),
                )
                .child(div().min_w_0().flex_1().child(value))
        };
        let text = |value: String| div().child(value).into_any_element();
        let mut spaces = div().flex().flex_wrap().gap_1();
        for space in &brain.spaces {
            let id = space.clone();
            spaces = spaces.child(
                button(SharedString::from(format!(
                    "brain-space-{repository}-{}",
                    space.0
                )))
                .link()
                .xsmall()
                .label(self.space_name(space))
                .tooltip("Open this Space’s settings")
                .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::OpenSpace(id.clone())))),
            );
        }
        if brain.spaces.is_empty() {
            spaces = spaces.child(hint("No Space names it any longer", p));
        }
        let mut card = card(p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("brain").text_color(p.subtle))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .flex_1()
                            .child(repository.clone()),
                    )
                    .child(pill(sync, tone.color(p))),
            )
            .child(fact("Spaces", spaces.into_any_element()))
            .child(fact(
                "Remote",
                mono(format!("github.com/{repository}"), p).into_any_element(),
            ))
            .child(fact(
                "Checkout",
                mono(brain.checkout.clone(), p).into_any_element(),
            ))
            .child(fact("Last PR", text(last_pr(brain, &batches))))
            .child(fact(
                "Compiles on",
                text(compiler_line(&brain.config, listing.this_host.as_ref())),
            ))
            .child(fact("Next compile", text(next_compile(&brain.config))))
            .child(fact("Last compile", text(LAST_COMPILE.into())));
        for (warning, tone) in warnings(brain, &batches) {
            card = card.child(banner(tone.color(p)).child(warning));
        }
        card.child(self.brain_actions(brain, listing, &batches, p, cx))
    }

    fn brain_actions(
        &self,
        brain: &Brain,
        listing: &Listing,
        batches: &[&BrainBatch],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let repository = brain.repository.clone();
        let host = &self.shared.host;
        match &self.editing {
            Some(Editing::Grant(r)) if *r == repository => {
                let confirm = repository.clone();
                return ui::confirm_card(
                    format!("Allow Wiffletree to push to {repository}?"),
                    grant_lines(&repository),
                    button("cancel-grant").on_click(cx.listener(|v, _, w, c| v.edit(None, w, c))),
                    ui::gated(button("confirm-grant").primary().label("Allow"), host).on_click(
                        cx.listener(move |v, _, w, c| {
                            let repository = confirm.clone();
                            v.request(Command::ConfirmBrain { repository }, w, c)
                        }),
                    ),
                    p,
                );
            }
            Some(Editing::Forget(r)) if *r == repository => {
                let confirm = repository.clone();
                return ui::confirm_card(
                    format!("Forget {repository} on this Mac?"),
                    [format!(
                        "Deletes the host’s own checkout of {repository} and its batches here. GitHub is untouched: open batch PRs stay for you to close."
                    )],
                    button("cancel-forget").on_click(cx.listener(|v, _, w, c| v.edit(None, w, c))),
                    ui::gated(button("confirm-forget").danger().label("Forget"), host).on_click(
                        cx.listener(move |v, _, w, c| {
                            let repository = confirm.clone();
                            v.request(Command::ForgetBrain { repository }, w, c)
                        }),
                    ),
                    p,
                );
            }
            Some(Editing::Compiler(r)) if *r == repository => {
                let chosen = repository.clone();
                return div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(hint(
                        "Another host compiles this brain once the config PR merges.",
                        p,
                    ))
                    .child(Input::new(&self.host_id).small())
                    .child(Input::new(&self.host_name).small())
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                button("cancel-compiler")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|v, _, w, c| v.edit(None, w, c))),
                            )
                            .child(
                                ui::gated(
                                    button("save-compiler").primary().label("Open the PR"),
                                    host,
                                )
                                .on_click(cx.listener(
                                    move |v, _, w, c| {
                                        let compiler = BrainCompiler {
                                            host_id: v.host_id.read(c).value().trim().to_owned(),
                                            name: v.host_name.read(c).value().trim().to_owned(),
                                            extra: Extra::new(),
                                        };
                                        let repository = chosen.clone();
                                        v.request(
                                            Command::SetBrainCompiler {
                                                repository,
                                                compiler,
                                            },
                                            w,
                                            c,
                                        )
                                    },
                                )),
                            ),
                    );
            }
            _ => {}
        }
        let mut actions = div().flex().flex_wrap().gap_2();
        if !brain.spaces.is_empty() {
            let r = repository.clone();
            actions = actions.child(
                button(SharedString::from(format!("browse-{repository}")))
                    .outline()
                    .icon(icon("book"))
                    .label("Open brain browser")
                    .on_click(cx.listener(move |_, _, _, c| {
                        c.emit(Intent::OpenBrain(BrainOpening {
                            repository: r.clone(),
                            project: None,
                            target: BrainTarget::View(BrainView::Status),
                        }))
                    })),
            );
        }
        if !brain.granted && !brain.spaces.is_empty() {
            let r = repository.clone();
            actions = actions.child(
                ui::gated(
                    button(SharedString::from(format!("grant-{repository}")))
                        .primary()
                        .label("Allow pushes…"),
                    host,
                )
                .on_click(
                    cx.listener(move |v, _, w, c| v.edit(Some(Editing::Grant(r.clone())), w, c)),
                ),
            );
        }
        let setting_up = batches
            .iter()
            .any(|b| b.kind == BrainBatchKind::SetUp && !b.is_finished());
        if brain.config == BrainConfigReading::Missing && !setting_up {
            let r = repository.clone();
            actions = actions.child(
                ui::gated(button(SharedString::from(format!("set-up-{repository}"))).outline().label("Set up"), host)
                    .tooltip("Opens one PR adding wiffletree-brain.json with this Mac as compiler, and the brain gate. It waits for your review.")
                    .on_click(cx.listener(move |v, _, w, c| {
                        let repository = r.clone();
                        v.request(Command::SetUpBrain { repository }, w, c)
                    })),
            );
        }
        if !brain.spaces.is_empty() {
            let r = repository.clone();
            actions = actions.child(
                ui::gated(button(SharedString::from(format!("send-now-{repository}"))).outline().label("Send now"), host)
                    .tooltip("Closes every open log batch of this brain, so each opens its PR on the next sync instead of waiting up to a day.")
                    .on_click(cx.listener(move |v, _, w, c| {
                        let repository = Some(r.clone());
                        v.request(Command::SendLogsNow { repository }, w, c)
                    })),
            );
        }
        if brain.legacy_import {
            let r = repository.clone();
            actions = actions.child(
                ui::gated(button(SharedString::from(format!("import-{repository}"))).outline().label("Import legacy logs"), host)
                    .tooltip("Copies each entry of the legacy logs/<repo>.md files into its own entry file, in log PRs of at most 200 files. The legacy files are not changed. Import again once the PRs merge to finish.")
                    .on_click(cx.listener(move |v, _, w, c| {
                        let repository = r.clone();
                        v.request(Command::ImportLegacyLogs { repository }, w, c)
                    })),
            );
        }
        if brain.config.config().is_some() {
            let weak = cx.entity().downgrade();
            let (here, r) = (listing.this_host.clone(), repository.clone());
            actions = actions.child(
                ui::gated(
                    button(SharedString::from(format!("compiler-{repository}")))
                        .outline()
                        .label("Change compiler")
                        .dropdown_caret(true),
                    host,
                )
                .dropdown_menu(move |mut menu, _, _| {
                    if let Some(here) = here.clone() {
                        let (weak, repository) = (weak.clone(), r.clone());
                        menu = menu.item(PopupMenuItem::new("This Mac").on_click(
                            move |_, window, cx| {
                                let (compiler, repository) = (here.clone(), repository.clone());
                                let _ = weak.update(cx, |view, cx| {
                                    view.request(
                                        Command::SetBrainCompiler {
                                            repository,
                                            compiler,
                                        },
                                        window,
                                        cx,
                                    )
                                });
                            },
                        ));
                    }
                    let (weak, repository) = (weak.clone(), r.clone());
                    menu.item(
                        PopupMenuItem::new("Another host…").on_click(move |_, window, cx| {
                            let repository = repository.clone();
                            let _ = weak.update(cx, |view, cx| {
                                view.edit(Some(Editing::Compiler(repository)), window, cx)
                            });
                        }),
                    )
                }),
            );
        }
        if brain.spaces.is_empty() {
            let r = repository.clone();
            actions = actions.child(
                ui::gated(
                    button(SharedString::from(format!("forget-{repository}")))
                        .ghost()
                        .label("Forget…"),
                    host,
                )
                .on_click(
                    cx.listener(move |v, _, w, c| v.edit(Some(Editing::Forget(r.clone())), w, c)),
                ),
            );
        }
        actions
    }

    fn gate_row(
        &self,
        batch: &BrainBatch,
        required: &[String],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = batch.id.clone();
        let open = self.open_batch.as_ref() == Some(&batch.id);
        let failed = batch.stage == BrainStage::Failed;
        let mut detail = vec![batch.repository.clone()];
        detail.extend(self.source_name(&batch.source));
        detail.extend(author(batch).map(str::to_owned));
        let mut row = card(p).child(
            div()
                .id(SharedString::from(format!("batch-{id}")))
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .on_click(cx.listener(move |v, _, _, c| {
                    v.open_batch = (v.open_batch.as_ref() != Some(&id)).then(|| id.clone());
                    c.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(format!("{} · {}", kind_label(batch.kind), batch.title)),
                        )
                        .child(hint(detail.join(" · "), p)),
                )
                .child(stepper(batch, p))
                .child(
                    div()
                        .w(px(220.))
                        .flex_none()
                        .text_right()
                        .text_size(px(font::SMALL))
                        .text_color(if failed { p.red } else { p.subtle })
                        .child(batch_note(batch, required)),
                ),
        );
        if let (true, BrainSource::Project(project)) = (failed, &batch.source)
            && let Some(coordinator) = self.coordinator_of(project)
        {
            row = row.child(
                div().flex().justify_end().child(
                    button(SharedString::from(format!("inbox-{}", batch.id)))
                        .link()
                        .xsmall()
                        .label("Open in Inbox")
                        .on_click(cx.listener(move |_, _, _, c| {
                            c.emit(Intent::Select(coordinator.clone()));
                            c.emit(Intent::ShowPanel(Panel::Attention));
                        })),
                ),
            );
        }
        if open {
            row = row.child(self.checks(batch, required, p, cx));
        }
        row
    }

    fn source_name(&self, source: &BrainSource) -> Option<String> {
        let snapshot = self.shared.snapshot()?;
        Some(match source {
            BrainSource::Project(id) => (snapshot.projects.iter())
                .find(|p| &p.id == id)
                .map_or_else(|| "a project".into(), |p| p.name.clone()),
            BrainSource::Space(id) => format!("{} Space", self.space_name(id)),
            BrainSource::BrainsPage => "Brains page".into(),
        })
    }

    fn coordinator_of(&self, project: &str) -> Option<String> {
        let snapshot = self.shared.snapshot()?;
        (snapshot.sessions.iter())
            .find(|s| {
                s.project_id == project
                    && s.parent_id.is_none()
                    && s.role == Role::ProjectOrchestrator
            })
            .map(|s| s.id.clone())
    }

    /// The batch's required checks, the files and lines its failed checks reported with what the
    /// human can do about each, and Discard.
    fn checks(
        &self,
        batch: &BrainBatch,
        required: &[String],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = div().flex().flex_col().gap_1().pt_1();
        let checks = required_checks(batch, required);
        if checks.is_empty() {
            list = list.child(hint(no_checks_yet(batch), p));
        }
        for check in checks {
            let (glyph, color) = match check.conclusion {
                BrainCheckConclusion::Success => ("circle-check", p.green),
                BrainCheckConclusion::Failure => ("circle-x", p.red),
                BrainCheckConclusion::Pending => ("loader-circle", p.subtle),
                BrainCheckConclusion::Neutral | BrainCheckConclusion::Skipped => {
                    ("circle", p.subtle)
                }
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(font::SMALL))
                    .child(icon(glyph).size(px(14.)).text_color(color))
                    .child(check.name.clone()),
            );
            for annotation in &check.annotations {
                list = list.child(self.annotation(batch, annotation, p, cx));
            }
        }
        if !batch.is_finished() {
            list = list.child(self.discard(batch, p, cx));
        }
        list
    }

    /// One reported file and line, never its text, with Redact and Not sensitive while the
    /// batch's PR is open.
    fn annotation(
        &self,
        batch: &BrainBatch,
        at: &BrainAnnotation,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut row = div()
            .pl_6()
            .flex()
            .items_center()
            .gap_2()
            .child(mono(format!("{} line {}", at.path, at.line), p).flex_1());
        if !batch.is_amendable() {
            return row;
        }
        let editing = Editing::NotSensitive {
            batch: batch.id.clone(),
            path: at.path.clone(),
            line: at.line,
            rule: String::new(),
        };
        if let Some(Editing::NotSensitive { rule, .. }) =
            (self.editing.as_ref()).filter(|e| same_line(e, &editing))
        {
            let (id, path, line, rule) = (batch.id.clone(), at.path.clone(), at.line, rule.clone());
            return div()
                .pl_6()
                .flex()
                .flex_col()
                .gap_2()
                .child(hint(
                    format!(
                        "{} line {} is not sensitive for {rule} because:",
                        at.path, at.line
                    ),
                    p,
                ))
                .child(Input::new(&self.reason).small())
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("cancel-not-sensitive")
                                .ghost()
                                .label("Cancel")
                                .on_click(cx.listener(|v, _, w, c| v.edit(None, w, c))),
                        )
                        .child(
                            ui::gated(
                                button("save-not-sensitive")
                                    .primary()
                                    .label("Mark not sensitive"),
                                &self.shared.host,
                            )
                            .on_click(cx.listener(
                                move |v, _, w, c| {
                                    let amendment = BrainAmendment::NotSensitive {
                                        path: path.clone(),
                                        line,
                                        rule: rule.clone(),
                                        reason: v.reason.read(c).value().trim().to_owned(),
                                    };
                                    v.amend(id.clone(), amendment, w, c)
                                },
                            )),
                        ),
                );
        }
        let weak = cx.entity().downgrade();
        let rules = move |marking: bool| {
            let weak = weak.clone();
            let editing = editing.clone();
            move |mut menu: gpui_component::menu::PopupMenu,
                  _: &mut Window,
                  _: &mut Context<gpui_component::menu::PopupMenu>| {
                for rule in GATE_RULES {
                    let (weak, editing) = (weak.clone(), editing.clone());
                    menu = menu.item(PopupMenuItem::new(rule).on_click(move |_, window, cx| {
                        let Editing::NotSensitive {
                            batch, path, line, ..
                        } = editing.clone()
                        else {
                            return;
                        };
                        let _ = weak.update(cx, |view, cx| match marking {
                            true => {
                                let rule = rule.to_owned();
                                view.edit(
                                    Some(Editing::NotSensitive {
                                        batch,
                                        path,
                                        line,
                                        rule,
                                    }),
                                    window,
                                    cx,
                                )
                            }
                            false => {
                                let amendment = BrainAmendment::Redact {
                                    path,
                                    line,
                                    rule: rule.to_owned(),
                                };
                                view.amend(batch, amendment, window, cx)
                            }
                        });
                    }));
                }
                menu
            }
        };
        let key = format!("{}-{}-{}", batch.id, at.path, at.line);
        row = row
            .child(
                ui::gated(
                    button(SharedString::from(format!("redact-{key}")))
                        .ghost()
                        .xsmall()
                        .label("Redact the line")
                        .dropdown_caret(true),
                    &self.shared.host,
                )
                .tooltip("Replaces the line’s value with [removed: ‹rule›] in one more commit")
                .dropdown_menu(rules(false)),
            )
            .child(
                ui::gated(
                    button(SharedString::from(format!("not-sensitive-{key}")))
                        .ghost()
                        .xsmall()
                        .label("Not sensitive")
                        .dropdown_caret(true),
                    &self.shared.host,
                )
                .tooltip("Adds the gate’s allow marker with your reason in one more commit")
                .dropdown_menu(rules(true)),
            );
        row
    }

    fn discard(&self, batch: &BrainBatch, p: Palette, cx: &mut Context<Self>) -> Div {
        let id = batch.id.clone();
        if self.editing == Some(Editing::Discard(batch.id.clone())) {
            return ui::confirm_card(
                "Discard this batch?",
                [discard_line(batch)],
                button("cancel-discard").on_click(cx.listener(|v, _, w, c| v.edit(None, w, c))),
                ui::gated(
                    button("confirm-discard").danger().label("Discard"),
                    &self.shared.host,
                )
                .on_click(
                    cx.listener(move |v, _, w, c| {
                        v.amend(id.clone(), BrainAmendment::Discard, w, c)
                    }),
                ),
                p,
            );
        }
        div().flex().justify_end().child(
            ui::gated(
                button(SharedString::from(format!("discard-{id}")))
                    .ghost()
                    .xsmall()
                    .label("Discard the batch…"),
                &self.shared.host,
            )
            .on_click(
                cx.listener(move |v, _, w, c| v.edit(Some(Editing::Discard(id.clone())), w, c)),
            ),
        )
    }

    fn amend(
        &mut self,
        batch_id: String,
        amendment: BrainAmendment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request(
            Command::AmendBrainBatch {
                batch_id,
                amendment,
            },
            window,
            cx,
        );
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
            Ok(answer) => {
                self.error = None;
                if let Command::ImportLegacyLogs { .. } = command
                    && let Ok(import) = serde_json::from_value::<LegacyImport>(answer)
                {
                    toast(import_line(&import), window, cx);
                }
                if !matches!(command, Command::SyncNow) {
                    self.edit(None, window, cx);
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

fn same_line(a: &Editing, b: &Editing) -> bool {
    match (a, b) {
        (
            Editing::NotSensitive {
                batch, path, line, ..
            },
            Editing::NotSensitive {
                batch: b,
                path: p,
                line: l,
                ..
            },
        ) => batch == b && path == p && line == l,
        _ => false,
    }
}

/// The brain's sync pill: held, failing, waiting for the grant, or when it last synced.
pub(super) fn sync_state(brain: &Brain) -> (String, Tone) {
    if brain.held.is_some() {
        ("Writing held".into(), Tone::Bad)
    } else if brain.problem.is_some() {
        ("Sync failed".into(), Tone::Bad)
    } else if !brain.granted {
        ("Waits for your grant".into(), Tone::Warn)
    } else {
        match brain.synced_at {
            Some(at) => (format!("Synced {}", age(at).to_lowercase()), Tone::Good),
            None => ("Not synced yet".into(), Tone::Quiet),
        }
    }
}

fn grant_lines(repository: &str) -> Vec<String> {
    vec![
        format!("Wiffletree may push its own wiffletree/brain/‹login›/‹batch› branches to {repository}, open pull requests from them into main, turn on auto-merge for them, close a batch PR you discard and delete a branch it pushed."),
        "It never pushes main, forces a push, or approves, comments on or merges a pull request itself.".into(),
        "Allowing this owner for ticket PRs does not grant this, and this grant allows no ticket PRs.".into(),
    ]
}

/// Who compiles the brain, from `wiffletree-brain.json` on `main`.
pub(super) fn compiler_line(
    config: &BrainConfigReading,
    this_host: Option<&BrainCompiler>,
) -> String {
    match config {
        BrainConfigReading::Unread => "Not read yet".into(),
        BrainConfigReading::Missing => "No compiler · Set up adds wiffletree-brain.json".into(),
        BrainConfigReading::Invalid { reason } => format!("No compiler · {reason}"),
        BrainConfigReading::Newer { version } => {
            format!(
                "Not compiled here · version {version} of wiffletree-brain.json is newer than this Wiffletree"
            )
        }
        BrainConfigReading::Read { config } => match &config.compiler {
            None => "No compiler named".into(),
            Some(c) if this_host.is_some_and(|h| h.host_id == c.host_id) => {
                format!("{} (this Mac)", c.name)
            }
            Some(c) => c.name.clone(),
        },
    }
}

pub(super) fn next_compile(config: &BrainConfigReading) -> String {
    match config.config().and_then(|c| c.compile.as_ref()) {
        Some(compile) => format!("Daily at {} {}", compile.at, compile.timezone),
        None => "Not scheduled".into(),
    }
}

/// Each Space the upgrade left with no brain, because its brain folder has no GitHub `origin`,
/// while it still names none: its id, its name and the folder.
fn without_origin(
    spaces: &[Space],
    locals: &BTreeMap<SpaceId, SpaceLocal>,
) -> Vec<(SpaceId, String, String)> {
    (spaces.iter())
        .filter(|space| space.brain.is_none())
        .filter_map(|space| {
            let folder = locals.get(&space.id)?.brain_without_origin.clone()?;
            Some((space.id.clone(), space.name.clone(), folder))
        })
        .collect()
}

fn without_origin_line(space: &str, folder: &str) -> String {
    format!(
        "The {space} Space has no brain: its brain folder {folder} has no GitHub origin, so the upgrade could not make it a brain repository. Name a brain repository in the Space's settings."
    )
}

/// No compile run is recorded until `brain-compile` adds them.
const LAST_COMPILE: &str = "None yet · this release does not compile brains";

/// The brain's most recent PR, however old, and how many of its batches are still in the gate.
fn last_pr(brain: &Brain, batches: &[&BrainBatch]) -> String {
    let in_gate = batches.iter().filter(|b| !b.is_finished()).count();
    let last = (brain.last_pull_request.as_ref()).map(|pr| {
        format!(
            "#{} {} · {}",
            pr.number,
            kind_label(pr.kind).to_lowercase(),
            stage_label(pr.stage)
        )
    });
    match (last, in_gate) {
        (None, 0) => "None yet".into(),
        (None, n) => format!("{n} {} in the gate", plural(n, "batch", "batches")),
        (Some(last), 0) => last,
        (Some(last), n) => format!("{last} · {n} in the gate"),
    }
}

/// The brain's warnings: writing held, a sync problem, auto-merge refused or waiting on `main`'s
/// required checks, the config, and the legacy pointer.
fn warnings(brain: &Brain, batches: &[&BrainBatch]) -> Vec<(String, Tone)> {
    let repository = &brain.repository;
    let mut warnings = vec![];
    match &brain.held {
        Some(BrainHold::ChangedFiles { files }) => warnings.push((
            format!("Writing is held: something other than Wiffletree changed {} in the host’s checkout. Wiffletree leaves the change as it is; settle it, then Sync now.", files.join(", ")),
            Tone::Bad,
        )),
        Some(BrainHold::NotFastForward) => warnings.push((
            "Writing is held: the host’s checkout has commits main does not, so it cannot fast-forward. Wiffletree never resets it; settle it, then Sync now.".into(),
            Tone::Bad,
        )),
        None => {}
    }
    if let Some(problem) = &brain.problem {
        warnings.push((problem.clone(), Tone::Bad));
    }
    if batches
        .iter()
        .any(|b| b.problem == Some(BrainProblem::AutoMergeRefused))
    {
        warnings.push((
            format!("GitHub refused auto-merge for {repository}. Turn on Allow auto-merge and protect main with the brain gate’s checks, as the gate’s README says, then Sync now."),
            Tone::Warn,
        ));
    }
    if !brain.missing_checks.is_empty() {
        let missing = &brain.missing_checks;
        warnings.push((
            format!(
                "Auto-merge waits: main does not require the brain gate’s {} {}. Protect main with all seven, then Sync now.",
                missing.join(", "),
                if missing.len() == 1 { "check" } else { "checks" }
            ),
            Tone::Warn,
        ));
    }
    if let BrainConfigReading::Invalid { reason } = &brain.config {
        warnings.push((
            format!("{reason}. Batches still open PRs; nothing compiles until it is fixed."),
            Tone::Warn,
        ));
    }
    if let BrainConfigReading::Newer { .. } = &brain.config {
        warnings.push(("A newer Wiffletree wrote wiffletree-brain.json. Update Wiffletree to compile this brain; logs still go in.".into(), Tone::Warn));
    }
    if brain.legacy_pointer {
        warnings.push((
            format!("The legacy pointer ~/.config/claude-second-brain names your own clone of {repository}. Its safety-net must open pull requests instead of pushing to main for this brain."),
            Tone::Warn,
        ));
    }
    warnings
}

fn kind_label(kind: BrainBatchKind) -> &'static str {
    match kind {
        BrainBatchKind::Log => "Log batch",
        BrainBatchKind::Compile => "Compile run",
        BrainBatchKind::Config => "Config change",
        BrainBatchKind::SetUp => "Set up",
    }
}

/// The GitHub login a batch branch `wiffletree/brain/<login>/<batch>` names.
fn author(batch: &BrainBatch) -> Option<&str> {
    batch
        .branch
        .as_deref()?
        .strip_prefix("wiffletree/brain/")?
        .split('/')
        .next()
}

/// The stepper's four steps, as the mockup draws them.
const STEPS: [&str; 4] = ["Queued", "PR open", "Checks", "Auto-merged"];

/// How far the batch is, from 0 for queued to 3 for auto-merging or merged.
fn step_of(batch: &BrainBatch) -> usize {
    match batch.stage {
        BrainStage::Queued => 0,
        BrainStage::PrOpen => 1,
        BrainStage::Checks => 2,
        BrainStage::AutoMerging | BrainStage::Merged => 3,
        // A failed batch stops where it failed: at its checks once its PR is open.
        BrainStage::Failed if batch.pull_request.is_some() => 2,
        BrainStage::Failed => 0,
        BrainStage::Discarded => 0,
    }
}

/// Each step's label and tone: done, the current one, or not reached.
fn steps(batch: &BrainBatch) -> Vec<(String, Option<Tone>)> {
    let at = step_of(batch);
    (STEPS.iter().enumerate())
        .map(|(i, step)| {
            let mut label = step.to_string();
            if i == 1
                && let Some(pr) = &batch.pull_request
            {
                label = format!("PR #{}", pr.number);
            }
            let tone = match (i.cmp(&at), batch.stage) {
                (std::cmp::Ordering::Less, _) => Some(Tone::Good),
                (std::cmp::Ordering::Equal, BrainStage::Failed) => {
                    if i == 2 {
                        label = "Checks failed".into();
                    }
                    Some(Tone::Bad)
                }
                (std::cmp::Ordering::Equal, BrainStage::AutoMerging) => {
                    label = "Auto-merging".into();
                    Some(Tone::Warn)
                }
                (std::cmp::Ordering::Equal, BrainStage::Merged) => Some(Tone::Good),
                (std::cmp::Ordering::Equal, _) => Some(Tone::Warn),
                (std::cmp::Ordering::Greater, _) => None,
            };
            (label, tone)
        })
        .collect()
}

pub(super) fn stepper(batch: &BrainBatch, p: Palette) -> Div {
    let mut row = div().flex_none().flex().items_center().gap_2();
    for (i, (label, tone)) in steps(batch).into_iter().enumerate() {
        if i > 0 {
            row = row.child(div().w(px(10.)).h(px(1.)).bg(p.edge));
        }
        let color = tone.map_or(p.subtle, |t| t.color(p));
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(font::META))
                .text_color(if tone.is_some() { p.text } else { p.subtle })
                .child(div().size(px(7.)).rounded_full().bg(if tone.is_some() {
                    color
                } else {
                    p.edge
                }))
                .child(label),
        );
    }
    row
}

pub(super) fn stage_label(stage: BrainStage) -> &'static str {
    match stage {
        BrainStage::Queued => "queued",
        BrainStage::PrOpen => "open",
        BrainStage::Checks => "checks running",
        BrainStage::AutoMerging => "auto-merging",
        BrainStage::Merged => "merged",
        BrainStage::Failed => "failed",
        BrainStage::Discarded => "discarded",
    }
}

/// The gate row's note: what stops the batch, or where it is. It names checks, never their text.
fn batch_note(batch: &BrainBatch, required: &[String]) -> String {
    match &batch.problem {
        Some(BrainProblem::ChecksFailed { checks }) => {
            format!("{} failed · auto-merge paused", checks.join(", "))
        }
        Some(BrainProblem::PushRefused { .. }) => "GitHub refused the push".into(),
        Some(BrainProblem::Conflict) => "Conflicts with main".into(),
        Some(BrainProblem::AutoMergeRefused) => "GitHub refused auto-merge".into(),
        Some(BrainProblem::GateNotRequired { missing }) => {
            format!(
                "Auto-merge waits: main does not require {}",
                missing.join(", ")
            )
        }
        Some(BrainProblem::AwaitingReview) => "Waits for your review on GitHub".into(),
        Some(BrainProblem::ChangedOutside) => "Changed outside Wiffletree · not pushed".into(),
        Some(BrainProblem::Error { .. }) => "Could not move on · retries next pass".into(),
        None => match batch.stage {
            BrainStage::Queued => "Waits to be pushed".into(),
            BrainStage::PrOpen => "PR open".into(),
            BrainStage::Checks => {
                let passed = (required_checks(batch, required).iter())
                    .filter(|c| c.conclusion == BrainCheckConclusion::Success)
                    .count();
                format!("Checks running · {passed} of {} passed", required.len())
            }
            BrainStage::AutoMerging => "All checks passed · auto-merging".into(),
            BrainStage::Merged => format!("Merged {}", age(batch.updated_at).to_lowercase()),
            BrainStage::Failed => "Failed".into(),
            BrainStage::Discarded => "Discarded".into(),
        },
    }
}

/// The batch's checks that `main` requires, which alone decide it, as the host's watch judges.
fn required_checks<'a>(batch: &'a BrainBatch, required: &[String]) -> Vec<&'a BrainCheck> {
    (batch.checks.iter())
        .filter(|check| required.contains(&check.name))
        .collect()
}

fn no_checks_yet(batch: &BrainBatch) -> &'static str {
    match batch.pull_request {
        None => "No checks until its PR opens.",
        Some(_) => "GitHub has reported none of the checks main requires yet.",
    }
}

fn discard_line(batch: &BrainBatch) -> String {
    match &batch.pull_request {
        Some(pr) => format!(
            "Closes PR #{} and deletes the branch Wiffletree pushed. Its entries can go in a new batch.",
            pr.number
        ),
        None => "Nothing has been pushed yet; the batch is dropped here.".into(),
    }
}

/// What Import legacy logs did, in a line.
fn import_line(import: &LegacyImport) -> String {
    if import.done {
        return "Every legacy entry is on main. The import is done.".into();
    }
    match (import.added, import.batches) {
        (0, _) => "Nothing new to import. Import again once the import PRs merge to finish.".into(),
        (1, _) => "1 entry queued in 1 log PR.".into(),
        (n, 1) => format!("{n} entries queued in 1 log PR."),
        (n, b) => format!("{n} entries queued in {b} log PRs."),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LAST_COMPILE, Tone, author, batch_note, compiler_line, grant_lines, import_line, last_pr,
        required_checks, steps, sync_state, warnings, without_origin, without_origin_line,
    };
    use crate::ui;
    use std::collections::BTreeMap;
    use workspace_core::*;

    fn brain() -> Brain {
        Brain {
            repository: "Sage-Social/sagehaven-docs".into(),
            spaces: vec![SpaceId("sage".into())],
            granted: true,
            checkout: "/data/repos/github.com/Sage-Social/sagehaven-docs".into(),
            push_url: None,
            head: None,
            synced_at: None,
            problem: None,
            held: None,
            config: BrainConfigReading::Missing,
            missing_checks: vec![],
            required_checks: vec![],
            legacy_pointer: false,
            last_pull_request: None,
            legacy_import: false,
        }
    }
    fn batch(stage: BrainStage, problem: Option<BrainProblem>) -> BrainBatch {
        BrainBatch {
            id: "01jbatch".into(),
            repository: "Sage-Social/sagehaven-docs".into(),
            kind: BrainBatchKind::Log,
            source: BrainSource::BrainsPage,
            title: "Log batch: Call relay retry (1 entry)".into(),
            files: vec!["second-brain/logs/a.md".into()],
            branch: Some("wiffletree/brain/mhohlios/01jbatch".into()),
            pull_request: (stage != BrainStage::Queued).then(|| BrainPullRequest {
                number: 214,
                url: "u".into(),
                auto_merge: false,
            }),
            stage,
            problem,
            checks: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
    fn compiler(id: &str, name: &str) -> BrainCompiler {
        BrainCompiler {
            host_id: id.into(),
            name: name.into(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn the_grant_names_the_repository_and_exactly_what_it_allows() {
        let lines = grant_lines("Sage-Social/sagehaven-docs").join(" ");
        assert!(lines.contains("to Sage-Social/sagehaven-docs"));
        for allowed in [
            "push its own wiffletree/brain/",
            "into main",
            "auto-merge",
            "close a batch PR",
            "delete a branch it pushed",
        ] {
            assert!(lines.contains(allowed), "{allowed}");
        }
        assert!(lines.contains("never pushes main"));
        assert!(lines.contains("no ticket PRs"));
    }

    #[test]
    fn a_brain_waits_for_the_grant_and_a_hold_or_problem_outranks_its_sync() {
        let mut waiting = brain();
        waiting.granted = false;
        assert_eq!(
            sync_state(&waiting),
            ("Waits for your grant".into(), Tone::Warn)
        );
        let mut held = brain();
        held.held = Some(BrainHold::NotFastForward);
        held.synced_at = Some(ui::now());
        assert_eq!(sync_state(&held).1, Tone::Bad);
        let mut synced = brain();
        synced.synced_at = Some(ui::now());
        assert_eq!(sync_state(&synced), ("Synced just now".into(), Tone::Good));
    }

    #[test]
    fn the_compiler_comes_from_the_config_on_main_and_this_mac_is_named() {
        let here = compiler("h1", "Michael’s Mac");
        let read = |name: &str, id: &str| BrainConfigReading::Read {
            config: Box::new(BrainConfig {
                version: 1,
                brain_dir: "second-brain".into(),
                compiler: Some(compiler(id, name)),
                compile: None,
                gate: None,
                extra: Extra::new(),
            }),
        };
        assert_eq!(
            compiler_line(&read("Michael’s Mac", "h1"), Some(&here)),
            "Michael’s Mac (this Mac)"
        );
        assert_eq!(
            compiler_line(&read("RB’s Mac", "h2"), Some(&here)),
            "RB’s Mac"
        );
        assert!(compiler_line(&BrainConfigReading::Missing, None).starts_with("No compiler"));
        assert!(compiler_line(&BrainConfigReading::Newer { version: 2 }, None).contains("newer"));
    }

    #[test]
    fn the_warnings_cover_auto_merge_required_checks_the_config_and_the_legacy_pointer() {
        let mut warned = brain();
        warned.missing_checks = vec!["sensitive-info".into()];
        warned.legacy_pointer = true;
        warned.config = BrainConfigReading::Invalid {
            reason: "wiffletree-brain.json is not valid JSON (line 1, column 2)".into(),
        };
        let refused = batch(BrainStage::PrOpen, Some(BrainProblem::AutoMergeRefused));
        let text: Vec<String> = warnings(&warned, &[&refused])
            .into_iter()
            .map(|(w, _)| w)
            .collect();
        assert_eq!(text.len(), 4, "{text:?}");
        assert!(text[0].contains("refused auto-merge") && text[0].contains("Allow auto-merge"));
        assert!(text[1].contains("does not require the brain gate’s sensitive-info check."));
        assert!(text[2].contains("not valid JSON"));
        assert!(
            text[3].contains("~/.config/claude-second-brain")
                && text[3].contains("open pull requests instead of pushing to main")
        );
        assert!(warnings(&brain(), &[]).is_empty());
    }

    #[test]
    fn the_stepper_stops_a_failed_batch_at_its_checks_and_shows_auto_merging() {
        let failed = batch(
            BrainStage::Failed,
            Some(BrainProblem::ChecksFailed {
                checks: vec!["sensitive-info".into()],
            }),
        );
        let labels: Vec<(String, Option<Tone>)> = steps(&failed);
        assert_eq!(labels[1], ("PR #214".into(), Some(Tone::Good)));
        assert_eq!(labels[2], ("Checks failed".into(), Some(Tone::Bad)));
        assert_eq!(labels[3].1, None);
        let merging = steps(&batch(BrainStage::AutoMerging, None));
        assert_eq!(merging[3], ("Auto-merging".into(), Some(Tone::Warn)));
        let queued = steps(&batch(BrainStage::Queued, None));
        assert_eq!(queued[0], ("Queued".into(), Some(Tone::Warn)));
        assert_eq!(queued[1], ("PR open".into(), None));
    }

    #[test]
    fn a_gate_row_names_the_failed_check_and_the_set_up_review_never_any_text() {
        let failed = batch(
            BrainStage::Failed,
            Some(BrainProblem::ChecksFailed {
                checks: vec!["sensitive-info".into()],
            }),
        );
        assert_eq!(
            batch_note(&failed, &[]),
            "sensitive-info failed · auto-merge paused"
        );
        let review = batch(BrainStage::PrOpen, Some(BrainProblem::AwaitingReview));
        assert_eq!(batch_note(&review, &[]), "Waits for your review on GitHub");
        let refused = batch(
            BrainStage::Failed,
            Some(BrainProblem::PushRefused {
                reason: "remote: secret 555".into(),
            }),
        );
        assert!(!batch_note(&refused, &[]).contains("555"));
        assert_eq!(author(&failed), Some("mhohlios"));
    }

    #[test]
    fn a_selected_batch_lists_and_counts_only_the_checks_main_requires() {
        let check = |name: &str, conclusion| BrainCheck {
            name: name.into(),
            conclusion,
            annotations: vec![],
        };
        let mut running = batch(BrainStage::Checks, None);
        running.checks = vec![
            check("secret-scan", BrainCheckConclusion::Success),
            check("sensitive-info", BrainCheckConclusion::Pending),
            check("summary-comment", BrainCheckConclusion::Failure),
        ];
        let required = [
            "secret-scan".to_owned(),
            "sensitive-info".to_owned(),
            "not-reported-yet".to_owned(),
        ];
        let listed: Vec<&str> = (required_checks(&running, &required).into_iter())
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(listed, ["secret-scan", "sensitive-info"]);
        assert_eq!(
            batch_note(&running, &required),
            "Checks running · 1 of 3 passed"
        );
        assert!(required_checks(&running, &[]).is_empty());
    }

    #[test]
    fn the_last_pr_is_the_brains_latest_however_old_with_the_batches_still_in_the_gate() {
        let mut merged_long_ago = brain();
        merged_long_ago.last_pull_request = Some(BrainLastPullRequest {
            number: 212,
            kind: BrainBatchKind::Compile,
            stage: BrainStage::Merged,
        });
        // No batch is on the page any longer, yet the last PR is still named.
        assert_eq!(last_pr(&merged_long_ago, &[]), "#212 compile run · merged");
        let open = batch(BrainStage::Checks, None);
        assert_eq!(
            last_pr(&merged_long_ago, &[&open]),
            "#212 compile run · merged · 1 in the gate"
        );
        assert_eq!(last_pr(&brain(), &[]), "None yet");
        let queued = batch(BrainStage::Queued, None);
        assert_eq!(
            last_pr(&brain(), &[&queued, &queued]),
            "2 batches in the gate"
        );
        assert!(LAST_COMPILE.starts_with("None yet"));
    }

    #[test]
    fn a_space_whose_brain_folder_has_no_github_origin_is_explained_until_it_names_a_brain() {
        let space = |name: &str, brain: Option<SpaceBrain>| Space {
            id: SpaceId(name.to_lowercase()),
            name: name.into(),
            description: String::new(),
            instructions: String::new(),
            brain,
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: "l".into() },
                folder: name.to_lowercase(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        };
        let left = SpaceLocal {
            brain_without_origin: Some("/Users/m/notes".into()),
            ..SpaceLocal::default()
        };
        let named = SpaceBrain {
            repository: BrainRepository::GitHub {
                owner: "Vyttle-LLC".into(),
                name: "vyttle-docs".into(),
                extra: Extra::new(),
            },
            folder: "second-brain".into(),
            extra: Extra::new(),
        };
        let spaces = [space("Notes", None), space("Vyttle", Some(named))];
        let locals = BTreeMap::from([
            (SpaceId("notes".into()), left.clone()),
            (SpaceId("vyttle".into()), left),
        ]);
        let shown = without_origin(&spaces, &locals);
        assert_eq!(
            shown,
            [(
                SpaceId("notes".into()),
                "Notes".into(),
                "/Users/m/notes".into()
            )],
            "a Space that names a brain since is not explained"
        );
        let line = without_origin_line("Notes", "/Users/m/notes");
        assert!(
            line.contains("/Users/m/notes has no GitHub origin")
                && line.contains("Space's settings")
        );
    }

    #[test]
    fn the_import_says_what_it_queued_and_when_it_is_done() {
        let import = |added, batches, done| LegacyImport {
            added,
            batches,
            compiled: 0,
            done,
        };
        assert_eq!(
            import_line(&import(202, 2, false)),
            "202 entries queued in 2 log PRs."
        );
        assert!(import_line(&import(0, 0, false)).contains("once the import PRs merge"));
        assert!(import_line(&import(0, 0, true)).contains("done"));
    }
}
