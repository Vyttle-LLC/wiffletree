//! The Settings window: this client's appearance, the host's workspace folder, its check-in
//! thresholds and the settings repository.
mod repository;

use super::*;
use gpui_component::{ActiveTheme, Disableable};
use repository::Confirm;
pub(super) use repository::{Mark, share_sync};
use ui::{hint, icon, mono, section, segment, setting};

/// The open Settings window, so ⌘, focuses it instead of opening a second one.
#[derive(Clone)]
struct SettingsWindow(WindowHandle<Root>, WeakEntity<Settings>);
impl Global for SettingsWindow {}

pub(super) struct Settings {
    bridge: Bridge,
    host: Option<HostSettings>,
    folder_error: Option<String>,
    check_in_error: Option<String>,
    choosing_folder: bool,
    scroll: ScrollHandle,
    repository_input: Entity<InputState>,
    /// The repository as the human named it, and what checking it found.
    checked: Option<(String, SettingsRepositoryCheck)>,
    /// The machine whose settings this Mac took when it joined, while the window stays open.
    joined_from: Option<String>,
    repository_error: Option<String>,
    /// A repository request is waiting for the host.
    asking: bool,
    confirming: Option<Confirm>,
}

/// The agent check-in presets; the host accepts any value in `CHECKIN_CHILD_TURNS`.
const AGENT_TURNS: [u32; 5] = [10, 25, 50, 100, 250];
/// The human check-in presets; the host accepts any value in `CHECKIN_HUMAN_TURNS`.
const COORDINATOR_TURNS: [u32; 5] = [50, 100, 250, 500, 1000];

impl Settings {
    /// Focuses the Settings window, opening it first if none is open.
    pub(super) fn open(bridge: Bridge, cx: &mut App) {
        Self::open_at(bridge, false, cx);
    }
    /// Opens Settings scrolled to the Settings repository section, as the sync mark does.
    pub(super) fn open_at_repository(bridge: Bridge, cx: &mut App) {
        Self::open_at(bridge, true, cx);
    }
    fn open_at(bridge: Bridge, at_repository: bool, cx: &mut App) {
        if let Some(SettingsWindow(handle, view)) = cx.try_global::<SettingsWindow>().cloned()
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            if at_repository {
                let _ = view.update(cx, |view, cx| view.show_repository(cx));
            }
            return;
        }
        let bounds = Bounds::centered(None, size(px(560.), px(820.)), cx);
        let mut settings = None;
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
                let repository_input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("owner/name or a GitHub URL")
                });
                let view = cx.new(|cx| {
                    let mut view = Self {
                        bridge,
                        host: None,
                        folder_error: None,
                        check_in_error: None,
                        choosing_folder: false,
                        scroll: ScrollHandle::new(),
                        repository_input,
                        checked: None,
                        joined_from: None,
                        repository_error: None,
                        asking: false,
                        confirming: None,
                    };
                    view.ask_host(Command::Settings, cx);
                    view.refresh_sync(cx);
                    cx.observe_global::<repository::SyncShown>(|_, cx| cx.notify())
                        .detach();
                    if at_repository {
                        view.show_repository(cx);
                    }
                    view
                });
                settings = Some(view.downgrade());
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        match (opened, settings) {
            (Ok(handle), Some(view)) => cx.set_global(SettingsWindow(handle, view)),
            (Err(error), _) => eprintln!("Opening Settings: {error:#}"),
            (Ok(_), None) => {}
        }
    }

    fn show_repository(&mut self, cx: &mut Context<Self>) {
        self.scroll.scroll_to_top_of_item(repository::SECTION);
        cx.notify();
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
    fn ask_host(&mut self, command: Command, cx: &mut Context<Self>) {
        let check_ins = matches!(command, Command::SetCheckIns { .. });
        let receiver = match self.bridge.request(command) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.show_error(check_ins, error);
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
                        view.host = Some(settings);
                        view.folder_error = None;
                        view.check_in_error = None;
                    }
                    Err(error) => view.show_error(check_ins, error),
                }
                view.choosing_folder = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn show_error(&mut self, check_ins: bool, error: String) {
        if check_ins {
            self.check_in_error = Some(error);
        } else {
            self.folder_error = Some(error);
        }
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
        let shown = self
            .host
            .as_ref()
            .map_or_else(|| "…".into(), |h| h.workspaces_dir.clone());
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

    fn check_in_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut section_body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("CHECK-INS", p));
        if let Some(host) = &self.host {
            let agents = threshold_selector(
                "agent-turns",
                AGENT_TURNS,
                host.checkin_child_turns,
                "turns",
                |h, n| h.checkin_child_turns = n,
                p,
                cx,
            );
            let coordinator = threshold_selector(
                "coordinator-turns",
                COORDINATOR_TURNS,
                host.checkin_human_turns,
                "coordinator turns",
                |h, n| h.checkin_human_turns = n,
                p,
                cx,
            );
            section_body = section_body
                .child(setting(
                    "Check in on an agent every",
                    agents,
                    "The coordinator hears about an agent that has worked this many turns without a message from it. Nothing pauses.",
                    p,
                ))
                .child(setting(
                    "Check in with you every",
                    coordinator,
                    "You get an inbox note when the coordinator has worked this many turns without hearing from you. Nothing pauses.",
                    p,
                ));
        }
        section_body.children(
            self.check_in_error
                .clone()
                .map(|error| hint(error, p).text_color(p.red)),
        )
    }
}

/// One button per preset; choosing one saves both thresholds at once.
fn threshold_selector(
    name: &str,
    presets: [u32; 5],
    current: u32,
    unit: &'static str,
    set: fn(&mut HostSettings, u32),
    p: Palette,
    cx: &mut Context<Settings>,
) -> Div {
    let mut segments = div().flex().gap_1();
    for n in presets {
        segments = segments.child(
            segment(
                SharedString::from(format!("{name}-{n}")),
                n.to_string(),
                n == current,
            )
            .on_click(cx.listener(move |view, _, _, cx| {
                if let Some(mut chosen) = view.host.clone() {
                    set(&mut chosen, n);
                    view.ask_host(
                        Command::SetCheckIns {
                            child_turns: chosen.checkin_child_turns,
                            human_turns: chosen.checkin_human_turns,
                        },
                        cx,
                    );
                }
            })),
        );
    }
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(segments)
        .child(hint(unit, p))
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
            .child({
                let rule = || div().h(px(1.)).bg(p.edge.opacity(0.25));
                let body = div()
                    .id("settings-body")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.appearance_section(p, cx))
                    .child(rule())
                    .child(self.folder_section(p, cx))
                    .child(rule())
                    .child(self.check_in_section(p, cx))
                    .child(rule())
                    .child(self.repository_section(p, cx));
                let dialog =
                    (self.confirming).and_then(|confirm| self.confirm_dialog(confirm, p, cx));
                let shown = div().flex_1().min_h_0().relative();
                match dialog {
                    Some(dialog) => shown.child(body.opacity(0.35)).child(
                        div()
                            .absolute()
                            .inset_0()
                            .occlude()
                            .flex()
                            .justify_center()
                            .pt(px(120.))
                            .child(dialog),
                    ),
                    None => shown.child(body),
                }
            })
    }
}
