//! Project tree, account quota and client appearance.
use super::*;
use crate::background::PendingUpdate;
use gpui_component::{
    Disableable, Selectable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use ui::{
    bar_color, brand_image, card, eyebrow, hint, icon, needs_you_badge, now, pill, progress_bar,
    status_glyph,
};
use wording::{plural, status_label};

const INDENT: f32 = 14.;

fn inset(depth: usize, grouped: bool) -> Pixels {
    px(8. + (depth + usize::from(grouped)) as f32 * INDENT)
}

const MOUSE_BUTTONS: [MouseButton; 5] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
    MouseButton::Navigate(NavigationDirection::Back),
    MouseButton::Navigate(NavigationDirection::Forward),
];

const SCROLL_TICK: std::time::Duration = std::time::Duration::from_millis(16);

/// A project or group being dragged in the tree, drawn beside the pointer, and where it would
/// land. It lives only as long as the drag.
pub(super) struct ProjectDrag {
    moving: tree::Movable,
    target: Option<tree::Drop>,
    /// The pointer has been off the dragged row, so a release back on it is not a click.
    left_row: bool,
    glyph: &'static str,
    label: SharedString,
    palette: Palette,
}

impl Render for ProjectDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        div()
            .h(px(30.))
            .max_w(px(240.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(p.edge)
            .bg(p.surface)
            .shadow_md()
            .text_size(px(font::TEXT))
            .text_color(p.text)
            .child(
                div()
                    .flex_none()
                    .text_color(p.subtle)
                    .child(icon(self.glyph).size(px(14.))),
            )
            .child(
                div()
                    .min_w_0()
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.label.clone()),
            )
    }
}

/// What a row needs to know about the drag in progress, if any.
struct Reorder<'a> {
    entries: &'a [tree::Entry<'a>],
    moving: Option<tree::Movable>,
}

/// Where the dragged row lands when released over the upper and the lower half of something in
/// the tree; none where it may not land.
type Halves = Option<(tree::Drop, tree::Drop)>;

impl Reorder<'_> {
    /// For the row of `over` itself; nothing while no row is dragged.
    fn onto(&self, over: &tree::Movable) -> Option<Halves> {
        let moving = self.moving.as_ref()?;
        Some(tree::drops(self.entries, moving, over))
    }
    /// For what is listed under the row of `over`, such as a project's tickets and agents: it
    /// all aims where the lower half of that row does.
    fn under(&self, over: &tree::Movable) -> Option<Halves> {
        let halves = self.onto(over)?;
        Some(halves.map(|(_, below)| (below.clone(), below)))
    }
}

/// Tells whatever a project or group is dragged over where the pointer is, as if it had moved.
fn repeat_pointer(window: &mut Window, cx: &mut App) {
    if cx.has_active_drag() {
        let moved = MouseMoveEvent {
            position: window.mouse_position(),
            pressed_button: Some(MouseButton::Left),
            modifiers: window.modifiers(),
        };
        window.dispatch_event(PlatformInput::MouseMove(moved), cx);
    }
}

/// While a project or group is dragged over `element`, aims the drop, or hides the insertion
/// line where the dragged row may not land.
fn aiming<E: InteractiveElement>(
    element: E,
    halves: Option<Halves>,
    cx: &mut Context<Sidebar>,
) -> E {
    let Some(halves) = halves else {
        return element;
    };
    element.on_drag_move(
        cx.listener(move |v, event: &DragMoveEvent<tree::Movable>, _, c| {
            let pointer = event.event.position;
            // A row scrolled out of sight still has bounds, so ask the tree too.
            if event.bounds.contains(&pointer) && v.tree_scroll.bounds().contains(&pointer) {
                let lower = pointer.y > event.bounds.center().y;
                let target = halves
                    .as_ref()
                    .map(|(upper, below)| if lower { below } else { upper });
                v.aim_project_drag(target.cloned(), c);
            }
        }),
    )
}

impl Appearance {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::System => "monitor",
            Self::Light => "sun",
            Self::Dark => "moon",
        }
    }
}

/// The project tree, the workspace pages, the account limits and the update card down the
/// window's left edge.
pub(super) struct Sidebar {
    bridge: Bridge,
    pub(super) shared: Shared,
    /// The project created by New project, until its first snapshot selects and names it.
    pub(super) pending_project_selection: Option<String>,
    pub(super) creating_project: bool,
    pub(super) project_name: Entity<InputState>,
    pub(super) project_edit: Option<project_name::NameEdit>,
    /// Collapsed project groups, by their first word in lowercase.
    collapsed_groups: BTreeSet<String>,
    /// The project or group being dragged in the sidebar; gone once the drag ends.
    project_drag: Option<WeakEntity<ProjectDrag>>,
    /// The press in progress ended a rename, so it does not also start a drag.
    rename_press: bool,
    /// The click count of the latest press on a project row.
    press_clicks: usize,
    /// The tree is scrolling under a row dragged to its edge.
    drag_scrolling: bool,
    tree_scroll: ScrollHandle,
    checking_update: bool,
    /// The host's version as last drawn, to redraw the restart line when it changes.
    host_version: Option<String>,
    /// Hides the update card; the footer keeps a smaller restart button.
    update_card_dismissed: bool,
}
impl EventEmitter<Intent> for Sidebar {}
impl Sidebar {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project_name = cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
        cx.subscribe_in(
            &project_name,
            window,
            |view, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    view.save_project_name(window, cx)
                }
                _ => {}
            },
        )
        .detach();
        let sidebar = Self {
            bridge,
            shared: Shared::default(),
            pending_project_selection: None,
            creating_project: false,
            project_name,
            project_edit: None,
            collapsed_groups: BTreeSet::new(),
            project_drag: None,
            rename_press: false,
            press_clicks: 0,
            drag_scrolling: false,
            tree_scroll: ScrollHandle::new(),
            checking_update: false,
            host_version: None,
            update_card_dismissed: false,
        };
        sidebar.watch_for_updates(window, cx);
        sidebar
    }
    /// Redraws when anything it lists changed, and selects and names a project it created once
    /// the project arrives.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.shared, shared);
        let (selection, moved) = self.shared.moved_from(&old);
        if moved {
            self.name_new_project(window, cx);
        }
        let host_version = self.bridge.host_version();
        let host_moved = host_version != self.host_version;
        self.host_version = host_version;
        let new = &self.shared;
        if moved
            || host_moved
            || selection
            || new.page != old.page
            || new.show_archived != old.show_archived
            || !Rc::ptr_eq(&new.progress, &old.progress)
            || !Rc::ptr_eq(&new.quotas, &old.quotas)
        {
            cx.notify();
        }
    }
    fn name_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = (self.pending_project_selection.as_ref()).and_then(|project| {
            (self.shared.index.iter())
                .flat_map(|index| index.sessions())
                .find(|s| &s.project_id == project && s.role == Role::ProjectOrchestrator)
        });
        let Some(root) = root.cloned() else {
            return;
        };
        let project_id = self.pending_project_selection.take().unwrap();
        self.creating_project = false;
        // Naming it needs its row, which a collapsed group would hide.
        self.collapsed_groups.remove(&tree::group_key(&root.name));
        cx.emit(Intent::Select(root.id));
        self.tree_scroll.set_offset(point(px(0.), px(0.)));
        self.begin_project_rename(project_id, window, cx);
    }
    /// Checks for a newer release now and hourly while the window is open. The app installs a
    /// staged one when it exits.
    fn watch_for_updates(&self, window: &mut Window, cx: &mut Context<Self>) {
        if update::VERSION.is_none() || update::installed_bundle().is_none() {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let checked = cx.update(|window, cx| {
                    this.update(cx, |view, cx| view.check_for_updates(false, window, cx))
                });
                if checked.is_err() {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(60 * 60))
                    .await;
            }
        })
        .detach();
    }
    /// Downloads and stages a newer release. A `manual` check also reports when there is none.
    pub(super) fn check_for_updates(
        &mut self,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if staged(cx).is_some() {
            if manual {
                self.update_card_dismissed = false;
                cx.notify();
            }
            return;
        }
        let (Some(current), Some(installed)) = (update::VERSION, update::installed_bundle()) else {
            if manual {
                window.push_notification(
                    Notification::info(
                        "This build can't update itself. Install a release to get updates.",
                    ),
                    cx,
                );
            }
            return;
        };
        if self.checking_update {
            return;
        }
        self.checking_update = true;
        cx.notify();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(update::check(current, &installed));
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = receiver.recv().await else {
                return;
            };
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.checking_update = false;
                    match result {
                        Ok(Some(staged)) => {
                            cx.global_mut::<PendingUpdate>().0 = Some(staged);
                            view.update_card_dismissed = false;
                        }
                        Ok(None) if manual => toast(
                            format!("Wiffletree {current} is the latest version"),
                            window,
                            cx,
                        ),
                        Ok(None) => {}
                        Err(error) if manual => window.push_notification(
                            Notification::error(format!("{error:#}"))
                                .title("Couldn’t check for updates"),
                            cx,
                        ),
                        Err(error) => eprintln!("Update check failed: {error:#}"),
                    }
                    cx.notify();
                })
            });
        })
        .detach();
    }
    /// Restarts into the staged release, confirming first if agents would be interrupted. A
    /// launchd host keeps them working through the app's restart.
    fn restart_to_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let working = !self.bridge.outlives_app()
            && self
                .shared
                .index
                .iter()
                .flat_map(|index| index.sessions())
                .any(|s| s.status == Status::Working);
        if !working {
            return self.install_update(window, cx);
        }
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .title("Restart to update?")
                .confirm()
                .ok_text("Restart")
                .child("Agents are working. Restarting interrupts their turns; you can retry the held input afterwards.")
                .on_ok(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.install_update(window, cx));
                    true
                })
        });
    }
    fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(staged) = cx.global_mut::<PendingUpdate>().0.take() else {
            return;
        };
        match staged.install_after_exit(true) {
            Ok(()) => crate::background::end_app(cx),
            Err(error) => {
                cx.global_mut::<PendingUpdate>().0 = Some(staged);
                toast_error(format!("{error:#}"), window, cx);
            }
        }
    }
    /// After an update, an older launchd host runs on until no agent works, then restarts into
    /// this version; the client asked it to as it connected.
    fn host_restart_line(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let older = self.host_version.as_deref()?;
        if !self.bridge.outlives_app() || older == build_version() {
            return None;
        }
        Some(
            div()
                .flex_none()
                .px_4()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .border_t_1()
                .border_color(p.edge.opacity(edge::LIGHT))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(font::CAPTION))
                        .text_color(p.subtle)
                        .child(format!(
                            "The host restarts into {} once no agent is working",
                            build_version()
                        )),
                )
                .child(
                    button("restart-host-now")
                        .xsmall()
                        .label("Restart now")
                        .on_click(cx.listener(|v, _, w, c| v.restart_host_now(w, c))),
                ),
        )
    }
    /// Restarts the host into this version now, which stops running turns as a quit does.
    fn restart_host_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let bridge = bridge.clone();
            dialog
                .title("Restart the host now?")
                .confirm()
                .ok_text("Restart now")
                .child("Running turns stop, as when Wiffletree quits; you can retry their held input afterwards.")
                .on_ok(move |_, window, cx| {
                    restart_host(bridge.clone(), RestartWhen::Now, window, cx);
                    true
                })
        });
    }

    fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let dark = is_dark(cx);
        // The snapshot lists every session that is not archived.
        let projects = self.shared.snapshot().map_or(0, |s| hosts::counts(s).0);
        let offers_new_project = self.shared.offers_new_project();
        let grouped = cx.global::<Preferences>().group_projects;
        let archived = self.shared.snapshot().map_or(0, |s| s.archived_sessions);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .pl_4()
                    .pr_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(brand_image(
                                if dark {
                                    "brand/symbol-dark.svg"
                                } else {
                                    "brand/symbol-light.svg"
                                },
                                28.,
                                14.,
                            ))
                            .child(brand_image(
                                if dark {
                                    "brand/wordmark-dark.svg"
                                } else {
                                    "brand/wordmark-light.svg"
                                },
                                112.,
                                21.,
                            ))
                            .when(workspace_core::data_dir::is_beta(), |d| {
                                d.child(pill("BETA", p.accent))
                            }),
                    )
                    .child(
                        button("new-project")
                            .ghost()
                            .icon(icon("plus"))
                            .tooltip(if offers_new_project {
                                "New project · ⌘N"
                            } else {
                                "New project is available once your models are set up"
                            })
                            .when(!offers_new_project, |d| d.opacity(0.4))
                            .disabled(self.creating_project)
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(|v, _, w, c| v.new_project(w, c))),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_1()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(eyebrow(self.shared.host.projects_heading(projects), p))
                    .child(
                        div()
                            .flex()
                            .gap(px(2.))
                            .child(
                                button("group-projects")
                                    .ghost()
                                    .xsmall()
                                    .icon(icon("folder").size(px(13.)))
                                    .label("Group")
                                    .selected(grouped)
                                    .tooltip("Group projects by first word")
                                    .on_click(cx.listener(move |_, _, _, c| {
                                        set_group_projects(!grouped, c);
                                        c.notify();
                                    })),
                            )
                            .when(archived > 0, |d| {
                                d.child(
                                    button("show-archived")
                                        .ghost()
                                        .xsmall()
                                        .icon(icon("archive").size(px(13.)))
                                        .label(if self.shared.show_archived {
                                            "Hide archived"
                                        } else {
                                            "Archived"
                                        })
                                        .tooltip(format!("{archived} archived sessions"))
                                        .on_click(cx.listener(|v, _, _, c| {
                                            c.emit(Intent::ShowArchived(!v.shared.show_archived));
                                            c.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("tree-scroll")
                            .track_scroll(&self.tree_scroll)
                            .size_full()
                            .overflow_y_scroll()
                            .px_2()
                            // Room for an insertion line above the first row.
                            .pt(px(2.))
                            .pb_3()
                            .on_drag_move(cx.listener(
                                |v, event: &DragMoveEvent<tree::Movable>, w, c| {
                                    if !event.bounds.contains(&event.event.position) {
                                        v.aim_project_drag(None, c);
                                    }
                                    v.scroll_under_drag(w, c);
                                },
                            ))
                            .on_drop(cx.listener(|v, moving: &tree::Movable, w, c| {
                                v.drop_project(moving, w, c)
                            }))
                            // With those for a release elsewhere, this hears every release.
                            .capture_any_mouse_up(cx.listener(|v, _, _, c| v.end_rename_press(c)))
                            .map(|tree| {
                                MOUSE_BUTTONS.into_iter().fold(tree, |tree, button| {
                                    tree.on_mouse_up_out(
                                        button,
                                        cx.listener(|v, _, _, c| v.end_rename_press(c)),
                                    )
                                })
                            })
                            .child(self.tree(p, cx)),
                    )
                    .vertical_scrollbar(&self.tree_scroll),
            )
            .child(self.workspace_nav(p, cx))
            .child(quotas::quota_strip(&self.shared.quotas, p, cx))
            .children(self.host_restart_line(p, cx))
            .child(
                self.sidebar_footer(p, cx)
                    .relative()
                    .children(self.update_card(p, cx)),
            )
    }

    /// Pages about the whole workspace, kept apart from the per-session tools on the right.
    fn workspace_nav(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut nav = div()
            .flex_none()
            .px_2()
            .py_2()
            .flex()
            .flex_col()
            .gap(px(1.))
            .border_t_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .child(eyebrow(self.shared.host.workspace_heading(), p)),
            );
        for (page, label, glyph) in [
            (Page::Repositories, "Repositories", "folder"),
            (Page::Usage, "Usage and limits", "activity"),
            (Page::Models, "Models", "settings"),
            (Page::Skills, "Skills", "book"),
        ] {
            let selected = self.shared.page == page;
            nav = nav.child(
                self.row(SharedString::from(label), selected, p)
                    .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::OpenPage(page))))
                    .child(
                        div()
                            .flex_none()
                            .text_color(if selected { p.text } else { p.subtle })
                            .child(icon(glyph).size(px(14.))),
                    )
                    .child(label)
                    // Until a setup is saved, Models is where to go next.
                    .when(
                        page == Page::Models && !self.shared.offers_new_project(),
                        |d| d.child(div().size(px(6.)).rounded_full().bg(p.accent)),
                    )
                    // A rewrite out of date waits on the Skills page.
                    .when(
                        page == Page::Skills && skills_view::a_rewrite_is_out_of_date(&self.shared),
                        |d| d.child(div().size(px(6.)).rounded_full().bg(p.accent)),
                    ),
            );
        }
        nav
    }

    fn sidebar_footer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .pl_4()
            .pr_3()
            .py_2()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .child(
                match staged(cx).cloned().filter(|_| self.update_card_dismissed) {
                    Some(staged) => button("restart-to-update")
                        .primary()
                        .xsmall()
                        .icon(icon("refresh").size(px(13.)))
                        .label("Restart to update")
                        .tooltip(format!("Wiffletree {} is ready", staged.version))
                        .on_click(cx.listener(|v, _, w, c| v.restart_to_update(w, c)))
                        .into_any_element(),
                    None => self.host_switcher(p, cx).into_any_element(),
                },
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(self.sync_mark(p))
                    .child(
                        div()
                            .id("version")
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_size(px(font::CAPTION))
                            .text_color(p.subtle)
                            .hover(|d| d.text_color(p.text))
                            .child(if self.checking_update {
                                "Checking…".to_owned()
                            } else {
                                version_label()
                            })
                            .tooltip(|w, c| Tooltip::new("Check for updates").build(w, c))
                            .on_click(cx.listener(|v, _, w, c| v.check_for_updates(true, w, c))),
                    )
                    .child(self.appearance_menu(cx)),
            )
    }

    /// The footer's host line, which opens the switcher: This Mac and each paired host with its
    /// state and what needs the human there, then Connect a computer… and Remote access
    /// settings….
    fn host_switcher(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let host = &self.shared.host;
        let dot = match host.remote.as_ref().map(|r| r.link) {
            None | Some(hosts::Link::Connected(_)) => p.green,
            Some(hosts::Link::Reconnecting) => p.yellow,
            Some(hosts::Link::Offline) => p.subtle,
            Some(hosts::Link::TurnedAway(_)) => p.red,
        };
        let hosts = cx.global::<Hosts>();
        let (projects, needs_you) = match (host.remote.as_ref(), self.shared.snapshot()) {
            (None, Some(snapshot)) => hosts::counts(snapshot),
            // Elsewhere, This Mac's host still counts what needs the human there.
            _ => {
                let (projects, counted) = hosts.this_mac_counts;
                (projects, hosts.this_mac.needs_you().unwrap_or(counted))
            }
        };
        let rows = hosts::switcher_rows(
            projects,
            needs_you,
            &hosts.remote_hosts(),
            &hosts.selected,
            now(),
        );
        button("host-switcher")
            .ghost()
            .xsmall()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(font::CAPTION))
                    .text_color(p.subtle)
                    .child(div().flex_none().size(px(6.)).rounded_full().bg(dot))
                    .child(div().min_w_0().truncate().child(hosts::footer_line(host)))
                    .child(icon("chevron-up").size(px(12.))),
            )
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, _| {
                for row in &rows {
                    let (shown, choice) = (row.clone(), row.choice.clone());
                    let item = PopupMenuItem::element(move |_, _| switcher_row(&shown, p))
                        .checked(row.selected)
                        .on_click(move |_, window, cx| select_host(choice.clone(), window, cx));
                    menu = menu.item(item);
                }
                menu.separator()
                    .item(
                        PopupMenuItem::new("Connect a computer…")
                            .icon(icon("plus"))
                            .on_click(|_, window, cx| connect_sheet::open(window, cx)),
                    )
                    .item(PopupMenuItem::new("Remote access settings…").on_click(
                        |_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx),
                    ))
            })
    }

    /// While a settings repository is chosen, how sync is doing; it opens Settings at its section.
    fn sync_mark(&self, p: Palette) -> Option<Stateful<Div>> {
        let snapshot = self.shared.snapshot()?;
        let status = snapshot.settings_sync.as_ref()?;
        let (notes, now) = (
            snapshot.paused_settings().cloned().collect::<Vec<_>>(),
            now(),
        );
        let mark = settings::Mark::of(status, notes.len(), now);
        let tint = mark.tint(p);
        let tooltip = mark.tooltip(status, &notes);
        Some(
            div()
                .id("sync-mark")
                .flex()
                .items_center()
                .gap_1()
                .px(px(7.))
                .py(px(2.))
                .rounded_md()
                .cursor_pointer()
                .text_size(px(font::DETAIL))
                .text_color(tint.unwrap_or(p.subtle))
                .when_some(tint, |d, tint| d.bg(tint.opacity(0.12)))
                .child(icon(mark.icon()).size(px(13.)))
                .child(mark.label(status, now))
                .tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c))
                .on_click(move |_, _, cx| settings::Settings::open_at_repository(cx)),
        )
    }

    /// What the downloaded release brings, with its changelog and the restart that installs it.
    /// It floats above the footer rather than taking room from the sidebar above it.
    fn update_card(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let staged = staged(cx)
            .cloned()
            .filter(|_| !self.update_card_dismissed)?;
        let page = staged.page.clone();
        Some(
            div()
                .absolute()
                .bottom(relative(1.))
                .left_0()
                .right_0()
                .px_3()
                .pb_2()
                .child(
                    card(p)
                        .bg(p.surface)
                        .shadow_lg()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(font::SMALL))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("Wiffletree {} is ready", staged.version)),
                                )
                                .child(
                                    button("dismiss-update")
                                        .ghost()
                                        .xsmall()
                                        .icon(icon("close").size(px(12.)))
                                        .tooltip("Later")
                                        .on_click(cx.listener(|v, _, _, c| {
                                            v.update_card_dismissed = true;
                                            c.notify();
                                        })),
                                ),
                        )
                        .when_some(staged.summary.clone(), |d, summary| {
                            d.child(
                                div()
                                    .text_size(px(font::SMALL))
                                    .line_height(relative(1.45))
                                    .text_color(p.subtle)
                                    .line_clamp(4)
                                    .child(summary),
                            )
                        })
                        // A row keeps the link's hit area to its text rather than the card's width.
                        .child(
                            div().flex().child(
                                div()
                                    .id("changelog")
                                    .cursor_pointer()
                                    .text_size(px(font::SMALL))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.accent)
                                    .hover(|d| d.underline())
                                    .child("Changelog →")
                                    .on_click(move |_, _, cx| cx.open_url(&page)),
                            ),
                        )
                        .child(
                            div().flex().child(
                                button("install-update")
                                    .primary()
                                    .small()
                                    .flex_1()
                                    .icon(icon("refresh").size(px(13.)))
                                    .label("Restart to update")
                                    .on_click(cx.listener(|v, _, w, c| v.restart_to_update(w, c))),
                            ),
                        ),
                ),
        )
    }

    fn appearance_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = cx.global::<Preferences>().appearance;
        button("appearance")
            .ghost()
            .icon(icon(current.icon()))
            .tooltip("Appearance")
            .dropdown_menu(move |mut menu, _, _| {
                for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
                    menu = menu.item(
                        PopupMenuItem::new(appearance.label())
                            .icon(icon(appearance.icon()))
                            .checked(appearance == current)
                            .on_click(move |_, _, cx| set_appearance(appearance, cx)),
                    );
                }
                menu
            })
    }

    fn tree(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut tree = div().flex().flex_col().gap(px(1.));
        let Some(index) = &self.shared.index else {
            return tree;
        };
        if index.snapshot().projects.is_empty() {
            return tree.child(div().px_2().py_3().child(hint(
                if self.shared.offers_new_project() {
                    "No projects yet. Create one to brief a coordinator."
                } else {
                    "Set up your models to create your first project."
                },
                p,
            )));
        }
        let has_tickets = &index.snapshot().tickets_owned;
        let grouping = cx.global::<Preferences>().group_projects;
        let entries = tree::entries(index, self.shared.show_archived, grouping);
        let drag = self.project_drag.as_ref().and_then(|drag| drag.upgrade());
        let (moving, target) = drag
            .map(|drag| {
                let drag = drag.read(cx);
                (drag.moving.clone(), drag.target.clone())
            })
            .unzip();
        let target = target.flatten();
        let reorder = Reorder {
            entries: &entries,
            moving,
        };
        let line = |drop: tree::Drop, grouped: bool| {
            (target.as_ref() == Some(&drop)).then(|| insertion_line(grouped, p))
        };
        let mut project = None;
        // Where a drag over the space under the last project row aims.
        let mut under = None;
        // The group whose projects are being listed, and whether the last project row is in it.
        let mut group = None;
        let mut in_group = false;
        let rows = tree::rows(
            index,
            self.shared.show_archived,
            grouping,
            &self.collapsed_groups,
        );
        for row in rows {
            // Every row completes the project above it, and a top-level row completes the
            // group above it.
            let top_level = match &row {
                tree::Row::Group(_) => true,
                tree::Row::Project { grouped, .. } => !grouped,
            };
            if let Some(previous) = project.take() {
                let under = under.take();
                tree = self.close_project(tree, previous, in_group, under, has_tickets, cx);
            }
            if let Some(key) = group.take_if(|_| top_level) {
                tree = tree.children(line(tree::Drop::EndOfGroup(key), true));
            }
            match row {
                tree::Row::Group(listed) => {
                    let movable = tree::Movable::Group(listed.key.clone());
                    tree = tree
                        .children(line(tree::Drop::Before(movable.clone()), false))
                        .child(self.group_row(&listed, &reorder, p, cx));
                    if self.collapsed_groups.contains(&listed.key) {
                        tree = tree.child(aiming(div().h_2(), reorder.under(&movable), cx));
                    }
                    group = Some(listed.key);
                }
                tree::Row::Project {
                    root,
                    label,
                    grouped,
                } => {
                    let movable = tree::Movable::Project(root.project_id.clone());
                    under = reorder.under(&movable);
                    tree = tree
                        .children(line(tree::Drop::Before(movable), grouped))
                        .child(self.project_row(root, label, grouped, &reorder, p, cx));
                    project = Some(root);
                    in_group = grouped;
                }
            }
        }
        if let Some(last) = project {
            tree = self.close_project(tree, last, in_group, under, has_tickets, cx);
        }
        if let Some(key) = group {
            tree = tree.children(line(tree::Drop::EndOfGroup(key), true));
        }
        tree.children(line(tree::Drop::End, false))
    }

    /// Makes a project or group row draggable, and a place other rows are dropped on.
    fn reorderable(
        &self,
        row: Stateful<Div>,
        movable: tree::Movable,
        label: &str,
        reorder: &Reorder,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        if self.rename_press {
            return row;
        }
        let glyph = match movable {
            tree::Movable::Project(_) => "project",
            tree::Movable::Group(_) => "folder",
        };
        let own = movable.clone();
        let leaving = movable.clone();
        let dragged = reorder.moving.as_ref() == Some(&movable);
        let halves = reorder.onto(&movable);
        let workspace = cx.weak_entity();
        let label = SharedString::from(label.to_owned());
        let row = row
            .when(dragged, |row| row.opacity(0.4))
            .on_drag(movable, move |moving, _, window, cx| {
                // GPUI tells no one of the movement that starts a drag, which may already have
                // left the row or reached the list's edge.
                window.defer(cx, repeat_pointer);

                let drag = cx.new(|_| ProjectDrag {
                    moving: moving.clone(),
                    target: None,
                    left_row: false,
                    glyph,
                    label: label.clone(),
                    palette: p,
                });
                let started = drag.downgrade();
                let _ = workspace.update(cx, |view, cx| {
                    view.project_drag = Some(started);
                    cx.notify();
                });
                drag
            })
            .on_drag_move(
                cx.listener(move |v, event: &DragMoveEvent<tree::Movable>, _, c| {
                    if *event.drag(c) == leaving && !event.bounds.contains(&event.event.position) {
                        v.project_drag_left_row(c);
                    }
                }),
            )
            .on_drop(cx.listener(move |v, moving: &tree::Movable, w, c| {
                v.drop_on_row(moving, &own, w, c)
            }));
        aiming(row, halves, cx)
    }

    fn project_drag_left_row(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.project_drag.as_ref().and_then(|drag| drag.upgrade()) {
            drag.update(cx, |drag, _| drag.left_row = true);
        }
    }

    /// A row dropped on another moves. GPUI turns a click that moves a few pixels into a drag,
    /// so one released on its own row without having left it does what the click would have.
    fn drop_on_row(
        &mut self,
        moving: &tree::Movable,
        row: &tree::Movable,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if moving != row {
            return self.drop_project(moving, window, cx);
        }
        let drag = self.project_drag.take().and_then(|drag| drag.upgrade());
        let left_row = drag.is_none_or(|drag| drag.read(cx).left_row);
        let release = tree::own_release(left_row, self.press_clicks);
        match (row, release) {
            (_, tree::OwnRelease::Nothing) => {}
            (tree::Movable::Group(key), _) => self.toggle_group(key, cx),
            (tree::Movable::Project(project_id), release) => {
                let root = (self.shared.index.as_ref()).and_then(|i| i.roots(project_id).next());
                if let Some(id) = root.map(|root| root.id.clone()) {
                    cx.emit(Intent::Select(id));
                    if release == tree::OwnRelease::Rename {
                        self.begin_project_rename(project_id.clone(), window, cx);
                    }
                }
            }
        }
    }

    fn toggle_group(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed_groups.remove(key) {
            self.collapsed_groups.insert(key.to_owned());
        }
        cx.notify();
    }

    /// Scrolls the tree for as long as a dragged row is held near its top or bottom edge.
    fn scroll_under_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag_scrolling || self.drag_scroll_step(window) == 0. {
            return;
        }
        self.drag_scrolling = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(SCROLL_TICK).await;
                let scrolled = cx.update(|window, cx| {
                    // The pointer rests while rows move under it, so the insertion line follows
                    // them only if they hear where it is.
                    repeat_pointer(window, cx);
                    this.update(cx, |view, cx| view.scroll_tree(window, cx))
                });
                if !matches!(scrolled, Ok(Ok(true))) {
                    break;
                }
            }
            let _ = this.update(cx, |view, _| view.drag_scrolling = false);
        })
        .detach();
    }

    fn drag_scroll_step(&self, window: &Window) -> f32 {
        let (bounds, pointer) = (self.tree_scroll.bounds(), window.mouse_position());
        if !bounds.contains(&pointer) {
            return 0.;
        }
        tree::edge_scroll(
            pointer.y.into(),
            bounds.top().into(),
            bounds.bottom().into(),
        )
    }

    /// One tick of scrolling under a dragged row; false once the drag has ended or left the
    /// edge.
    fn scroll_tree(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let dragging = self.project_drag.as_ref().and_then(|drag| drag.upgrade());
        let step = self.drag_scroll_step(window);
        if dragging.is_none() || step == 0. {
            return false;
        }
        let mut offset = self.tree_scroll.offset();
        let end = -self.tree_scroll.max_offset().y;
        offset.y = (offset.y - px(step)).max(end).min(px(0.));
        self.tree_scroll.set_offset(offset);
        cx.notify();
        true
    }

    fn end_rename_press(&mut self, cx: &mut Context<Self>) {
        if self.rename_press {
            self.rename_press = false;
            cx.notify();
        }
    }

    /// Moves the insertion line; `None` hides it, and a drop then moves nothing.
    fn aim_project_drag(&mut self, target: Option<tree::Drop>, cx: &mut Context<Self>) {
        let Some(drag) = self.project_drag.as_ref().and_then(|drag| drag.upgrade()) else {
            return;
        };
        if drag.read(cx).target != target {
            drag.update(cx, |drag, _| drag.target = target);
            cx.notify();
        }
    }

    /// Asks the host for the move the insertion line shows.
    fn drop_project(
        &mut self,
        moving: &tree::Movable,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = self.project_drag.take().and_then(|drag| drag.upgrade());
        let Some(target) = drag.and_then(|drag| drag.read(cx).target.clone()) else {
            return;
        };
        let Some(index) = &self.shared.index else {
            return;
        };
        let grouping = cx.global::<Preferences>().group_projects;
        let entries = tree::entries(index, self.shared.show_archived, grouping);
        if let Some(moved) = tree::project_move(&entries, moving, &target) {
            let ask = ask::move_projects(moved.project_ids, moved.before);
            self.ask(ask, window, cx, |_, projects, window, cx| match projects {
                Ok(projects) => cx.emit(Intent::Amend(Amendment::Projects(projects))),
                Err(error) => toast_error(error, window, cx),
            });
        }
    }

    /// A group's row: activating it collapses or expands the group. Collapsed, it shows the most
    /// urgent state among its projects.
    fn group_row(
        &self,
        group: &tree::Group,
        reorder: &Reorder,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let key = group.key.clone();
        let collapsed = self.collapsed_groups.contains(&key);
        let count = group.members.len();
        let detail = format!("{count} projects starting with “{}”", group.label);
        let state = collapsed
            .then(|| {
                let needs_you = (self.shared.snapshot().iter())
                    .flat_map(|s| &s.attention)
                    .filter(|a| group.members.iter().any(|m| m.project_id == a.project_id))
                    .count();
                let stuck = (group.members.iter())
                    .map(|root| self.agent_counts(root).stuck)
                    .sum();
                let shown: Vec<Status> = group
                    .members
                    .iter()
                    .map(|root| self.shared.shown_status(root))
                    .collect();
                tree::rollup(needs_you, stuck, &shown)
            })
            .flatten()
            .map(|rollup| match rollup {
                tree::Rollup::NeedsYou(count) => needs_you_badge(count, p).into_any_element(),
                tree::Rollup::Stuck(count) => stuck_pill(count, p).into_any_element(),
                tree::Rollup::Blocked => status_glyph(Status::Blocked, 12., p),
                tree::Rollup::Working => status_glyph(Status::Working, 12., p),
            });
        let toggle = key.clone();
        let row = self
            .row(SharedString::from(format!("group-{key}")), false, p)
            .pl(inset(0, false) - px(4.))
            .text_size(px(font::SMALL))
            .text_color(p.subtle)
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .on_click(cx.listener(move |v, _, _, c| v.toggle_group(&toggle, c)))
            .child(
                div()
                    .flex_none()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        icon(if collapsed {
                            "chevron-right"
                        } else {
                            "chevron-down"
                        })
                        .size(px(12.)),
                    ),
            )
            .child(div().flex_none().child(icon("folder").size(px(14.))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .font_weight(FontWeight::MEDIUM)
                    .child(group.label.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(".SystemMonoFont")
                    .text_size(px(font::META))
                    .child(count.to_string()),
            )
            .children(state.map(|state| div().flex_none().child(state)));
        let movable = tree::Movable::Group(key);
        self.reorderable(row, movable, &group.label, reorder, p, cx)
    }

    /// Offers a first ticket under a selected coordinator that has none, then spaces projects.
    fn close_project(
        &self,
        mut tree: Div,
        root: &Session,
        grouped: bool,
        under: Option<Halves>,
        has_tickets: &BTreeMap<String, u32>,
        cx: &mut Context<Self>,
    ) -> Div {
        if !has_tickets.contains_key(&root.id) && self.shared.selected.as_ref() == Some(&root.id) {
            let add = button(SharedString::from(format!("add-ticket-{}", root.id)))
                .ghost()
                .icon(icon("plus"))
                .label("New ticket")
                .ml(inset(1, grouped))
                .on_click(cx.listener(|_, _, _, c| {
                    c.emit(Intent::Create {
                        kind: Creation::Ticket,
                        ticket: None,
                    })
                }));
            tree = tree.child(aiming(div().flex().child(add), under.clone(), cx));
        }
        tree.child(aiming(div().h_2(), under, cx))
    }

    fn status_detail(&self, session: &Session) -> String {
        let label = status_label(session.status);
        let mut detail = if self.shared.shown_status(session) == session.status {
            label.to_owned()
        } else {
            format!("{label} · Team working")
        };
        if let Some(waiting) = self.waiting_input(session) {
            detail = format!("{detail} · {waiting}");
        }
        detail
    }

    /// Names input a session is not running: held while it is stopped or after an interrupted
    /// turn, or queued while it is idle.
    fn waiting_input(&self, session: &Session) -> Option<&'static str> {
        let input = self
            .shared
            .snapshot()?
            .undelivered
            .iter()
            .find(|u| u.session_id == session.id)?;
        let stopped = matches!(session.status, Status::Paused | Status::Disconnected);
        let idle = matches!(session.status, Status::Ready | Status::Done);
        if input.held > 0 || (stopped && input.queued > 0) {
            Some("held")
        } else if idle && input.queued > 0 {
            Some("queued input")
        } else {
            None
        }
    }

    /// A single-line tree row.
    fn row(&self, id: SharedString, selected: bool, p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .min_h(px(30.))
            .flex()
            .items_center()
            .gap_2()
            .pl(inset(0, false))
            .pr_2()
            .rounded_md()
            .text_size(px(font::MESSAGE))
            .when(selected, |d| d.bg(p.overlay))
            .text_color(p.text)
            .cursor_pointer()
            .hover(move |d| d.bg(p.overlay.opacity(if selected { 1. } else { 0.6 })))
    }

    /// `label` is the name the row shows; a grouped row's leaves out the group's word.
    fn project_row(
        &self,
        root: &Session,
        label: String,
        grouped: bool,
        reorder: &Reorder,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = root.id.clone();
        let project_id = root.project_id.clone();
        // The coordinator or any of its agents being open selects the project.
        let selected = self.shared.page == Page::Conversation
            && self
                .shared
                .selected_session()
                .is_some_and(|s| s.project_id == project_id);
        let agents = self.agent_counts(root);
        let edit = self
            .project_edit
            .as_ref()
            .filter(|e| e.project_id == project_id);
        let editing = edit.is_some();
        let live = self
            .shared
            .snapshot()
            .is_some_and(|s| s.live_projects.contains(&project_id));
        let attention = self.shared.snapshot().map_or(0, |s| {
            needs_you(&s.attention, &s.workspace_notes, &project_id)
        });
        let detail = format!(
            "{} · {}{}{}{}",
            root.name,
            self.status_detail(root),
            if live { " · Running" } else { " · Stopped" },
            match attention {
                0 => String::new(),
                n => format!(" · {n} {} you", plural(n, "item needs", "items need")),
            },
            stuck_label(agents.stuck).map_or(String::new(), |stuck| format!(" · {stuck}")),
        );
        let project = self.shared.progress.project(&project_id);
        let archived = root.archived;
        let archive_id = id.clone();
        let rename_id = project_id.clone();
        let click_project = project_id.clone();
        let movable = tree::Movable::Project(project_id.clone());
        let row = self
            .row(SharedString::from(format!("tree-{id}")), selected, p)
            // Its children wrap, so the progress track and the agents line added last each take
            // their own line under the label.
            .min_h(px(36.))
            .py(px(5.))
            .flex_wrap()
            .gap_y(px(3.))
            // The icon lines up with a group row's, past its chevron.
            .pl(inset(0, grouped) + px(22.))
            .text_size(px(font::TEXT))
            .when(root.archived, |d| d.opacity(0.55))
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .capture_any_mouse_down(cx.listener(|v, event: &MouseDownEvent, _, _| {
                v.press_clicks = event.click_count;
            }))
            .on_click(cx.listener(move |v, event: &ClickEvent, w, c| {
                if !editing {
                    c.emit(Intent::Select(id.clone()));
                    if event.click_count() == 2 {
                        v.begin_project_rename(click_project.clone(), w, c);
                    }
                }
            }))
            .child(
                div()
                    .flex_none()
                    .text_color(if selected { p.text } else { p.subtle })
                    .child(icon("project")),
            )
            .child(div().flex_1().min_w_0().child(if let Some(edit) = edit {
                div()
                    .w_full()
                    .py_1()
                    .on_mouse_down_out(cx.listener(|v, _, w, c| {
                        v.rename_press = true;
                        v.save_project_name(w, c);
                        c.notify();
                    }))
                    .on_action(cx.listener(|v, _: &gpui_component::input::Escape, w, c| {
                        v.cancel_project_name(w, c);
                        c.stop_propagation();
                    }))
                    .child(
                        Input::new(&self.project_name)
                            .small()
                            .w_full()
                            .disabled(edit.saving),
                    )
                    .when(!edit.error.is_empty(), |d| {
                        d.child(hint(edit.error.clone(), p).text_color(p.red))
                    })
                    .into_any_element()
            } else {
                div()
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label.clone())
                    .into_any_element()
            }))
            .when(selected && !editing, |d| {
                let weak = cx.weak_entity();
                d.child(
                    button(SharedString::from(format!("project-actions-{project_id}")))
                        .ghost()
                        .xsmall()
                        .icon(icon("ellipsis").size(px(14.)))
                        .tooltip("Project actions")
                        .dropdown_menu(move |menu, _, _| {
                            let rename = weak.clone();
                            let rename_id = rename_id.clone();
                            let add_repositories = weak.clone();
                            let add_ticket = weak.clone();
                            let archive = weak.clone();
                            let archive_id = archive_id.clone();
                            menu.item(PopupMenuItem::new("Rename").icon(icon("rename")).on_click(
                                move |_, window, cx| {
                                    let _ = rename.update(cx, |view, cx| {
                                        view.begin_project_rename(rename_id.clone(), window, cx)
                                    });
                                },
                            ))
                            .item(
                                PopupMenuItem::new("Add repositories…")
                                    .icon(icon("folder"))
                                    .on_click(move |_, _, cx| {
                                        let _ = add_repositories.update(cx, |_, cx| {
                                            cx.emit(Intent::Create {
                                                kind: Creation::Import,
                                                ticket: None,
                                            })
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("New ticket")
                                    .icon(icon("plus"))
                                    .on_click(move |_, _, cx| {
                                        let _ = add_ticket.update(cx, |_, cx| {
                                            cx.emit(Intent::Create {
                                                kind: Creation::Ticket,
                                                ticket: None,
                                            })
                                        });
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new(if archived {
                                    "Restore project"
                                } else {
                                    "Archive project"
                                })
                                .icon(icon("archive"))
                                .on_click(move |_, window, cx| {
                                    let _ = archive.update(cx, |view, cx| {
                                        view.set_archived(archive_id.clone(), !archived, window, cx)
                                    });
                                }),
                            )
                        }),
                )
            })
            .children(figure(
                project.and_then(wording::project_figure),
                project.map(|project| project.state),
                p,
            ))
            // Needs you outranks every other state, so it takes the status slot.
            .child(if attention > 0 {
                needs_you_badge(attention, p)
            } else {
                div()
                    .flex_none()
                    .child(status_glyph(self.shared.shown_status(root), 12., p))
            })
            .when(!root.archived, |row| {
                row.child(track(
                    project.map(|project| (project.position, project.state)),
                    p,
                ))
            })
            .children(agents_line(&agents, p));
        if editing {
            row
        } else {
            self.reorderable(row, movable, &label, reorder, p, cx)
        }
    }

    fn agent_counts(&self, coordinator: &Session) -> tree::AgentCounts {
        let index = self.shared.index.as_deref();
        index.map_or_else(Default::default, |index| {
            tree::agent_counts(&self.shared.progress, index, coordinator)
        })
    }
}

/// Where the dragged row would land. It takes no room, so no row shifts under the pointer.
impl Requests for Sidebar {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        _: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = reply {
            toast_error(error, window, cx);
        }
    }
}
impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Sidebar");
        let p = palette(is_dark(cx));
        self.sidebar(p, cx)
    }
}

fn insertion_line(grouped: bool, p: Palette) -> Div {
    div().relative().h_0().mt(px(-1.)).child(
        div()
            .absolute()
            .top(px(-1.))
            .left(inset(0, grouped) + px(20.))
            .right(px(6.))
            .h(px(2.))
            .rounded_full()
            .bg(p.accent),
    )
}

/// The short progress figure before a row's status glyph, red when blocked and green when
/// finished.
fn figure(text: Option<String>, state: Option<BarState>, p: Palette) -> Option<Div> {
    let color = bar_color(state?, p.subtle, p);
    Some(
        div()
            .flex_none()
            .text_size(px(font::CAPTION))
            .text_color(color)
            .child(text?),
    )
}

/// The thin progress track under a row's label. A row with nothing to show keeps an empty
/// track, so its height never depends on its progress.
fn track(progress: Option<(Option<u16>, BarState)>, p: Palette) -> Div {
    let (position, state) = progress.unwrap_or((None, BarState::InProgress));
    progress_bar(position, state, 3., p)
}

/// "1 agent stuck", "3 agents stuck"; none when no agent is.
fn stuck_label(stuck: usize) -> Option<String> {
    (stuck > 0).then(|| format!("{stuck} {} stuck", plural(stuck, "agent", "agents")))
}

/// Yellow, like needs-you: a stuck agent is the one other case the human hears about.
fn stuck_pill(stuck: usize, p: Palette) -> Div {
    pill(stuck_label(stuck).unwrap_or_default(), p.yellow)
}

/// A project row's last line: how many of its agents are working, and how many are stuck.
/// None when neither has anything to say.
fn agents_line(agents: &tree::AgentCounts, p: Palette) -> Option<Div> {
    let working = (agents.working > 0).then(|| {
        let noun = plural(agents.working, "agent", "agents");
        format!("{} {noun} working", agents.working)
    });
    if working.is_none() && agents.stuck == 0 {
        return None;
    }
    Some(
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            // Under the label, clear of the icon.
            .pl(px(22.))
            .text_size(px(font::CAPTION))
            .text_color(p.subtle)
            .children(working)
            .when(agents.stuck > 0, |d| d.child(stuck_pill(agents.stuck, p))),
    )
}

/// An idle coordinator shows its team's work, so a quiet parent never hides it.
pub(super) fn shown_status(session: &Session, index: &WorkspaceIndex) -> Status {
    let idle = matches!(session.status, Status::Ready | Status::Done);
    if idle && index.team_working(&session.id) {
        Status::Working
    } else {
        session.status
    }
}

/// A host in the switcher: its name over its state, and what needs the human there, with a
/// connected host's network delay in its tooltip.
fn switcher_row(row: &hosts::SwitcherRow, p: Palette) -> Stateful<Div> {
    let tooltip = row.tooltip.clone();
    div()
        .id(SharedString::from(format!("host-row-{}", row.name)))
        .when_some(tooltip, |d, tooltip| {
            d.tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c))
        })
        .min_w(px(240.))
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().child(row.name.clone()))
                .child(
                    div()
                        .truncate()
                        .text_size(px(font::CAPTION))
                        .text_color(p.subtle)
                        .child(row.detail.clone()),
                ),
        )
        .when(row.needs_you > 0, |d| {
            d.child(needs_you_badge(row.needs_you, p))
        })
}

/// What a project's badge counts: its attention items, and the workspace notes, as the Inbox rail does.
fn needs_you(attention: &[Attention], notes: &[WorkspaceNote], project_id: &str) -> usize {
    attention
        .iter()
        .filter(|a| a.project_id == project_id)
        .count()
        + notes.len()
}

/// The release the app has downloaded, waiting for it to exit.
fn staged(cx: &App) -> Option<&update::Staged> {
    cx.global::<PendingUpdate>().0.as_ref()
}

/// Asks the host to restart off the main thread, and says so if it could not.
pub(crate) fn restart_host(bridge: Bridge, when: RestartWhen, window: &mut Window, cx: &mut App) {
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send_blocking(bridge.restart_host(when));
    });
    window
        .spawn(cx, async move |cx| {
            if let Ok(Err(error)) = receiver.recv().await {
                let _ = cx.update(|window, cx| {
                    toast_error(format!("The host did not restart: {error}"), window, cx)
                });
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::{needs_you, shown_status};
    use workspace_core::{
        Attention, Paused, Provider, Role, Session, Snapshot, Status, WorkspaceIndex, WorkspaceNote,
    };

    fn index(sessions: &[Session]) -> WorkspaceIndex {
        let sessions = sessions.to_vec();
        let snapshot = Snapshot {
            sessions,
            ..Default::default()
        };
        WorkspaceIndex::new(snapshot, Default::default())
    }

    fn session(id: &str, parent: Option<&str>, status: Status) -> Session {
        Session {
            id: id.into(),
            project_id: "project".into(),
            parent_id: parent.map(Into::into),
            repository_id: None,
            name: id.into(),
            role: Role::Implementer,
            provider: Provider::Claude,
            status,
            archived: false,
        }
    }

    #[test]
    fn idle_coordinators_show_work_anywhere_below_them() {
        let sessions = [
            session("main", None, Status::Ready),
            session("team", Some("main"), Status::Done),
            session("implementer", Some("team"), Status::Working),
            session("other-team", Some("main"), Status::Ready),
        ];
        let index = index(&sessions);
        assert_eq!(shown_status(&sessions[0], &index), Status::Working);
        assert_eq!(shown_status(&sessions[1], &index), Status::Working);
        assert_eq!(shown_status(&sessions[3], &index), Status::Ready);
    }

    #[test]
    fn a_coordinators_own_attention_state_is_not_masked() {
        let mut sessions = [
            session("team", None, Status::Blocked),
            session("implementer", Some("team"), Status::Working),
        ];
        assert_eq!(
            shown_status(&sessions[0], &index(&sessions)),
            Status::Blocked
        );

        sessions[0].status = Status::Ready;
        sessions[1].archived = true;
        assert_eq!(shown_status(&sessions[0], &index(&sessions)), Status::Ready);
    }

    #[test]
    fn workspace_notes_count_in_each_projects_badge() {
        let attention = |project: &str| Attention {
            id: project.into(),
            project_id: project.into(),
            session_id: "s".into(),
            host: "claude".into(),
            operation_id: "o".into(),
            prompt: "Q".into(),
            options: Vec::new(),
            answer: None,
        };
        let note = WorkspaceNote::Paused(workspace_core::PausedSetting {
            path: "skills/implementing/".into(),
            setting: "Implementing".into(),
            paused: Paused::OutsideWiffletree,
            text: String::new(),
        });
        let items = [attention("a"), attention("b")];
        assert_eq!(needs_you(&items, &[], "a"), 1);
        assert_eq!(needs_you(&items, std::slice::from_ref(&note), "a"), 2);
        assert_eq!(needs_you(&[], std::slice::from_ref(&note), "c"), 1);
    }
}
