//! The Models page's Review card: the review levels, who verifies tickets, rounds per review and
//! full reviews per ticket (the round and cycle caps). Each change saves at once, apart from the
//! page's Save, because the host keeps it in its settings file. Until the machine's first setup
//! is saved, changes stay in the draft and the page's Save sends them.
use super::*;
use gpui_component::{ActiveTheme, Sizable, button::ButtonVariants, input::Input};
use ui::{eyebrow, hint, icon, segment, setting};

pub(super) struct VerifierEditor {
    bridge: Bridge,
    verification: Option<VerificationSettings>,
    /// The verifiers are part of a setup the human has not saved yet: until the workspace
    /// reports a saved one, changes stay here and nothing is asked of the host.
    unsaved: bool,
    error: Option<String>,
    /// The verifier being added.
    draft: VerifierConfig,
    focus: Entity<InputState>,
    instruction: Entity<InputState>,
    /// The level whose name and "Use it" line are open in `level_name` and `level_use_when`.
    level_form: Option<LevelForm>,
    level_name: Entity<InputState>,
    level_use_when: Entity<InputState>,
}

enum LevelForm {
    /// The saved level at this index. Its ticks and kind keep saving at once.
    Edit(usize),
    /// A level being added, with the kind and ticks chosen so far. Nothing is saved until the
    /// human confirms.
    Add(ReviewLevel),
}

/// What Add level opens with: "Light" with the first reviewer ticked while no level is a
/// re-check, none is named "Light" and a reviewer is configured; otherwise an empty re-check.
fn new_level(settings: &VerificationSettings) -> ReviewLevel {
    let light_fits = settings.levels.iter().all(|level| {
        level.kind != LevelKind::Recheck && !level.name.trim().eq_ignore_ascii_case("Light")
    });
    light_level(&settings.verifiers)
        .filter(|_| light_fits)
        .unwrap_or(ReviewLevel {
            name: String::new(),
            kind: LevelKind::Recheck,
            verifiers: vec![],
            use_when: String::new(),
        })
}

/// The configured verifiers `runs` keeps, in configured order, as a level's verifiers. A focus
/// that is no longer configured is dropped.
fn focuses(configured: &[VerifierConfig], runs: impl Fn(&str) -> bool) -> Vec<String> {
    configured
        .iter()
        .map(|verifier| &verifier.focus)
        .filter(|focus| runs(focus))
        .cloned()
        .collect()
}

/// Ticks or unticks the verifier `focus` in `level`.
fn toggle_verifier(configured: &[VerifierConfig], level: &mut ReviewLevel, focus: &str) {
    level.verifiers = focuses(configured, |f| level.runs(f) != (f == focus));
}

/// The setting with the form's level saved under `name` and `use_when`, or why the host would
/// refuse it.
fn with_level(
    mut settings: VerificationSettings,
    form: &LevelForm,
    name: String,
    use_when: String,
) -> anyhow::Result<VerificationSettings> {
    match form {
        LevelForm::Edit(index) => {
            let level = &mut settings.levels[*index];
            (level.name, level.use_when) = (name, use_when);
        }
        LevelForm::Add(level) => settings.levels.push(ReviewLevel {
            name,
            use_when,
            ..level.clone()
        }),
    }
    settings.validate()?;
    Ok(settings)
}

/// Removes the verifier at `index` and takes it out of every level, in the same save.
fn remove_verifier(settings: &mut VerificationSettings, index: usize) {
    settings.verifiers.remove(index);
    for level in &mut settings.levels {
        level.verifiers = focuses(&settings.verifiers, |f| level.runs(f));
    }
}

/// What a verifier's provider setting allows the coordinator to choose from.
fn provider_note(verifier: &VerifierConfig) -> String {
    match (verifier.provider, verifier.role) {
        (Some(provider), _) => provider.label().into(),
        (None, Role::Reviewer) => "Any enabled provider".into(),
        (None, role) => format!("{}'s providers", role.label()),
    }
}

impl VerifierEditor {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            bridge,
            verification: None,
            unsaved: true,
            error: None,
            draft: VerifierConfig {
                role: Role::Reviewer,
                focus: String::new(),
                instruction: None,
                provider: None,
            },
            focus: cx.new(|cx| InputState::new(window, cx).placeholder("Focus, e.g. Style")),
            instruction: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Optional instruction, e.g. Run reviso:style with Codex")
            }),
            level_form: None,
            level_name: cx.new(|cx| InputState::new(window, cx).placeholder("Name, e.g. Light")),
            level_use_when: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("When should the coordinator pick this level?")
            }),
        }
    }

    pub(super) fn verification(&self) -> Option<&VerificationSettings> {
        self.verification.as_ref()
    }
    /// Shows `verification` as an unsaved draft, which the page's Save sends.
    pub(super) fn hold_draft(
        &mut self,
        verification: VerificationSettings,
        cx: &mut Context<Self>,
    ) {
        self.verification = Some(verification);
        self.level_form = None;
        self.unsaved = true;
        self.error = None;
        cx.notify();
    }
    /// The store has a saved setup: shows the host's verifiers, and changes save at once.
    pub(super) fn setup_saved(&mut self, cx: &mut Context<Self>) {
        if self.unsaved {
            self.unsaved = false;
            self.ask_host(Command::Settings, cx);
        }
    }

    /// Shows the host's saved setting, or its error beside the setting it still uses.
    fn ask_host(&mut self, command: Command, cx: &mut Context<Self>) {
        let receiver = match self.bridge.request(command) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = receiver
                .recv()
                .await
                .unwrap_or_else(|_| Err("Host disconnected".into()))
                .and_then(|value| {
                    serde_json::from_value::<HostSettings>(value).map_err(|e| e.to_string())
                });
            let _ = this.update(cx, |view, cx| {
                match answer {
                    // An unsaved draft is not replaced by the host's defaults.
                    Ok(_) if view.unsaved => {}
                    Ok(settings) => {
                        view.verification = Some(settings.verification);
                        view.error = None;
                    }
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves a changed setting; the host refuses an invalid one and keeps its own.
    fn save(&mut self, change: impl FnOnce(&mut VerificationSettings), cx: &mut Context<Self>) {
        let Some(mut verification) = self.verification.clone() else {
            return;
        };
        change(&mut verification);
        if self.unsaved {
            match verification.validate() {
                Ok(()) => {
                    self.verification = Some(verification);
                    self.error = None;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            cx.notify();
            return;
        }
        self.ask_host(Command::SetVerification { verification }, cx);
    }

    fn add_verifier(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.read(cx).value().trim().to_owned();
        let instruction = self.instruction.read(cx).value().trim().to_owned();
        let verifier = VerifierConfig {
            focus,
            instruction: (!instruction.is_empty()).then_some(instruction),
            ..self.draft.clone()
        };
        self.save(|v| v.verifiers.push(verifier), cx);
        for input in [&self.focus, &self.instruction] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    /// Opens the form on the saved level at `index`, or on a new level.
    fn open_level(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(verification) = &self.verification else {
            return;
        };
        let (form, level) = match index {
            Some(index) => (LevelForm::Edit(index), verification.levels[index].clone()),
            None => {
                let level = new_level(verification);
                (LevelForm::Add(level.clone()), level)
            }
        };
        for (input, value) in [
            (&self.level_name, level.name),
            (&self.level_use_when, level.use_when),
        ] {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.level_form = Some(form);
        self.error = None;
        cx.notify();
    }

    /// Saves the form's name and "Use it" line. A level the host would refuse keeps the form
    /// open with the reason.
    fn confirm_level(&mut self, cx: &mut Context<Self>) {
        let (Some(form), Some(verification)) = (&self.level_form, self.verification.clone()) else {
            return;
        };
        let name = self.level_name.read(cx).value().trim().to_owned();
        let use_when = self.level_use_when.read(cx).value().trim().to_owned();
        match with_level(verification, form, name, use_when) {
            Ok(verification) => {
                self.level_form = None;
                self.save(|v| *v = verification, cx);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    /// Changes the saved level at `index` at once, or the level being added in its form.
    fn change_level(
        &mut self,
        index: Option<usize>,
        change: impl FnOnce(&[VerifierConfig], &mut ReviewLevel),
        cx: &mut Context<Self>,
    ) {
        match (index, &self.verification, &mut self.level_form) {
            (Some(index), ..) => self.save(|v| change(&v.verifiers, &mut v.levels[index]), cx),
            (None, Some(verification), Some(LevelForm::Add(level))) => {
                change(&verification.verifiers, level);
                cx.notify();
            }
            _ => {}
        }
    }

    /// A level's kind as two segments and a tick chip per configured verifier, for the saved
    /// level at `index` or the one being added.
    fn level_controls(
        &self,
        index: Option<usize>,
        level: &ReviewLevel,
        configured: &[VerifierConfig],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        let id = index.map_or("new".to_owned(), |index| index.to_string());
        let mut kinds = div().flex().flex_none().gap_1();
        for (label, kind) in [
            ("Full review", LevelKind::FullReview),
            ("Re-check", LevelKind::Recheck),
        ] {
            kinds = kinds.child(
                segment(
                    SharedString::from(format!("level-{id}-{kind:?}")),
                    label,
                    level.kind == kind,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.change_level(index, |_, level| level.kind = kind, cx)
                })),
            );
        }
        let mut chips = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(lead("Runs", p));
        for verifier in configured {
            let focus = verifier.focus.clone();
            chips = chips.child(
                chip(
                    SharedString::from(format!("level-{id}-runs-{focus}")),
                    &focus,
                    level.runs(&focus),
                    p,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.change_level(
                        index,
                        |configured, level| toggle_verifier(configured, level, &focus),
                        cx,
                    )
                })),
            );
        }
        (kinds, chips)
    }

    /// A saved level at rest: its name, kind, ticks and "Use it" line.
    fn level_row(
        &self,
        index: usize,
        level: &ReviewLevel,
        configured: &[VerifierConfig],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (kinds, chips) = self.level_controls(Some(index), level, configured, p, cx);
        level_group(p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::MEDIUM)
                            .child(level.name.clone()),
                    )
                    .child(kinds)
                    .child(
                        button(SharedString::from(format!("edit-level-{index}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("pencil"))
                            .tooltip("Rename, or rewrite when to use it")
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.open_level(Some(index), window, cx)
                            })),
                    )
                    .child(
                        button(SharedString::from(format!("remove-level-{index}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip("Remove this level")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.level_form = None;
                                view.save(
                                    |v| {
                                        v.levels.remove(index);
                                    },
                                    cx,
                                )
                            })),
                    ),
            )
            .child(chips)
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .child(lead("Use it", p))
                    .child(div().min_w_0().child(level.use_when.clone())),
            )
    }

    /// The open form: the level's name and "Use it" line as inputs, with its kind and ticks.
    fn level_form(
        &self,
        form: &LevelForm,
        level: &ReviewLevel,
        configured: &[VerifierConfig],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (index, confirm) = match form {
            LevelForm::Edit(index) => (Some(*index), "Done"),
            LevelForm::Add(_) => (None, "Add level"),
        };
        let (kinds, chips) = self.level_controls(index, level, configured, p, cx);
        level_group(p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(Input::new(&self.level_name).w(px(180.)))
                    .child(kinds),
            )
            .child(chips)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(lead("Use it", p))
                    .child(Input::new(&self.level_use_when).flex_1().min_w_0()),
            )
            .children(index.is_none().then(|| {
                hint(
                    "A new level starts as a re-check, the kind that cannot pass a ticket by itself.",
                    p,
                )
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(button("cancel-level").label("Cancel").on_click(cx.listener(
                        |view, _, _, cx| {
                            view.level_form = None;
                            view.error = None;
                            cx.notify();
                        },
                    )))
                    .child(
                        button("confirm-level")
                            .primary()
                            .label(confirm)
                            .on_click(cx.listener(|view, _, _, cx| view.confirm_level(cx))),
                    ),
            )
    }

    /// The Levels block: one group per level, then Add level or its form.
    fn levels(
        &self,
        verification: &VerificationSettings,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let configured = &verification.verifiers;
        let mut levels = div()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(p.edge.opacity(0.4))
            .bg(p.surface);
        for (index, level) in verification.levels.iter().enumerate() {
            levels = levels.child(match &self.level_form {
                Some(form @ LevelForm::Edit(open)) if *open == index => {
                    self.level_form(form, level, configured, p, cx)
                }
                _ => self.level_row(index, level, configured, p, cx),
            });
        }
        levels.child(match &self.level_form {
            Some(form @ LevelForm::Add(level)) => self.level_form(form, level, configured, p, cx),
            _ => level_group(p).border_b_0().child(
                div().flex().child(
                    button("add-level")
                        .icon(icon("plus"))
                        .label("Add level")
                        .on_click(
                            cx.listener(|view, _, window, cx| view.open_level(None, window, cx)),
                        ),
                ),
            ),
        })
    }
}

/// One level's rows inside the Levels block.
fn level_group(p: Palette) -> Div {
    div()
        .px_3()
        .py(px(10.))
        .flex()
        .flex_col()
        .gap(px(7.))
        .border_b_1()
        .border_color(p.edge.opacity(0.25))
}

/// The label that starts a level's "Runs" and "Use it" rows.
fn lead(label: &'static str, p: Palette) -> Div {
    div()
        .w(px(54.))
        .flex_none()
        .text_size(px(11.5))
        .text_color(p.subtle)
        .child(label)
}

/// A verifier's tick in a level.
fn chip(id: SharedString, focus: &str, on: bool, p: Palette) -> Stateful<Div> {
    let tick = div()
        .size(px(11.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .border_1();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(5.))
        .pl(px(6.))
        .pr(px(9.))
        .py(px(2.))
        .rounded_full()
        .border_1()
        .text_size(px(11.5))
        .cursor_pointer()
        .map(|chip| match on {
            true => chip
                .border_color(p.focus.opacity(0.45))
                .bg(p.focus.opacity(0.1))
                .child(
                    tick.border_color(p.focus)
                        .bg(p.focus)
                        .child(icon("check").size(px(8.)).text_color(p.surface)),
                ),
            false => chip
                .border_dashed()
                .border_color(p.edge)
                .text_color(p.subtle)
                .child(tick.border_color(p.subtle)),
        })
        .child(focus.to_owned())
}

/// One button per allowed cap; choosing one saves at once.
fn cap_selector(
    name: &str,
    current: u32,
    maximum: u32,
    set: fn(&mut VerificationSettings, u32),
    cx: &mut Context<VerifierEditor>,
) -> Div {
    let mut segments = div().flex().gap_1();
    for n in 1..=maximum {
        segments = segments.child(
            segment(
                SharedString::from(format!("{name}-{n}")),
                n.to_string(),
                n == current,
            )
            .on_click(cx.listener(move |view, _, _, cx| view.save(|v| set(v, n), cx))),
        );
    }
    segments
}

impl Render for VerifierEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx.theme().is_dark());
        let Some(verification) = &self.verification else {
            return div().children(self.error.clone().map(|e| hint(e, p).text_color(p.red)));
        };
        let rounds = cap_selector(
            "rounds",
            verification.max_rounds,
            MAX_VERIFICATION_ROUNDS,
            |v, n| v.max_rounds = n,
            cx,
        );
        let cycles = cap_selector(
            "cycles",
            verification.max_cycles,
            MAX_VERIFICATION_CYCLES,
            |v, n| v.max_cycles = n,
            cx,
        );
        let mut list = div().flex().flex_col().gap_1();
        for (index, verifier) in verification.verifiers.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(180.))
                            .flex_none()
                            .text_ellipsis()
                            .child(verifier.role.agent_label(Some(&verifier.focus))),
                    )
                    .child(hint(provider_note(verifier), p).w(px(150.)).flex_none())
                    .child(
                        hint(verifier.instruction.clone().unwrap_or_default(), p)
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis(),
                    )
                    .child(
                        button(SharedString::from(format!("remove-verifier-{index}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip("Remove this verifier")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.save(|v| remove_verifier(v, index), cx)
                            })),
                    ),
            );
        }
        let draft = &self.draft;
        let mut roles = div().flex().gap_1();
        for role in [Role::Tester, Role::Reviewer] {
            roles = roles.child(
                segment(
                    SharedString::from(format!("verifier-role-{role:?}")),
                    role.label(),
                    draft.role == role,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.draft.role = role;
                    cx.notify();
                })),
            );
        }
        let mut providers = div().flex().gap_1();
        for (label, provider) in [
            ("Any", None),
            ("Claude", Some(Provider::Claude)),
            ("Codex", Some(Provider::Codex)),
        ] {
            providers = providers.child(
                segment(
                    SharedString::from(format!("verifier-provider-{label}")),
                    label,
                    draft.provider == provider,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.draft.provider = provider;
                    cx.notify();
                })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(eyebrow(
                "LEVELS · THE COORDINATOR NAMES ONE EACH TIME IT STARTS A REVIEW",
                p,
            ))
            .child(hint(
                "A level is a set of the verifiers below. Every verifier in it must pass. The “Use it” line tells the coordinator when to pick the level; write it in your own words.",
                p,
            ))
            .child(self.levels(verification, p, cx))
            .child(hint(
                "Full review can be a ticket’s only review. Re-check starts only on a ticket whose last review passed. Wiffletree enforces this; the coordinator cannot choose otherwise.",
                p,
            ))
            .child(eyebrow(
                format!("VERIFIERS · {}", verification.verifiers.len()),
                p,
            ))
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(roles)
                    .child(providers)
                    .child(Input::new(&self.focus).w(px(180.)))
                    .child(Input::new(&self.instruction).flex_1().min_w_0())
                    .child(
                        button("add-verifier")
                            .icon(icon("plus"))
                            .label("Add")
                            .on_click(
                                cx.listener(|view, _, window, cx| view.add_verifier(window, cx)),
                            ),
                    ),
            )
            .child(setting(
                "Rounds per review",
                rounds,
                "The first review plus re-checks after the implementer's fixes. If the last round still finds blocking problems, the review stops and the coordinator decides. The same at every level.",
                p,
            ))
            .child(setting(
                "Full reviews per ticket",
                cycles,
                "Reviews at a level marked Full review, with fresh reviewers. After the last one, the coordinator accepts, accepts with a waiver, closes the ticket or asks you. Re-checks are not counted.",
                p,
            ))
            .child(hint(
                format!(
                    "verify_ticket starts every verifier of the level the coordinator names, at once, on the ticket's current commit, each on the model the coordinator picks by the guide within its provider. {}",
                    if self.unsaved {
                        "Changes here are saved with the setup."
                    } else {
                        "Changes here save at once."
                    }
                ),
                p,
            ))
            .children(self.error.clone().map(|error| hint(error, p).text_color(p.red)))
    }
}

#[cfg(test)]
mod tests {
    use super::{LevelForm, new_level, remove_verifier, toggle_verifier, with_level};
    use workspace_core::*;
    use workspace_host::Host;

    fn verifier(role: Role, focus: &str) -> VerifierConfig {
        VerifierConfig {
            role,
            focus: focus.into(),
            instruction: None,
            provider: None,
        }
    }

    /// A host whose saved setting is Tests, Correctness, Regressions and Style with the levels
    /// Full, which runs all four, and Light, a re-check that runs Correctness.
    fn host() -> (tempfile::TempDir, Host) {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(directory.path()).unwrap();
        let mut settings = VerificationSettings::one_level(
            vec![
                verifier(Role::Tester, "Tests"),
                verifier(Role::Reviewer, "Correctness"),
                verifier(Role::Reviewer, "Regressions"),
                verifier(Role::Reviewer, "Style"),
            ],
            3,
            2,
        );
        settings.levels.extend(light_level(&settings.verifiers));
        host.set_verification(settings).unwrap();
        (directory, host)
    }

    /// What the card's save does: sends the changed setting, which the host keeps or refuses
    /// with the reason the card then shows.
    fn save(host: &mut Host, change: impl FnOnce(&mut VerificationSettings)) -> Result<(), String> {
        let mut verification = host.settings().verification;
        change(&mut verification);
        host.execute(Command::SetVerification { verification })
            .map(drop)
            .map_err(|error| error.to_string())
    }

    fn runs(host: &Host, level: usize) -> Vec<String> {
        host.settings().verification.levels[level].verifiers.clone()
    }

    #[test]
    fn a_tick_saves_at_once_survives_restart_and_reaches_the_coordinator() {
        let (directory, mut host) = host();

        save(&mut host, |v| {
            toggle_verifier(&v.verifiers, &mut v.levels[1], "Tests")
        })
        .unwrap();

        assert_eq!(runs(&host, 1), ["Tests", "Correctness"]);
        drop(host);
        let mut host = Host::open_set_up(directory.path()).unwrap();
        host.create_project("Calls").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let context = host.agent_context(&coordinator.id).unwrap();
        assert_eq!(
            context["model_selection"]["review_levels"][1]["verifiers"],
            serde_json::json!(["Tests", "Correctness"])
        );
        // Unticking takes it out again.
        save(&mut host, |v| {
            toggle_verifier(&v.verifiers, &mut v.levels[1], "Tests")
        })
        .unwrap();
        assert_eq!(runs(&host, 1), ["Correctness"]);
    }

    #[test]
    fn an_existing_setup_opens_as_one_full_level_with_every_verifier_ticked() {
        let directory = tempfile::tempdir().unwrap();
        let host = Host::open_set_up(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("settings.json"),
            r#"{"workspaces_dir":"/tmp/w","verification":{"verifiers":[{"role":"tester","focus":"Tests"},{"role":"reviewer","focus":"Claude"},{"role":"reviewer","focus":"Codex"},{"role":"reviewer","focus":"Style"}],"max_rounds":2,"max_cycles":3}}"#,
        )
        .unwrap();

        let shown = host.settings().verification;

        let [full] = shown.levels.as_slice() else {
            panic!("{:?}", shown.levels);
        };
        assert_eq!(
            (full.name.as_str(), full.kind),
            ("Full", LevelKind::FullReview)
        );
        assert_eq!(shown.verifiers.len(), 4);
        assert!(shown.verifiers.iter().all(|v| full.runs(&v.focus)));
        assert_eq!((shown.max_rounds, shown.max_cycles), (2, 3));
        // Light is suggested on Add level, and nothing is saved until it is confirmed.
        let light = new_level(&shown);
        assert_eq!(
            (light.name.as_str(), light.kind, light.use_when.as_str()),
            ("Light", LevelKind::Recheck, LIGHT_USE_WHEN)
        );
        assert_eq!(light.verifiers, ["Claude"]);
        assert_eq!(host.settings().verification, shown);
    }

    #[test]
    fn add_level_opens_empty_when_light_would_not_fit() {
        let empty = |settings: &VerificationSettings| {
            let level = new_level(settings);
            assert_eq!(level.kind, LevelKind::Recheck);
            level.name.is_empty() && level.verifiers.is_empty() && level.use_when.is_empty()
        };
        let (_directory, host) = host();
        let mut settings = host.settings().verification;
        assert!(empty(&settings), "a level is already a re-check");
        // "Light" as a full review: the filled-in form would be refused as a duplicate name.
        settings.levels[1].kind = LevelKind::FullReview;
        settings.levels[1].name = "light".into();
        assert!(empty(&settings));
        // No reviewer to tick.
        let testers = VerificationSettings::one_level(vec![verifier(Role::Tester, "Tests")], 3, 2);
        assert!(empty(&testers));
    }

    #[test]
    fn a_confirmed_level_is_added_below_the_others_as_a_re_check() {
        let (_directory, mut host) = host();
        let settings = host.settings().verification;
        let mut docs = new_level(&settings);
        toggle_verifier(&settings.verifiers, &mut docs, "Style");
        let form = LevelForm::Add(docs);
        let note = "for a ticket that changes only documentation.";

        let named_twice = with_level(settings.clone(), &form, "light".into(), note.into());
        let added = with_level(settings, &form, "Docs".into(), note.into()).unwrap();
        save(&mut host, |v| *v = added).unwrap();

        assert_eq!(
            named_twice.unwrap_err().to_string(),
            "Two levels are named \"light\"."
        );
        let levels = host.settings().verification.levels;
        let names: Vec<&str> = levels.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Full", "Light", "Docs"]);
        assert_eq!(
            (
                levels[2].kind,
                levels[2].verifiers.as_slice(),
                levels[2].use_when.as_str()
            ),
            (LevelKind::Recheck, &["Style".to_owned()][..], note)
        );
    }

    #[test]
    fn an_edited_level_saves_its_new_name_and_note_and_keeps_its_kind_and_ticks() {
        let (_directory, mut host) = host();
        let before = host.settings().verification;

        let edited = with_level(
            before.clone(),
            &LevelForm::Edit(1),
            "Rebase".into(),
            "after a rebase.".into(),
        )
        .unwrap();
        // Cancelling sends nothing, so the saved line stays.
        assert_eq!(host.settings().verification, before);
        save(&mut host, |v| *v = edited).unwrap();

        let level = &host.settings().verification.levels[1];
        assert_eq!(
            (level.name.as_str(), level.use_when.as_str()),
            ("Rebase", "after a rebase.")
        );
        assert_eq!(
            (level.kind, &level.verifiers),
            (LevelKind::Recheck, &before.levels[1].verifiers)
        );
    }

    #[test]
    fn the_host_refuses_a_change_in_the_cards_words_and_keeps_what_was_saved() {
        let (_directory, mut host) = host();
        let saved = host.settings().verification;
        let no_full_review = "Keep at least one level marked Full review.";

        // Switching the only full review to a re-check, or removing it.
        assert_eq!(
            save(&mut host, |v| v.levels[0].kind = LevelKind::Recheck).unwrap_err(),
            no_full_review
        );
        assert_eq!(
            save(&mut host, |v| {
                v.levels.remove(0);
            })
            .unwrap_err(),
            no_full_review
        );
        // Removing the only verifier a level runs, or unticking it.
        let no_verifier =
            "Light would run no verifier. Tick another verifier for Light, or remove Light.";
        assert_eq!(
            save(&mut host, |v| remove_verifier(v, 1)).unwrap_err(),
            no_verifier
        );
        assert_eq!(
            save(&mut host, |v| toggle_verifier(
                &v.verifiers,
                &mut v.levels[1],
                "Correctness"
            ))
            .unwrap_err(),
            no_verifier
        );
        assert_eq!(host.settings().verification, saved);
    }

    #[test]
    fn removing_a_verifier_takes_it_out_of_every_level_in_one_save() {
        let (_directory, mut host) = host();

        save(&mut host, |v| remove_verifier(v, 3)).unwrap();

        let settings = host.settings().verification;
        let focuses: Vec<&str> = settings
            .verifiers
            .iter()
            .map(|v| v.focus.as_str())
            .collect();
        assert_eq!(focuses, ["Tests", "Correctness", "Regressions"]);
        assert_eq!(runs(&host, 0), ["Tests", "Correctness", "Regressions"]);
        assert_eq!(runs(&host, 1), ["Correctness"]);
    }
}
