//! Choose repositories… (OpenSpec change `choose-space-repositories`, screens 2 to 5 of its
//! mockup): draws a `Tree` and hands the choice to the Space's form on Done. Nothing is written
//! until the form's Save.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
};
use repository_tree::{NOT_CLONED, Row, ShownFolder, Tick, Tree};
use spaces_view::{SpacesPage, space_color, space_mark};
use ui::{hint, icon};

/// Tree height, as drawn.
const TREE_HEIGHT: f32 = 430.;
/// How far a row sits in from its folder's edge, past the chevron and tick.
const ROW_INDENT: f32 = 38.;

pub(super) struct RepositoryChooser {
    page: WeakEntity<SpacesPage>,
    tree: Tree,
    /// Every Space, for the chips and their colors.
    spaces: Vec<Space>,
    filter: Entity<InputState>,
    typed: Entity<InputState>,
    error: Option<String>,
}

/// Opens the dialog over `window`, for the Space named `name`.
pub(super) fn open(
    page: WeakEntity<SpacesPage>,
    name: &str,
    tree: Tree,
    spaces: Vec<Space>,
    window: &mut Window,
    cx: &mut App,
) {
    let chooser = cx.new(|cx| RepositoryChooser::new(page, tree, spaces, window, cx));
    let title = format!("Code repositories for {name}");
    window.open_dialog(cx, move |dialog, window, _| {
        dialog
            .title(title.clone())
            .width(px(640.).min(window.viewport_size().width - px(48.)))
            .child(chooser.clone())
    });
}

impl RepositoryChooser {
    fn new(
        page: WeakEntity<SpacesPage>,
        tree: Tree,
        spaces: Vec<Space>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Filter by name or path, such as ~/dev/vyttle/*")
        });
        cx.subscribe(&filter, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        filter.update(cx, |input, cx| input.focus(window, cx));
        let typed = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Not cloned here? owner/name, such as Sage-Social/api")
        });
        Self {
            page,
            tree,
            spaces,
            filter,
            typed,
            error: None,
        }
    }

    fn shown_filter(&self, cx: &App) -> repository_tree::Filter {
        self.tree.filter(&self.filter.read(cx).value())
    }

    fn add_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let identity = self.typed.read(cx).value().trim().to_owned();
        if identity.is_empty() {
            return;
        }
        self.error = validate_repository_identity(&identity)
            .err()
            .map(|error| error.to_string());
        if self.error.is_none() {
            self.tree.add(&identity);
            self.typed
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    fn done(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choice = self.tree.finish();
        let _ = self.page.update(cx, |page, cx| page.chosen(choice, cx));
        window.close_dialog(cx);
    }

    fn open_repositories(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        let _ = self.page.update(cx, |_, cx| {
            cx.emit(Intent::OpenRepositories(None));
        });
    }

    fn folder_head(
        &self,
        at: usize,
        shown: &ShownFolder,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let folder = shown.folder;
        let key = folder.key.clone();
        let opener = key.clone();
        let tick = folder_tick(
            SharedString::from(format!("folder-tick-{at}")),
            shown.tick,
            p,
        )
        .when(shown.tick != Tick::Unavailable, |tick| {
            tick.cursor_pointer()
                .on_click(cx.listener(move |view, _, _, cx| {
                    let filter = view.shown_filter(cx);
                    view.tree.toggle_folder(&key, &filter);
                    cx.notify();
                }))
        });
        let follow = shown.follow.map(|following| {
            let root = folder.key.clone();
            let button = button(SharedString::from(format!("folder-follow-{at}"))).xsmall();
            match following {
                true => button.primary().label("Following"),
                false => button.outline().label("Follow"),
            }
            .on_click(cx.listener(move |view, _, _, cx| {
                view.tree.set_following(&root, !following);
                cx.notify();
            }))
        });
        div()
            .px(px(10.))
            .py(px(7.))
            .flex()
            .items_center()
            .gap_2()
            .bg(p.base)
            .when(at > 0, |d| d.border_t_1().border_color(p.edge))
            .child(
                div()
                    .id(SharedString::from(format!("folder-open-{at}")))
                    .w(px(10.))
                    .cursor_pointer()
                    .text_size(px(font::SMALL))
                    .text_color(p.subtle)
                    .child(if shown.open { "▾" } else { "▸" })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.tree.toggle_open(&opener);
                        cx.notify();
                    })),
            )
            .child(tick)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(folder.label.clone()),
            )
            .child(hint(shown.count.clone(), p))
            .children(follow)
    }

    fn row(
        &self,
        id: String,
        row: &Row,
        not_cloned: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let identity = row.identity.clone();
        let ticked = identity.as_deref().is_some_and(|i| self.tree.is_ticked(i));
        let new = identity.as_deref().is_some_and(|i| self.tree.is_new(i));
        let tick = Checkbox::new(SharedString::from(format!("tick-{id}")))
            .checked(ticked)
            .disabled(identity.is_none())
            .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                if let Some(identity) = &identity {
                    view.tree.set_ticked(identity, *checked);
                }
                cx.notify();
            }));
        let chips = row.others.iter().filter_map(|space| {
            let named = self.spaces.iter().find(|s| s.id == *space)?;
            Some(space_chip(
                &named.name,
                space_color(&self.spaces, space, p),
                p,
            ))
        });
        let (name, side) = match (&row.identity, not_cloned) {
            (None, _) => (
                hint(row.name.clone(), p),
                Some(hint("Not on GitHub, so in no Space", p)),
            ),
            (Some(_), true) => (
                div().child(row.name.clone()),
                Some(hint("not cloned on this Mac", p).text_color(p.yellow)),
            ),
            (Some(identity), false) => (
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(div().flex_none().child(row.name.clone()))
                    .child(
                        hint(identity.clone(), p)
                            .text_size(px(font::DETAIL))
                            .min_w_0()
                            .text_ellipsis(),
                    ),
                None,
            ),
        };
        div()
            .id(SharedString::from(format!("row-{id}")))
            .pl(px(ROW_INDENT))
            .pr(px(10.))
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .border_t_1()
            .border_color(p.edge.opacity(0.45))
            .when(new, |d| d.bg(p.green.opacity(0.09)))
            .child(tick)
            .child(div().flex_1().min_w_0().child(name))
            .children(side)
            .children(chips)
    }

    fn tree_view(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = self.shown_filter(cx);
        let folders = self.tree.shown(&filter);
        let mut list = div()
            .id("repository-tree")
            .max_h(px(TREE_HEIGHT))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .text_size(px(font::MESSAGE));
        if folders.is_empty() {
            list = list.child(div().p_3().child(hint("No repository matches.", p)));
        }
        for (at, shown) in folders.iter().enumerate() {
            list = list.child(self.folder_head(at, shown, p, cx));
            if shown.open {
                let not_cloned = shown.folder.key == NOT_CLONED;
                for (row_at, row) in shown.rows.iter().enumerate() {
                    let id = format!("{at}-{row_at}");
                    list = list.child(self.row(id, row, not_cloned, p, cx));
                }
            }
        }
        div()
            .rounded_lg()
            .border_1()
            .border_color(p.edge)
            .overflow_hidden()
            .child(list)
    }
}

impl Render for RepositoryChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let line = self.tree.footer(&self.shown_filter(cx));
        let note = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(hint(line, p))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(hint("Not listed?", p))
                    .child(
                        button("chooser-repositories-page")
                            .link()
                            .xsmall()
                            .label("Add it on the Repositories page.")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.open_repositories(window, cx)
                            })),
                    ),
            );
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(Input::new(&self.filter).small())
            .child(self.tree_view(p, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Input::new(&self.typed).small().flex_1().min_w_0())
                    .child(
                        button("add-space-repository")
                            .xsmall()
                            .icon(icon("plus"))
                            .label("Add")
                            .on_click(
                                cx.listener(|view, _, window, cx| view.add_typed(window, cx)),
                            ),
                    ),
            )
            .children(
                self.error
                    .clone()
                    .map(|error| hint(error, p).text_color(p.red)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(note)
                    .child(
                        button("chooser-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        button("chooser-done")
                            .primary()
                            .label("Done")
                            .on_click(cx.listener(|view, _, window, cx| view.done(window, cx))),
                    ),
            )
    }
}

/// A folder's tri-state tick, drawn because `Checkbox` has no half state.
fn folder_tick(id: SharedString, tick: Tick, p: Palette) -> Stateful<Div> {
    let (mark, fill, edge) = match tick {
        Tick::On => (Some("✓"), Some(p.accent), p.accent),
        Tick::Some => (Some("–"), Some(p.overlay), p.accent),
        Tick::Off | Tick::Unavailable => (None, None, p.edge),
    };
    div()
        .id(id)
        .size(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .border_1()
        .border_color(edge)
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .text_color(if tick == Tick::On {
            p.on_accent
        } else {
            p.accent
        })
        .when(tick == Tick::Unavailable, |d| d.opacity(0.45))
        .when_some(fill, |d, fill| d.bg(fill))
        .children(mark)
}

/// Another Space that lists a repository.
fn space_chip(name: &str, color: Hsla, p: Palette) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .rounded_md()
        .border_1()
        .border_color(p.edge)
        .text_size(px(font::META))
        .text_color(p.subtle)
        .child(space_mark(color))
        .child(name.to_owned())
}
