//! The Settings window, in two parts (design 10 of `remote-hosts`): this Mac's app, with its
//! appearance, layout, finding other Macs and the computers it is paired with, and the host the window drives, with its
//! workspace folder, check-in thresholds, builds at once and settings repository, and on This Mac its remote
//! access. A setting belongs to the Mac whose agents it runs.
mod remote_access;
mod repository;

use super::*;
use gpui_component::{ActiveTheme, Disableable, button::ButtonVariants, switch::Switch};
use repository::{Confirm, Joined};
pub(super) use repository::{Mark, share_sync};
use ui::{card, hint, icon, mono, over_dimmed, section, segment, setting};

/// The open Settings window, so ⌘, focuses it instead of opening a second one.
#[derive(Clone)]
struct SettingsWindow(WindowHandle<Root>, WeakEntity<Settings>);
impl Global for SettingsWindow {}

pub(super) struct Settings {
    bridge: Bridge,
    /// The host the window drives, as the switcher selected it.
    driving: HostView,
    host: Option<HostSettings>,
    folder_error: Option<String>,
    check_in_error: Option<String>,
    builds_error: Option<String>,
    choosing_folder: bool,
    scroll: ScrollHandle,
    repository_input: Entity<InputState>,
    /// The repository as the human named it, and what checking it found.
    checked: Option<(String, SettingsRepositoryCheck)>,
    /// How this Mac joined, while the window stays open.
    joined: Option<Joined>,
    repository_error: Option<String>,
    /// A repository request is waiting for the host.
    asking: bool,
    confirming: Option<Confirm>,
    /// The paused setting open in Compare.
    compared: crate::workspace_notes::Compared,
    /// This Mac's remote access, as the host last answered.
    remote: Option<RemoteAccess>,
    remote_error: Option<String>,
    /// The page is looking again on a timer.
    watching_remote: bool,
}

/// A check-in threshold's presets, the unit they count and how a choice sets it.
struct Threshold {
    name: &'static str,
    presets: [u32; 5],
    unit: &'static str,
    set: fn(&mut HostSettings, u32),
}
/// The agent check-in presets; the host accepts any value in `CHECKIN_CHILD_TURNS`.
const AGENT_TURNS: Threshold = Threshold {
    name: "agent-turns",
    presets: [10, 25, 50, 100, 250],
    unit: "turns",
    set: |h, n| h.checkin_child_turns = n,
};
/// The human check-in presets; the host accepts any value in `CHECKIN_HUMAN_TURNS`.
const COORDINATOR_TURNS: Threshold = Threshold {
    name: "coordinator-turns",
    presets: [50, 100, 250, 500, 1000],
    unit: "coordinator turns",
    set: |h, n| h.checkin_human_turns = n,
};

impl Settings {
    /// Focuses the Settings window, opening it first if none is open.
    pub(super) fn open(cx: &mut App) {
        Self::open_at(false, cx);
    }
    /// Opens Settings scrolled to the Settings repository section, as the sync mark does.
    pub(super) fn open_at_repository(cx: &mut App) {
        Self::open_at(true, cx);
    }
    /// The switcher selected another host: an open Settings window shows that host's settings.
    pub(super) fn host_selected(cx: &mut App) {
        let Some(SettingsWindow(_, view)) = cx.try_global::<SettingsWindow>().cloned() else {
            return;
        };
        let hosts = cx.global::<Hosts>();
        let (bridge, driving) = (hosts.bridge(), hosts.view());
        let _ = view.update(cx, |view, cx| {
            view.bridge = bridge;
            view.driving = driving;
            view.host = None;
            view.checked = None;
            view.joined = None;
            view.ask_host(ask::settings(), cx);
            view.ask_remote_here(cx);
            view.refresh_sync(cx);
            cx.notify();
        });
    }
    /// Follows the driven host's connection: saving turns off while it does not answer, and its
    /// settings are read again once it is back.
    fn follow_link(&mut self, cx: &mut Context<Self>) {
        let driving = cx.global::<Hosts>().view_of(&self.bridge);
        if driving == self.driving {
            return;
        }
        let answering = |view: &HostView| {
            (view.remote.as_ref()).is_none_or(|r| matches!(r.link, hosts::Link::Connected(_)))
        };
        let back = !answering(&self.driving) && answering(&driving);
        self.driving = driving;
        if back {
            self.ask_host(ask::settings(), cx);
            self.refresh_sync(cx);
        }
        cx.notify();
    }
    /// Remote access changes only at the host itself, so only This Mac's is asked for.
    fn ask_remote_here(&mut self, cx: &mut Context<Self>) {
        if self.driving.remote.is_none() {
            self.ask_remote(ask::remote_access(), cx);
        }
    }
    fn open_at(at_repository: bool, cx: &mut App) {
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
        let hosts = cx.global::<Hosts>();
        let (bridge, driving) = (hosts.bridge(), hosts.view());
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
                        driving,
                        host: None,
                        folder_error: None,
                        check_in_error: None,
                        builds_error: None,
                        choosing_folder: false,
                        scroll: ScrollHandle::new(),
                        repository_input,
                        checked: None,
                        joined: None,
                        repository_error: None,
                        asking: false,
                        confirming: None,
                        compared: None,
                        remote: None,
                        remote_error: None,
                        watching_remote: false,
                    };
                    view.ask_host(ask::settings(), cx);
                    view.ask_remote_here(cx);
                    view.refresh_sync(cx);
                    cx.observe_global::<repository::SyncByHost>(|view, cx| view.sync_shown(cx))
                        .detach();
                    cx.spawn(async move |this, cx| {
                        loop {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(1))
                                .await;
                            if this.update(cx, |view, cx| view.follow_link(cx)).is_err() {
                                break;
                            }
                        }
                    })
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
        let paths = folders::choose(
            &self.bridge,
            "Choose the workspace folder",
            false,
            window,
            cx,
        );
        self.choosing_folder = true;
        let origin = self.host_id();
        cx.spawn_in(window, async move |this, cx| {
            let chosen = paths.recv().await.ok().and_then(|p| p.into_iter().next());
            let _ = this.update(cx, |view, cx| {
                // A folder chosen on a host the window no longer shows is not saved elsewhere.
                match chosen.filter(|_| view.host_id() == origin) {
                    Some(path) => {
                        let path = path.to_string_lossy().into_owned();
                        view.ask_host(ask::set_workspaces_dir(path), cx)
                    }
                    None => view.choosing_folder = false,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The paired host the window shows, or `None` for This Mac's.
    fn host_id(&self) -> Option<String> {
        self.bridge.remote().map(|r| r.id.clone())
    }

    /// Shows the host's answer, or its error beside the setting it still uses. An answer from a
    /// host the window no longer shows is dropped.
    fn ask_host(&mut self, ask: Ask<HostSettings>, cx: &mut Context<Self>) {
        let command = ask.command().clone();
        let origin = self.host_id();
        let answer = match self.bridge.ask(ask) {
            Ok(answer) => answer,
            Err(error) => {
                self.show_error(&command, error);
                self.choosing_folder = false;
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |view, cx| {
                shared::replied(&command, answer.is_ok(), cx);
                if view.host_id() != origin {
                    return;
                }
                match answer {
                    Ok(settings) => {
                        view.host = Some(settings);
                        view.folder_error = None;
                        view.check_in_error = None;
                        view.builds_error = None;
                    }
                    Err(error) => view.show_error(&command, error),
                }
                view.choosing_folder = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows an error beside the setting `command` saves.
    fn show_error(&mut self, command: &Command, error: String) {
        let shown = match command {
            Command::SetCheckIns { .. } => &mut self.check_in_error,
            Command::SetBuildsAtOnce { .. } => &mut self.builds_error,
            _ => &mut self.folder_error,
        };
        *shown = Some(error);
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

    /// The sidebar's and the transcript's own layout toggles, here too, saved the same way.
    fn layout_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let preferences = cx.global::<Preferences>();
        let (grouped, minimized) = (preferences.group_projects, preferences.minimize_activity);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("LAYOUT", p))
            .child(switch_row(
                Switch::new("group-projects")
                    .checked(grouped)
                    .on_click(|on: &bool, _, cx| {
                        set_group_projects(*on, cx);
                        cx.refresh_windows();
                    }),
                "Group projects that share a first word",
            ))
            .child(switch_row(
                Switch::new("minimize-activity")
                    .checked(minimized)
                    .on_click(|on: &bool, _, cx| {
                        set_minimize_activity(*on, cx);
                        cx.refresh_windows();
                    }),
                "Show a working agent's activity as one line",
            ))
            .child(hint(
                "The same choices as the sidebar's and the transcript's own toggles.",
                p,
            ))
    }

    /// Whether this Mac looks around the network for Wiffletree, which macOS asks about.
    fn other_macs_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let finding = cx.global::<Preferences>().find_on_network;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("OTHER MACS", p))
            .child(switch_row(
                Switch::new("find-on-network")
                    .checked(finding)
                    .on_click(|on: &bool, _, cx| set_find_on_network(*on, cx)),
                "Find Wiffletree on other Macs on this network",
            ))
            .child(hint(
                "Off: this Mac never looks around the network, and macOS doesn't ask about it. You can still connect a computer by typing its name or address, and paired computers still reconnect.",
                p,
            ))
            .child(hint(
                "On: Connect a computer lists the Macs it finds, and a paired Mac that changes address is found again. macOS asks once to allow it.",
                p,
            ))
    }

    /// The hosts this Mac is paired with, which only this Mac keeps.
    fn paired_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let paired = &cx.global::<Preferences>().paired_hosts;
        let mut list = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("COMPUTERS YOU'VE PAIRED WITH", p));
        if paired.is_empty() {
            return list.child(hint(
                "None yet. Connect a computer… in the sidebar's host menu pairs one.",
                p,
            ));
        }
        for (index, host) in paired.iter().enumerate() {
            let id = host.id.clone();
            let seen = (host.last_seen).map_or("Not reached yet".into(), |seen| {
                format!("Last seen {}", hosts::spelled_age(seen, ui::now()))
            });
            let reached = host
                .addresses
                .first()
                .map_or(seen.clone(), |address| format!("{seen} · {address}"));
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(host.name.clone())
                            .child(hint(reached, p)),
                    )
                    .child(
                        button(("forget-host", index))
                            .ghost()
                            .label("Remove")
                            .tooltip("Removes it from this Mac; pair again to use it")
                            .on_click(move |_, _, cx| forget_host(&id, cx)),
                    ),
            );
        }
        list
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
                        .map(|b| ui::gated(b, &self.driving))
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
                    .text_size(px(font::SMALL))
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
            let agents =
                threshold_selector(&self.driving, &AGENT_TURNS, host.checkin_child_turns, p, cx);
            let coordinator = threshold_selector(
                &self.driving,
                &COORDINATOR_TURNS,
                host.checkin_human_turns,
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

impl Settings {
    /// How many builds agents may run at once on the host, across every project.
    fn builds_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut body = div().flex().flex_col().gap_2().child(section("BUILDS", p));
        if let Some(host) = &self.host {
            let mut segments = div().flex().gap_1();
            for n in 1..=MAX_BUILDS_AT_ONCE {
                segments = segments.child(
                    segment(
                        SharedString::from(format!("builds-at-once-{n}")),
                        n.to_string(),
                        n == host.builds_at_once,
                    )
                    .map(|b| ui::gated(b, &self.driving))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.ask_host(ask::set_builds_at_once(n), cx)
                    })),
                );
            }
            body = body.child(setting(
                "Builds at once",
                segments,
                "How many agent builds and test runs may run at once, across every project. Each uses half the cores; the rest wait their turn.",
                p,
            ));
        }
        body.children(
            self.builds_error
                .clone()
                .map(|error| hint(error, p).text_color(p.red)),
        )
    }
}

/// The heading of one part of the window: whose settings follow, and what they are.
fn part(title: &str, holds: &'static str, p: Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(font::BODY))
                .child(title.to_owned()),
        )
        .child(hint(holds, p))
}

/// A setting's label with its switch at the end of the row.
fn switch_row(switch: Switch, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().flex_1().min_w_0().child(label))
        .child(switch)
}

/// One button per preset; choosing one saves both thresholds at once.
fn threshold_selector(
    driving: &HostView,
    threshold: &Threshold,
    current: u32,
    p: Palette,
    cx: &mut Context<Settings>,
) -> Div {
    let mut segments = div().flex().gap_1();
    let set = threshold.set;
    for n in threshold.presets {
        segments = segments.child(
            segment(
                SharedString::from(format!("{}-{n}", threshold.name)),
                n.to_string(),
                n == current,
            )
            .map(|b| ui::gated(b, driving))
            .on_click(cx.listener(move |view, _, _, cx| {
                if let Some(mut chosen) = view.host.clone() {
                    set(&mut chosen, n);
                    let ask =
                        ask::set_check_ins(chosen.checkin_child_turns, chosen.checkin_human_turns);
                    view.ask_host(ask, cx);
                }
            })),
        );
    }
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(segments)
        .child(hint(threshold.unit, p))
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx.theme().is_dark());
        div()
            .size_full()
            .bg(p.base)
            .text_color(p.text)
            .font_family(".SystemUIFont")
            .text_size(px(font::TEXT))
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
                let rule = || div().h(px(1.)).bg(p.edge.opacity(edge::HAIRLINE));
                let body = div()
                    .id("settings-body")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(part(
                        "This Mac · the app",
                        "Appearance · layout · other Macs · computers you've paired with",
                        p,
                    ))
                    .child(self.appearance_section(p, cx))
                    .child(rule())
                    .child(self.layout_section(p, cx))
                    .child(rule())
                    .child(self.other_macs_section(p, cx))
                    .child(rule())
                    .child(self.paired_section(p, cx))
                    .child(rule())
                    .child(part(
                        &match self.driving.name() {
                            Some(host) => format!("{host} · the host you're driving"),
                            None => "This Mac · the host".into(),
                        },
                        "Spaces · models · repositories and checks · workspace folder · PR owners allowed · check-ins · builds at once · reviews · skills · settings repository",
                        p,
                    ))
                    .children(self.driving.changes_refused().map(|reason| hint(reason, p)))
                    .child(self.folder_section(p, cx))
                    .child(rule())
                    .child(self.check_in_section(p, cx))
                    .child(rule())
                    .child(self.builds_section(p, cx))
                    .child(rule())
                    .child(ui::locked(self.repository_section(p, cx), &self.driving, p))
                    .child(rule())
                    .child(match self.driving.name() {
                        None => self.remote_access_section(p, cx),
                        Some(host) => hint(
                            format!(
                                "Remote access (pairing and who can control it) changes only at {host} itself."
                            ),
                            p,
                        ),
                    });
                let dialog =
                    (self.confirming).and_then(|confirm| self.confirm_dialog(confirm, p, cx));
                match dialog {
                    Some(dialog) => over_dimmed(body, dialog).flex_1().min_h_0(),
                    None => div().flex_1().min_h_0().relative().child(body),
                }
            })
    }
}
