//! Project tree, account quota and client appearance.
use super::*;
use gpui_component::{
    Disableable, Selectable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use ui::{
    bar_color, brand_image, card, eyebrow, hint, humanize, icon, needs_you_badge, now, pill,
    progress_bar, session_icon, status_glyph, status_label, ticket_state_color,
};

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
            .text_size(px(13.))
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
    cx: &mut Context<Workspace>,
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

impl Workspace {
    pub(super) fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let dark = is_dark(cx);
        let roots = || {
            self.snapshot
                .iter()
                .flat_map(|s| &s.sessions)
                .filter(|s| s.parent_id.is_none())
        };
        let projects = roots().filter(|s| !s.archived).count();
        let offers_new_project = self.offers_new_project();
        let grouped = cx.global::<Preferences>().group_projects;
        let archived = self
            .snapshot
            .iter()
            .flat_map(|s| &s.sessions)
            .filter(|s| s.archived)
            .count();
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
                            .when(super::data_dir::is_beta(), |d| {
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
                    .child(eyebrow(format!("PROJECTS · {projects}"), p))
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
                                        .label(if self.show_archived {
                                            "Hide archived"
                                        } else {
                                            "Archived"
                                        })
                                        .tooltip(format!("{archived} archived sessions"))
                                        .on_click(cx.listener(|v, _, _, c| {
                                            v.show_archived = !v.show_archived;
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
            .child(self.quota_strip(p, cx))
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
            .border_color(p.edge.opacity(0.35))
            .child(div().px_2().pb_1().child(eyebrow("WORKSPACE", p)));
        for (page, label, glyph) in [
            (Page::Repositories, "Repositories", "folder"),
            (Page::Usage, "Usage and limits", "activity"),
            (Page::Models, "Models", "settings"),
            (Page::Skills, "Skills", "book"),
        ] {
            let selected = self.view == page;
            nav = nav.child(
                self.row(SharedString::from(label), 0, selected, p)
                    .on_click(cx.listener(move |v, _, w, c| v.open_page(page, w, c)))
                    .child(
                        div()
                            .flex_none()
                            .text_color(if selected { p.text } else { p.subtle })
                            .child(icon(glyph).size(px(14.))),
                    )
                    .child(label)
                    // Until a setup is saved, Models is where to go next.
                    .when(page == Page::Models && !self.offers_new_project(), |d| {
                        d.child(div().size(px(6.)).rounded_full().bg(p.accent))
                    })
                    // A rewrite out of date waits on the Skills page.
                    .when(
                        page == Page::Skills && self.a_rewrite_is_out_of_date(),
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
            .border_color(p.edge.opacity(0.35))
            .child(
                match self.update.as_ref().filter(|_| self.update_card_dismissed) {
                    Some(staged) => button("restart-to-update")
                        .primary()
                        .xsmall()
                        .icon(icon("refresh").size(px(13.)))
                        .label("Restart to update")
                        .tooltip(format!("Wiffletree {} is ready", staged.version))
                        .on_click(cx.listener(|v, _, w, c| v.restart_to_update(w, c)))
                        .into_any_element(),
                    None => div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(11.))
                        .text_color(p.subtle)
                        .child(div().flex_none().size(px(6.)).rounded_full().bg(p.green))
                        .child(div().min_w_0().truncate().child("Local workspace"))
                        .into_any_element(),
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
                            .text_size(px(11.))
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

    /// While a settings repository is chosen, how sync is doing; it opens Settings at its section.
    fn sync_mark(&self, p: Palette) -> Option<Stateful<Div>> {
        let snapshot = self.snapshot.as_ref()?;
        let status = snapshot.settings_sync.as_ref()?;
        let (notes, now) = (&snapshot.workspace_notes, now());
        let mark = settings::Mark::of(status, notes.len(), now);
        let tint = mark.tint(p);
        let tooltip = mark.tooltip(status, notes);
        let bridge = self.bridge.clone();
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
                .text_size(px(11.5))
                .text_color(tint.unwrap_or(p.subtle))
                .when_some(tint, |d, tint| d.bg(tint.opacity(0.12)))
                .child(icon(mark.icon()).size(px(13.)))
                .child(mark.label(status, now))
                .tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c))
                .on_click(move |_, _, cx| {
                    settings::Settings::open_at_repository(bridge.clone(), cx)
                }),
        )
    }

    /// What the downloaded release brings, with its changelog and the restart that installs it.
    /// It floats above the footer rather than taking room from the sidebar above it.
    fn update_card(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let staged = self
            .update
            .as_ref()
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
                                        .text_size(px(12.))
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
                                    .text_size(px(12.))
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
                                    .text_size(px(12.))
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
        let Some(snapshot) = &self.snapshot else {
            return tree;
        };
        if snapshot.projects.is_empty() {
            return tree.child(div().px_2().py_3().child(hint(
                if self.offers_new_project() {
                    "No projects yet. Create one to brief a coordinator."
                } else {
                    "Set up your models to create your first project."
                },
                p,
            )));
        }
        let mut has_tickets = std::collections::BTreeSet::new();
        for ticket in &snapshot.tickets {
            has_tickets.insert(ticket.coordinator_id.clone());
        }
        let grouping = cx.global::<Preferences>().group_projects;
        let entries = tree::entries(snapshot, self.show_archived, grouping);
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
        // Where a drag over what is listed under the last project row aims.
        let mut under = None;
        // The group whose projects are being listed, and whether the last project row is in it.
        let mut group = None;
        let mut in_group = false;
        for row in tree::rows(
            snapshot,
            self.show_archived,
            grouping,
            &self.collapsed_projects,
            &self.collapsed_groups,
        ) {
            // A project or group row completes the project above it, and a top-level row
            // completes the group above it.
            let top_level = match &row {
                tree::Row::Group(_) => Some(true),
                tree::Row::Project { grouped, .. } => Some(!grouped),
                tree::Row::Ticket { .. } | tree::Row::Agent { .. } => None,
            };
            if let Some(top_level) = top_level {
                if let Some(previous) = project.take() {
                    let under = under.take();
                    tree = self.close_project(tree, previous, in_group, under, &has_tickets, cx);
                }
                if let Some(key) = group.take_if(|_| top_level) {
                    tree = tree.children(line(tree::Drop::EndOfGroup(key), true));
                }
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
                tree::Row::Ticket { ticket, label } => {
                    let row = self.ticket_row(ticket, label, p, cx);
                    tree = tree.child(aiming(row.pl(inset(1, in_group)), under.clone(), cx));
                }
                tree::Row::Agent {
                    session,
                    depth,
                    label,
                } => {
                    let row = self.session_row(session, depth, label, p, cx);
                    tree = tree.child(aiming(row.pl(inset(depth, in_group)), under.clone(), cx));
                }
            }
        }
        if let Some(last) = project {
            tree = self.close_project(tree, last, in_group, under, &has_tickets, cx);
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
                v.drop_on_row(moving, &own, false, w, c)
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
        on_chevron: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if moving != row {
            return self.drop_project(moving, window, cx);
        }
        let drag = self.project_drag.take().and_then(|drag| drag.upgrade());
        let left_row = drag.is_none_or(|drag| drag.read(cx).left_row);
        let release = tree::own_release(left_row, self.row_press, on_chevron);
        match (row, release) {
            (_, tree::OwnRelease::Nothing) => {}
            (tree::Movable::Group(key), _) => self.toggle_group(key, cx),
            (tree::Movable::Project(project_id), tree::OwnRelease::Toggle) => {
                self.toggle_project(project_id, cx)
            }
            (tree::Movable::Project(project_id), release) => {
                let root = self
                    .snapshot
                    .iter()
                    .flat_map(|s| &s.sessions)
                    .find(|s| s.project_id == *project_id && s.parent_id.is_none());
                if let Some(id) = root.map(|root| root.id.clone()) {
                    self.select(id, window, cx);
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

    fn toggle_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if !self.collapsed_projects.remove(project_id) {
            self.collapsed_projects.insert(project_id.to_owned());
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
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let grouping = cx.global::<Preferences>().group_projects;
        let entries = tree::entries(snapshot, self.show_archived, grouping);
        if let Some(moved) = tree::project_move(&entries, moving, &target) {
            let command = Command::MoveProjects {
                project_ids: moved.project_ids,
                before: moved.before,
            };
            self.request(command, window, cx);
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
                let needs_you = self
                    .snapshot
                    .iter()
                    .flat_map(|s| &s.attention)
                    .filter(|a| group.members.iter().any(|m| m.project_id == a.project_id))
                    .count();
                let shown: Vec<Status> = group
                    .members
                    .iter()
                    .map(|root| self.shown_status(root))
                    .collect();
                tree::rollup(needs_you, &shown)
            })
            .flatten()
            .map(|rollup| match rollup {
                tree::Rollup::NeedsYou(count) => needs_you_badge(count, p).into_any_element(),
                tree::Rollup::Blocked => status_glyph(Status::Blocked, 12., p),
                tree::Rollup::Working => status_glyph(Status::Working, 12., p),
            });
        let toggle = key.clone();
        let row = self
            .row(SharedString::from(format!("group-{key}")), 0, false, p)
            .pl(inset(0, false) - px(4.))
            .text_size(px(12.))
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
                    .text_size(px(10.5))
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
        has_tickets: &std::collections::BTreeSet<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        if !has_tickets.contains(&root.id)
            && !self.collapsed_projects.contains(&root.project_id)
            && self.selected.as_ref() == Some(&root.id)
        {
            let add = button(SharedString::from(format!("add-ticket-{}", root.id)))
                .ghost()
                .icon(icon("plus"))
                .label("New ticket")
                .ml(inset(1, grouped))
                .on_click(cx.listener(|v, _, w, c| v.open_creation(Creation::Ticket, None, w, c)));
            tree = tree.child(aiming(div().flex().child(add), under.clone(), cx));
        }
        tree.child(aiming(div().h_2(), under, cx))
    }

    pub(super) fn shown_status(&self, session: &Session) -> Status {
        let sessions = self.snapshot.as_ref().map_or(&[][..], |s| &s.sessions);
        shown_status(session, sessions)
    }

    fn status_detail(&self, session: &Session) -> String {
        let label = status_label(session.status);
        let mut detail = if self.shown_status(session) == session.status {
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
            .snapshot
            .as_ref()?
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
    fn row(&self, id: SharedString, depth: usize, selected: bool, p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .min_h(px(30.))
            .flex()
            .items_center()
            .gap_2()
            .pl(inset(depth, false))
            .pr_2()
            .rounded_md()
            .text_size(px(12.5))
            .when(selected, |d| d.bg(p.overlay))
            .text_color(if depth >= 2 && !selected {
                p.subtle
            } else {
                p.text
            })
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
        let selected = self.view == Page::Conversation && self.selected.as_ref() == Some(&id);
        let collapsed = self.collapsed_projects.contains(&project_id);
        let edit = self
            .project_edit
            .as_ref()
            .filter(|e| e.project_id == project_id);
        let editing = edit.is_some();
        let live = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.live_projects.contains(&project_id));
        let attention = self
            .snapshot
            .iter()
            .flat_map(|s| &s.attention)
            .filter(|a| a.project_id == project_id)
            .count();
        let detail = format!(
            "{} · {}{}{}",
            root.name,
            self.status_detail(root),
            if live { " · Running" } else { " · Stopped" },
            match attention {
                0 => String::new(),
                1 => " · 1 item needs you".to_owned(),
                n => format!(" · {n} items need you"),
            }
        );
        let project = self.progress.project(&project_id);
        let archived = root.archived;
        let archive_id = id.clone();
        let toggle_id = project_id.clone();
        let rename_id = project_id.clone();
        let click_project = project_id.clone();
        let movable = tree::Movable::Project(project_id.clone());
        let chevron_row = movable.clone();
        let row = self
            .row(SharedString::from(format!("tree-{id}")), 0, selected, p)
            // Its children wrap, so the progress track added last takes its own line under the label.
            .min_h(px(36.))
            .py(px(5.))
            .flex_wrap()
            .gap_y(px(3.))
            .pl(inset(0, grouped) - px(4.))
            .text_size(px(13.))
            .when(root.archived, |d| d.opacity(0.55))
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .capture_any_mouse_down(cx.listener(|v, event: &MouseDownEvent, _, _| {
                v.row_press = tree::RowPress {
                    on_chevron: false,
                    click_count: event.click_count,
                };
            }))
            .on_click(cx.listener(move |v, event: &ClickEvent, w, c| {
                if !editing {
                    v.select(id.clone(), w, c);
                    if event.click_count() == 2 {
                        v.begin_project_rename(click_project.clone(), w, c);
                    }
                }
            }))
            .child(
                div()
                    .id(SharedString::from(format!("collapse-{project_id}")))
                    .flex_none()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_color(p.subtle)
                    .hover(move |d| d.bg(p.edge.opacity(0.5)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|v, _, _, _| v.row_press.on_chevron = true),
                    )
                    .on_click(cx.listener(move |v, _, _, c| {
                        c.stop_propagation();
                        v.toggle_project(&toggle_id, c);
                    }))
                    .on_drop(cx.listener(move |v, moving: &tree::Movable, w, c| {
                        v.drop_on_row(moving, &chevron_row, true, w, c)
                    }))
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
                                    .on_click(move |_, window, cx| {
                                        let _ = add_repositories.update(cx, |view, cx| {
                                            view.open_creation(Creation::Import, None, window, cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("New ticket")
                                    .icon(icon("plus"))
                                    .on_click(move |_, window, cx| {
                                        let _ = add_ticket.update(cx, |view, cx| {
                                            view.open_creation(Creation::Ticket, None, window, cx)
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
                project.and_then(tree::project_figure),
                project.map(|project| project.state),
                p,
            ))
            // Needs you outranks every other state, so it takes the status slot.
            .child(if attention > 0 {
                needs_you_badge(attention, p)
            } else {
                div()
                    .flex_none()
                    .child(status_glyph(self.shown_status(root), 12., p))
            })
            .when(!root.archived, |row| {
                row.child(track(
                    project.map(|project| (project.position, project.state)),
                    p,
                ))
            });
        if editing {
            row
        } else {
            self.reorderable(row, movable, &label, reorder, p, cx)
        }
    }

    fn session_row(
        &self,
        session: &Session,
        depth: usize,
        label: String,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = session.id.clone();
        let selected = self.view == Page::Conversation && self.selected.as_ref() == Some(&id);
        let status = self.shown_status(session);
        let questions = self
            .snapshot
            .as_ref()
            .map_or(0, |s| s.questions_waiting(&session.id));
        let detail = format!(
            "{} · {} · {:?} · {}",
            session.name,
            session.role.label(),
            session.provider,
            self.status_detail(session)
        );
        self.row(SharedString::from(format!("tree-{id}")), depth, selected, p)
            .when(session.archived, |d| d.opacity(0.55))
            // Working agents keep full-strength text; idle ones recede.
            .when(status == Status::Working, |d| d.text_color(p.text))
            .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
            .on_click(cx.listener(move |v, _, w, c| v.select(id.clone(), w, c)))
            .child(
                div()
                    .flex_none()
                    .text_color(if selected { p.text } else { p.subtle })
                    .child(icon(session_icon(session.role)).size(px(14.))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .when(depth == 1, |d| d.font_weight(FontWeight::MEDIUM))
                    .child(label),
            )
            .when_some(self.waiting_input(session), |d, waiting| {
                d.child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(p.subtle)
                        .child(waiting),
                )
            })
            .when_some(tree::questions_figure(questions), |d, questions| {
                d.child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(p.subtle)
                        .child(questions),
                )
            })
            .child(div().flex_none().child(status_glyph(status, 12., p)))
    }

    fn ticket_row(
        &self,
        ticket: &Ticket,
        label: String,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let coordinator = ticket.coordinator_id.clone();
        let waiver = tree::waiver_note(ticket);
        let state = waiver.clone().unwrap_or_else(|| humanize(&ticket.state));
        let waiting = tree::publish_waiting(ticket);
        // A PR or a waiting publish step adds a second line.
        let two_lines = ticket.pull_request.is_some() || waiting.is_some();
        let mut detail = format!("{label} · {state}");
        if let Some(waiting) = &waiting {
            detail.push('\n');
            detail.push_str(waiting);
        }
        if let Some(pr) = &ticket.pull_request {
            detail.push('\n');
            detail.push_str(&tree::pr_sentence(pr));
            if let Some(checked) = self
                .snapshot
                .as_ref()
                .and_then(|s| tree::pr_checked(s, ticket))
            {
                detail.push('\n');
                detail.push_str(&checked);
            }
        }
        // A waived acceptance is never drawn as passed.
        let mark = match waiver {
            Some(_) => icon("waived")
                .size(px(12.))
                .text_color(p.yellow)
                .into_any_element(),
            None => div()
                .size(px(6.))
                .mr(px(3.))
                .rounded_full()
                .bg(ticket_state_color(&ticket.state, p))
                .into_any_element(),
        };
        self.row(
            SharedString::from(format!("ticket-{}", ticket.id)),
            1,
            false,
            p,
        )
        .tooltip(move |w, c| Tooltip::new(detail.clone()).build(w, c))
        .on_click(cx.listener(move |v, _, w, c| {
            v.select(coordinator.clone(), w, c);
            v.show_panel(Panel::Team, w, c);
        }))
        // The icon and mark stay beside the first line.
        .when(two_lines, |row| row.items_start().pt(px(6.)).pb(px(5.)))
        .child(
            div()
                .flex_none()
                .when(two_lines, |d| d.mt(px(1.)))
                .child(icon("task").size(px(14.))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(div().text_ellipsis().text_color(p.text).child(label))
                .children(ticket.pull_request.as_ref().map(|pr| {
                    let keeper = self
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.keeper_parts.get(&ticket.id));
                    let keeper = ticket.try_part().or(keeper.copied());
                    let comments = ticket.waiting_comments();
                    pr_line(pr, ticket.unreviewed(), keeper, comments, p)
                }))
                .children(waiting.map(|waiting| {
                    div()
                        .text_size(px(11.))
                        .text_color(p.yellow)
                        .text_ellipsis()
                        .child(waiting)
                })),
        )
        .child(
            div()
                .flex_none()
                .when(two_lines, |d| d.mt(px(5.)))
                .child(mark),
        )
    }
}

/// Where the dragged row would land. It takes no room, so no row shifts under the pointer.
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
            .text_size(px(11.))
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

/// A ticket row's second line: its PR, muted except for passed or failed checks, a conflict,
/// a merge and commits no review has checked, and the keeper's part at its end.
fn pr_line(
    pr: &PullRequest,
    unreviewed: Option<&Unreviewed>,
    keeper: Option<KeeperPart>,
    comments: Option<&KeeperComments>,
    p: Palette,
) -> Div {
    let glyph = if pr.state == PrState::Merged {
        "git-merge"
    } else {
        "git-pull-request"
    };
    let mut line = div()
        .flex()
        .items_center()
        .gap(px(5.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(11.))
        .text_color(p.subtle)
        .child(icon(glyph).size(px(11.)));
    for (i, part) in tree::pr_line(pr, unreviewed, keeper, comments, now())
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            line = line.child(div().text_color(p.subtle.opacity(0.6)).child("·"));
        }
        let color = match part.tone {
            tree::Tone::Muted => p.subtle,
            tree::Tone::Good => p.green,
            tree::Tone::Bad => p.red,
            tree::Tone::Merged => p.focus,
            tree::Tone::Warn => p.yellow,
            tree::Tone::Busy => p.focus,
        };
        line = line.child(div().text_color(color).child(part.text));
    }
    line
}

/// An idle coordinator shows its team's work, so a collapsed or quiet parent never hides it.
fn shown_status(session: &Session, sessions: &[Session]) -> Status {
    let idle = matches!(session.status, Status::Ready | Status::Done);
    if idle && team_working(&session.id, sessions) {
        Status::Working
    } else {
        session.status
    }
}

fn team_working(coordinator_id: &str, sessions: &[Session]) -> bool {
    let parent_of = |id: &str| {
        sessions
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.parent_id.as_deref())
    };
    let reports_to = |session: &Session| {
        std::iter::successors(session.parent_id.as_deref(), |id| parent_of(id))
            .any(|id| id == coordinator_id)
    };
    sessions
        .iter()
        .any(|s| !s.archived && s.status == Status::Working && reports_to(s))
}

#[cfg(test)]
mod tests {
    use super::shown_status;
    use workspace_core::{Provider, Role, Session, Status};

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
        assert_eq!(shown_status(&sessions[0], &sessions), Status::Working);
        assert_eq!(shown_status(&sessions[1], &sessions), Status::Working);
        assert_eq!(shown_status(&sessions[3], &sessions), Status::Ready);
    }

    #[test]
    fn a_coordinators_own_attention_state_is_not_masked() {
        let mut sessions = [
            session("team", None, Status::Blocked),
            session("implementer", Some("team"), Status::Working),
        ];
        assert_eq!(shown_status(&sessions[0], &sessions), Status::Blocked);

        sessions[0].status = Status::Ready;
        sessions[1].archived = true;
        assert_eq!(shown_status(&sessions[0], &sessions), Status::Ready);
    }
}
