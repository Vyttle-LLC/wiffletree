//! The Models page's Review card: who verifies tickets, the round cap and the cycle cap. Each
//! change saves at once, apart from the page's Save, because the host keeps it in its settings
//! file.
use super::*;
use gpui_component::{ActiveTheme, Sizable, button::ButtonVariants, input::Input};
use ui::{hint, icon, segment};

pub(super) struct VerifierEditor {
    bridge: Bridge,
    verification: Option<VerificationSettings>,
    error: Option<String>,
    /// The verifier being added.
    draft: VerifierConfig,
    focus: Entity<InputState>,
    instruction: Entity<InputState>,
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
        let mut editor = Self {
            bridge,
            verification: None,
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
        };
        editor.ask_host(Command::Settings, cx);
        editor
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
                                view.save(
                                    |v| {
                                        v.verifiers.remove(index);
                                    },
                                    cx,
                                )
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child("Round cap")
                    .child(rounds)
                    .child(div().w(px(12.)))
                    .child("Cycle cap")
                    .child(cycles),
            )
            .child(hint(
                "verify_ticket starts every verifier at once on the ticket's current commit, each on the model the coordinator picks by the guide within its provider. Only failed verifiers re-run, up to the round cap, and a ticket may start up to the cycle cap of cycles. Changes here save at once.",
                p,
            ))
            .children(self.error.clone().map(|error| hint(error, p).text_color(p.red)))
    }
}
