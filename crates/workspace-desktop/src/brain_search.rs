//! The brain search overlay (OpenSpec change `brain-browser`): ⌘K or Ctrl+K anywhere, and `/`
//! outside a text field, searches one brain's notes and log entries. The brain is the brain
//! browser's, else the selected project's Space's, so results never mix brains. Arrows move,
//! Enter opens the note in Wiki or the entry in Logs, Tab cycles the scope chips, and Esc closes
//! and gives focus back to where it was.
use super::*;
use brain_browser_view::{BrainOpening, BrainTarget, github_brain, short_id};
use gpui_component::input::Input;
use ui::{hint, icon, mono, pill};

actions!(
    brain_search,
    [
        OpenBrainSearch,
        MoveUp,
        MoveDown,
        OpenResult,
        CycleScope,
        CloseSearch
    ]
);

/// The overlay's own keys, over the query field's.
pub(super) fn bindings() -> Vec<KeyBinding> {
    const FIELD: &str = "BrainSearch > Input";
    vec![
        KeyBinding::new("cmd-k", OpenBrainSearch, None),
        KeyBinding::new("ctrl-k", OpenBrainSearch, None),
        KeyBinding::new("/", OpenBrainSearch, Some("!Input")),
        KeyBinding::new("up", MoveUp, Some(FIELD)),
        KeyBinding::new("down", MoveDown, Some(FIELD)),
        KeyBinding::new("enter", OpenResult, Some(FIELD)),
        KeyBinding::new("tab", CycleScope, Some(FIELD)),
        KeyBinding::new("escape", CloseSearch, Some(FIELD)),
        KeyBinding::new("escape", CloseSearch, Some("BrainSearch")),
    ]
}

/// What the overlay searches: one brain, with the project whose scope it may narrow to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Searched {
    Brain {
        repository: String,
        /// `(id, name)` of the project offered as a scope chip.
        project: Option<(String, String)>,
    },
    /// The Space has no brain the host indexes: none, or a local folder.
    NoBrain {
        space: Option<(SpaceId, String)>,
        local: bool,
    },
}

struct Opened {
    searched: Searched,
    /// The project whose scope is chosen; `None` is Whole brain.
    scope: Option<String>,
    results: Option<Result<Vec<BrainHit>, String>>,
    selected: usize,
    /// Where focus was, to give it back.
    restore: Option<FocusHandle>,
}

pub(super) struct BrainSearch {
    bridge: Bridge,
    query: Entity<InputState>,
    opened: Option<Opened>,
    /// Counts openings, so a reply to an earlier one, perhaps over another brain, is dropped.
    openings: u64,
    /// The results list, scrolled to keep the selected row in view.
    results: ScrollHandle,
}
impl EventEmitter<Intent> for BrainSearch {}
impl BrainSearch {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search notes and logs"));
        cx.subscribe_in(&query, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.search(window, cx);
            }
        })
        .detach();
        Self {
            bridge,
            query,
            opened: None,
            openings: 0,
            results: ScrollHandle::new(),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.opened.is_some()
    }

    /// Opens over `searched`, in `scope`, with the query field focused.
    pub(super) fn open(
        &mut self,
        searched: Searched,
        scope: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restore = match self.opened.take() {
            Some(opened) => opened.restore,
            None => window.focused(cx),
        };
        let scope = scope.filter(|scope| {
            matches!(&searched, Searched::Brain { project: Some((id, _)), .. } if id == scope)
        });
        self.openings += 1;
        self.opened = Some(Opened {
            searched,
            scope,
            results: None,
            selected: 0,
            restore,
        });
        self.query.update(cx, |query, cx| {
            query.set_value("", window, cx);
            query.focus(window, cx);
        });
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(restore) = self.opened.take().and_then(|o| o.restore) {
            restore.focus(window, cx);
        }
        cx.notify();
    }

    fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_owned();
        let Some(opened) = &mut self.opened else {
            return;
        };
        let Searched::Brain { repository, .. } = &opened.searched else {
            return;
        };
        opened.selected = 0;
        if query.is_empty() {
            opened.results = None;
            return cx.notify();
        }
        let (repository, scope) = (repository.clone(), opened.scope.clone());
        let asked = (self.openings, query.clone(), scope.clone());
        self.ask(
            ask::search_brain(repository, scope, query),
            window,
            cx,
            move |view, hits, _, cx| {
                let current = view.query.read(cx).value().trim().to_owned();
                if let Some(opened) = &mut view.opened
                    && (view.openings, current, opened.scope.clone()) == asked
                {
                    opened.results = Some(hits);
                    opened.selected = 0;
                    view.results.scroll_to_item(0);
                    cx.notify();
                }
            },
        );
    }

    fn hits(&self) -> &[BrainHit] {
        (self.opened.as_ref())
            .and_then(|o| o.results.as_ref()?.as_ref().ok())
            .map_or(&[], Vec::as_slice)
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let count = self.hits().len();
        if let Some(opened) = &mut self.opened
            && count > 0
        {
            opened.selected = opened.selected.saturating_add_signed(by).min(count - 1);
            // Each result is one child of the list, so its index is the row's.
            self.results.scroll_to_item(opened.selected);
            cx.notify();
        }
    }

    fn cycle_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opened) = &mut self.opened else {
            return;
        };
        if let Searched::Brain {
            project: Some((id, _)),
            ..
        } = &opened.searched
        {
            opened.scope = match opened.scope {
                Some(_) => None,
                None => Some(id.clone()),
            };
            self.search(window, cx);
        }
    }

    fn set_scope(&mut self, scope: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opened) = &mut self.opened {
            opened.scope = scope;
        }
        self.search(window, cx);
    }

    fn open_result(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opened) = &self.opened else {
            return;
        };
        let Searched::Brain { repository, .. } = &opened.searched else {
            return;
        };
        let Some(hit) = self.hits().get(at.unwrap_or(opened.selected)) else {
            return;
        };
        let target = match (hit.hit, &hit.entry_id) {
            (BrainHitKind::Entry, Some(id)) => BrainTarget::Entry {
                id: id.clone(),
                pending: false,
            },
            _ => BrainTarget::Note(hit.path.clone()),
        };
        let opening = BrainOpening {
            repository: repository.clone(),
            project: opened.scope.clone(),
            target,
        };
        self.close(window, cx);
        cx.emit(Intent::OpenBrain(opening));
    }

    fn panel(&self, opened: &Opened, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut panel = div()
            .w(px(640.))
            .max_h(px(520.))
            .flex()
            .flex_col()
            .rounded_xl()
            .border_1()
            .border_color(p.edge)
            .bg(p.surface)
            .shadow_lg()
            .overflow_hidden()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let field = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .child(icon("search").text_color(p.subtle))
            .child(Input::new(&self.query).appearance(false).flex_1())
            .child(mono("esc", p));
        panel = panel.child(field);
        let (repository, project) = match &opened.searched {
            Searched::NoBrain { space, local } => {
                return panel.child(self.no_brain(space.as_ref(), *local, p, cx));
            }
            Searched::Brain {
                repository,
                project,
            } => (repository, project),
        };
        let mut chips = div().flex().flex_wrap().gap_1().px_3().py_2();
        if let Some((id, name)) = project {
            let id = id.clone();
            chips = chips.child(
                ui::segment(
                    "search-scope-project",
                    format!("This project · {name}"),
                    opened.scope.is_some(),
                )
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.set_scope(Some(id.clone()), window, cx)
                })),
            );
        }
        chips = chips.child(
            ui::segment(
                "search-scope-brain",
                format!("Whole brain · {repository}"),
                opened.scope.is_none(),
            )
            .on_click(cx.listener(|view, _, window, cx| view.set_scope(None, window, cx))),
        );
        panel = panel.child(chips);
        let mut list = div()
            .id("brain-search-results")
            .track_scroll(&self.results)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .pb_2()
            .flex()
            .flex_col()
            .gap(px(2.));
        let foot = match &opened.results {
            None => {
                list = list
                    .child(hint("Type to search this brain’s notes and log entries.", p).px_2());
                "Decisions rank first".to_owned()
            }
            Some(Err(error)) => {
                list = list.child(hint(format!("Could not search the brain: {error}"), p).px_2());
                String::new()
            }
            Some(Ok(hits)) if hits.is_empty() => {
                list = list.child(hint("No notes or entries match.", p).px_2());
                "0 results".to_owned()
            }
            Some(Ok(hits)) => {
                for (i, hit) in hits.iter().enumerate() {
                    list = list.child(result_row(hit, i, i == opened.selected, p).on_click(
                        cx.listener(move |view, _, window, cx| {
                            view.open_result(Some(i), window, cx)
                        }),
                    ));
                }
                format!(
                    "{} {} · decisions first",
                    hits.len(),
                    wording::plural(hits.len(), "result", "results")
                )
            }
        };
        panel.child(list).child(
            div()
                .flex()
                .gap_3()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(p.edge.opacity(edge::SEPARATOR))
                .text_size(px(font::CAPTION))
                .text_color(p.subtle)
                .child("↑↓ move")
                .child("↵ open")
                .child("Tab scope")
                .child(div().flex_1())
                .child(foot),
        )
    }

    fn no_brain(
        &self,
        space: Option<&(SpaceId, String)>,
        local: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let line = no_brain_line(space.map(|(_, name)| name.as_str()), local);
        let mut body = div().p_3().flex().flex_col().gap_2().child(hint(line, p));
        if let Some((id, _)) = space {
            let id = id.clone();
            body = body.child(
                ui::button("search-space-settings")
                    .outline()
                    .label("Settings › Spaces")
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.close(window, cx);
                        cx.emit(Intent::OpenSpace(id.clone()));
                    })),
            );
        }
        body
    }
}

impl Requests for BrainSearch {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

impl Render for BrainSearch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let Some(opened) = &self.opened else {
            return div().into_any_element();
        };
        let panel = self.panel(opened, p, cx);
        div()
            .id("brain-search")
            .key_context("BrainSearch")
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.3))
            .flex()
            .justify_center()
            .pt(px(96.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.close(window, cx)),
            )
            .on_action(cx.listener(|view, _: &MoveUp, _, cx| view.step(-1, cx)))
            .on_action(cx.listener(|view, _: &MoveDown, _, cx| view.step(1, cx)))
            .on_action(
                cx.listener(|view, _: &OpenResult, window, cx| view.open_result(None, window, cx)),
            )
            .on_action(cx.listener(|view, _: &CycleScope, window, cx| view.cycle_scope(window, cx)))
            .on_action(cx.listener(|view, _: &CloseSearch, window, cx| view.close(window, cx)))
            .child(panel)
            .into_any_element()
    }
}

fn result_row(hit: &BrainHit, index: usize, selected: bool, p: Palette) -> Stateful<Div> {
    let place = match hit.hit {
        BrainHitKind::Note => format!("note · {}", hit.path),
        BrainHitKind::Entry => {
            let mut place = vec!["log".to_owned()];
            place.extend(hit.repository.clone());
            place.extend(hit.date.clone());
            place.extend(hit.entry_id.as_deref().map(short_id));
            place.join(" · ")
        }
    };
    let mut title = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            icon(if hit.hit == BrainHitKind::Note {
                "book"
            } else {
                "activity"
            })
            .size(px(13.))
            .text_color(p.subtle),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .min_w_0()
                .text_ellipsis()
                .child(hit.title.clone()),
        );
    if let Some(kind) = hit.kind {
        let color = if kind == BrainKind::Decision {
            p.accent
        } else {
            p.focus
        };
        title = title.child(pill(kind.name(), color));
    }
    title = title
        .child(div().flex_1())
        .child(mono(place, p).flex_none().max_w(px(260.)));
    let (text, marks) = excerpt(&hit.excerpt);
    let mark = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        background_color: Some(p.yellow.opacity(0.25)),
        ..Default::default()
    };
    div()
        .id(("brain-search-result", index))
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .flex()
        .flex_col()
        .gap(px(2.))
        .when(selected, |d| {
            d.bg(p.overlay).border_1().border_color(p.focus)
        })
        .when(!selected, |d| {
            d.border_1().border_color(gpui::transparent_black())
        })
        .hover(|d| d.bg(p.overlay))
        .child(title)
        .child(
            div()
                .text_size(px(font::SMALL))
                .text_color(p.subtle)
                .child(StyledText::new(text).with_highlights(marks.into_iter().map(|r| (r, mark)))),
        )
}

/// An excerpt as one line, with the byte ranges of its matched terms.
fn excerpt(parts: &[ExcerptPart]) -> (String, Vec<std::ops::Range<usize>>) {
    let mut text = String::new();
    let mut marks = vec![];
    for part in parts {
        let start = text.len();
        text.push_str(&part.text.replace('\n', " "));
        if part.matched {
            marks.push(start..text.len());
        }
    }
    (text, marks)
}

fn no_brain_line(space: Option<&str>, local: bool) -> String {
    match (space, local) {
        (Some(space), true) => format!(
            "The {space} Space’s brain is a folder on this Mac, which search does not index. Name a GitHub brain in the Space’s settings to search it."
        ),
        (Some(space), false) => format!(
            "The {space} Space names no brain, so there is nothing to search. Name one in Settings › Spaces."
        ),
        (None, _) => "Open a project or a brain to search its brain.".into(),
    }
}

/// What ⌘K searches outside the brain browser: the selected project's Space's brain, with that
/// project as a scope, or else the brain of `last_space`, the Space last used.
pub(super) fn searched_for(shared: &Shared, last_space: Option<&SpaceId>) -> Searched {
    let Some(snapshot) = shared.snapshot() else {
        return Searched::NoBrain {
            space: None,
            local: false,
        };
    };
    let project =
        (shared.project_id()).and_then(|id| snapshot.projects.iter().find(|p| p.id == id));
    let space_id = project.map(|p| &p.space).or(last_space);
    let Some(space) = space_id.and_then(|id| snapshot.spaces.iter().find(|s| &s.id == id)) else {
        return Searched::NoBrain {
            space: None,
            local: false,
        };
    };
    match github_brain(space) {
        Some(repository) => Searched::Brain {
            repository,
            project: project.map(|p| (p.id.clone(), p.name.clone())),
        },
        None => Searched::NoBrain {
            space: Some((space.id.clone(), space.name.clone())),
            local: space.brain.is_some(),
        },
    }
}

/// The Space of the project selected, which the overlay falls back to once none is.
pub(super) fn space_in_use(shared: &Shared) -> Option<SpaceId> {
    let project = shared.project_id()?;
    Some(shared.index.as_ref()?.project(&project)?.space.clone())
}

#[cfg(test)]
mod tests {
    use super::{Searched, excerpt, no_brain_line, searched_for};
    use crate::Shared;
    use std::rc::Rc;
    use workspace_core::*;

    #[test]
    fn without_a_selected_project_search_uses_the_brain_of_the_space_last_used() {
        let space = Space {
            id: SpaceId("sage".into()),
            name: "Sage".into(),
            description: String::new(),
            instructions: String::new(),
            brain: Some(SpaceBrain {
                repository: BrainRepository::GitHub {
                    owner: "Sage-Social".into(),
                    name: "sagehaven-docs".into(),
                    extra: Extra::new(),
                },
                folder: "second-brain".into(),
                extra: Extra::new(),
            }),
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: "l".into() },
                folder: "sage".into(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        };
        let snapshot = Snapshot {
            spaces: vec![space],
            ..Default::default()
        };
        let shared = Shared {
            index: Some(Rc::new(WorkspaceIndex::new(snapshot, Rc::default()))),
            ..Default::default()
        };
        let last = SpaceId("sage".into());
        assert_eq!(
            searched_for(&shared, Some(&last)),
            Searched::Brain {
                repository: "Sage-Social/sagehaven-docs".into(),
                project: None,
            }
        );
        assert!(matches!(
            searched_for(&shared, None),
            Searched::NoBrain { space: None, .. }
        ));
    }

    #[test]
    fn an_excerpt_marks_each_matched_term() {
        let part = |text: &str, matched| ExcerptPart {
            text: text.into(),
            matched,
        };
        let (text, marks) = excerpt(&[
            part("…the relay ", false),
            part("retries", true),
            part(" once on a\n", false),
            part("Telnyx", true),
            part(" timeout", false),
        ]);
        assert_eq!(text, "…the relay retries once on a Telnyx timeout");
        let marked: Vec<&str> = marks.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(marked, ["retries", "Telnyx"]);
    }

    #[test]
    fn a_space_without_a_searchable_brain_says_why_and_where_to_name_one() {
        assert!(no_brain_line(Some("Vyttle"), false).contains("Settings › Spaces"));
        assert!(no_brain_line(Some("Notes"), true).contains("folder on this Mac"));
        assert!(no_brain_line(None, false).contains("Open a project"));
    }
}
