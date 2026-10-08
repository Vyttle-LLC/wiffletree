//! The Settings window: this client's appearance, the host's workspace folder and who verifies
//! tickets.
use super::*;
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::ButtonVariants,
    input::{Input, InputState},
};
use ui::{hint, icon, mono, section, segment};

/// The open Settings window, so ⌘, focuses it instead of opening a second one.
#[derive(Clone, Copy)]
struct SettingsWindow(WindowHandle<Root>);
impl Global for SettingsWindow {}

pub(super) struct Settings {
    bridge: Bridge,
    workspaces_dir: Option<String>,
    folder_error: Option<String>,
    choosing_folder: bool,
    verification: Option<VerificationSettings>,
    verification_error: Option<String>,
    /// The verifier being added.
    draft: VerifierConfig,
    focus: Entity<InputState>,
    instruction: Entity<InputState>,
}

/// Which setting a host request changes, so its error shows beside it.
#[derive(Clone, Copy)]
enum Setting {
    Folder,
    Verification,
}

impl Settings {
    /// Focuses the Settings window, opening it first if none is open.
    pub(super) fn open(bridge: Bridge, cx: &mut App) {
        if let Some(SettingsWindow(handle)) = cx.try_global::<SettingsWindow>().copied()
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = Bounds::centered(None, size(px(560.), px(680.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                is_resizable: true,
                is_minimizable: false,
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Settings");
                let view = cx.new(|cx| {
                    let mut view = Self {
                        bridge,
                        workspaces_dir: None,
                        folder_error: None,
                        choosing_folder: false,
                        verification: None,
                        verification_error: None,
                        draft: VerifierConfig {
                            role: Role::Tester,
                            focus: String::new(),
                            instruction: None,
                            provider: None,
                            size: None,
                        },
                        focus: cx.new(|cx| {
                            InputState::new(window, cx).placeholder("Focus, e.g. Codex correctness")
                        }),
                        instruction: cx.new(|cx| {
                            InputState::new(window, cx).placeholder(
                                "Optional instruction, e.g. Run the Reviso style review",
                            )
                        }),
                    };
                    view.ask_host(Command::Settings, Setting::Folder, cx);
                    view
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        match opened {
            Ok(handle) => cx.set_global(SettingsWindow(handle)),
            Err(error) => eprintln!("Opening Settings: {error:#}"),
        }
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose the workspace folder".into()),
        });
        self.choosing_folder = true;
        cx.spawn_in(window, async move |this, cx| {
            let chosen = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let _ = this.update(cx, |view, cx| {
                match chosen {
                    Some(path) => view.ask_host(
                        Command::SetWorkspacesDir {
                            path: path.to_string_lossy().into_owned(),
                        },
                        Setting::Folder,
                        cx,
                    ),
                    None => view.choosing_folder = false,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows the host's answer, or its error beside the setting it still uses.
    fn ask_host(&mut self, command: Command, setting: Setting, cx: &mut Context<Self>) {
        let receiver = match self.bridge.request(command) {
            Ok(receiver) => receiver,
            Err(error) => {
                *self.error_for(setting) = Some(error);
                self.choosing_folder = false;
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
                        view.workspaces_dir = Some(settings.workspaces_dir);
                        view.verification = Some(settings.verification);
                        *view.error_for(setting) = None;
                    }
                    Err(error) => *view.error_for(setting) = Some(error),
                }
                view.choosing_folder = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn error_for(&mut self, setting: Setting) -> &mut Option<String> {
        match setting {
            Setting::Folder => &mut self.folder_error,
            Setting::Verification => &mut self.verification_error,
        }
    }

    /// Saves a changed verification setting; the host refuses an invalid one and keeps its own.
    fn save_verification(
        &mut self,
        change: impl FnOnce(&mut VerificationSettings),
        cx: &mut Context<Self>,
    ) {
        let Some(mut verification) = self.verification.clone() else {
            return;
        };
        change(&mut verification);
        self.ask_host(
            Command::SetVerification { verification },
            Setting::Verification,
            cx,
        );
    }

    fn add_verifier(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.read(cx).value().trim().to_owned();
        let instruction = self.instruction.read(cx).value().trim().to_owned();
        let verifier = VerifierConfig {
            focus,
            instruction: (!instruction.is_empty()).then_some(instruction),
            ..self.draft.clone()
        };
        self.save_verification(|v| v.verifiers.push(verifier), cx);
        for input in [&self.focus, &self.instruction] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    fn verification_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(verification) = &self.verification else {
            return div();
        };
        let cap = verification.max_rounds;
        let mut rounds = div().flex().gap_1();
        for n in 1..=MAX_VERIFICATION_ROUNDS {
            rounds = rounds.child(
                segment(
                    SharedString::from(format!("rounds-{n}")),
                    n.to_string(),
                    n == cap,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.save_verification(|v| v.max_rounds = n, cx)
                })),
            );
        }
        let mut list = div().flex().flex_col().gap_1();
        for (index, verifier) in verification.verifiers.iter().enumerate() {
            let model = match (verifier.provider, verifier.size) {
                (None, None) => "role default".to_owned(),
                (provider, size) => [
                    provider.map(|p| format!("{p:?}")),
                    size.map(|s| format!("{s:?}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(verifier.role.agent_label(Some(&verifier.focus))),
                    )
                    .child(hint(model, p))
                    .child(
                        button(SharedString::from(format!("remove-verifier-{index}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip("Remove this verifier")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.save_verification(
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
            ("Default", None),
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
        let mut sizes = div().flex().gap_1();
        for (label, size) in [
            ("Default", None),
            ("Big", Some(ProfileSize::Big)),
            ("Small", Some(ProfileSize::Small)),
        ] {
            sizes = sizes.child(
                segment(
                    SharedString::from(format!("verifier-size-{label}")),
                    label,
                    draft.size == size,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.draft.size = size;
                    cx.notify();
                })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("VERIFICATION", p))
            .child(hint(
                "verify_ticket starts every verifier at once on the ticket's current commit. Only failed verifiers re-run, up to the round cap.",
                p,
            ))
            .child(div().flex().items_center().gap_2().child("Round cap").child(rounds))
            .child(list)
            .child(div().flex().gap_2().child(roles).child(providers).child(sizes))
            .child(Input::new(&self.focus))
            .child(Input::new(&self.instruction))
            .child(
                div().flex().child(
                    button("add-verifier")
                        .icon(icon("plus"))
                        .label("Add verifier")
                        .on_click(cx.listener(|view, _, window, cx| view.add_verifier(window, cx))),
                ),
            )
            .children(self.verification_error.clone().map(|error| {
                div()
                    .text_size(px(12.))
                    .line_height(relative(1.5))
                    .text_color(p.red)
                    .child(error)
            }))
    }

    fn appearance_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let current = cx.global::<Preferences>().appearance;
        let mut options = div().flex().gap_1();
        for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
            options = options.child(
                segment(
                    SharedString::from(format!("appearance-{}", appearance.label())),
                    appearance.label(),
                    appearance == current,
                )
                .icon(icon(appearance.icon()).size(px(13.)))
                .on_click(move |_, _, cx| set_appearance(appearance, cx)),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("APPEARANCE", p))
            .child(options)
            .child(hint(
                "System follows macOS. This Mac remembers its own choice.",
                p,
            ))
    }

    fn folder_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let shown = self.workspaces_dir.clone().unwrap_or_else(|| "…".into());
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                section("WORKSPACE FOLDER", p).child(
                    button("choose-workspaces-dir")
                        .label("Choose…")
                        .disabled(self.choosing_folder)
                        .on_click(
                            cx.listener(|view, _, window, cx| view.choose_folder(window, cx)),
                        ),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("folder").size(px(14.)).text_color(p.subtle))
                    .child(mono(shown, p)),
            )
            .children(self.folder_error.clone().map(|error| {
                div()
                    .text_size(px(12.))
                    .line_height(relative(1.5))
                    .text_color(p.red)
                    .child(error)
            }))
            .child(hint(
                "New projects and tickets go here. Existing ones stay where they are.",
                p,
            ))
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx.theme().is_dark());
        div()
            .size_full()
            .bg(p.base)
            .text_color(p.text)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .flex()
            .flex_col()
            .child(
                TitleBar::new().child(
                    div()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .pr(px(70.))
                        .child("Settings"),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.appearance_section(p, cx))
                    .child(div().h(px(1.)).bg(p.edge.opacity(0.25)))
                    .child(self.folder_section(p, cx))
                    .child(div().h(px(1.)).bg(p.edge.opacity(0.25)))
                    .child(self.verification_section(p, cx)),
            )
    }
}
