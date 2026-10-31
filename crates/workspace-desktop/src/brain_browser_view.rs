//! The brain browser (OpenSpec change `brain-browser`, `design/mockups/current/brains.html`):
//! one brain's `main` as the host indexed it, scoped to a project or the whole brain, in four
//! views. Status renders STATUS.md beside the compile and the open PRs; Wiki has the note index
//! tree, the rendered note with its backlinks, and the sources of each claim; Logs is the
//! filtered timeline, 50 entries a page, with this Mac's pending entries; Conflicts lists the
//! reconcile queue. Nothing here edits the brain, and brain text is drawn by `brain_markdown`.
use super::*;
use brain_markdown::{LinkTarget, link_target};
use brains_view::{Tone, compiler_line, next_compile, stage_label};
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    text::TextView,
};
use ui::{banner, card, eyebrow, hint, icon, mono, pill, section, segment};
use wording::age;

/// How long a reading stands before a change to the workspace asks again.
const FRESH_MS: i64 = 10_000;
const NO_COMPILE: &str = "No compile yet";
const OUTSIDE: &str =
    "Logged outside this install: no Wiffletree ticket or conversation on this Mac";
const RULINGS: [&str; 4] = ["Uphold existing", "Keep new", "Keep both", "Defer"];
const RULINGS_LATER: &str = "Rulings come with brain compile, which writes the decision entry";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BrainView {
    Status,
    Wiki,
    Logs,
    Conflicts,
}
impl BrainView {
    const ALL: [Self; 4] = [Self::Status, Self::Wiki, Self::Logs, Self::Conflicts];
    fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Wiki => "Wiki",
            Self::Logs => "Logs",
            Self::Conflicts => "Conflicts",
        }
    }
}

/// What the browser opens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BrainTarget {
    View(BrainView),
    /// A note, by its path in the brain folder.
    Note(String),
    /// A log entry, by its id; `pending` when it is not on `main` yet.
    Entry {
        id: String,
        pending: bool,
    },
}

/// Opens the browser on `repository` (`owner/name`), in `project`'s scope or the whole brain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BrainOpening {
    pub(super) repository: String,
    pub(super) project: Option<String>,
    pub(super) target: BrainTarget,
}

/// The GitHub brain a Space names, as `owner/name`; a local brain has no index to browse.
pub(super) fn github_brain(space: &Space) -> Option<String> {
    match &space.brain.as_ref()?.repository {
        BrainRepository::GitHub { owner, name, .. } => Some(format!("{owner}/{name}")),
        BrainRepository::Local { .. } => None,
    }
}

/// The projects whose Space names `repository` as its brain: the browser's scope chips.
pub(super) fn projects_of_brain<'a>(snapshot: &'a Snapshot, repository: &str) -> Vec<&'a Project> {
    let names = |project: &&Project| {
        (snapshot.spaces.iter())
            .find(|s| s.id == project.space)
            .and_then(github_brain)
            .is_some_and(|brain| brain.eq_ignore_ascii_case(repository))
    };
    snapshot.projects.iter().filter(names).collect()
}

type Reading<T> = Option<Result<T, String>>;

/// The note shown, with its Markdown prepared once.
struct ShownNote {
    note: BrainNote,
    markdown: SharedString,
}

pub(super) struct BrainBrowser {
    bridge: Bridge,
    shared: Shared,
    repository: Option<String>,
    scope: Option<String>,
    view: BrainView,
    asked_at: i64,
    brains: Reading<BrainsPage>,
    status: Reading<(BrainStatus, SharedString)>,
    tree: Reading<BrainNoteTree>,
    /// The selected note's path, and the note once read.
    note_path: Option<String>,
    note: Reading<ShownNote>,
    claim: Option<String>,
    logs: Reading<BrainLogPage>,
    filter: BrainLogFilter,
    from: BrainLogsFrom,
    focused_entry: Option<String>,
    /// The repositories and authors the brain's pages listed, which the filters offer.
    seen_repositories: BTreeSet<String>,
    seen_authors: BTreeSet<String>,
    conflicts: Reading<Vec<BrainConflict>>,
}
impl EventEmitter<Intent> for BrainBrowser {}
impl BrainBrowser {
    pub(super) fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            shared: Shared::default(),
            repository: None,
            scope: None,
            view: BrainView::Status,
            asked_at: 0,
            brains: None,
            status: None,
            tree: None,
            note_path: None,
            note: None,
            claim: None,
            logs: None,
            filter: BrainLogFilter::default(),
            from: BrainLogsFrom::Newest,
            focused_entry: None,
            seen_repositories: BTreeSet::new(),
            seen_authors: BTreeSet::new(),
            conflicts: None,
        }
    }

    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        self.shared = shared;
        if self.shared.page != Page::BrainBrowser {
            return;
        }
        if moved && ui::now() - self.asked_at > FRESH_MS {
            self.read(window, cx);
        }
        if moved {
            cx.notify();
        }
    }

    /// Shows `opening`: a new brain or scope reads everything again.
    pub(super) fn open(
        &mut self,
        opening: BrainOpening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let brain_changed = self.repository.as_deref() != Some(opening.repository.as_str());
        if brain_changed {
            self.seen_repositories.clear();
            self.seen_authors.clear();
            self.note_path = None;
        }
        self.repository = Some(opening.repository);
        self.scope = opening.project;
        self.claim = None;
        self.focused_entry = None;
        self.filter = BrainLogFilter::default();
        self.from = BrainLogsFrom::Newest;
        match opening.target {
            BrainTarget::View(view) => self.view = view,
            BrainTarget::Note(path) => {
                self.view = BrainView::Wiki;
                self.note_path = Some(path);
            }
            BrainTarget::Entry { id, pending } => {
                self.view = BrainView::Logs;
                if !pending {
                    self.from = BrainLogsFrom::At(id.clone());
                }
                self.focused_entry = Some(id);
            }
        }
        self.read(window, cx);
        cx.notify();
    }

    /// The brain and scope ⌘K searches while the browser shows them.
    pub(super) fn searched(&self) -> Option<(String, Option<String>)> {
        Some((self.repository.clone()?, self.scope.clone()))
    }

    fn opening(&self, target: BrainTarget) -> Option<BrainOpening> {
        Some(BrainOpening {
            repository: self.repository.clone()?,
            project: self.scope.clone(),
            target,
        })
    }

    /// Opens a target in the same brain and scope.
    fn go(&mut self, target: BrainTarget, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opening) = self.opening(target) {
            self.open(opening, window, cx);
        }
    }

    fn set_scope(&mut self, scope: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = scope;
        self.from = BrainLogsFrom::Newest;
        self.focused_entry = None;
        self.read(window, cx);
        cx.notify();
    }

    fn set_view(&mut self, view: BrainView, window: &mut Window, cx: &mut Context<Self>) {
        self.view = view;
        self.read(window, cx);
        cx.notify();
    }

    /// Reads what the view shows, with the brains and the conflicts every view counts.
    fn read(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repository) = self.repository.clone() else {
            return;
        };
        self.asked_at = ui::now();
        self.ask(ask::brains(), window, cx, |view, read, _, cx| {
            view.brains = Some(read);
            cx.notify();
        });
        let scope = self.scope.clone();
        let asked = (repository.clone(), scope.clone());
        self.ask(
            ask::brain_conflicts(repository.clone(), scope.clone()),
            window,
            cx,
            move |view, read, _, cx| {
                if view.asked() == asked {
                    view.conflicts = Some(read);
                    cx.notify();
                }
            },
        );
        match self.view {
            BrainView::Status => {
                let asked = repository.clone();
                self.ask(
                    ask::brain_status(repository),
                    window,
                    cx,
                    move |view, read, _, cx| {
                        if view.repository.as_ref() == Some(&asked) {
                            view.status = Some(read.map(|status| {
                                let body = status.body.as_deref().unwrap_or_default();
                                let markdown = brain_markdown::prepare(body, &status.links);
                                (status, markdown.into())
                            }));
                            cx.notify();
                        }
                    },
                );
            }
            BrainView::Wiki => {
                let asked = (repository.clone(), scope.clone());
                self.ask(
                    ask::brain_notes(repository, scope),
                    window,
                    cx,
                    move |view, read, window, cx| {
                        if view.asked() == asked {
                            view.tree = Some(read);
                            view.read_note(window, cx);
                        }
                    },
                );
            }
            BrainView::Logs => self.read_logs(window, cx),
            BrainView::Conflicts => {}
        }
    }

    fn asked(&self) -> (String, Option<String>) {
        (
            self.repository.clone().unwrap_or_default(),
            self.scope.clone(),
        )
    }

    /// Reads the selected note, or the tree's first when none is selected.
    fn read_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = (self.tree.as_ref().and_then(|t| t.as_ref().ok()))
            .and_then(|tree| tree_rows(tree).next().map(|row| row.path.clone()));
        let Some(path) = self.note_path.clone().or(first) else {
            self.note = None;
            return cx.notify();
        };
        let Some(repository) = self.repository.clone() else {
            return;
        };
        self.note_path = Some(path.clone());
        let asked = path.clone();
        self.ask(
            ask::brain_note(repository, path),
            window,
            cx,
            move |view, read, _, cx| {
                if view.note_path.as_ref() == Some(&asked) {
                    view.note = Some(read.map(|note| ShownNote {
                        markdown: brain_markdown::prepare(&note.body, &note.links).into(),
                        note,
                    }));
                    cx.notify();
                }
            },
        );
    }

    fn select_note(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.note_path = Some(path);
        self.claim = None;
        self.view = BrainView::Wiki;
        if self.tree.is_none() {
            return self.read(window, cx);
        }
        self.read_note(window, cx);
        cx.notify();
    }

    fn read_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let asked = (self.asked(), self.filter.clone(), self.from.clone());
        let ask = ask::brain_logs(
            repository,
            self.scope.clone(),
            self.filter.clone(),
            self.from.clone(),
        );
        self.ask(ask, window, cx, move |view, read, _, cx| {
            if (view.asked(), view.filter.clone(), view.from.clone()) != asked {
                return;
            }
            if let Ok(page) = &read {
                for entry in page.entries.iter().chain(&page.pending) {
                    view.seen_repositories.insert(entry.repository.clone());
                    view.seen_authors.insert(entry.author.clone());
                }
            }
            view.logs = Some(read);
            cx.notify();
        });
    }

    fn page_logs(&mut self, from: BrainLogsFrom, window: &mut Window, cx: &mut Context<Self>) {
        self.from = from;
        self.focused_entry = None;
        self.read_logs(window, cx);
        cx.notify();
    }

    fn set_filter(&mut self, filter: BrainLogFilter, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = filter;
        self.page_logs(BrainLogsFrom::Newest, window, cx);
    }

    /// Acts on a clicked link in a rendered note or STATUS, only ever because the human clicked.
    fn follow(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        match link_target(url) {
            LinkTarget::Note(path) => self.select_note(path, window, cx),
            LinkTarget::NoteNamed(key) => {
                let found = (self.tree.as_ref().and_then(|t| t.as_ref().ok())).and_then(|tree| {
                    tree_rows(tree)
                        .find(|row| {
                            brain_markdown::link_key(&row.path) == key
                                || row.title.to_lowercase() == key
                        })
                        .map(|row| row.path.clone())
                });
                match found {
                    Some(path) => self.select_note(path, window, cx),
                    None => toast_error("The brain has no such note in this scope.", window, cx),
                }
            }
            LinkTarget::Claim(label) => {
                self.claim = Some(label);
                cx.notify();
            }
            LinkTarget::Entry(id) => self.go(BrainTarget::Entry { id, pending: false }, window, cx),
            LinkTarget::Web(url) => cx.open_url(&url),
            LinkTarget::Ignored => {}
        }
    }

    fn markdown(&self, id: String, text: &SharedString, cx: &mut Context<Self>) -> TextView {
        let weak = cx.weak_entity();
        TextView::markdown(SharedString::from(id), text.clone())
            .selectable(true)
            .on_link_click(move |url, _, window, cx| {
                let url = url.to_string();
                let _ = weak.update(cx, |view, cx| view.follow(&url, window, cx));
            })
            .text_size(px(font::BODY))
            .line_height(relative(1.5))
    }

    fn listing(&self) -> Option<&BrainsPage> {
        self.brains.as_ref()?.as_ref().ok()
    }

    fn brain(&self) -> Option<&Brain> {
        let repository = self.repository.as_deref()?;
        (self.listing()?.brains.iter()).find(|b| b.repository.eq_ignore_ascii_case(repository))
    }

    fn open_conflicts(&self) -> usize {
        (self.conflicts.as_ref().and_then(|c| c.as_ref().ok())).map_or(0, Vec::len)
    }

    fn head(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let repository = self.repository.clone().unwrap_or_default();
        let weak = cx.weak_entity();
        let others: Vec<String> = (self.listing().into_iter())
            .flat_map(|l| l.brains.iter())
            .filter(|b| !b.spaces.is_empty())
            .map(|b| b.repository.clone())
            .collect();
        let switcher = button("brain-switcher")
            .outline()
            .label("Switch brain")
            .dropdown_caret(true)
            .disabled(others.len() < 2)
            .dropdown_menu(move |mut menu, _, _| {
                for brain in &others {
                    let (weak, brain) = (weak.clone(), brain.clone());
                    menu = menu.item(PopupMenuItem::new(brain.clone()).on_click(
                        move |_, window, cx| {
                            let opening = BrainOpening {
                                repository: brain.clone(),
                                project: None,
                                target: BrainTarget::View(BrainView::Status),
                            };
                            let _ = weak.update(cx, |view, cx| view.open(opening, window, cx));
                        },
                    ));
                }
                menu
            });
        let synced = self.brain().map(brains_view::sync_state);
        let mut top = div()
            .flex()
            .items_center()
            .gap_2()
            .child(icon("brain").text_color(p.subtle))
            .child(
                div()
                    .text_size(px(font::HEADING))
                    .font_weight(FontWeight::MEDIUM)
                    .child(repository.clone()),
            )
            .child(switcher);
        if let Some((sync, tone)) = synced {
            top = top.child(pill(sync, tone.color(p)));
        }
        top = top.child(div().flex_1()).child(
            button("brain-search")
                .ghost()
                .icon(icon("search"))
                .label("Search notes and logs  ⌘K")
                .on_click(cx.listener(|view, _, _, cx| {
                    if let Some(repository) = view.repository.clone() {
                        cx.emit(Intent::SearchBrain {
                            repository,
                            project: view.scope.clone(),
                        })
                    }
                })),
        );
        let mut chips = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(hint("Show", p));
        let projects: Vec<(String, String, usize)> = (self.shared.snapshot().into_iter())
            .flat_map(|s| {
                projects_of_brain(s, &repository)
                    .into_iter()
                    .map(|project| {
                        let repositories = s.project_repositories(&project.id).len();
                        (project.id.clone(), project.name.clone(), repositories)
                    })
            })
            .collect();
        for (id, name, repositories) in projects {
            let on = self.scope.as_ref() == Some(&id);
            chips = chips.child(
                segment(
                    SharedString::from(format!("scope-{id}")),
                    format!(
                        "This project · {name} ({repositories} {})",
                        wording::plural(repositories, "repo", "repos")
                    ),
                    on,
                )
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.set_scope(Some(id.clone()), window, cx)
                })),
            );
        }
        chips = chips.child(
            segment("scope-whole", "Whole brain", self.scope.is_none())
                .on_click(cx.listener(|view, _, window, cx| view.set_scope(None, window, cx))),
        );
        let mut tabs = div().flex().gap_1();
        for tab in BrainView::ALL {
            let count = self.open_conflicts();
            let label = match tab {
                BrainView::Conflicts if count > 0 => format!("Conflicts · {count}"),
                _ => tab.label().to_owned(),
            };
            tabs = tabs.child(
                segment(
                    SharedString::from(format!("tab-{}", tab.label())),
                    label,
                    self.view == tab,
                )
                .on_click(cx.listener(move |view, _, window, cx| view.set_view(tab, window, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .pb_3()
            .border_b_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .child(top)
            .child(chips)
            .child(tabs)
    }

    fn body(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        if self.repository.is_none() {
            return div().child(hint(
                "Open a brain from the Brains page or a project's Memory panel.",
                p,
            ));
        }
        let view = match self.view {
            BrainView::Status => self.status_view(p, cx),
            BrainView::Wiki => self.wiki_view(p, cx),
            BrainView::Logs => self.logs_view(p, cx),
            BrainView::Conflicts => self.conflicts_view(p, cx),
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.head(p, cx))
            .child(view)
    }

    fn status_view(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let (status, markdown) = match &self.status {
            None => return hint("Reading STATUS…", p),
            Some(Err(error)) => return unreadable(error, p),
            Some(Ok((status, markdown))) => (status, markdown),
        };
        let mut article = div().flex_1().min_w_0().flex().flex_col().gap_2().child(
            div()
                .text_size(px(font::FIGURE_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .child("Status"),
        );
        article = match &status.body {
            Some(_) => article.child(mono("STATUS.md", p)).child(self.markdown(
                "brain-status".into(),
                markdown,
                cx,
            )),
            None => article.child(hint(
                format!("{NO_COMPILE}: STATUS.md is written by the first compile."),
                p,
            )),
        };
        let config = self.brain().map(|b| &b.config);
        let here = self.listing().and_then(|l| l.this_host.as_ref());
        let fact = |label: &'static str, value: String| ui::property(label, value, p);
        let compile = card(p)
            .child(eyebrow("COMPILE", p))
            .child(fact(
                "Last",
                status.last_compile.map_or(NO_COMPILE.into(), age),
            ))
            .child(fact("Next", config.map_or("—".into(), next_compile)))
            .child(fact(
                "Runs on",
                config.map_or("—".into(), |c| compiler_line(c, here)),
            ))
            .child(fact("Pending entries", status.pending_entries.to_string()));
        let mut prs = card(p).child(eyebrow("OPEN PRS", p));
        let repository = self.repository.as_deref().unwrap_or_default();
        let open: Vec<&BrainBatch> = (self.listing().into_iter())
            .flat_map(|l| l.batches.iter())
            .filter(|b| b.repository.eq_ignore_ascii_case(repository) && !b.is_finished())
            .collect();
        if open.is_empty() {
            prs = prs.child(hint("None", p));
        }
        for batch in open {
            let id = batch.id.clone();
            let failed = batch.stage == BrainStage::Failed;
            let number = batch
                .pull_request
                .as_ref()
                .map_or("queued".into(), |pr| format!("#{}", pr.number));
            prs = prs.child(
                div()
                    .id(SharedString::from(format!("status-pr-{id}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .text_size(px(font::SMALL))
                    .hover(|d| d.bg(p.overlay))
                    .rounded_md()
                    .px_1()
                    .on_click(
                        cx.listener(move |_, _, _, cx| cx.emit(Intent::OpenBatch(id.clone()))),
                    )
                    .child(icon("git-pull-request").size(px(13.)).text_color(p.subtle))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(format!("{number} · {}", batch.title)),
                    )
                    .child(
                        div()
                            .text_color(if failed { p.red } else { p.subtle })
                            .child(stage_name(batch.stage)),
                    ),
            );
        }
        div().flex().items_start().gap_4().child(article).child(
            div()
                .w(px(260.))
                .flex_none()
                .flex()
                .flex_col()
                .gap_3()
                .child(compile)
                .child(prs),
        )
    }

    fn wiki_view(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let tree = match &self.tree {
            None => return hint("Reading the note index…", p),
            Some(Err(error)) => return unreadable(error, p),
            Some(Ok(tree)) => tree,
        };
        let mut nav = div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(mono("notes/INDEX.md", p));
        let groups = (tree.groups.iter())
            .map(|g| (g.heading.as_str(), &g.notes))
            .chain(
                (!tree.not_in_index.is_empty()).then_some(("Not in the index", &tree.not_in_index)),
            );
        for (heading, notes) in groups {
            if notes.is_empty() {
                continue;
            }
            nav = nav.child(eyebrow(heading.to_uppercase(), p).pt_2().pb_1());
            for row in notes {
                nav = nav.child(self.tree_row(row, p, cx));
            }
        }
        if tree.hidden > 0 {
            nav = nav.child(
                div()
                    .pt_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(hint(hidden_line(tree.hidden), p))
                    .child(
                        button("show-all-notes")
                            .link()
                            .xsmall()
                            .label("show all")
                            .on_click(
                                cx.listener(|view, _, window, cx| view.set_scope(None, window, cx)),
                            ),
                    ),
            );
        }
        if tree_rows(tree).next().is_none() {
            nav = nav.child(hint(format!("{NO_COMPILE}: no notes in this scope."), p));
        }
        let (article, sources) = match &self.note {
            None if self.note_path.is_none() => (hint("No note to show.", p), None),
            None => (hint("Reading the note…", p), None),
            Some(Err(error)) => (unreadable(error, p), None),
            Some(Ok(shown)) => (
                self.article(shown, p, cx),
                Some(self.sources(&shown.note, p, cx)),
            ),
        };
        div()
            .flex()
            .items_start()
            .gap_4()
            .child(nav)
            .child(article.flex_1().min_w_0())
            .children(sources.map(|s| s.w(px(260.)).flex_none()))
    }

    fn tree_row(&self, row: &BrainNoteRow, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let current = self.note_path.as_ref() == Some(&row.path);
        let path = row.path.clone();
        div()
            .id(SharedString::from(format!("note-{}", row.path)))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(3.))
            .rounded_md()
            .cursor_pointer()
            .text_size(px(font::SMALL))
            .when(current, |d| d.bg(p.overlay).font_weight(FontWeight::MEDIUM))
            .hover(|d| d.bg(p.overlay))
            .on_click(
                cx.listener(move |view, _, window, cx| view.select_note(path.clone(), window, cx)),
            )
            .child(
                div()
                    .size(px(7.))
                    .flex_none()
                    .rounded_full()
                    .bg(kind_color(row.kind, p)),
            )
            .child(div().min_w_0().text_ellipsis().child(row.title.clone()))
    }

    fn article(&self, shown: &ShownNote, p: Palette, cx: &mut Context<Self>) -> Div {
        let note = &shown.note;
        let mut meta = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .text_size(px(font::SMALL))
            .text_color(p.subtle);
        if let Some(kind) = note.kind {
            meta = meta.child(pill(
                format!("kind: {}", kind.name()),
                kind_color(Some(kind), p),
            ));
        }
        if let Some(updated) = &note.updated {
            meta = meta.child(format!("updated {updated}"));
        }
        meta = meta.child(format!(
            "· {} sourced {}",
            note.claims.len(),
            wording::plural(note.claims.len(), "claim", "claims")
        ));
        let mut backlinks = div().flex().flex_wrap().gap_1();
        for row in &note.backlinks {
            let path = row.path.clone();
            let status = path == "STATUS.md";
            backlinks = backlinks.child(
                button(SharedString::from(format!("backlink-{path}")))
                    .ghost()
                    .xsmall()
                    .label(if status {
                        "STATUS".into()
                    } else {
                        row.title.clone()
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        if status {
                            view.set_view(BrainView::Status, window, cx)
                        } else {
                            view.select_note(path.clone(), window, cx)
                        }
                    })),
            );
        }
        if note.backlinks.is_empty() {
            backlinks = backlinks.child(hint("No notes link here.", p));
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(font::FIGURE_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .child(note.title.clone()),
            )
            .child(meta)
            .child(self.markdown(format!("brain-note-{}", note.path), &shown.markdown, cx))
            .child(
                div()
                    .pt_3()
                    .mt_2()
                    .border_t_1()
                    .border_color(p.edge.opacity(edge::SEPARATOR))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(eyebrow("BACKLINKS", p))
                    .child(backlinks),
            )
    }

    fn sources(&self, note: &BrainNote, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut list = card(p)
            .child(eyebrow("SOURCES", p))
            .child(hint("claim → log entry → commit and PR", p));
        if !note.sources_readable {
            list = list.child(
                banner(p.yellow).child("Some sources are not readable: a footnote names no entry."),
            );
        }
        if note.claims.is_empty() {
            list = list.child(hint("This note cites no entries.", p));
        }
        for claim in &note.claims {
            let lit = self.claim.as_ref() == Some(&claim.label);
            let label = claim.label.clone();
            let mut block = div()
                .id(SharedString::from(format!("claim-{}", claim.label)))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(if lit {
                    p.focus
                } else {
                    p.edge.opacity(edge::HAIRLINE)
                })
                .when(lit, |d| d.bg(p.focus.opacity(0.08)))
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.claim = Some(label.clone());
                    cx.notify();
                }))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(font::SMALL))
                        .child(format!("[{}]", claim.label)),
                );
            for source in &claim.sources {
                block = block.child(self.source_row(source, p, cx));
            }
            list = list.child(block);
        }
        list
    }

    fn source_row(&self, source: &ClaimSource, p: Palette, cx: &mut Context<Self>) -> Div {
        match source {
            ClaimSource::Broken { target } => div()
                .text_size(px(font::SMALL))
                .text_color(p.red)
                .child(format!("Missing entry · {target}")),
            ClaimSource::Entry {
                id,
                date,
                author,
                anchors,
                verification,
                ..
            } => {
                let open = id.clone();
                let mut row = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .text_size(px(font::SMALL))
                    .child(
                        button(SharedString::from(format!("source-{id}")))
                            .link()
                            .xsmall()
                            .label(format!("{date} · {}", short_id(id)))
                            .tooltip("Open the entry in Logs")
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.go(
                                    BrainTarget::Entry {
                                        id: open.clone(),
                                        pending: false,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(div().text_color(p.subtle).child(author.clone()));
                for anchor in anchor_labels(anchors) {
                    row = row
                        .child(div().text_color(p.subtle).child("→"))
                        .child(mono(anchor, p));
                }
                row.child(verification_pill(verification, p))
            }
        }
    }

    fn logs_view(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut filters = div().flex().flex_wrap().items_center().gap_2();
        let repositories: Vec<String> = self.seen_repositories.iter().cloned().collect();
        let authors: Vec<String> = self.seen_authors.iter().cloned().collect();
        filters = filters
            .child(self.filter_menu(
                "Repo",
                self.filter.repository.clone(),
                repositories,
                |f, v| f.repository = v,
                cx,
            ))
            .child(self.filter_menu(
                "Type",
                self.filter.kind.map(|k| k.name().to_owned()),
                vec!["decision".into(), "observation".into()],
                |f, v| f.kind = v.as_deref().and_then(BrainKind::parse),
                cx,
            ))
            .child(self.filter_menu(
                "Author",
                self.filter.author.clone(),
                authors,
                |f, v| f.author = v,
                cx,
            ))
            .child(self.filter_menu(
                "Verification",
                self.filter.verification.map(|v| v.name().to_owned()),
                vec!["verified".into(), "claimed".into(), "unverified".into()],
                |f, v| f.verification = v.as_deref().and_then(RefState::parse),
                cx,
            ));
        if !self.filter.is_empty() {
            filters = filters.child(
                button("clear-filters")
                    .link()
                    .xsmall()
                    .label("Clear filters")
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.set_filter(BrainLogFilter::default(), window, cx)
                    })),
            );
        }
        filters = filters
            .child(div().flex_1())
            .child(hint("Newest first · 50 a page", p));
        let page = match &self.logs {
            None => {
                return div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(filters)
                    .child(hint("Reading the logs…", p));
            }
            Some(Err(error)) => {
                return div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(filters)
                    .child(unreadable(error, p));
            }
            Some(Ok(page)) => page,
        };
        let mut list = div().flex().flex_col().gap_3().child(filters);
        if !page.pending.is_empty() {
            list = list.child(section("PENDING · THIS MAC'S ENTRIES NOT ON MAIN YET", p));
            for entry in &page.pending {
                list = list.child(self.entry_card(entry, p, cx));
            }
            list = list.child(section("ON MAIN", p));
        }
        if page.entries.is_empty() {
            list = list.child(hint(
                if self.filter.is_empty() {
                    "No entries in this scope yet."
                } else {
                    "No entries match these filters."
                },
                p,
            ));
        }
        for entry in &page.entries {
            list = list.child(self.entry_card(entry, p, cx));
        }
        let (newer, older) = (page.newer.clone(), page.older.clone());
        list.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    button("logs-newer")
                        .ghost()
                        .label("‹ Newer")
                        .disabled(newer.is_none())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            if let Some(id) = newer.clone() {
                                view.page_logs(BrainLogsFrom::After(id), window, cx)
                            }
                        })),
                )
                .child(
                    button("logs-older")
                        .ghost()
                        .label("Older ›")
                        .disabled(older.is_none())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            if let Some(id) = older.clone() {
                                view.page_logs(BrainLogsFrom::Before(id), window, cx)
                            }
                        })),
                ),
        )
    }

    /// One filter's menu: All, then each value; `set` writes the choice into the filter.
    fn filter_menu(
        &self,
        label: &'static str,
        chosen: Option<String>,
        values: Vec<String>,
        set: fn(&mut BrainLogFilter, Option<String>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let weak = cx.weak_entity();
        let shown = chosen.clone().unwrap_or_else(|| "All".into());
        let filter = self.filter.clone();
        button(SharedString::from(format!("filter-{label}")))
            .outline()
            .xsmall()
            .label(format!("{label}: {shown}"))
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for value in std::iter::once(None).chain(values.iter().cloned().map(Some)) {
                    let (weak, mut filter) = (weak.clone(), filter.clone());
                    let name = value.clone().unwrap_or_else(|| "All".into());
                    let checked = value == chosen;
                    set(&mut filter, value);
                    menu = menu.item(PopupMenuItem::new(name).checked(checked).on_click(
                        move |_, window, cx| {
                            let filter = filter.clone();
                            let _ = weak.update(cx, |view, cx| view.set_filter(filter, window, cx));
                        },
                    ));
                }
                menu
            })
    }

    fn entry_card(&self, entry: &BrainEntry, p: Palette, cx: &mut Context<Self>) -> Div {
        let focused = self.focused_entry.as_ref() == Some(&entry.id);
        let (pr, tone) = pull_request_label(&entry.pull_request);
        let mut head = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(entry.title.clone()),
            )
            .child(pill(entry.kind.name(), kind_color(Some(entry.kind), p)))
            .child(verification_pill(&entry.verification, p));
        if entry.pending {
            head = head.child(pill("pending", p.yellow));
        }
        head = head.child(div().flex_1()).child(pill(pr, tone.color(p)));
        let field = |name: &'static str, value: Div| {
            div()
                .flex()
                .gap_3()
                .text_size(px(font::SMALL))
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_color(p.subtle)
                        .child(name),
                )
                .child(value.flex_1().min_w_0())
        };
        let mut links = div().flex().flex_wrap().gap_2();
        match &entry.origin {
            Some(origin) => {
                links = links.children(self.origin_links(origin, cx));
            }
            None => links = links.child(hint(OUTSIDE, p)),
        }
        card(p)
            .when(focused, |d| {
                d.border_color(p.focus).bg(p.focus.opacity(0.06))
            })
            .child(head)
            .child(hint(
                format!(
                    "{} · {} · {} · via {}",
                    entry.date,
                    entry.repository,
                    entry.author,
                    source_name(&entry.source)
                ),
                p,
            ))
            .child(mono(entry.path.clone(), p))
            .child(field("ref", mono(entry.reference.clone(), p)))
            .child(field("changed", div().child(entry.changed.clone())))
            .child(field("why", div().child(entry.why.clone())))
            .child(field("state", div().child(entry.state.clone())))
            .child(links)
    }

    /// The ticket and conversation an entry this Mac wrote came from.
    fn origin_links(&self, origin: &EntryOrigin, cx: &mut Context<Self>) -> Vec<Button> {
        let mut links = vec![];
        let index = self.shared.index.as_deref();
        if let Some(ticket) = (origin.ticket_id.as_deref()).and_then(|id| index?.ticket(id)) {
            let agent = (index.and_then(|i| i.agents(&ticket.id).next()))
                .map(|s| s.id.clone())
                .unwrap_or_else(|| ticket.coordinator_id.clone());
            links.push(
                button(SharedString::from(format!(
                    "ticket-{}-{}",
                    origin.session_id, ticket.id
                )))
                .link()
                .xsmall()
                .icon(icon("task"))
                .label(format!("Ticket · {}", ticket.title))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Intent::Select(agent.clone())))),
            );
        }
        let session = origin.session_id.clone();
        let name = (index.and_then(|i| i.session(&session)))
            .map_or_else(|| "this Mac".into(), |s| s.name.clone());
        links.push(
            button(SharedString::from(format!("conversation-{session}")))
                .link()
                .xsmall()
                .icon(icon("activity"))
                .label(format!("Conversation · {name}"))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Intent::Select(session.clone())))),
        );
        links
    }

    fn conflicts_view(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let intro = hint(
            "Contradictions of established decisions, disagreements between agents and deprecations wait here instead of changing the wiki.",
            p,
        );
        let conflicts = match &self.conflicts {
            None => return hint("Reading the conflicts…", p),
            Some(Err(error)) => return unreadable(error, p),
            Some(Ok(conflicts)) => conflicts,
        };
        let mut list = div().flex().flex_col().gap_3().child(intro);
        if conflicts.is_empty() {
            return list.child(hint("No conflicts in this scope.", p));
        }
        for conflict in conflicts {
            list = list.child(self.conflict_card(conflict, p, cx));
        }
        list
    }

    fn conflict_card(&self, conflict: &BrainConflict, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut head = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(conflict.title.clone()),
            )
            .child(pill("conflict", p.yellow));
        if let Some(rule) = &conflict.rule {
            head = head.child(pill(rule.clone(), p.subtle));
        }
        if let Some(repository) = &conflict.repository {
            head = head.child(hint(repository.clone(), p));
        }
        let side = |label: &'static str, text: &str| {
            div()
                .flex_1()
                .min_w_0()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(p.edge.opacity(edge::HAIRLINE))
                .flex()
                .flex_col()
                .gap_1()
                .child(eyebrow(label, p))
                .child(div().text_size(px(font::SMALL)).child(text.to_owned()))
        };
        let mut claim = side("NEW CLAIM", &conflict.claim);
        if let Some(id) = conflict.entry_id.clone() {
            claim = claim.child(
                button(SharedString::from(format!(
                    "conflict-entry-{}",
                    conflict.id
                )))
                .link()
                .xsmall()
                .label(format!("Entry {}", short_id(&id)))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.go(
                        BrainTarget::Entry {
                            id: id.clone(),
                            pending: false,
                        },
                        window,
                        cx,
                    )
                })),
            );
        }
        let mut rulings = div().flex().flex_wrap().gap_2();
        for ruling in RULINGS {
            rulings = rulings.child(
                button(SharedString::from(format!("rule-{}-{ruling}", conflict.id)))
                    .outline()
                    .xsmall()
                    .label(ruling)
                    .disabled(true)
                    .tooltip(RULINGS_LATER),
            );
        }
        card(p)
            .child(head)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(claim)
                    .child(side("ESTABLISHED DECISION", &conflict.conflicts_with)),
            )
            .child(eyebrow("EVIDENCE", p))
            .child(
                div()
                    .text_size(px(font::SMALL))
                    .child(conflict.evidence.clone()),
            )
            .child(hint(format!("Recommended: {}", conflict.recommend), p))
            .child(rulings)
    }
}

impl Requests for BrainBrowser {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

impl Render for BrainBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("BrainBrowser");
        let p = palette(is_dark(cx));
        ui::page(
            "Brain browser",
            "Brains",
            self.body(p, cx),
            None,
            &self.shared.host,
            p,
            cx,
        )
    }
}

/// Every note the tree lists, in its order.
fn tree_rows(tree: &BrainNoteTree) -> impl Iterator<Item = &BrainNoteRow> {
    (tree.groups.iter())
        .flat_map(|g| g.notes.iter())
        .chain(&tree.not_in_index)
}

fn unreadable(error: &str, p: Palette) -> Div {
    hint(format!("Could not read the brain: {error}"), p)
}

fn hidden_line(hidden: usize) -> String {
    format!(
        "{hidden} {} outside this project hidden ·",
        wording::plural(hidden, "note", "notes")
    )
}

fn kind_color(kind: Option<BrainKind>, p: Palette) -> Hsla {
    match kind {
        Some(BrainKind::Decision) => p.accent,
        Some(BrainKind::Observation) => p.focus,
        None => p.subtle,
    }
}

pub(super) fn verification_pill(verification: &RefVerification, p: Palette) -> Div {
    let color = match verification.state {
        RefState::Verified => p.green,
        RefState::Claimed => p.yellow,
        RefState::Unverified => p.subtle,
    };
    let text = match &verification.reason {
        Some(reason) => format!("{} · {reason}", verification.state.name()),
        None => verification.state.name().to_owned(),
    };
    pill(text, color)
}

/// An entry's PR as its pill says it: the batch's live stage, merged, or no PR.
pub(super) fn pull_request_label(pr: &EntryPullRequest) -> (String, Tone) {
    match pr {
        EntryPullRequest::Batch { number, stage } => {
            let tone = match stage {
                BrainStage::Failed => Tone::Bad,
                BrainStage::Merged => Tone::Good,
                BrainStage::AutoMerging | BrainStage::Checks | BrainStage::PrOpen => Tone::Warn,
                BrainStage::Queued | BrainStage::Discarded => Tone::Quiet,
            };
            let name = stage_name(*stage);
            match number {
                Some(number) => (format!("PR #{number} · {name}"), tone),
                None => (format!("Batch · {name}"), tone),
            }
        }
        EntryPullRequest::Merged { number } => (format!("PR #{number} · merged"), Tone::Good),
        EntryPullRequest::None => ("no PR".into(), Tone::Quiet),
    }
}

/// A batch's stage as an entry's PR shows it, with a failed check named as one.
fn stage_name(stage: BrainStage) -> &'static str {
    match stage {
        BrainStage::Failed => "failed check",
        stage => stage_label(stage),
    }
}

fn source_name(source: &str) -> &str {
    match source {
        "skill" => "brain-log skill",
        "wiffletree" => "Wiffletree",
        other => other,
    }
}

/// A commit, PR and branch an entry's `ref` names, as the Sources list chains them.
fn anchor_labels(anchors: &RefAnchors) -> Vec<String> {
    let mut labels: Vec<String> = (anchors.commits.iter())
        .map(|sha| format!("commit {}", sha.get(..7).unwrap_or(sha)))
        .collect();
    labels.extend(anchors.pull_request.map(|n| format!("PR #{n}")));
    labels.extend(anchors.branch.as_ref().map(|b| format!("branch {b}")));
    labels
}

/// An entry id shortened for a line, as the mockup shows it.
pub(super) fn short_id(id: &str) -> String {
    match id.get(..10) {
        Some(head) if id.len() > 10 => format!("{head}…"),
        _ => id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{anchor_labels, hidden_line, projects_of_brain, pull_request_label, short_id};
    use crate::brains_view::Tone;
    use workspace_core::*;

    #[test]
    fn an_entrys_pr_shows_the_batch_stage_merged_or_no_pr() {
        let failed = EntryPullRequest::Batch {
            number: Some(214),
            stage: BrainStage::Failed,
        };
        assert_eq!(
            pull_request_label(&failed),
            ("PR #214 · failed check".into(), Tone::Bad)
        );
        let merging = EntryPullRequest::Batch {
            number: Some(213),
            stage: BrainStage::AutoMerging,
        };
        assert_eq!(pull_request_label(&merging).0, "PR #213 · auto-merging");
        let queued = EntryPullRequest::Batch {
            number: None,
            stage: BrainStage::Queued,
        };
        assert_eq!(pull_request_label(&queued).0, "Batch · queued");
        assert_eq!(
            pull_request_label(&EntryPullRequest::Merged { number: 81 }),
            ("PR #81 · merged".into(), Tone::Good)
        );
        assert_eq!(pull_request_label(&EntryPullRequest::None).0, "no PR");
    }

    #[test]
    fn a_source_chains_its_commits_pr_and_branch() {
        let anchors = RefAnchors {
            branch: Some("main".into()),
            commits: vec!["a41f2c0ffee".into()],
            pull_request: Some(81),
        };
        assert_eq!(
            anchor_labels(&anchors),
            ["commit a41f2c0", "PR #81", "branch main"]
        );
        assert_eq!(short_id("01jbzb7r5k1n3p5s7u9w1z3b5d"), "01jbzb7r5k…");
        assert_eq!(hidden_line(2), "2 notes outside this project hidden ·");
    }

    #[test]
    fn the_scope_chips_are_the_projects_of_spaces_that_name_the_brain() {
        let space = |id: &str, owner: &str, name: &str| Space {
            id: SpaceId(id.into()),
            name: id.into(),
            description: String::new(),
            instructions: String::new(),
            brain: Some(SpaceBrain {
                repository: BrainRepository::GitHub {
                    owner: owner.into(),
                    name: name.into(),
                    extra: Extra::new(),
                },
                folder: "second-brain".into(),
                extra: Extra::new(),
            }),
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: "l".into() },
                folder: id.into(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        };
        let snapshot = Snapshot {
            projects: serde_json::from_value(serde_json::json!([
                {"id": "relay", "name": "Watch app call relay", "space": "sage"},
                {"id": "site", "name": "Vyttle site", "space": "vyttle"}
            ]))
            .unwrap(),
            spaces: vec![
                space("sage", "Sage-Social", "sagehaven-docs"),
                space("vyttle", "Vyttle-LLC", "vyttle-docs"),
            ],
            ..Default::default()
        };
        let names: Vec<&str> = projects_of_brain(&snapshot, "sage-social/sagehaven-docs")
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["Watch app call relay"]);
    }
}
