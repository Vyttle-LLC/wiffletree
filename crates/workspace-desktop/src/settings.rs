//! The Settings window. Its sidebar groups every setting as the settings-brain-fixes study does:
//! Shared with your Macs, Spaces & brains, and This Mac, opening the pages that live in the main
//! window there, each at its own card. Its body keeps the parts of design 10 of `remote-hosts`:
//! the shared settings, this Mac's app, with its appearance, layout, finding other Macs and the
//! computers it is paired with, and the host the window drives, with its workspace folder, builds
//! at once and, on This Mac, its remote access. A setting belongs to the Mac whose agents it runs.
mod appearance;
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
    /// The assistant's name while the user renames it.
    assistant_input: Entity<InputState>,
    renaming_assistant: bool,
    assistant_error: Option<String>,
}

/// The router thresholds Settings offers; the host accepts any from 0.5 to 1.0.
const ROUTER_THRESHOLDS: [f64; 6] = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

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
        let bounds = Bounds::centered(None, size(px(760.), px(820.)), cx);
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
                let assistant_input =
                    cx.new(|cx| InputState::new(window, cx).placeholder("Wren, Assistant"));
                let view = cx.new(|cx| {
                    cx.subscribe(&assistant_input, |view: &mut Self, _, event, cx| {
                        if matches!(event, InputEvent::PressEnter { .. }) {
                            view.save_assistant_name(cx);
                        }
                    })
                    .detach();
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
                        assistant_input,
                        renaming_assistant: false,
                        assistant_error: None,
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
        self.scroll_to(Block::Repository, cx);
    }
    fn scroll_to(&mut self, block: Block, cx: &mut Context<Self>) {
        self.scroll.scroll_to_top_of_item(block.index());
        cx.notify();
    }
    /// Follows a sidebar entry: a section of this window, or a page of the main window, which
    /// comes forward.
    fn go(&mut self, to: &Go, cx: &mut Context<Self>) {
        let intent = match to {
            Go::Section(block) => return self.scroll_to(*block, cx),
            Go::Page(page) => Intent::OpenPage(*page),
            Go::Models(card) => Intent::OpenModelsCard(*card),
            Go::Repositories(card) => Intent::OpenRepositories(*card),
            Go::Space(id) => Intent::OpenSpace(id.clone()),
        };
        with_workspace(cx, move |view, window, cx| {
            window.activate_window();
            view.handle(&intent, window, cx)
        });
    }
    /// The sidebar: what is shared with the human's Macs, their Spaces, and what is this Mac's.
    fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut side = div()
            .w(px(190.))
            .flex_none()
            .h_full()
            .px_2()
            .py_3()
            .flex()
            .flex_col()
            .gap(px(1.))
            .bg(p.base)
            .border_r_1()
            .border_color(p.edge.opacity(edge::LIGHT));
        for (group, entries) in sidebar_groups(&main_window_spaces(cx)) {
            side = side.child(
                div()
                    .px_2()
                    .pt_3()
                    .pb_1()
                    .text_size(px(font::TINY))
                    .text_color(p.subtle)
                    .child(group.to_uppercase()),
            );
            for (at, (label, to)) in entries.into_iter().enumerate() {
                side = side.child(
                    div()
                        .id(SharedString::from(format!("settings-{group}-{at}")))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .text_size(px(font::SMALL))
                        .hover(|d| d.bg(p.overlay))
                        .child(label)
                        .on_click(cx.listener(move |view, _, _, cx| view.go(&to, cx))),
                );
            }
        }
        side
    }
    /// One block of the scrolled body, with a rule above each section that follows another.
    fn block(&self, block: Block, p: Palette, cx: &mut Context<Self>) -> Div {
        let rule = || div().h(px(1.)).bg(p.edge.opacity(edge::HAIRLINE));
        let section = match block {
            Block::Shared => {
                return part(
                    "Shared with your Macs",
                    "Assistant · check-ins · settings repository. Behaviors, reviews and the PR keeper open in the main window.",
                    p,
                );
            }
            Block::App => {
                return part(
                    "This Mac · the app",
                    "Appearance · layout · other Macs · computers you've paired with",
                    p,
                );
            }
            Block::Host => {
                let title = match self.driving.name() {
                    Some(host) => format!("{host} · the host you're driving"),
                    None => "This Mac · the host".into(),
                };
                return part(
                    &title,
                    "Workspace folder · builds at once · remote access. Who runs what, repositories and their checks, and allowed owners open in the main window.",
                    p,
                )
                .children(self.driving.changes_refused().map(|reason| hint(reason, p)));
            }
            Block::Assistant => ui::locked(self.assistant_section(p, cx), &self.driving, p),
            Block::CheckIns => self.check_in_section(p, cx),
            Block::Repository => ui::locked(self.repository_section(p, cx), &self.driving, p),
            Block::Appearance => self.appearance_section(p, cx),
            Block::Layout => self.layout_section(p, cx),
            Block::OtherMacs => self.other_macs_section(p, cx),
            Block::Paired => self.paired_section(p, cx),
            Block::Folder => self.folder_section(p, cx),
            Block::Builds => self.builds_section(p, cx),
            Block::RemoteAccess => match self.driving.name() {
                None => self.remote_access_section(p, cx),
                Some(host) => hint(
                    format!(
                        "Remote access (pairing and who can control it) changes only at {host} itself."
                    ),
                    p,
                ),
            },
        };
        let follows_a_part = BLOCKS
            .get(block.index().wrapping_sub(1))
            .is_some_and(|before| before.is_part());
        div()
            .flex()
            .flex_col()
            .gap_4()
            .when(!follows_a_part, |d| d.child(rule()))
            .child(section)
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
        appearance::section(p, cx)
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
                "Collapse a working agent's live work to its status line",
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

    /// The assistant's name, which every Mac sharing these settings shows.
    fn assistant_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let name = self.host.as_ref().map(|h| h.assistant_name.clone());
        let control = match self.renaming_assistant {
            true => div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .w(px(200.))
                        .child(Input::new(&self.assistant_input).small()),
                )
                .child(
                    button("save-assistant-name")
                        .primary()
                        .small()
                        .label("Save")
                        .on_click(cx.listener(|v, _, _, c| v.save_assistant_name(c))),
                ),
            false => div()
                .flex()
                .items_center()
                .gap_2()
                .child(name.clone().unwrap_or_default())
                .child(
                    button("rename-assistant")
                        .small()
                        .label("Rename")
                        .disabled(name.is_none())
                        .on_click(cx.listener(move |v, _, w, c| {
                            v.renaming_assistant = true;
                            let current = name.clone().unwrap_or_default();
                            v.assistant_input.update(c, |input, c| {
                                input.set_value(current, w, c);
                                input.focus(w, c);
                            });
                            c.notify();
                        })),
                ),
        };
        let threshold = self.host.as_ref().map(|h| h.router_threshold);
        let mut thresholds = div().flex().gap_1();
        for preset in ROUTER_THRESHOLDS {
            thresholds = thresholds.child(
                segment(
                    SharedString::from(format!("router-threshold-{preset}")),
                    format!("{preset:.1}"),
                    threshold == Some(preset),
                )
                .disabled(threshold.is_none())
                .map(|b| ui::gated(b, &self.driving))
                .on_click(cx.listener(move |v, _, _, c| v.set_router_threshold(preset, c))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("ASSISTANT", p))
            .child(setting(
                "Name",
                control,
                "Shown in the sidebar and the composer. Your other Macs get it with the shared settings.",
                p,
            ))
            .child(setting(
                "Ask below",
                thresholds,
                "When the router is less sure than this where a message goes, it asks you. Your other Macs get it with the shared settings.",
                p,
            ))
            .children(
                (self.assistant_error.clone()).map(|error| hint(error, p).text_color(p.red)),
            )
    }
    fn save_assistant_name(&mut self, cx: &mut Context<Self>) {
        let name = self.assistant_input.read(cx).value().to_string();
        let answer = match self.bridge.ask(ask::set_assistant_name(name)) {
            Ok(answer) => answer,
            Err(error) => {
                self.assistant_error = Some(error);
                return cx.notify();
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |view, cx| {
                match answer {
                    Ok(name) => {
                        view.renaming_assistant = false;
                        view.assistant_error = None;
                        if let Some(host) = &mut view.host {
                            host.assistant_name = name;
                        }
                    }
                    Err(error) => view.assistant_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn set_router_threshold(&mut self, threshold: f64, cx: &mut Context<Self>) {
        let answer = match self.bridge.ask(ask::set_router_threshold(threshold)) {
            Ok(answer) => answer,
            Err(error) => {
                self.assistant_error = Some(error);
                return cx.notify();
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |view, cx| {
                match answer {
                    Ok(settings) => {
                        view.host = Some(settings);
                        view.assistant_error = None;
                    }
                    Err(error) => view.assistant_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
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

/// The scrolled body's children, in order: each group's heading, then its sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Shared,
    Assistant,
    CheckIns,
    Repository,
    App,
    Appearance,
    Layout,
    OtherMacs,
    Paired,
    Host,
    Folder,
    Builds,
    RemoteAccess,
}
const BLOCKS: [Block; 13] = [
    Block::Shared,
    Block::Assistant,
    Block::CheckIns,
    Block::Repository,
    Block::App,
    Block::Appearance,
    Block::Layout,
    Block::OtherMacs,
    Block::Paired,
    Block::Host,
    Block::Folder,
    Block::Builds,
    Block::RemoteAccess,
];
impl Block {
    /// Its place among the body's children, which scrolling to it names.
    fn index(self) -> usize {
        BLOCKS.iter().position(|block| *block == self).unwrap_or(0)
    }
    fn is_part(self) -> bool {
        matches!(self, Self::Shared | Self::App | Self::Host)
    }
}

/// Where a sidebar entry goes: a section of this window, or a page of the main window.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Go {
    Section(Block),
    Page(Page),
    /// A card of the Models page at this Mac's scope.
    Models(ModelsCard),
    /// A card of the Repositories page, or its top with `None`.
    Repositories(Option<RepositoriesCard>),
    Space(SpaceId),
}

/// The sidebar's groups, as the settings-cascade study's Settings window draws them: what
/// cascades from the human's Workspace, each Space, and what is this Mac's alone. Pages that
/// live in the main window open there.
fn sidebar_groups(spaces: &[(SpaceId, String)]) -> Vec<(&'static str, Vec<(String, Go)>)> {
    let entry = |label: &str, to: Go| (label.to_owned(), to);
    let mut space_entries: Vec<(String, Go)> = (spaces.iter())
        .map(|(id, name)| (name.clone(), Go::Space(id.clone())))
        .collect();
    if space_entries.is_empty() {
        space_entries.push(entry("Spaces", Go::Page(Page::Spaces)));
    }
    space_entries.push(entry("Brains", Go::Page(Page::Brains)));
    vec![
        (
            "Shared with your Macs",
            vec![
                entry("Behaviors", Go::Page(Page::Behaviors)),
                entry("Reviews", Go::Models(ModelsCard::Reviews)),
                entry(
                    "PR keeper",
                    Go::Repositories(Some(RepositoriesCard::Keeper)),
                ),
                entry("Check-ins", Go::Section(Block::CheckIns)),
                entry("Assistant & router", Go::Section(Block::Assistant)),
                entry("Settings repository", Go::Section(Block::Repository)),
            ],
        ),
        ("Spaces & brains", space_entries),
        (
            "This Mac",
            vec![
                entry("Who runs what", Go::Models(ModelsCard::WhoRunsWhat)),
                entry("Subscriptions", Go::Models(ModelsCard::Subscriptions)),
                entry("Repositories", Go::Repositories(None)),
                entry(
                    "Allowed owners",
                    Go::Repositories(Some(RepositoriesCard::AllowedOwners)),
                ),
                entry("Workspace folder", Go::Section(Block::Folder)),
                entry("Builds & remote access", Go::Section(Block::Builds)),
                entry("Appearance", Go::Section(Block::Appearance)),
            ],
        ),
    ]
}

/// The Spaces of the host the main window shows, which is the one this window drives.
fn main_window_spaces(cx: &App) -> Vec<(SpaceId, String)> {
    let workspace = (cx.try_global::<MainWindow>())
        .and_then(|main| main.0.clone())
        .and_then(|(_, workspace)| workspace.upgrade());
    let Some(workspace) = workspace else {
        return vec![];
    };
    let snapshot = workspace.read(cx).shared.snapshot();
    (snapshot.map_or(&[][..], |s| &s.spaces).iter())
        .map(|space| (space.id.clone(), space.name.clone()))
        .collect()
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
            .font_family(ui_font())
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
                let mut body = div()
                    .id("settings-body")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4();
                for block in BLOCKS {
                    body = body.child(self.block(block, p, cx));
                }
                let body = div()
                    .size_full()
                    .flex()
                    .child(self.sidebar(p, cx))
                    .child(div().flex_1().min_w_0().h_full().child(body));
                let dialog =
                    (self.confirming).and_then(|confirm| self.confirm_dialog(confirm, p, cx));
                match dialog {
                    Some(dialog) => over_dimmed(body, dialog).flex_1().min_h_0(),
                    None => div().flex_1().min_h_0().relative().child(body),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{BLOCKS, Block, Go, sidebar_groups};
    use crate::{ModelsCard, Page, RepositoriesCard};
    use workspace_core::SpaceId;

    #[test]
    fn the_sidebar_groups_shared_settings_spaces_and_this_macs() {
        let spaces = [
            (SpaceId("sage".into()), "Sage".to_owned()),
            (SpaceId("vyttle".into()), "Vyttle".to_owned()),
        ];
        let groups = sidebar_groups(&spaces);
        let names: Vec<&str> = groups.iter().map(|(group, _)| *group).collect();
        assert_eq!(
            names,
            ["Shared with your Macs", "Spaces & brains", "This Mac"]
        );
        let labels = |at: usize| -> Vec<&str> {
            (groups[at].1.iter())
                .map(|(label, _)| label.as_str())
                .collect()
        };
        assert_eq!(labels(0)[0], "Behaviors");
        assert_eq!(groups[0].1[0].1, Go::Page(Page::Behaviors));
        assert_eq!(labels(1), ["Sage", "Vyttle", "Brains"]);
        assert_eq!(groups[1].1[0].1, Go::Space(SpaceId("sage".into())));
        assert_eq!(groups[1].1[2].1, Go::Page(Page::Brains));
        // Each entry on Models or Repositories opens its own place, so none looks a duplicate.
        let destination = |label: &str| {
            let entries = groups.iter().flat_map(|(_, entries)| entries);
            let found = entries.into_iter().find(|(l, _)| l == label);
            found.map(|(_, to)| to.clone()).unwrap()
        };
        for (label, to) in [
            ("Reviews", Go::Models(ModelsCard::Reviews)),
            ("Who runs what", Go::Models(ModelsCard::WhoRunsWhat)),
            ("Subscriptions", Go::Models(ModelsCard::Subscriptions)),
            (
                "PR keeper",
                Go::Repositories(Some(RepositoriesCard::Keeper)),
            ),
            ("Repositories", Go::Repositories(None)),
            (
                "Allowed owners",
                Go::Repositories(Some(RepositoriesCard::AllowedOwners)),
            ),
        ] {
            assert_eq!(destination(label), to, "{label}");
        }
        // Repository facts and the allowed owners are permissions of this Mac, not shared.
        for this_macs in [
            "Who runs what",
            "Repositories",
            "Allowed owners",
            "Workspace folder",
        ] {
            assert!(
                labels(2).contains(&this_macs),
                "{this_macs} is under This Mac"
            );
            assert!(!labels(0).contains(&this_macs));
        }
        // Each entry for this window scrolls to a section it draws, under its own group.
        let part_of = |block: Block| {
            let before = &BLOCKS[..block.index()];
            *before.iter().rev().find(|b| b.is_part()).unwrap()
        };
        for (group, entries) in &groups {
            for (label, to) in entries {
                if let Go::Section(block) = to {
                    assert!(!block.is_part(), "{label}");
                    let part = if *group == "This Mac" {
                        [Block::App, Block::Host].contains(&part_of(*block))
                    } else {
                        part_of(*block) == Block::Shared
                    };
                    assert!(part, "{label} is drawn under {group}");
                }
            }
        }
        // With no Space yet, the Spaces page is where to add one.
        assert_eq!(
            sidebar_groups(&[])[1].1,
            [
                ("Spaces".to_owned(), Go::Page(Page::Spaces)),
                ("Brains".to_owned(), Go::Page(Page::Brains)),
            ]
        );
    }
}
