//! Project tree, account quota and client appearance.
use super::*;
use crate::background::PendingUpdate;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use ui::{
    badge_ring, bar_color, brand_image, card, eyebrow, hint, hollow_badge, icon, needs_you_badge,
    now, pill, progress_bar, sent_on_badge, status_glyph,
};
use wording::{counted, plural, status_label};

const INDENT: f32 = 14.;

fn inset(depth: usize) -> Pixels {
    px(8. + depth as f32 * INDENT)
}

const MOUSE_BUTTONS: [MouseButton; 5] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
    MouseButton::Navigate(NavigationDirection::Back),
    MouseButton::Navigate(NavigationDirection::Forward),
];

const SCROLL_TICK: std::time::Duration = std::time::Duration::from_millis(16);

/// A project, a parent with its children, being dragged in the tree, drawn beside the pointer,
/// and where it would land. It lives only as long as the drag.
pub(super) struct ProjectDrag {
    moving: tree::Movable,
    target: Option<tree::Drop>,
    /// The other Space whose row the pointer is over, where the dragged row may not land.
    refused_space: Option<SpaceId>,
    /// The pointer has been off the dragged row, so a release back on it is not a click.
    left_row: bool,
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
                    .child(icon("project").size(px(14.))),
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
    sections: &'a [tree::Section<'a>],
    collapsed_parents: &'a BTreeSet<String>,
    moving: Option<tree::Movable>,
    /// Where the dragged row would land now.
    target: Option<tree::Drop>,
}

impl Reorder<'_> {
    /// For the row of `over` itself; nothing while no row is dragged.
    fn onto(&self, over: &tree::Movable) -> Option<Option<tree::Aims>> {
        let moving = self.moving.as_ref()?;
        let aims = tree::aims(self.sections, moving, over, self.collapsed_parents);
        Some(aims)
    }
    /// For the row of `space`, where nothing lands: the Space, when it is not the dragged row's
    /// own; nothing while no row is dragged.
    fn refused(&self, space: &SpaceId) -> Option<Option<SpaceId>> {
        let moving = self.moving.as_ref()?;
        let own = tree::space_of(self.sections, moving) == Some(space);
        Some((!own).then(|| space.clone()))
    }
    /// For the space under the row of `over`, such as the spacer and New ticket button under a
    /// project: all of it aims where the bottom of that row does.
    fn under(&self, over: &tree::Movable) -> Option<Option<tree::Aims>> {
        let aims = self.onto(over)?;
        Some(aims.map(|aims| tree::Aims {
            above: aims.below.clone(),
            onto: aims.below.clone(),
            below: aims.below,
        }))
    }
}

/// Where a project row is drawn.
enum Place<'a> {
    /// In the Outcome tree, where rows are dragged and renamed.
    Tree(&'a Reorder<'a>),
    /// In a lens's bucket, with its muted line beneath it.
    Bucket(Option<String>),
}

struct SectionHead<'a> {
    space: SpaceId,
    name: &'a str,
    /// Its projects' coordinators, for its state while collapsed.
    projects: &'a [&'a Session],
    /// Its projects, or in the Me lens its rows.
    count: usize,
    /// It is the list's first row, with no hairline above it.
    first: bool,
}

/// Tells whatever a project is dragged over where the pointer is, as if it had moved.
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

/// While a project is dragged over a Space's row `element`, hides the insertion line,
/// and shows why when `refused` names that Space because the dragged row belongs to another.
fn refusing(element: Div, refused: Option<Option<SpaceId>>, cx: &mut Context<Sidebar>) -> Div {
    let Some(refused) = refused else {
        return element;
    };
    element.on_drag_move(
        cx.listener(move |v, event: &DragMoveEvent<tree::Movable>, _, c| {
            let pointer = event.event.position;
            if event.bounds.contains(&pointer) && v.tree_scroll.bounds().contains(&pointer) {
                v.aim_at_space_row(refused.clone(), c);
            }
        }),
    )
}

/// While a project is dragged over `element`, aims the drop, or hides the insertion line where
/// the dragged row may not land.
fn aiming<E: InteractiveElement>(
    element: E,
    aims: Option<Option<tree::Aims>>,
    cx: &mut Context<Sidebar>,
) -> E {
    let Some(aims) = aims else {
        return element;
    };
    element.on_drag_move(
        cx.listener(move |v, event: &DragMoveEvent<tree::Movable>, _, c| {
            let pointer = event.event.position;
            // A row scrolled out of sight still has bounds, so ask the tree too.
            if event.bounds.contains(&pointer) && v.tree_scroll.bounds().contains(&pointer) {
                let fraction = (pointer.y - event.bounds.top()) / event.bounds.size.height;
                let target = aims.as_ref().and_then(|aims| aims.at(fraction));
                v.aim_project_drag(target, c);
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
    /// New project's first message, sent to that project's coordinator once it arrives.
    pub(super) pending_first_message: Option<String>,
    pub(super) creating_project: bool,
    pub(super) project_name: Entity<InputState>,
    pub(super) project_edit: Option<project_name::NameEdit>,
    /// What each lens has collapsed. Everything starts expanded at app start, but for the Me
    /// lens, which starts the first time it is drawn with a selection.
    folds: BTreeMap<Lens, lenses::Folds>,
    me_started: bool,
    /// The Me lens's rows, worked out again only for a new snapshot.
    me: Vec<lenses::LensSection>,
    /// The bucket `open_lens_bucket` opened, scrolled into view once it is drawn.
    scroll_to_bucket: Option<(SpaceId, lenses::Bucket)>,
    /// The project being dragged in the sidebar; gone once the drag ends.
    project_drag: Option<WeakEntity<ProjectDrag>>,
    /// The press in progress ended a rename, so it does not also start a drag.
    rename_press: bool,
    /// The click count of the latest press on a project row.
    press_clicks: usize,
    /// The latest press on a project row was on a parent's chevron.
    press_on_chevron: bool,
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
            pending_first_message: None,
            creating_project: false,
            project_name,
            project_edit: None,
            folds: Lens::ALL
                .map(|lens| (lens, lenses::Folds::default()))
                .into(),
            me_started: false,
            me: vec![],
            scroll_to_bucket: None,
            project_drag: None,
            rename_press: false,
            press_clicks: 0,
            press_on_chevron: false,
            drag_scrolling: false,
            tree_scroll: ScrollHandle::new(),
            checking_update: false,
            host_version: None,
            update_card_dismissed: false,
        };
        sidebar.watch_for_updates(window, cx);
        sidebar
    }
    /// Redraws when anything it lists changed, and selects a project it created once the
    /// project arrives.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.shared, shared);
        let (selection, moved) = self.shared.moved_from(&old);
        if moved {
            self.me = (self.shared.index.as_deref())
                .map_or_else(Vec::new, |index| lenses::me_sections(index, now()));
            self.name_new_project(window, cx);
        }
        if selection {
            self.reveal_selected(cx);
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
    /// Opens the collapsed Space and parent of a project selected from elsewhere, such as the
    /// Inbox, so its row shows.
    fn reveal_selected(&mut self, cx: &App) {
        let Some(project_id) = self.shared.selected_session().map(|s| s.project_id.clone()) else {
            return;
        };
        // Assistant work opens from the Assistant, which has no section to expand.
        if self.shared.assistant_work_open() {
            return;
        }
        self.reveal(&project_id, cx);
    }
    /// Opens what hides the project's row in the lens shown; the other lenses keep their folds.
    fn reveal(&mut self, project_id: &str, cx: &App) {
        let lens = cx.global::<Preferences>().lens;
        let Some(index) = &self.shared.index else {
            return;
        };
        if let Some(folds) = self.folds.get_mut(&lens) {
            folds.reveal(index, project_id);
        }
    }
    fn folds(&self, lens: Lens) -> &lenses::Folds {
        &self.folds[&lens]
    }
    fn folds_mut(&mut self, lens: Lens) -> &mut lenses::Folds {
        self.folds.entry(lens).or_default()
    }
    /// The Me lens starts with only the selected project's Space expanded, the first time it is
    /// drawn with the workspace and a selection.
    fn start_me_lens(&mut self, cx: &App) {
        if self.me_started || cx.global::<Preferences>().lens != Lens::Me {
            return;
        }
        let (Some(index), Some(selected)) = (&self.shared.index, self.shared.selected_session())
        else {
            return;
        };
        let folds = lenses::Folds::me(index, &selected.project_id);
        self.folds.insert(Lens::Me, folds);
        self.me_started = true;
    }
    /// Shows `lens` with `space`'s section and its `bucket` expanded and scrolled into view. The
    /// selected conversation stays open.
    pub(super) fn open_lens_bucket(
        &mut self,
        lens: Lens,
        space: SpaceId,
        bucket: lenses::Bucket,
        cx: &mut Context<Self>,
    ) {
        set_lens(lens, cx);
        self.start_me_lens(cx);
        self.folds_mut(lens).open_bucket(&space, bucket.clone());
        self.scroll_to_bucket = Some((space, bucket));
        cx.notify();
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
        self.pending_project_selection = None;
        self.creating_project = false;
        // Its row shows, even under a collapsed parent.
        self.reveal(&root.project_id, cx);
        cx.emit(Intent::Select(root.id.clone()));
        self.tree_scroll.set_offset(point(px(0.), px(0.)));
        if let Some(body) = self.pending_first_message.take() {
            let first = Command::Send {
                id: new_id(),
                sender: None,
                recipient: root.id,
                body,
                attachments: vec![],
                uploads: vec![],
            };
            self.request(first, window, cx);
        }
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
        let archived = self.shared.snapshot().map_or(0, |s| s.archived_sessions);
        let lens = cx.global::<Preferences>().lens;
        let waiting_on_me = lenses::me_count(&self.me);
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
            .child(self.assistant_row(p, cx))
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_1()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(eyebrow(self.shared.host.projects_heading(projects), p))
                    .child(div().flex().gap(px(2.)).when(archived > 0, |d| {
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
                    })),
            )
            .child(lens_control(lens, waiting_on_me, p))
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
                            .map(|list| match lens {
                                Lens::Outcome => list.child(self.tree(p, cx)),
                                Lens::Me if self.shared.index.is_some() => {
                                    list.children(self.lens_rows(lens, &self.me, p, cx))
                                }
                                _ => list.children(self.tag_lens_rows(lens, p, cx)),
                            }),
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

    /// The assistant and Boards, pinned above Projects. The assistant goes by the name the user
    /// gave it, with a working mark while it, its work, an issue thread or a loop run is in a
    /// turn, the count of what its rail lists under Needs you, and a note with Stop for its own
    /// turn and each loop run past 30 minutes. It shows as selected while one of its work
    /// items' details is open. Dragging projects never moves them.
    fn assistant_row(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        use assistant_view::work;
        let snapshot = self.shared.snapshot();
        let index = self.shared.index.as_deref();
        let working = index.is_some_and(work::assistant_working);
        let selected = self.shared.page == Page::Assistant || self.shared.assistant_work_open();
        let jobs = snapshot.map_or(&[][..], |s| &s.jobs);
        let (needs_you, long_turn) = assistant_marks(snapshot, ui::now());
        let mut notes = div().flex().flex_col();
        if let Some(long_turn) = long_turn {
            notes = notes.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(30.))
                    .pr_2()
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(div().flex_1().child(long_turn))
                    .child(
                        button("stop-assistant-long-turn")
                            .ghost()
                            .xsmall()
                            .label("Stop")
                            .on_click(cx.listener(|_, _, _, c| c.emit(Intent::StopAssistantTurn))),
                    ),
            );
        }
        for (name, run, minutes) in assistant_view::jobs::long_runs(jobs) {
            notes = notes.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(30.))
                    .pr_2()
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(
                        div()
                            .flex_1()
                            .child(assistant_view::jobs::long_run_text(&name, minutes)),
                    )
                    .child(
                        button(SharedString::from(format!("stop-long-run-{run}")))
                            .ghost()
                            .xsmall()
                            .label("Stop")
                            .on_click(cx.listener(move |_, _, _, c| {
                                c.emit(Intent::StopJobRun(run.clone()))
                            })),
                    ),
            );
        }
        div()
            .flex_none()
            .px_2()
            .pt_2()
            .pb_1()
            .border_b_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .child(
                self.row("assistant-row".into(), selected, p)
                    .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Assistant))))
                    .child(
                        div()
                            .flex_none()
                            .text_color(if selected { p.text } else { p.subtle })
                            .child(icon("people").size(px(14.))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(assistant_view::assistant_name(snapshot)),
                    )
                    .when(working, |d| {
                        d.child(ui::Spinner::new().xsmall().color(p.accent))
                    })
                    .when(needs_you > 0, |d| d.child(needs_you_badge(needs_you, p))),
            )
            .child(notes)
            .child({
                let selected = self.shared.page == Page::Boards;
                self.row("boards-row".into(), selected, p)
                    .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Boards))))
                    .child(
                        div()
                            .flex_none()
                            .text_color(if selected { p.text } else { p.subtle })
                            .child(icon("book").size(px(14.))),
                    )
                    .child(div().flex_1().child("Boards"))
            })
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
            (Page::Spaces, "Spaces", "spaces"),
            (Page::Issues, "Issues", "list-checks"),
            (Page::Repositories, "Repositories", "folder"),
            (Page::Brains, "Brains", "brain"),
            (Page::Usage, "Usage and limits", "activity"),
            (Page::Models, "Models", "settings"),
            (Page::Behaviors, "Behaviors", "book"),
        ] {
            // The brain browser is a page of Brains.
            let selected = self.shared.page == page
                || (page == Page::Brains && self.shared.page == Page::BrainBrowser);
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
                    // A rewrite out of date waits under the Behaviors page's Advanced.
                    .when(
                        page == Page::Behaviors
                            && skills_view::a_rewrite_is_out_of_date(&self.shared),
                        |d| d.child(div().size(px(6.)).rounded_full().bg(p.accent)),
                    )
                    // Log entries a scan held on this Mac, and alerts from pull scans.
                    .when(page == Page::Brains, |d| {
                        let held = (self.shared.workspace_notes().iter())
                            .filter(|note| {
                                matches!(
                                    note,
                                    WorkspaceNote::HeldEntry(_) | WorkspaceNote::Alert(_)
                                )
                            })
                            .count();
                        d.when(held > 0, |d| {
                            d.child(div().flex_1())
                                .child(ui::pill(held.to_string(), p.yellow))
                        })
                    })
                    .when(page == Page::Issues, |d| {
                        let unread = (self.shared.snapshot())
                            .map_or(0, |s| issues_view::unread(&s.issue_trackers));
                        d.when(unread > 0, |d| {
                            d.child(div().flex_1())
                                .child(ui::pill(format!("{unread} new"), p.accent))
                        })
                    }),
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
                    .item(
                        PopupMenuItem::new("Remote access settings…")
                            .on_click(|_, _, cx| settings::Settings::open_at_remote_access(cx)),
                    )
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
        let sections = tree::sections(index, self.shared.show_archived);
        let drag = self.project_drag.as_ref().and_then(|drag| drag.upgrade());
        let (moving, target, refused_space) = match drag {
            Some(drag) => {
                let drag = drag.read(cx);
                let (moving, target) = (drag.moving.clone(), drag.target.clone());
                (Some(moving), target, drag.refused_space.clone())
            }
            None => (None, None, None),
        };
        let reorder = Reorder {
            sections: &sections,
            collapsed_parents: &self.folds(Lens::Outcome).parents,
            moving,
            target: target.clone(),
        };
        let line = |drop: tree::Drop, depth: usize| {
            (target.as_ref() == Some(&drop)).then(|| insertion_line(depth, p))
        };
        // The last project row, with its depth.
        let mut project = None;
        // Where a drag over the space under the last project row aims.
        let mut under = None;
        // The Space whose projects are being listed, and the expanded parent whose children are.
        let mut space = None;
        let mut parent = None;
        let rows = tree::rows(
            index,
            self.shared.show_archived,
            &self.folds(Lens::Outcome).spaces,
            &self.folds(Lens::Outcome).parents,
        );
        for row in rows {
            // Every row completes the project above it, a top-level row completes the children
            // above it, and a Space's row completes the Space above it.
            let top_level = !matches!(row, tree::Row::Project { child: true, .. });
            if let Some((previous, depth)) = project.take() {
                let under = under.take();
                tree = self.close_project(tree, previous, depth, under, has_tickets, cx);
            }
            if let Some(id) = parent.take_if(|_| top_level) {
                tree = tree.children(line(tree::Drop::EndOfChildren(id), 1));
            }
            match row {
                tree::Row::Space {
                    space: listed,
                    name,
                    projects,
                } => {
                    let previous = space.replace(listed.clone());
                    let first = previous.is_none();
                    let end = previous.and_then(|previous| line(tree::Drop::End(previous), 0));
                    let refused = reorder.refused(&listed);
                    let hint = (refused_space.as_ref() == Some(&listed)).then(|| {
                        hint("Projects move between Spaces from their Overview.", p)
                            .text_size(px(font::META))
                            .mt(px(4.))
                            .ml(inset(0) + px(20.))
                            .mr(px(6.))
                            .px_2()
                            .py(px(4.))
                            .rounded_md()
                            .bg(p.overlay)
                    });
                    let head = SectionHead {
                        space: listed,
                        name: &name,
                        projects: &projects,
                        count: projects.len(),
                        first,
                    };
                    let row = self.space_row(Lens::Outcome, head, p, cx);
                    tree = tree
                        .children(end)
                        .child(refusing(row, refused, cx))
                        .children(hint);
                }
                tree::Row::Project { root, child } => {
                    let depth = usize::from(child);
                    under = reorder.under(&tree::Movable(root.project_id.clone()));
                    tree = tree
                        .children(line(tree::Drop::Before(root.project_id.clone()), depth))
                        .child(self.project_row(root, child, None, Place::Tree(&reorder), p, cx));
                    project = Some((root, depth));
                }
                tree::Row::Parent { root, children } => {
                    under = reorder.under(&tree::Movable(root.project_id.clone()));
                    tree = tree
                        .children(line(tree::Drop::Before(root.project_id.clone()), 0))
                        .child(self.project_row(
                            root,
                            false,
                            Some(&children[..]),
                            Place::Tree(&reorder),
                            p,
                            cx,
                        ));
                    if !self.folds(Lens::Outcome).parents.contains(&root.project_id) {
                        parent = Some(root.project_id.clone());
                    }
                    project = Some((root, 0));
                }
            }
        }
        if let Some((last, depth)) = project {
            tree = self.close_project(tree, last, depth, under, has_tickets, cx);
        }
        if let Some(id) = parent {
            tree = tree.children(line(tree::Drop::EndOfChildren(id), 1));
        }
        tree.children(space.and_then(|space| line(tree::Drop::End(space), 0)))
    }

    fn tag_lens_rows(&self, lens: Lens, p: Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (Some(index), Some(kind)) = (&self.shared.index, lenses::tag_kind(lens)) else {
            return vec![];
        };
        let sections = lenses::tag_sections(index, kind, self.shared.show_archived);
        self.lens_rows(lens, &sections, p, cx)
    }

    /// A lens's rows, each a child of the scrolled list so a bucket can be scrolled to: each
    /// Space's section row, then each bucket's row and its project rows, or its first names
    /// while it is collapsed.
    fn lens_rows(
        &self,
        lens: Lens,
        sections: &[lenses::LensSection],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let folds = self.folds(lens);
        if sections.is_empty() {
            let empty = match lens {
                Lens::Me => "Nothing waits on you.",
                _ => "No projects yet.",
            };
            return vec![div().px_2().py_3().child(hint(empty, p)).into_any_element()];
        }
        let mut rows = vec![];
        for (at, section) in sections.iter().enumerate() {
            let projects = section.projects();
            let head = SectionHead {
                space: section.space.clone(),
                name: &section.name,
                projects: &projects,
                count: match lens {
                    Lens::Me => section.entries(),
                    _ => projects.len(),
                },
                first: at == 0,
            };
            rows.push(self.space_row(lens, head, p, cx).into_any_element());
            if folds.spaces.contains(&section.space) {
                continue;
            }
            for group in &section.groups {
                let bucket = (section.space.clone(), group.bucket.clone());
                if self.scroll_to_bucket.as_ref() == Some(&bucket) {
                    self.tree_scroll.scroll_to_top_of_item(rows.len());
                }
                let folded = folds.buckets.contains(&bucket);
                let id = format!("bucket-{}-{:?}", section.space, group.bucket);
                rows.push(
                    self.bucket_row(lens, bucket, group.projects.len(), folded, p, cx)
                        .into_any_element(),
                );
                let listed = if folded {
                    div()
                        .pl(inset(1) + px(4.))
                        .pr_2()
                        .pb_1()
                        .text_size(px(font::CAPTION))
                        .text_color(p.subtle)
                        .text_ellipsis()
                        .child(lenses::folded_line(&group.projects))
                } else {
                    // A project listed in several buckets keeps its rows apart by the bucket's id.
                    let project_rows = group.projects.iter().map(|listed| {
                        let place = Place::Bucket(listed.line.clone());
                        self.project_row(&listed.root, listed.child, None, place, p, cx)
                    });
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .children(project_rows.collect::<Vec<_>>())
                };
                rows.push(listed.id(SharedString::from(id)).into_any_element());
            }
        }
        rows
    }

    /// A bucket's row: its tag as a chip, or its name in the Me lens, and how many rows it
    /// holds. Activating it folds or unfolds the bucket.
    fn bucket_row(
        &self,
        lens: Lens,
        bucket: (SpaceId, lenses::Bucket),
        count: usize,
        folded: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let label = bucket.1.label(lens);
        let name = if bucket.1.is_tag() {
            let app = lens == Lens::App && matches!(bucket.1, lenses::Bucket::Tag(_));
            tag_chip(label.clone(), app, p)
        } else {
            div().child(label.clone())
        };
        div()
            .id(SharedString::from(format!(
                "bucket-row-{}-{label}",
                bucket.0
            )))
            .min_h(px(28.))
            .mt(px(6.))
            .pl(inset(0) + px(10.))
            .pr_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(font::META))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.subtle)
            .hover(move |d| d.bg(p.overlay.opacity(0.6)))
            .on_click(cx.listener(move |v, _, _, c| v.toggle_bucket(lens, bucket.clone(), c)))
            .child(
                icon(if folded {
                    "chevron-right"
                } else {
                    "chevron-down"
                })
                .size(px(12.)),
            )
            .child(name)
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .font_family(mono_font())
                    .child(count.to_string()),
            )
    }

    /// Makes a project row draggable, and a place other rows are dropped on.
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
        let own = movable.clone();
        let leaving = movable.clone();
        let dragged = reorder.moving.as_ref() == Some(&movable);
        let aims = reorder.onto(&movable);
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
                    refused_space: None,
                    left_row: false,
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
        aiming(row, aims, cx)
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
        let project_id = &row.0;
        if release == tree::OwnRelease::Nothing {
            return;
        }
        if self.press_on_chevron {
            return self.toggle_parent(project_id, cx);
        }
        let root = (self.shared.index.as_ref()).and_then(|i| i.roots(project_id).next());
        if let Some(id) = root.map(|root| root.id.clone()) {
            cx.emit(Intent::Select(id));
            if release == tree::OwnRelease::Rename {
                self.begin_project_rename(project_id.clone(), window, cx);
            }
        }
    }

    fn toggle_parent(&mut self, project_id: &str, cx: &mut Context<Self>) {
        lenses::toggle(
            &mut self.folds_mut(Lens::Outcome).parents,
            project_id.to_owned(),
        );
        cx.notify();
    }

    fn toggle_space(&mut self, lens: Lens, space: SpaceId, cx: &mut Context<Self>) {
        lenses::toggle(&mut self.folds_mut(lens).spaces, space);
        cx.notify();
    }

    fn toggle_bucket(
        &mut self,
        lens: Lens,
        bucket: (SpaceId, lenses::Bucket),
        cx: &mut Context<Self>,
    ) {
        lenses::toggle(&mut self.folds_mut(lens).buckets, bucket);
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
        self.aim(target, None, cx);
    }

    /// Hides the insertion line over a Space's row, and over another Space's row explains why.
    fn aim_at_space_row(&mut self, refused: Option<SpaceId>, cx: &mut Context<Self>) {
        self.aim(None, refused, cx);
    }

    fn aim(
        &mut self,
        target: Option<tree::Drop>,
        refused: Option<SpaceId>,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.project_drag.as_ref().and_then(|drag| drag.upgrade()) else {
            return;
        };
        let aimed = drag.read(cx);
        if aimed.target != target || aimed.refused_space != refused {
            drag.update(cx, |drag, _| {
                drag.target = target;
                drag.refused_space = refused;
            });
            cx.notify();
        }
    }

    /// Asks the host for the move the insertion line shows, or first asks the human to confirm
    /// filing or unfiling the project.
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
        // A drag the human switched lens during lands nowhere: only the Outcome lens drags.
        if cx.global::<Preferences>().lens != Lens::Outcome {
            return;
        }
        let Some(index) = self.shared.index.clone() else {
            return;
        };
        let sections = tree::sections(&index, self.shared.show_archived);
        match tree::landing(&index, &sections, moving, &target) {
            Some(tree::Landing::Move(moved)) => self.move_projects(moved, window, cx),
            Some(tree::Landing::Refile {
                project,
                parent,
                then,
            }) => {
                let named = |id: String| {
                    let name = index
                        .project(&id)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    (id, name)
                };
                self.confirm_parent(&named(project), parent.map(named), then, window, cx)
            }
            None => {}
        }
    }

    fn move_projects(
        &mut self,
        moved: tree::ProjectMove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::move_projects(moved.project_ids, moved.before);
        self.ask(ask, window, cx, |_, projects, window, cx| match projects {
            Ok(projects) => cx.emit(Intent::Amend(Amendment::Projects(projects))),
            Err(error) => toast_error(error, window, cx),
        });
    }

    /// The most urgent state among `projects`, for a collapsed parent or Space, its needs-you
    /// badge `ringed` as a parent's is for its sub-projects' questions.
    fn rollup(&self, projects: &[&Session], ringed: bool, p: Palette) -> Option<AnyElement> {
        let index = self.shared.index.as_deref()?;
        Some(
            match family_state(index, &self.shared.progress, projects)? {
                tree::Rollup::NeedsYou(count) => needs_you_badge(count, p)
                    .when(ringed, |badge| badge_ring(badge, p))
                    .into_any_element(),
                tree::Rollup::Stuck(count) => stuck_pill(count, p).into_any_element(),
                tree::Rollup::Blocked => status_glyph(Status::Blocked, 12., p),
                tree::Rollup::Working => status_glyph(Status::Working, 12., p),
            },
        )
    }

    /// A Space's section row in `lens`: its name and count, and collapsed, the most urgent state
    /// among its projects. Activating it collapses or expands the section. A hairline parts it
    /// from the section above.
    fn space_row(
        &self,
        lens: Lens,
        SectionHead {
            space,
            name,
            projects,
            count,
            first,
        }: SectionHead,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let collapsed = self.folds(lens).spaces.contains(&space);
        let state = collapsed.then(|| self.rollup(projects, false, p)).flatten();
        let row = self
            .row(SharedString::from(format!("space-{space}")), false, p)
            .pl(inset(0) - px(4.))
            .text_color(p.subtle)
            .on_click(cx.listener(move |v, _, _, c| v.toggle_space(lens, space.clone(), c)))
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
            .child(
                eyebrow(name.to_uppercase(), p)
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis(),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(mono_font())
                    .text_size(px(font::META))
                    .child(count.to_string()),
            )
            .children(state.map(|state| div().flex_none().child(state)));
        div()
            .when(!first, |d| {
                d.mt_2()
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(p.edge.opacity(0.6))
            })
            .child(row)
    }

    /// Offers a first ticket under a selected coordinator that has none, then spaces projects.
    fn close_project(
        &self,
        mut tree: Div,
        root: &Session,
        depth: usize,
        under: Option<Option<tree::Aims>>,
        has_tickets: &BTreeMap<String, u32>,
        cx: &mut Context<Self>,
    ) -> Div {
        if !has_tickets.contains_key(&root.id) && self.shared.selected.as_ref() == Some(&root.id) {
            let add = button(SharedString::from(format!("add-ticket-{}", root.id)))
                .ghost()
                .icon(icon("plus"))
                .label("New ticket")
                .ml(inset(1 + depth))
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

    /// A single-line tree row. The selected one has an accent bar on its left edge and a fill
    /// tinted past the faint overlay that hover uses.
    fn row(&self, id: SharedString, selected: bool, p: Palette) -> Stateful<Div> {
        let selected_fill = p.overlay.blend(p.accent.opacity(0.14));
        div()
            .id(id)
            .min_h(px(30.))
            .flex()
            .items_center()
            .gap_2()
            .pl(inset(0))
            .pr_2()
            .rounded_md()
            .text_size(px(font::MESSAGE))
            .when(selected, |d| {
                d.bg(selected_fill).child(
                    div()
                        .absolute()
                        .left(px(2.))
                        .top(px(7.))
                        .bottom(px(7.))
                        .w(px(3.))
                        .rounded_full()
                        .bg(p.accent),
                )
            })
            .text_color(p.text)
            .cursor_pointer()
            .hover(move |d| {
                d.bg(if selected {
                    selected_fill
                } else {
                    p.overlay.opacity(0.6)
                })
            })
    }

    /// A project's row, indented with a branch icon for a `child`. A parent's row, which is given
    /// its shown `children`, has a chevron that collapses them; collapsed, it shows how many and
    /// the most urgent state among it and them. Only the Outcome tree's rows are dragged and
    /// renamed in place, as a lens may list a project more than once.
    fn project_row(
        &self,
        root: &Session,
        child: bool,
        children: Option<&[&Session]>,
        place: Place,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = root.id.clone();
        let project_id = root.project_id.clone();
        let label = root.name.clone();
        let depth = usize::from(child);
        let outcome_parents = &self.folds(Lens::Outcome).parents;
        let collapsed = (children.filter(|_| outcome_parents.contains(&project_id)))
            .map(|children| (children.len(), self.parent_rollup(root, children, p)));
        let index = self.shared.index.as_ref();
        let own = index.and_then(|index| index.project(&project_id));
        let standing = own.is_some_and(|project| project.standing);
        let parent_name = (own.and_then(|project| project.parent_id.as_deref()))
            .and_then(|parent| Some(index?.project(parent)?.name.clone()))
            .unwrap_or_default();
        let with_parent = self
            .shared
            .snapshot()
            .map_or(0, |s| with_parent(s, &project_id));
        let (reorder, beneath) = match place {
            Place::Tree(reorder) => (Some(reorder), None),
            Place::Bucket(beneath) => (None, beneath),
        };
        let renamable = reorder.is_some();
        // The coordinator or any of its agents being open selects the project.
        let selected = self.shared.page == Page::Conversation
            && self
                .shared
                .selected_session()
                .is_some_and(|s| s.project_id == project_id);
        let agents = self.agent_counts(root);
        let edit = (self.project_edit.as_ref()).filter(|e| renamable && e.project_id == project_id);
        let editing = edit.is_some();
        let live = self
            .shared
            .snapshot()
            .is_some_and(|s| s.live_projects.contains(&project_id));
        let parent = own.and_then(|project| project.parent_id.as_deref());
        let badge = (self.shared.snapshot())
            .and_then(|s| badge(&s.attention, &s.workspace_notes, &project_id, parent));
        let detail = format!(
            "{} · {}{}{}{}{}",
            root.name,
            self.status_detail(root),
            if live { " · Running" } else { " · Stopped" },
            match &badge {
                None => String::new(),
                Some(Badge::Answer { count: n, .. }) => {
                    format!(" · {n} {} you", plural(*n, "item needs", "items need"))
                }
                Some(Badge::InParent(n)) => {
                    format!(" · {} for you in {parent_name}", counted(*n, "question"))
                }
            },
            match with_parent {
                0 => String::new(),
                n => format!(" · {} with its parent", counted(n, "question")),
            },
            stuck_label(agents.stuck).map_or(String::new(), |stuck| format!(" · {stuck}")),
        );
        let project = self.shared.progress.project(&project_id);
        let allowed = (self.shared.snapshot())
            .and_then(|s| s.evaluator_permissions.get(&project_id))
            .copied();
        let archived = root.archived;
        let archive_id = id.clone();
        let rename_id = project_id.clone();
        let click_project = project_id.clone();
        let toggled = project_id.clone();
        let movable = tree::Movable(project_id.clone());
        let filing_target =
            reorder.is_some_and(|r| r.target == Some(tree::Drop::Under(project_id.clone())));
        let row = self
            .row(SharedString::from(format!("tree-{id}")), selected, p)
            // The dragged project would be filed under this one.
            .when(filing_target, |d| d.bg(p.accent.opacity(0.18)))
            // Its children wrap, so the progress track and the agents line added last each take
            // their own line under the label.
            .min_h(px(36.))
            .py(px(5.))
            .flex_wrap()
            .gap_y(px(3.))
            // The icon lines up past a parent's chevron.
            .pl(if children.is_some() {
                inset(0) - px(4.)
            } else {
                inset(depth) + px(22.)
            })
            .text_size(px(font::TEXT))
            .when(root.archived, |d| d.opacity(0.55))
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .capture_any_mouse_down(cx.listener(|v, event: &MouseDownEvent, _, _| {
                v.press_clicks = event.click_count;
                v.press_on_chevron = false;
            }))
            .on_click(cx.listener(move |v, event: &ClickEvent, w, c| {
                if !editing {
                    c.emit(Intent::Select(id.clone()));
                    if renamable && event.click_count() == 2 {
                        v.begin_project_rename(click_project.clone(), w, c);
                    }
                }
            }))
            .when(children.is_some(), |row| {
                row.child(
                    div()
                        .id(SharedString::from(format!("toggle-{project_id}")))
                        .flex_none()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(p.subtle)
                        .child(
                            icon(if collapsed.is_some() {
                                "chevron-right"
                            } else {
                                "chevron-down"
                            })
                            .size(px(12.)),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|v, _, _, _| v.press_on_chevron = true),
                        )
                        .on_click(cx.listener(move |v, _, _, c| {
                            c.stop_propagation();
                            v.toggle_parent(&toggled, c);
                        })),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_color(if selected { p.text } else { p.subtle })
                    .child(icon(if child { "git" } else { "project" })),
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
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected { p.text } else { p.subtle })
                            .child(label.clone()),
                    )
                    .when(standing, |d| d.child(pill("standing", p.subtle)))
                    .into_any_element()
            }))
            .when(selected && !editing, |d| {
                let weak = cx.weak_entity();
                // Only the selected row has a menu, so Move under… is worked out once a render.
                let filing = self.filing(&project_id);
                d.child(
                    button(SharedString::from(format!("project-actions-{project_id}")))
                        .ghost()
                        .xsmall()
                        .icon(icon("ellipsis").size(px(14.)))
                        .tooltip("Project actions")
                        .dropdown_menu(move |menu, window, cx| {
                            let rename = weak.clone();
                            let rename_id = rename_id.clone();
                            let add_repositories = weak.clone();
                            let add_ticket = weak.clone();
                            let archive = weak.clone();
                            let archive_id = archive_id.clone();
                            let filed = weak.clone();
                            let filing = filing.clone();
                            let (evaluators, evaluators_project) =
                                (weak.clone(), rename_id.clone());
                            menu.when(renamable, |menu| {
                                menu.item(
                                    PopupMenuItem::new("Rename").icon(icon("rename")).on_click(
                                        move |_, window, cx| {
                                            let _ = rename.update(cx, |view, cx| {
                                                view.begin_project_rename(
                                                    rename_id.clone(),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        },
                                    ),
                                )
                            })
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
                            .when_some(filing, |menu, filing| {
                                filing_items(menu, filed, filing, window, cx)
                            })
                            .submenu("UI evaluators", window, cx, move |menu, _, _| {
                                evaluator_choices(
                                    menu,
                                    evaluators.clone(),
                                    evaluators_project.clone(),
                                    allowed,
                                )
                            })
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
            .when(with_parent > 0, |d| {
                let handling = format!("{parent_name} is handling it");
                d.child(
                    div()
                        .id(SharedString::from(format!("with-parent-{project_id}")))
                        .child(hollow_badge(with_parent, p))
                        .tooltip(move |w, c| Tooltip::new(handling.clone()).build(w, c)),
                )
            })
            .when_some(collapsed.as_ref(), |d, (count, _)| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(mono_font())
                        .text_size(px(font::META))
                        .text_color(p.subtle)
                        .child(format!("+{count}")),
                )
            })
            // Needs you outranks every other state, so it takes the status slot; a collapsed
            // parent's slot holds the most urgent state among it and its children.
            .child(match (collapsed, badge) {
                (Some((_, rollup)), _) => div().flex_none().children(rollup),
                (None, None) => {
                    div()
                        .flex_none()
                        .child(status_glyph(self.shared.shown_status(root), 12., p))
                }
                (
                    None,
                    Some(Badge::Answer {
                        count,
                        from_children,
                    }),
                ) => needs_you_badge(count, p).when(from_children, |badge| badge_ring(badge, p)),
                // Grayed out: the parent sent these on, so the human answers them there.
                (None, Some(Badge::InParent(count))) => {
                    let answer_there = format!("Answer in {parent_name}");
                    div().flex_none().child(
                        div()
                            .id(SharedString::from(format!("in-parent-{project_id}")))
                            .child(sent_on_badge(count, p))
                            .tooltip(move |w, c| Tooltip::new(answer_there.clone()).build(w, c)),
                    )
                }
            })
            .children(beneath.map(|line| {
                div()
                    .w_full()
                    .min_w_0()
                    .pl(px(22.))
                    .text_size(px(font::CAPTION))
                    .text_color(p.subtle)
                    .text_ellipsis()
                    .child(line)
            }))
            .when(!root.archived, |row| {
                row.child(track(
                    project.map(|project| (project.position, project.state)),
                    p,
                ))
            })
            .children(agents_line(&agents, p))
            .children(
                evaluations::sidebar_line(self.shared.evaluations(), &project_id).map(|line| {
                    div()
                        .w_full()
                        .pl(px(22.))
                        .text_size(px(font::CAPTION))
                        .text_color(p.focus)
                        .child(format!("◐ {line}"))
                }),
            );
        match reorder {
            Some(reorder) if !editing => self.reorderable(row, movable, &label, reorder, p, cx),
            _ => row,
        }
    }

    /// What the project menu offers to file `project_id` and mark it standing.
    fn filing(&self, project_id: &str) -> Option<Rc<Filing>> {
        let snapshot = self.shared.snapshot()?;
        let project = snapshot.projects.iter().find(|p| p.id == project_id)?;
        Some(Rc::new(Filing {
            project: (project.id.clone(), project.name.clone()),
            move_under: filing::move_under(snapshot, project),
            standing: project.standing,
        }))
    }

    /// A collapsed parent's state: the most urgent among it and its shown children.
    fn parent_rollup(
        &self,
        parent: &Session,
        children: &[&Session],
        p: Palette,
    ) -> Option<AnyElement> {
        let family: Vec<&Session> = [parent]
            .into_iter()
            .chain(children.iter().copied())
            .collect();
        let snapshot = self.shared.snapshot()?;
        let ringed =
            (snapshot.attention.iter()).any(|a| inbox::escalated_via(a, &parent.project_id));
        self.rollup(&family, ringed, p)
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
        self.start_me_lens(cx);
        let sidebar = self.sidebar(p, cx);
        // Drawing it asked the list to scroll to the bucket, once.
        self.scroll_to_bucket = None;
        sidebar
    }
}

/// What the project menu needs to file a project and mark it standing.
struct Filing {
    /// Its id and name.
    project: (String, String),
    move_under: filing::MoveUnder,
    standing: bool,
}

/// Move under… and Standing in a project's menu.
fn filing_items(
    menu: PopupMenu,
    sidebar: WeakEntity<Sidebar>,
    filing: Rc<Filing>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = match &filing.move_under {
        filing::MoveUnder::Disabled(why) => {
            let why = *why;
            menu.item(
                PopupMenuItem::element(move |_, _| {
                    div()
                        .id("move-under")
                        .child("Move under…")
                        .tooltip(move |w, c| Tooltip::new(why).build(w, c))
                })
                .icon(icon("project"))
                .disabled(true),
            )
        }
        filing::MoveUnder::Choices { .. } => {
            let (sidebar, filing) = (sidebar.clone(), filing.clone());
            menu.submenu_with_icon(
                Some(icon("project")),
                "Move under…",
                window,
                cx,
                move |mut submenu, _, _| {
                    let filing::MoveUnder::Choices { parents, no_parent } = &filing.move_under
                    else {
                        return submenu;
                    };
                    if parents.is_empty() && !no_parent {
                        return submenu.label("No other loose project in this Space");
                    }
                    let choices = (parents.iter().map(|(id, name)| Some((id, name))))
                        .chain(no_parent.then_some(None));
                    for parent in choices {
                        let (sidebar, filing) = (sidebar.clone(), filing.clone());
                        let parent = parent.map(|(id, name)| (id.clone(), name.clone()));
                        let label = parent.as_ref().map_or("No parent".into(), |p| p.1.clone());
                        submenu = submenu.item(PopupMenuItem::new(label).on_click(
                            move |_, window, cx| {
                                let parent = parent.clone();
                                let _ = sidebar.update(cx, |view, cx| {
                                    view.confirm_parent(&filing.project, parent, None, window, cx)
                                });
                            },
                        ));
                    }
                    submenu
                },
            )
        }
    };
    let (project_id, standing) = (filing.project.0.clone(), filing.standing);
    menu.item(
        PopupMenuItem::new("Standing")
            .checked(standing)
            .on_click(move |_, window, cx| {
                let command = Command::SetProjectStanding {
                    project_id: project_id.clone(),
                    standing: !standing,
                };
                let _ = sidebar.update(cx, |view, cx| view.request(command, window, cx));
            }),
    )
}

impl Sidebar {
    /// Asks first, naming the project and its new parent or saying it becomes loose, then
    /// files it and, after a drag, places it where it was dropped with `then`. The host's
    /// refusal shows as a toast; cancelling changes nothing.
    fn confirm_parent(
        &mut self,
        (project_id, project_name): &(String, String),
        parent: Option<(String, String)>,
        then: Option<tree::ProjectMove>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, line) = filing::confirmation(project_name, parent.as_ref().map(|p| &*p.1));
        let (project_id, parent_id) = (project_id.clone(), parent.map(|p| p.0));
        let then = Rc::new(std::cell::Cell::new(then));
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, then) = (weak.clone(), then.clone());
            let (project_id, parent_id) = (project_id.clone(), parent_id.clone());
            dialog
                .title(title.clone())
                .confirm()
                .ok_text("Move")
                .child(line.clone())
                .on_ok(move |_, window, cx| {
                    let ask = ask::set_project_parent(project_id.clone(), parent_id.clone());
                    let then = then.take();
                    let _ = weak.update(cx, |view, cx| {
                        view.ask(ask, window, cx, |view, filed, window, cx| match filed {
                            Ok(_) => {
                                if let Some(moved) = then {
                                    view.move_projects(moved, window, cx);
                                }
                            }
                            Err(error) => toast_error(error, window, cx),
                        })
                    });
                    true
                })
        });
    }
}

/// The lenses under the PROJECTS header: Outcome, App, Layer and Me, which counts what waits on
/// the human across every Space, whichever lens is shown.
fn lens_control(current: Lens, waiting_on_me: usize, p: Palette) -> Div {
    let mut control = div()
        .flex_none()
        .mx_3()
        .mb_1()
        .p(px(2.))
        .flex()
        .gap(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(p.edge)
        .bg(p.base);
    for (at, lens) in Lens::ALL.into_iter().enumerate() {
        let on = lens == current;
        let tooltip = format!("{} · ⌘{}", lens.label(), at + 1);
        let badge = (lens == Lens::Me && waiting_on_me > 0).then(|| {
            div()
                .flex_none()
                .px(px(4.))
                .rounded_full()
                .bg(p.yellow)
                .text_color(p.on_accent)
                .font_weight(FontWeight::BOLD)
                .child(waiting_on_me.to_string())
        });
        let segment = div()
            .id(SharedString::from(format!("lens-{}", lens.label())))
            .flex_1()
            .py(px(3.))
            .flex()
            .justify_center()
            .items_center()
            .gap_1()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(font::META))
            .text_color(if on { p.text } else { p.subtle })
            .when(on, |d| {
                d.bg(p.surface)
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_sm()
            })
            .tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c))
            .child(lens.label())
            .children(badge)
            .on_click(move |_, _, cx| set_lens(lens, cx));
        control = control.child(segment);
    }
    control
}

/// An app or layer, tinted for an app, as the lenses and a project's header show them.
fn tag_chip(label: String, app: bool, p: Palette) -> Div {
    let tint = if app { p.focus } else { p.subtle };
    div()
        .flex_none()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(if app { tint.opacity(0.14) } else { p.overlay })
        .text_color(tint)
        .font_family(mono_font())
        .font_weight(FontWeight::NORMAL)
        .child(label)
}

fn insertion_line(depth: usize, p: Palette) -> Div {
    div().relative().h_0().mt(px(-1.)).child(
        div()
            .absolute()
            .top(px(-1.))
            .left(inset(depth) + px(20.))
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
    (stuck > 0).then(|| format!("{} stuck", counted(stuck, "agent")))
}

/// Yellow, like needs-you: a stuck agent is the one other case the human hears about.
pub(super) fn stuck_pill(stuck: usize, p: Palette) -> Div {
    pill(stuck_label(stuck).unwrap_or_default(), p.yellow)
}

/// A project row's last line: how many of its agents are working, and how many are stuck.
/// None when neither has anything to say.
fn agents_line(agents: &tree::AgentCounts, p: Palette) -> Option<Div> {
    let working =
        (agents.working > 0).then(|| format!("{} working", counted(agents.working, "agent")));
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

/// The project menu's UI evaluators submenu: the human's three choices, the current one checked,
/// and a note while none is made. Only the human changes it, here or on the Inbox card.
fn evaluator_choices(
    mut menu: PopupMenu,
    sidebar: WeakEntity<Sidebar>,
    project_id: String,
    allowed: Option<EvaluatorPermission>,
) -> PopupMenu {
    if allowed.is_none() {
        menu = menu.label("Not chosen: the first run asks");
    }
    for (label, permission) in EvaluatorPermission::CHOICES {
        let (sidebar, project_id) = (sidebar.clone(), project_id.clone());
        menu = menu.item(
            PopupMenuItem::new(label)
                .checked(allowed == Some(permission))
                .on_click(move |_, window, cx| {
                    let command = Command::SetEvaluatorPermission {
                        project_id: project_id.clone(),
                        permission,
                    };
                    let _ = sidebar.update(cx, |view, cx| view.request(command, window, cx));
                }),
        );
    }
    menu
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

/// A project's needs-you badge.
#[derive(Debug, PartialEq)]
pub(super) enum Badge {
    /// Items the human answers here, `from_children` when sub-projects' questions this project
    /// sent on are among them.
    Answer { count: usize, from_children: bool },
    /// Only its own questions its parent sent on, which the human answers there.
    InParent(usize),
}
impl Badge {
    pub(super) fn count(&self) -> usize {
        match *self {
            Self::Answer { count, .. } | Self::InParent(count) => count,
        }
    }
}

/// What a project's badge counts, as its Inbox lists them: its attention items, the
/// sub-projects' questions it sent on, and the workspace notes. None when nothing needs you.
/// Its own questions count as answered in `parent` only when that parent sent them on: one a
/// former parent escalated is still answered here.
pub(super) fn badge(
    attention: &[Attention],
    notes: &[WorkspaceNote],
    project_id: &str,
    parent: Option<&str>,
) -> Option<Badge> {
    let (sent_on, asked_here): (Vec<&Attention>, Vec<&Attention>) = (attention.iter())
        .filter(|a| a.project_id == project_id)
        .partition(|a| parent.is_some_and(|parent| inbox::escalated_via(a, parent)));
    let from_children = (attention.iter())
        .filter(|a| inbox::escalated_via(a, project_id))
        .count();
    let notes = (notes.iter())
        .filter(|note| note.is_for(project_id) && note.needs_you())
        .count();
    match (asked_here.len() + from_children + notes, sent_on.len()) {
        (0, 0) => None,
        (0, sent_on) => Some(Badge::InParent(sent_on)),
        (here, sent_on) => Some(Badge::Answer {
            count: here + sent_on,
            from_children: from_children > 0,
        }),
    }
}

/// What a child's hollow badge counts: its questions waiting with its parent.
fn with_parent(snapshot: &Snapshot, project_id: &str) -> usize {
    (snapshot.questions_with_parent.get(project_id)).map_or(0, |&count| count as usize)
}

/// The most urgent state among `projects`, for a collapsed parent or section: the items that
/// need the human, each once, then their stuck agents, then the status each row shows.
fn family_state(
    index: &WorkspaceIndex,
    board: &progress::ProgressBoard,
    projects: &[&Session],
) -> Option<tree::Rollup> {
    let snapshot = index.snapshot();
    let ids: Vec<&str> = (projects.iter())
        .map(|root| root.project_id.as_str())
        .collect();
    let needs_you = family_needs_you(&snapshot.attention, &snapshot.workspace_notes, &ids);
    let stuck = (projects.iter())
        .map(|root| tree::agent_counts(board, index, root).stuck)
        .sum();
    let shown: Vec<Status> = (projects.iter())
        .map(|root| shown_status(root, index))
        .collect();
    tree::rollup(needs_you, stuck, &shown)
}

/// What the badges of `projects` count together, each item once however many of them it is for.
fn family_needs_you(attention: &[Attention], notes: &[WorkspaceNote], projects: &[&str]) -> usize {
    let attention = (attention.iter()).filter(|a| projects.contains(&a.project_id.as_str()));
    let notes = (notes.iter())
        .filter(|note| note.needs_you() && projects.iter().any(|project| note.is_for(project)));
    attention.count() + notes.count()
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

/// The Assistant row's Needs-you count, what its rail lists there, and the note for its own
/// turn once that has run 30 minutes.
fn assistant_marks(snapshot: Option<&Snapshot>, now: i64) -> (usize, Option<String>) {
    use assistant_view::work;
    let needs_you = snapshot.map_or(0, |s| work::needs_you(s).len());
    let long_turn = (snapshot.and_then(|s| s.conversation.as_ref()))
        .and_then(|c| work::long_turn_minutes(c, now))
        .map(assistant_view::long_turn_text);
    (needs_you, long_turn)
}

#[cfg(test)]
mod tests {
    use super::{
        Badge, assistant_marks, badge, family_needs_you, family_state, shown_status, with_parent,
    };
    use crate::{progress::ProgressBoard, tree};
    use workspace_core::{
        Attention, Paused, Provider, QuestionRoute, Role, Session, Snapshot, Status,
        WorkspaceIndex, WorkspaceNote,
    };

    /// What a project's badge counts.
    fn needs_you(attention: &[Attention], notes: &[WorkspaceNote], project_id: &str) -> usize {
        badge(attention, notes, project_id, None).map_or(0, |badge| badge.count())
    }

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
            owner: None,
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
    fn a_question_sent_on_counts_in_the_parents_badge_and_grays_the_childs() {
        let asked = |id: &str, route: QuestionRoute| Attention {
            id: id.into(),
            project_id: "child".into(),
            session_id: "s".into(),
            host: "claude".into(),
            operation_id: "o".into(),
            prompt: "Q".into(),
            options: Vec::new(),
            answer: None,
            repository_request: None,
            route,
        };
        let sent_on = asked(
            "sent-on",
            QuestionRoute::Escalated {
                parent_id: "parent".into(),
                note: None,
            },
        );
        let items = [sent_on.clone()];
        let child = |items: &[Attention], parent| badge(items, &[], "child", parent);
        assert_eq!(
            badge(&items, &[], "parent", None),
            Some(Badge::Answer {
                count: 1,
                from_children: true
            })
        );
        assert_eq!(child(&items, Some("parent")), Some(Badge::InParent(1)));
        // A collapsed parent counts the question once.
        assert_eq!(family_needs_you(&items, &[], &["parent", "child"]), 1);
        // Moved under another parent, or made loose, the child answers it itself.
        let normal = Some(Badge::Answer {
            count: 1,
            from_children: false,
        });
        assert_eq!(child(&items, Some("another")), normal);
        assert_eq!(child(&items, None), normal);

        // A question of the child's own keeps its badge normal.
        let items = [sent_on, asked("own", QuestionRoute::Human)];
        assert_eq!(
            child(&items, Some("parent")),
            Some(Badge::Answer {
                count: 2,
                from_children: false
            })
        );
        assert_eq!(
            badge(&items, &[], "parent", None).map(|b| b.count()),
            Some(1)
        );
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
            repository_request: None,
            route: Default::default(),
        };
        let note = WorkspaceNote::Paused(workspace_core::PausedSetting {
            path: "skills/implementing/".into(),
            setting: "Implementing".into(),
            paused: Paused::OutsideWiffletree,
            text: String::new(),
            repository: None,
        });
        let items = [attention("a"), attention("b")];
        assert_eq!(needs_you(&items, &[], "a"), 1);
        assert_eq!(needs_you(&items, std::slice::from_ref(&note), "a"), 2);
        assert_eq!(needs_you(&[], std::slice::from_ref(&note), "c"), 1);
        // A brain conflict counts only in the projects its card names.
        let conflict = WorkspaceNote::Conflict(Box::new(workspace_core::ConflictCard {
            repository: "Sage-Social/sagehaven-docs".into(),
            conflict: workspace_core::BrainConflict {
                id: "c-1".into(),
                title: "Retry cap".into(),
                rule: None,
                claim: String::new(),
                conflicts_with: String::new(),
                evidence: String::new(),
                recommend: String::new(),
                entry_id: None,
                repository: None,
            },
            existing: None,
            claim: None,
            projects: vec!["a".into()],
            state: workspace_core::ConflictState::Open,
            recommended: None,
        }));
        assert_eq!(needs_you(&[], std::slice::from_ref(&conflict), "a"), 1);
        assert_eq!(needs_you(&[], std::slice::from_ref(&conflict), "c"), 0);
        // A collapsed parent counts its child's conflict, and a note for several of the family
        // once.
        let family = ["parent", "a"];
        assert_eq!(
            family_needs_you(&items, std::slice::from_ref(&conflict), &family),
            2
        );
        assert_eq!(
            family_needs_you(&[], std::slice::from_ref(&note), &family),
            1
        );
        // Once ruled it waits for a compile, not for you.
        let WorkspaceNote::Conflict(mut ruled) = conflict else {
            unreachable!()
        };
        ruled.state = workspace_core::ConflictState::Ruled {
            ruling: "Keep new".into(),
            by: "mhohlios".into(),
            landed: false,
        };
        let ruled = WorkspaceNote::Conflict(ruled);
        assert_eq!(needs_you(&[], std::slice::from_ref(&ruled), "a"), 0);
    }

    #[test]
    fn the_assistant_row_counts_what_needs_you_and_notes_a_long_turn() {
        use workspace_core::assistant::conversation::{
            CardKind, ConversationCard, ConversationState, Decision,
        };
        let minute = 60_000;
        let decision = ConversationCard {
            id: "shade".into(),
            created_at: 0,
            read: vec![],
            kind: CardKind::Decision(Decision {
                project_id: None,
                question: "Which shade?".into(),
                recommendation: "The lighter one".into(),
                options: vec!["Lighter".into(), "Warmer".into()],
                answer: None,
            }),
        };
        let snapshot = Snapshot {
            conversation: Some(ConversationState {
                session_id: "assistant".into(),
                status: Status::Working,
                turn_started_at: Some(0),
                message_revision: 0,
                profile: None,
                cards: vec![decision],
                routes: vec![],
            }),
            ..Default::default()
        };
        assert_eq!(assistant_marks(None, 0), (0, None));
        assert_eq!(assistant_marks(Some(&snapshot), 29 * minute), (1, None));
        assert_eq!(
            assistant_marks(Some(&snapshot), 31 * minute),
            (1, Some("Working 30 min".into()))
        );
    }

    /// A started parent with one child, and their coordinators.
    fn family() -> (tempfile::TempDir, workspace_host::Host, Session, Session) {
        let directory = tempfile::tempdir().unwrap();
        let mut host = workspace_host::Host::open_set_up(directory.path().join("home")).unwrap();
        host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let parent = host.create_project("Accessible contrast").unwrap();
        let child = host.create_project("Contrast warnings").unwrap();
        for project in [&parent, &child] {
            host.set_live(&project.id, true).unwrap();
        }
        host.set_project_parent(&child.id, Some(&parent.id))
            .unwrap();
        let coordinator = |project: &str| {
            let mut sessions = host.sessions().unwrap().into_iter();
            sessions.find(|s| s.project_id == project).unwrap()
        };
        let (parent, child) = (coordinator(&parent.id), coordinator(&child.id));
        (directory, host, parent, child)
    }

    /// The snapshot, with the child's coordinator working.
    fn with_child_working(host: &workspace_host::Host, child: &Session) -> WorkspaceIndex {
        let mut snapshot = host.snapshot().unwrap();
        let mut sessions = snapshot.sessions.iter_mut();
        sessions.find(|s| s.id == child.id).unwrap().status = Status::Working;
        WorkspaceIndex::new(snapshot, Default::default())
    }

    #[test]
    fn a_collapsed_parent_rolls_up_its_childs_questions_only_once_they_reach_the_human() {
        let (_directory, mut host, parent, child) = family();
        let asked = host
            .agent_tool(
                &child.id,
                "ask_user",
                serde_json::json!({"request_id": "r", "question": "Warn?", "options": []}),
            )
            .unwrap();
        let board = ProgressBoard::default();
        let rollup = |index: &WorkspaceIndex| {
            let family = [&parent, &child].map(|s| index.session(&s.id).unwrap());
            family_state(index, &board, &family)
        };

        // With the parent, the question is the child's hollow badge and the family is working.
        let index = with_child_working(&host, &child);
        assert_eq!(with_parent(index.snapshot(), &child.project_id), 1);
        assert_eq!(with_parent(index.snapshot(), &parent.project_id), 0);
        assert_eq!(rollup(&index), Some(tree::Rollup::Working));

        host.agent_tool(
            &parent.id,
            "escalate_question",
            serde_json::json!({"question_id": asked["id"]}),
        )
        .unwrap();
        let index = with_child_working(&host, &child);
        assert_eq!(with_parent(index.snapshot(), &child.project_id), 0);
        assert_eq!(rollup(&index), Some(tree::Rollup::NeedsYou(1)));
        let attention = &index.snapshot().attention;
        assert_eq!(
            badge(attention, &[], &child.project_id, Some(&parent.project_id)),
            Some(Badge::InParent(1))
        );
        let parents = badge(attention, &[], &parent.project_id, None);
        assert_eq!(parents.map(|b| b.count()), Some(1));
    }
}

#[cfg(test)]
mod lens_tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::Sidebar;
    use crate::bridge::{Bridge, RemoteName};
    use crate::conversation_header::chip_opens;
    use crate::lenses::Bucket;
    use crate::preferences::{Lens, Preferences};
    use crate::shared::{Intent, Shared};
    use crate::test_support::ticket_store;
    use gpui::{AppContext as _, Entity, IntoElement, TestAppContext, VisualTestContext, Window};
    use std::{cell::Cell, rc::Rc};
    use workspace_core::{ProjectTags, Session, TagKind, WorkspaceIndex};

    /// A window that draws nothing, so the sidebar in it needs none of the app's drawing state.
    struct Blank;
    impl gpui::Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            gpui::Empty
        }
    }

    /// A sidebar, never drawn, with preferences kept in `home`.
    fn open<'a>(
        home: &tempfile::TempDir,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Sidebar>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(Preferences::load(home.path()));
        });
        let (_, cx) = cx.add_window_view(|_, _| Blank);
        let sidebar = cx.update(|window, cx| {
            let unreachable = RemoteName {
                id: "away".into(),
                name: "Away".into(),
            };
            cx.new(|cx| Sidebar::new(Bridge::unreachable(unreachable), window, cx))
        });
        (sidebar, cx)
    }

    fn share(
        sidebar: &Entity<Sidebar>,
        index: &Rc<WorkspaceIndex>,
        selected: Option<&Session>,
        cx: &mut VisualTestContext,
    ) {
        let shared = Shared {
            index: Some(index.clone()),
            selected: selected.map(|session| session.id.clone()),
            ..Default::default()
        };
        cx.update(|window, cx| sidebar.update(cx, |s, cx| s.share(shared, window, cx)));
    }

    #[gpui::test]
    fn a_chip_switches_lens_without_changing_the_conversation(cx: &mut TestAppContext) {
        let (_directory, host, root, _) = ticket_store();
        let mut snapshot = host.snapshot().unwrap();
        snapshot.project_tags.insert(
            root.project_id.clone(),
            ProjectTags {
                started: true,
                apps: vec![],
                layers: vec!["frontend".into()],
            },
        );
        let index = Rc::new(WorkspaceIndex::new(snapshot, Rc::default()));
        let home = tempfile::tempdir().unwrap();
        let (sidebar, cx) = open(&home, cx);
        share(&sidebar, &index, Some(&root), cx);
        let selects = Rc::new(Cell::new(false));
        let heard = selects.clone();
        cx.update(|_, cx| {
            cx.subscribe(&sidebar, move |_, intent: &Intent, _| {
                heard.set(heard.get() || matches!(intent, Intent::Select(_)))
            })
            .detach()
        });

        let chip = chip_opens(&index, &root.project_id, TagKind::Layer, "frontend".into());
        let Some(Intent::OpenLensBucket {
            lens,
            space,
            bucket,
        }) = chip
        else {
            panic!("a layer chip opens a bucket");
        };
        assert_eq!(
            (lens, &bucket),
            (Lens::Layer, &Bucket::Tag("frontend".into()))
        );
        cx.update(|_, cx| {
            sidebar.update(cx, |sidebar, cx| {
                let folds = sidebar.folds_mut(lens);
                folds.spaces.insert(space.clone());
                folds.buckets.insert((space.clone(), bucket.clone()));
                sidebar.open_lens_bucket(lens, space.clone(), bucket.clone(), cx);
            })
        });

        cx.update(|_, cx| {
            assert_eq!(cx.global::<Preferences>().lens, Lens::Layer);
            let sidebar = sidebar.read(cx);
            let folds = sidebar.folds(Lens::Layer);
            assert!(folds.spaces.is_empty() && folds.buckets.is_empty());
            assert_eq!(sidebar.scroll_to_bucket, Some((space, bucket)));
            assert_eq!(sidebar.shared.selected.as_deref(), Some(root.id.as_str()));
        });
        assert!(!selects.get(), "the conversation stays selected");
    }

    #[gpui::test]
    fn the_me_lens_starts_its_folds_once_a_project_is_selected(cx: &mut TestAppContext) {
        let (_directory, host, root, _) = ticket_store();
        let index = Rc::new(WorkspaceIndex::new(host.snapshot().unwrap(), Rc::default()));
        let home = tempfile::tempdir().unwrap();
        let (sidebar, cx) = open(&home, cx);
        cx.update(|_, cx| cx.global_mut::<Preferences>().lens = Lens::Me);
        let started = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                sidebar.update(cx, |sidebar, cx| {
                    sidebar.start_me_lens(cx);
                    sidebar.me_started
                })
            })
        };

        share(&sidebar, &index, None, cx);
        assert!(!started(cx), "nothing is selected yet");
        share(&sidebar, &index, Some(&root), cx);
        assert!(started(cx));
    }
}
