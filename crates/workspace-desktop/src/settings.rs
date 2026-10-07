//! The Settings window: this client's appearance and the host's workspace folder.
use super::*;
use gpui_component::{ActiveTheme, Disableable};
use ui::{hint, icon, mono, section, segment};

/// The open Settings window, so ⌘, focuses it instead of opening a second one.
#[derive(Clone, Copy)]
struct SettingsWindow(WindowHandle<Root>);
impl Global for SettingsWindow {}

pub(super) struct Settings {
    workspace: WeakEntity<Workspace>,
    bridge: Bridge,
    workspaces_dir: Option<String>,
    folder_error: Option<String>,
    choosing_folder: bool,
    _redraw: Subscription,
}

impl Settings {
    /// Focuses the Settings window, opening it first if none is open.
    pub(super) fn open(workspace: &Entity<Workspace>, bridge: Bridge, cx: &mut App) {
        if let Some(SettingsWindow(handle)) = cx.try_global::<SettingsWindow>().copied()
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = Bounds::centered(None, size(px(520.), px(300.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Settings");
                let view = cx.new(|cx| {
                    let mut view = Self {
                        workspace: workspace.downgrade(),
                        bridge,
                        workspaces_dir: None,
                        folder_error: None,
                        choosing_folder: false,
                        // The appearance can also change from the sidebar.
                        _redraw: cx.observe(workspace, |_, _, cx| cx.notify()),
                    };
                    view.ask_host(HostRequest::Read, cx);
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

    fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        let _ = self
            .workspace
            .update(cx, |workspace, cx| workspace.set_appearance(appearance, cx));
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
                        HostRequest::SetWorkspacesDir(path.to_string_lossy().into_owned()),
                        cx,
                    ),
                    None => view.choosing_folder = false,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows the host's answer, or its error beside the folder it still uses.
    fn ask_host(&mut self, request: HostRequest, cx: &mut Context<Self>) {
        let receiver = match request.send(&self.bridge) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.folder_error = Some(error);
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
                        view.folder_error = None;
                    }
                    Err(error) => view.folder_error = Some(error),
                }
                view.choosing_folder = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn appearance_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let current = self
            .workspace
            .read_with(cx, |workspace, _| workspace.preferences.appearance)
            .unwrap_or_default();
        let mut options = div().flex().gap_1();
        for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
            options = options.child(
                segment(
                    SharedString::from(format!("appearance-{}", appearance.label())),
                    appearance.label(),
                    appearance == current,
                )
                .icon(icon(appearance.icon()).size(px(13.)))
                .on_click(cx.listener(move |view, _, _, cx| view.set_appearance(appearance, cx))),
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
                    .child(self.folder_section(p, cx)),
            )
    }
}

/// Stand-in for the host contract (`HostSettings`, `Command::Settings` and
/// `Command::SetWorkspacesDir`) until the host branch lands.
#[derive(serde::Deserialize)]
struct HostSettings {
    workspaces_dir: String,
}
enum HostRequest {
    Read,
    SetWorkspacesDir(String),
}
impl HostRequest {
    fn send(
        &self,
        _bridge: &Bridge,
    ) -> Result<async_channel::Receiver<Result<Value, String>>, String> {
        match self {
            Self::Read => Err("This host cannot report its workspace folder yet.".into()),
            Self::SetWorkspacesDir(path) => Err(format!("This host cannot use {path} yet.")),
        }
    }
}
