//! The Issues page (OpenSpec change `shared-issues`, tasks 2.1 and 2.2, mockup studies a and b):
//! the issues of every Space on this Mac, as a list grouped by state or as a board whose cards
//! move by dragging, with filters, search and unread marks. Filters stay in this window and
//! unread marks in the host's `issues.sql`; neither reaches a tracker. An open issue shows in the
//! list's place (`issue_view`). Issue text is untrusted, so it is shown as plain text.
use super::*;
use gpui_component::{
    Sizable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
};
use issue_view::{IssueView, IssueViewEvent, issue_id};
use new_issue::{NewIssueForm, Trackable};
use ui::{empty_state, hint, icon, pill, segment};
// GPUI exports a `Priority` too; the issue's wins over both globs.
use workspace_core::Priority;

pub(super) const PRIORITIES: [Priority; 5] = [
    Priority::Urgent,
    Priority::High,
    Priority::Medium,
    Priority::Low,
    Priority::None,
];
/// The list's groups, the work furthest along first; Done and Canceled close it.
const LIST_ORDER: [IssueState; 7] = [
    IssueState::InReview,
    IssueState::InProgress,
    IssueState::Todo,
    IssueState::Triage,
    IssueState::Backlog,
    IssueState::Done,
    IssueState::Canceled,
];
/// An error shorter than this is a pass retrying; longer, the footer turns red.
const ERROR_TURNS_RED_MS: i64 = 60 * 60 * 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    List,
    Board,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Assignee {
    Me,
    Nobody,
    Login(String),
}

/// The page's filters besides the Space, which the host filters by, and search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Filters {
    pub(super) all_states: bool,
    pub(super) assignee: Option<Assignee>,
    pub(super) priority: Option<Priority>,
    pub(super) label: Option<String>,
    pub(super) repository: Option<String>,
}

pub(super) struct IssuesPage {
    bridge: Bridge,
    shared: Shared,
    layout: Layout,
    space: Option<SpaceId>,
    filters: Filters,
    search: Entity<InputState>,
    listings: Option<Result<Vec<IssueListing>, String>>,
    /// Counts the lists asked for, so only the latest reply is shown.
    asked: u64,
    /// The trackers as the last snapshot summarized them; a change asks for the list again.
    trackers: Vec<IssueTracker>,
    open: Option<Entity<IssueView>>,
    /// The Space of the last issue filed here, where New issue starts without a Space filter.
    last_filed: Option<SpaceId>,
}
impl EventEmitter<Intent> for IssuesPage {}
impl IssuesPage {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search titles, text and comments…"));
        cx.subscribe_in(&search, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.load(window, cx);
            }
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            layout: Layout::List,
            space: None,
            filters: Filters::default(),
            search,
            listings: None,
            asked: 0,
            trackers: vec![],
            open: None,
            last_filed: None,
        }
    }

    /// Reads the issues when the page opens or a tracker changes while it shows.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let opened = shared.page == Page::Issues && self.shared.page != Page::Issues;
        self.shared = shared;
        if self.shared.page != Page::Issues {
            return;
        }
        let trackers = (self.shared.snapshot()).map_or(vec![], |s| s.issue_trackers.clone());
        let changed = trackers != self.trackers;
        self.trackers = trackers;
        if opened || changed {
            self.load(window, cx);
            if let Some(open) = &self.open {
                open.update(cx, |issue, cx| issue.load(window, cx));
            }
            cx.notify();
        }
    }
    /// Shows the issues of one Space, or of all, as the Space page's Open Issues does.
    pub(super) fn filter_space(
        &mut self,
        space: Option<SpaceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.space = space;
        self.open = None;
        self.load(window, cx);
        cx.notify();
    }
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asked += 1;
        let asked = self.asked;
        let search = self.search.read(cx).value().trim().to_owned();
        let search = (!search.is_empty()).then_some(search);
        self.ask(
            ask::issues(self.space.clone(), search),
            window,
            cx,
            move |view, listings, _, cx| {
                if view.asked == asked {
                    view.listings = Some(listings);
                    cx.notify();
                }
            },
        );
    }

    fn spaces(&self) -> &[Space] {
        self.shared.snapshot().map_or(&[], |s| &s.spaces)
    }
    /// The Spaces with a tracker, which New issue and the Space filter offer.
    fn trackable(&self) -> Vec<Trackable> {
        (self.spaces().iter())
            .filter_map(|space| {
                let tracker = self.trackers.iter().find(|t| t.space_id == space.id)?;
                Some(Trackable {
                    space: space.clone(),
                    prefix: tracker.prefix.clone()?,
                })
            })
            .collect()
    }
    fn login(&self) -> Option<String> {
        self.trackers.iter().find_map(|t| t.login.clone())
    }

    fn open_issue(
        &mut self,
        id: IssueId,
        unread: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bridge = self.bridge.clone();
        let shared = self.shared.clone();
        let issue = cx.new(|cx| IssueView::new(bridge, id, unread, &shared, window, cx));
        cx.subscribe_in(&issue, window, |view, _, event, window, cx| match event {
            IssueViewEvent::Back => {
                view.open = None;
                view.load(window, cx);
                cx.notify();
            }
            IssueViewEvent::Changed => view.load(window, cx),
            IssueViewEvent::Open(id) => view.open_issue(id.clone(), false, window, cx),
        })
        .detach();
        self.open = Some(issue);
        cx.notify();
    }

    /// Moves a dragged card's issue to the column it was dropped on, showing it there at once.
    fn move_to(
        &mut self,
        id: &IssueId,
        state: IssueState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(listings)) = &mut self.listings else {
            return;
        };
        let Some(listing) = listings.iter_mut().find(|l| &l.id == id) else {
            return;
        };
        if listing.state == Some(state) {
            return;
        }
        listing.state = Some(state);
        self.ask(
            ask::move_issue(id.clone(), state),
            window,
            cx,
            |view, moved, window, cx| {
                if let Err(error) = moved {
                    toast_error(error, window, cx);
                }
                view.load(window, cx);
            },
        );
        cx.notify();
    }

    fn new_issue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trackable = self.trackable();
        let ids: Vec<SpaceId> = trackable.iter().map(|t| t.space.id.clone()).collect();
        let default = new_issue::default_space(self.space.as_ref(), self.last_filed.as_ref(), &ids);
        let Some(space) = default else {
            return toast_error(
                "No Space here has a tracker yet. Set up issues on a Space’s page.",
                window,
                cx,
            );
        };
        let login = self.login();
        let form = cx.new(|cx| NewIssueForm::new(trackable, space, login, window, cx));
        let focused = form.clone();
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let (weak, form) = (weak.clone(), form.clone());
            dialog
                .title("New issue")
                .width(px(600.).min(window.viewport_size().width - px(48.)))
                .max_h(window.viewport_size().height - px(100.))
                .confirm()
                .ok_text("File issue")
                .child(form.clone())
                .on_ok(move |_, window, cx| match form.read(cx).chosen(cx) {
                    Ok((space, issue)) => {
                        let _ = weak.update(cx, |view, cx| view.file(space, issue, window, cx));
                        true
                    }
                    Err(error) => {
                        form.update(cx, |form, cx| {
                            form.error = error;
                            cx.notify();
                        });
                        false
                    }
                })
        });
        cx.on_next_frame(window, move |_, window, cx| {
            focused.update(cx, |form, cx| form.focus(window, cx))
        });
    }
    fn file(
        &mut self,
        space: SpaceId,
        issue: NewIssue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask(
            ask::create_issue(space.clone(), issue),
            window,
            cx,
            move |view, filed, window, cx| match filed {
                Ok(issue) => {
                    view.last_filed = Some(space);
                    toast(format!("Filed {}", issue.id), window, cx);
                    view.load(window, cx);
                }
                Err(error) => toast_error(error, window, cx),
            },
        );
    }

    fn filter_bar(&self, listings: &[IssueListing], p: Palette, cx: &mut Context<Self>) -> Div {
        // Applies one filter's change; the listings shown are filtered again as they draw.
        let weak = cx.weak_entity();
        let set = move |change: Box<dyn Fn(&mut Self)>| {
            let weak = weak.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |view, cx| {
                    change(view);
                    cx.notify();
                });
            }
        };
        let mut spaces: Vec<(Option<SpaceId>, SharedString)> = vec![(None, "All Spaces".into())];
        spaces.extend(
            (self.trackable().into_iter())
                .map(|t| (Some(t.space.id.clone()), t.space.name.clone().into())),
        );
        let space_label = (self.spaces().iter())
            .find(|s| Some(&s.id) == self.space.as_ref())
            .map_or("All Spaces".into(), |s| s.name.clone());
        let weak = cx.weak_entity();
        let space = picker(
            "issues-space",
            format!("Space: {space_label}"),
            spaces,
            Some(self.space.clone()),
            move |space, window, cx| {
                let _ = weak.update(cx, |view, cx| view.filter_space(space, window, cx));
            },
        );
        let open = segment("issues-open", "Open", !self.filters.all_states).on_click(cx.listener(
            |view, _, _, cx| {
                view.filters.all_states = !view.filters.all_states;
                cx.notify();
            },
        ));
        let seen = Seen::of(listings);
        let mut assignees = vec![
            (None, "Anyone".into()),
            (Some(Assignee::Me), "Me".into()),
            (Some(Assignee::Nobody), "Nobody".into()),
        ];
        assignees.extend(
            (seen.assignees.iter())
                .map(|login| (Some(Assignee::Login(login.clone())), login.clone().into())),
        );
        let assignee_label = match &self.filters.assignee {
            None => "anyone".to_owned(),
            Some(Assignee::Me) => "me".into(),
            Some(Assignee::Nobody) => "nobody".into(),
            Some(Assignee::Login(login)) => login.clone(),
        };
        let assignee = picker(
            "issues-assignee",
            format!("Assignee: {assignee_label}"),
            assignees,
            Some(self.filters.assignee.clone()),
            {
                let set = set.clone();
                move |assignee, window, cx| {
                    set(Box::new(move |v| v.filters.assignee = assignee.clone()))(window, cx)
                }
            },
        );
        let mut priorities = vec![(None, "Any priority".into())];
        priorities.extend(PRIORITIES.map(|p| (Some(p), priority_label(p).into())));
        let priority = picker(
            "issues-priority",
            match self.filters.priority {
                Some(priority) => format!("Priority: {}", priority_label(priority)),
                None => "+ Priority".into(),
            },
            priorities,
            Some(self.filters.priority),
            {
                let set = set.clone();
                move |priority, window, cx| {
                    set(Box::new(move |v| v.filters.priority = priority))(window, cx)
                }
            },
        );
        let label = text_picker(
            "issues-label",
            "Label",
            &seen.labels,
            &self.filters.label,
            {
                let set = set.clone();
                move |label: Option<String>, window: &mut Window, cx: &mut App| {
                    set(Box::new(move |v| v.filters.label = label.clone()))(window, cx)
                }
            },
        );
        let repository = text_picker(
            "issues-repository",
            "Repository",
            &seen.repositories,
            &self.filters.repository,
            move |repository: Option<String>, window: &mut Window, cx: &mut App| {
                set(Box::new(move |v| v.filters.repository = repository.clone()))(window, cx)
            },
        );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(space)
            .child(open)
            .child(assignee)
            .child(priority)
            .child(label)
            .child(repository)
            .child(
                div()
                    .flex_1()
                    .min_w(px(220.))
                    .child(Input::new(&self.search).small().cleanable(true)),
            )
            .text_color(p.text)
    }

    fn list(&self, shown: &[&IssueListing], p: Palette, cx: &mut Context<Self>) -> Div {
        let me = self.login();
        let mut list = div().flex().flex_col().gap_4();
        for (state, issues) in grouped(shown) {
            let (title, note) = match state {
                Some(state) => (state.label(), group_note(state)),
                None => (
                    "Can’t be read",
                    Some("issue.md does not parse; open one to see it"),
                ),
            };
            let count = issues.len();
            let mut rows = div().flex().flex_col();
            for listing in issues {
                rows = rows.child(self.row(listing, me.as_deref(), p, cx));
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title)
                            .child(hint(count.to_string(), p))
                            .children(note.map(|note| hint(format!("· {note}"), p))),
                    )
                    .child(rows),
            );
        }
        list
    }

    fn row(
        &self,
        listing: &IssueListing,
        me: Option<&str>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (id, unread) = (listing.id.clone(), listing.unread);
        let title = match &listing.problem {
            Some(_) => div().text_color(p.yellow).child("This issue can’t be read"),
            None => div().child(listing.title.clone()),
        };
        div()
            .id(SharedString::from(format!("issue-row-{}", listing.id)))
            .flex()
            .items_center()
            .gap_3()
            .px_2()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|d| d.bg(p.overlay))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_issue(id.clone(), unread, window, cx)
            }))
            .child(priority_mark(listing.priority, p))
            .child(div().w(px(96.)).flex_none().child(issue_id(&listing.id, p)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(title.text_ellipsis())
                    .when(listing.unread, |d| d.child(unread_dot(p))),
            )
            .children(
                listing
                    .labels
                    .iter()
                    .map(|l| pill(l.clone(), label_color(l, p))),
            )
            .children(
                (listing.repositories.iter())
                    .map(|r| pill(short_repository(r).to_owned(), p.subtle)),
            )
            .child(hint(work_line(listing), p).flex_none())
            .child(avatar(listing.assignee.as_deref(), me, p))
    }

    fn board(&self, shown: &[&IssueListing], p: Palette, cx: &mut Context<Self>) -> Div {
        let me = self.login();
        let unreadable = shown.iter().filter(|l| l.state.is_none()).count();
        let mut columns = div().flex().items_start().gap_3();
        for state in IssueState::ALL {
            let cards: Vec<&&IssueListing> =
                shown.iter().filter(|l| l.state == Some(state)).collect();
            let mut column = div()
                .id(SharedString::from(format!(
                    "issues-column-{}",
                    state.label()
                )))
                .w(px(220.))
                .flex_none()
                .min_h(px(120.))
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_lg()
                .bg(p.surface)
                .drag_over::<CardDrag>(move |style, _, _, _| style.bg(p.overlay))
                .on_drop(cx.listener(move |view, drag: &CardDrag, window, cx| {
                    view.move_to(&drag.id, state, window, cx)
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(state.label())
                        .child(hint(cards.len().to_string(), p)),
                );
            for listing in cards {
                column = column.child(self.card(listing, me.as_deref(), p, cx));
            }
            columns = columns.child(column);
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(unreadable > 0, |d| {
                d.child(hint(
                    format!(
                        "{} can’t be read and {} not on the board; the list shows them.",
                        wording::counted(unreadable, "issue"),
                        if unreadable == 1 { "is" } else { "are" }
                    ),
                    p,
                ))
            })
            .child(
                div()
                    .id("issues-board")
                    .overflow_x_scroll()
                    .pb_2()
                    .child(columns),
            )
    }

    fn card(
        &self,
        listing: &IssueListing,
        me: Option<&str>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (id, unread) = (listing.id.clone(), listing.unread);
        let drag = CardDrag {
            id: listing.id.clone(),
            title: listing.title.clone().into(),
            palette: p,
        };
        div()
            .id(SharedString::from(format!("issue-card-{}", listing.id)))
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(p.edge.opacity(edge::CARD))
            .when(listing.unread, |d| d.border_color(p.accent.opacity(0.6)))
            .bg(p.base)
            .cursor_pointer()
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_issue(id.clone(), unread, window, cx)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(issue_id(&listing.id, p))
                    .when(listing.priority != Priority::None, |d| {
                        d.child(priority_mark(listing.priority, p))
                    })
                    .child(div().flex_1())
                    .child(avatar(listing.assignee.as_deref(), me, p)),
            )
            .child(
                div()
                    .text_size(px(font::SMALL))
                    .child(listing.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(
                        listing
                            .labels
                            .iter()
                            .map(|l| pill(l.clone(), label_color(l, p))),
                    )
                    .children(
                        (listing.repositories.iter())
                            .map(|r| pill(short_repository(r).to_owned(), p.subtle)),
                    ),
            )
    }

    /// The footer: each shown tracker, when it last synced and what waits.
    fn footer(&self, p: Palette) -> Div {
        let mut footer = div().flex().flex_wrap().gap_4();
        for (line, tone) in footer_lines(
            self.spaces(),
            &self.trackers,
            self.space.as_ref(),
            ui::now(),
        ) {
            let color = match tone {
                Tone::Plain => p.subtle,
                Tone::Yellow => p.yellow,
                Tone::Red => p.red,
            };
            footer = footer.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(font::SMALL))
                    .text_color(color)
                    .child(div().size(px(6.)).rounded_full().bg(match tone {
                        Tone::Plain => p.green,
                        _ => color,
                    }))
                    .child(line),
            );
        }
        footer
    }
}

impl Render for IssuesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Issues");
        let p = palette(is_dark(cx));
        let footer = Some(self.footer(p));
        if let Some(open) = &self.open {
            let body = open.clone();
            return ui::page("Issues", "Issues", body, footer, &self.shared.host, p, cx);
        }
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        segment("issues-list", "List", self.layout == Layout::List).on_click(
                            cx.listener(|view, _, _, cx| {
                                view.layout = Layout::List;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        segment("issues-board", "Board", self.layout == Layout::Board).on_click(
                            cx.listener(|view, _, _, cx| {
                                view.layout = Layout::Board;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(div().flex_1())
            .child(
                ui::gated(
                    button("issues-new")
                        .primary()
                        .icon(icon("plus"))
                        .label("New issue"),
                    &self.shared.host,
                )
                .on_click(cx.listener(|view, _, window, cx| view.new_issue(window, cx))),
            );
        let listings = match &self.listings {
            Some(Ok(listings)) => listings.clone(),
            Some(Err(error)) => {
                let body = div()
                    .child(head)
                    .child(hint(error.clone(), p).text_color(p.red));
                return ui::page("Issues", "Issues", body, footer, &self.shared.host, p, cx);
            }
            None => vec![],
        };
        let shown = filtered(&listings, &self.filters, self.login().as_deref());
        let content = if self.listings.is_none() {
            hint("Reading…", p)
        } else if shown.is_empty() {
            empty_state(
                "list-checks",
                "No issues here",
                "File one with New issue, or change the filters.",
                p,
            )
        } else {
            match self.layout {
                Layout::List => self.list(&shown, p, cx),
                Layout::Board => self.board(&shown, p, cx),
            }
        };
        let body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(head)
            .child(self.filter_bar(&listings, p, cx))
            .child(content);
        ui::page("Issues", "Issues", body, footer, &self.shared.host, p, cx)
    }
}

impl Requests for IssuesPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

/// What the board shows under the pointer while a card is dragged.
#[derive(Clone)]
struct CardDrag {
    id: IssueId,
    title: SharedString,
    palette: Palette,
}
impl Render for CardDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        div()
            .w(px(220.))
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(p.focus)
            .bg(p.base)
            .text_size(px(font::SMALL))
            .child(issue_id(&self.id, p))
            .child(self.title.clone())
    }
}

/// The labels, assignees and repositories the listed issues use, for the filters.
#[derive(Default)]
struct Seen {
    labels: Vec<String>,
    assignees: Vec<String>,
    repositories: Vec<String>,
}
impl Seen {
    fn of(listings: &[IssueListing]) -> Self {
        let mut seen = Self::default();
        for listing in listings {
            seen.labels.extend(listing.labels.iter().cloned());
            seen.assignees.extend(listing.assignee.iter().cloned());
            seen.repositories
                .extend(listing.repositories.iter().cloned());
        }
        for list in [
            &mut seen.labels,
            &mut seen.assignees,
            &mut seen.repositories,
        ] {
            list.sort();
            list.dedup();
        }
        seen
    }
}

/// A small button opening a menu of `options`, `current` ticked; choosing one calls `chose`.
pub(super) fn picker<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    options: Vec<(T, SharedString)>,
    current: Option<T>,
    chose: impl Fn(T, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let chose = Rc::new(chose);
    button(id)
        .xsmall()
        .label(label)
        .dropdown_caret(true)
        .dropdown_menu(move |mut menu, _, _| {
            for (value, text) in &options {
                let (value, chose) = (value.clone(), chose.clone());
                menu = menu.item(
                    PopupMenuItem::new(text.clone())
                        .checked(current.as_ref() == Some(&value))
                        .on_click(move |_, window, cx| chose(value.clone(), window, cx)),
                );
            }
            menu
        })
}

/// A filter over text values: "+ Label" until one is chosen, then "Label: bug".
fn text_picker(
    id: &'static str,
    noun: &'static str,
    values: &[String],
    current: &Option<String>,
    chose: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut options = vec![(None, format!("Any {}", noun.to_lowercase()).into())];
    let short = |value: &String| match noun {
        "Repository" => short_repository(value).to_owned(),
        _ => value.clone(),
    };
    options.extend(values.iter().map(|v| (Some(v.clone()), short(v).into())));
    let label = match current {
        Some(value) => format!("{noun}: {}", short(value)),
        None => format!("+ {noun}"),
    };
    picker(id, label, options, Some(current.clone()), chose)
}

pub(super) fn priority_label(priority: Priority) -> &'static str {
    match priority {
        Priority::Urgent => "Urgent",
        Priority::High => "High",
        Priority::Medium => "Medium",
        Priority::Low => "Low",
        Priority::None => "No priority",
    }
}

fn priority_glyph(priority: Priority) -> &'static str {
    match priority {
        Priority::Urgent => "!!!",
        Priority::High => "!!",
        Priority::Medium => "!",
        Priority::Low => "·",
        Priority::None => "–",
    }
}

fn priority_mark(priority: Priority, p: Palette) -> Div {
    div()
        .w(px(22.))
        .flex_none()
        .font_weight(FontWeight::BOLD)
        .text_size(px(font::SMALL))
        .text_color(match priority {
            Priority::Urgent => p.red,
            Priority::High => p.accent,
            _ => p.subtle,
        })
        .child(priority_glyph(priority))
}

/// A label's pill color: a bug stands out.
pub(super) fn label_color(label: &str, p: Palette) -> Hsla {
    match label {
        "bug" => p.red,
        _ => p.subtle,
    }
}

/// `owner/name` as its name only.
pub(super) fn short_repository(repository: &str) -> &str {
    repository.rsplit('/').next().unwrap_or(repository)
}

fn unread_dot(p: Palette) -> Div {
    div().flex_none().size(px(7.)).rounded_full().bg(p.accent)
}

/// The assignee's initials in a circle, accented for the human; an empty circle for nobody.
fn avatar(login: Option<&str>, me: Option<&str>, p: Palette) -> Div {
    let circle = div()
        .flex_none()
        .size(px(22.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(font::MICRO))
        .font_weight(FontWeight::SEMIBOLD);
    match login {
        None => circle.border_1().border_color(p.edge.opacity(edge::STRONG)),
        Some(login) => {
            let mine = Some(login) == me;
            circle
                .bg(if mine { p.accent } else { p.overlay })
                .text_color(if mine { p.on_accent } else { p.text })
                .child(initials(login))
        }
    }
}

/// Up to two letters of a login: `pshah` is PS, `tomas-reyes` TR.
pub(super) fn initials(login: &str) -> String {
    let parts: Vec<&str> = login
        .split(['-', '_', '.'])
        .filter(|p| !p.is_empty())
        .collect();
    let letters: String = match parts.as_slice() {
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        [only] => only.chars().take(2).collect(),
        [] => String::new(),
    };
    letters.to_uppercase()
}

/// The unread issues across this Mac's trackers, a tracker two Spaces share counted once.
pub(super) fn unread(trackers: &[IssueTracker]) -> usize {
    let mut counted: Vec<&str> = vec![];
    let mut unread = 0;
    for tracker in trackers {
        if !counted.contains(&tracker.tracker.as_str()) {
            counted.push(&tracker.tracker);
            unread += tracker.unread;
        }
    }
    unread
}

/// The listings the filters keep: open ones unless all states are asked for, then by assignee,
/// priority, label and repository. One that cannot be read passes every filter but the state.
pub(super) fn filtered<'a>(
    listings: &'a [IssueListing],
    filters: &Filters,
    me: Option<&str>,
) -> Vec<&'a IssueListing> {
    listings
        .iter()
        .filter(|l| filters.all_states || l.state.is_none_or(IssueState::is_open))
        .filter(|l| match &filters.assignee {
            None => true,
            Some(Assignee::Me) => me.is_some() && l.assignee.as_deref() == me,
            Some(Assignee::Nobody) => l.assignee.is_none(),
            Some(Assignee::Login(login)) => l.assignee.as_ref() == Some(login),
        })
        .filter(|l| filters.priority.is_none_or(|p| l.priority == p))
        .filter(|l| (filters.label.as_ref()).is_none_or(|label| l.labels.contains(label)))
        .filter(|l| (filters.repository.as_ref()).is_none_or(|r| l.repositories.contains(r)))
        .collect()
}

/// The listings grouped by state in the list's order, with those that cannot be read first.
pub(super) fn grouped<'a>(
    listings: &[&'a IssueListing],
) -> Vec<(Option<IssueState>, Vec<&'a IssueListing>)> {
    let states = std::iter::once(None).chain(LIST_ORDER.map(Some));
    states
        .filter_map(|state| {
            let issues: Vec<&IssueListing> = listings
                .iter()
                .copied()
                .filter(|l| l.state == state)
                .collect();
            (!issues.is_empty()).then_some((state, issues))
        })
        .collect()
}

fn group_note(state: IssueState) -> Option<&'static str> {
    (state == IssueState::Triage).then_some("new requests nobody has sorted yet")
}

/// What a row says about the issue's work. Linked work arrives with phase 3; until then, who
/// filed it and when.
pub(super) fn work_line(listing: &IssueListing) -> String {
    match listing.author.is_empty() {
        true => String::new(),
        false => format!(
            "filed by {} · {}",
            listing.author,
            issue_view::ago(&listing.created)
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    Plain,
    Yellow,
    Red,
}

/// One footer line per tracker shown, the filtered Space's or every one here: the repository,
/// when it last synced and what waits; yellow while a file is paused, red once an error has
/// lasted more than an hour.
pub(super) fn footer_lines(
    spaces: &[Space],
    trackers: &[IssueTracker],
    filtered: Option<&SpaceId>,
    now: i64,
) -> Vec<(String, Tone)> {
    let mut lines = vec![];
    let mut named: Vec<&str> = vec![];
    for tracker in trackers {
        let shown = filtered.is_none_or(|id| &tracker.space_id == id);
        if !shown || tracker.prefix.is_none() || named.contains(&tracker.tracker.as_str()) {
            continue;
        }
        named.push(&tracker.tracker);
        let Some(space) = spaces.iter().find(|s| s.id == tracker.space_id) else {
            continue;
        };
        if tracker.local {
            lines.push((
                format!("{}’s local storage · on this Mac only", space.name),
                Tone::Plain,
            ));
            continue;
        }
        let mut parts = vec![tracker.tracker.clone()];
        parts.push(match tracker.synced_at {
            Some(at) => format!("synced {}", age_at(at, now)),
            None => "not synced yet".into(),
        });
        if tracker.waiting > 0 {
            let changes = wording::counted(tracker.waiting, "change");
            let wait = if tracker.waiting == 1 {
                "waits"
            } else {
                "wait"
            };
            parts.push(format!("{changes} {wait} to be pushed"));
        }
        let mut tone = Tone::Plain;
        if tracker.paused > 0 {
            parts.push(format!(
                "{} paused",
                wording::counted(tracker.paused, "file")
            ));
            tone = Tone::Yellow;
        }
        if let Some(error) = &tracker.error {
            let lasting = tracker
                .synced_at
                .is_none_or(|at| now - at > ERROR_TURNS_RED_MS);
            match lasting {
                true => {
                    parts.push(error.clone());
                    tone = Tone::Red;
                }
                false => parts.push("can’t reach GitHub, trying again".into()),
            }
        }
        lines.push((parts.join(" · "), tone));
    }
    lines
}

/// `wording::age` against a given now, so the footer can be tested.
fn age_at(at: i64, now: i64) -> String {
    let minutes = (now - at).max(0) / 60_000;
    match minutes {
        0 => "just now".into(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Assignee, Filters, Tone, filtered, footer_lines, grouped, initials, short_repository,
        unread, work_line,
    };
    use workspace_core::*;

    fn listing(id: &str, state: Option<IssueState>) -> IssueListing {
        IssueListing {
            id: IssueId::parse(id).unwrap(),
            tracker: "celadon-theme/storage".into(),
            title: "Theme previews look blurry on Retina displays".into(),
            state,
            priority: Priority::None,
            assignee: None,
            labels: vec![],
            repositories: vec![],
            author: "pshah".into(),
            created: "2026-10-29T14:02:11Z".into(),
            problem: state
                .is_none()
                .then(|| "The front matter is not valid".into()),
            unread: false,
        }
    }
    fn tracker(space: &str, key: &str) -> IssueTracker {
        IssueTracker {
            space_id: SpaceId(space.into()),
            tracker: key.into(),
            prefix: Some("CEL".into()),
            local: false,
            login: Some("jlin".into()),
            issues: 4,
            open: 3,
            unread: 2,
            waiting: 0,
            paused: 0,
            synced_at: Some(1_000_000),
            error: None,
        }
    }
    fn space(id: &str, name: &str) -> Space {
        Space {
            id: SpaceId(id.into()),
            name: name.into(),
            description: String::new(),
            instructions: String::new(),
            brain: None,
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: "7k2q".into() },
                folder: id.into(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        }
    }

    #[test]
    fn the_list_groups_by_state_with_unreadable_issues_first_and_closed_ones_last() {
        let listings = [
            listing("CEL-151", Some(IssueState::Todo)),
            listing("CEL-152", Some(IssueState::Triage)),
            listing("CEL-142", Some(IssueState::InProgress)),
            listing("CEL-127", Some(IssueState::Done)),
            listing("CEL-160", None),
        ];
        let all = Filters {
            all_states: true,
            ..Filters::default()
        };
        let shown = filtered(&listings, &all, None);
        let groups: Vec<Option<IssueState>> = grouped(&shown)
            .into_iter()
            .map(|(state, _)| state)
            .collect();
        assert_eq!(
            groups,
            [
                None,
                Some(IssueState::InProgress),
                Some(IssueState::Todo),
                Some(IssueState::Triage),
                Some(IssueState::Done)
            ]
        );
        let open = filtered(&listings, &Filters::default(), None);
        assert_eq!(open.len(), 4, "Done is hidden; an unreadable issue is not");
    }

    #[test]
    fn filters_keep_the_assignee_priority_label_and_repository_asked_for() {
        let mut mine = listing("CEL-142", Some(IssueState::Todo));
        mine.assignee = Some("jlin".into());
        mine.priority = Priority::High;
        mine.labels = vec!["bug".into()];
        mine.repositories = vec!["celadon-theme/celadon-chat".into()];
        let theirs = listing("CEL-148", Some(IssueState::Todo));
        let listings = [mine, theirs];
        let ids = |filters: Filters| -> Vec<String> {
            (filtered(&listings, &filters, Some("jlin")).into_iter())
                .map(|l| l.id.to_string())
                .collect()
        };
        let only = |assignee| Filters {
            assignee: Some(assignee),
            ..Filters::default()
        };
        assert_eq!(ids(only(Assignee::Me)), ["CEL-142"]);
        assert_eq!(ids(only(Assignee::Nobody)), ["CEL-148"]);
        assert_eq!(
            ids(only(Assignee::Login("pshah".into()))),
            Vec::<String>::new()
        );
        let urgent = Filters {
            priority: Some(Priority::High),
            label: Some("bug".into()),
            repository: Some("celadon-theme/celadon-chat".into()),
            ..Filters::default()
        };
        assert_eq!(ids(urgent), ["CEL-142"]);
    }

    #[test]
    fn the_sidebar_counts_a_shared_trackers_unread_issues_once() {
        let trackers = [
            tracker("sage", "Sage-Social/storage"),
            tracker("sage-watch", "Sage-Social/storage"),
            tracker("personal", "local-7k2q"),
        ];
        assert_eq!(unread(&trackers), 4);
    }

    #[test]
    fn the_footer_names_the_filtered_spaces_tracker_and_turns_red_after_an_hours_error() {
        let spaces = [space("sage", "Sage"), space("personal", "Personal")];
        let minute = 60_000;
        let mut sage = tracker("sage", "Sage-Social/storage");
        sage.synced_at = Some(0);
        sage.waiting = 1;
        let mut personal = tracker("personal", "local-7k2q");
        personal.local = true;
        let lines = |trackers: &[IssueTracker], filtered: Option<&SpaceId>, now| {
            footer_lines(&spaces, trackers, filtered, now)
        };
        let both = [sage.clone(), personal.clone()];
        assert_eq!(
            lines(&both, Some(&SpaceId("sage".into())), 2 * minute),
            [(
                "Sage-Social/storage · synced 2 min ago · 1 change waits to be pushed".to_owned(),
                Tone::Plain
            )]
        );
        assert_eq!(
            lines(&both, None, 2 * minute)[1],
            (
                "Personal’s local storage · on this Mac only".to_owned(),
                Tone::Plain
            )
        );
        sage.paused = 1;
        assert_eq!(lines(&[sage.clone()], None, minute)[0].1, Tone::Yellow);
        sage.error = Some("GitHub can’t be reached".into());
        assert_eq!(
            lines(&[sage.clone()], None, 30 * minute)[0].0,
            "Sage-Social/storage · synced 30 min ago · 1 change waits to be pushed · 1 file paused · can’t reach GitHub, trying again"
        );
        let red = &lines(&[sage], None, 61 * minute)[0];
        assert_eq!(red.1, Tone::Red);
        assert!(red.0.ends_with("GitHub can’t be reached"), "{}", red.0);
    }

    #[test]
    fn rows_name_who_filed_an_issue_and_initials_stand_for_logins() {
        let line = work_line(&listing("CEL-152", Some(IssueState::Triage)));
        assert!(line.starts_with("filed by pshah · "), "{line}");
        assert_eq!(initials("pshah"), "PS");
        assert_eq!(initials("tomas-reyes"), "TR");
        assert_eq!(
            short_repository("celadon-theme/celadon-chat"),
            "celadon-chat"
        );
    }
}
