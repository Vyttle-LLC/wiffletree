//! Settings › Spaces (OpenSpec changes `spaces` and `one-space-repository`): the Spaces down the
//! left, and the shown Space's page with its Repository box, or the forms that add a Space, new
//! or from a repository. Only the human edits a Space; no agent tool can.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use linear_import_view::LinearImportPanel;
use repository_box::BringingIn;
use repository_tree::{Choice, Tree, merged_list, shown_path};
use setup::{RepositoryChoice, shown_folder};
use std::collections::BTreeMap;
use ui::{banner, card, hint, icon, mono, section};
use wording::{age, counted, counted_as};

mod repository_box;
mod setup;

const SCANS: &str = "Before pushing, Wiffletree runs its personal-data and secret scans on this Mac and holds a file that fails, instead of pushing it.";
const ROUTER: &str = "The router reads this to decide where a message goes.";
const ALSO_SETTINGS: &str =
    "Your settings repository. Settings, the Space and issues each sync only their own folders.";
const SOURCES_FORMAT: &str =
    "One a line: mcp ‹server›: ‹tool›, ‹tool› or cli ‹command›: ‹prefix›, ‹prefix›.";
/// How long a storage reading stands before a change to the workspace asks again.
const STORAGE_FRESH_MS: i64 = 10_000;
/// Rows the page shows before "and N more", so a long list does not push Sources off the screen.
const REPOSITORY_ROWS: usize = 6;

/// What the page shows beside the list.
#[derive(Clone, PartialEq, Eq)]
enum Shown {
    Space(SpaceId),
    New,
    FromRepository,
}

/// What "From a repository" found and the human's choices for it so far.
struct Joining {
    repository: String,
    check: StorageCheck,
    /// Ticked Spaces, by folder.
    ticked: BTreeSet<String>,
    /// By folder and `owner/name`, where each of a Space's code repositories is cloned here.
    clones: BTreeMap<(String, String), String>,
    /// Also on this Mac: this Mac compiles the wiki too.
    compile_here: bool,
}

pub(super) struct SpacesPage {
    bridge: Bridge,
    shared: Shared,
    shown: Option<Shown>,
    /// The definition the form was filled from, and the form as it read then.
    loaded: Option<(Space, Space)>,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    sources: Entity<TextareaState>,
    /// The repository New chose, or the repository to add Spaces from.
    storage_input: Entity<InputState>,
    prefix_input: Entity<InputState>,
    /// New Space's folder, compile time and timezone, and Bring in's brain folder.
    folder_input: Entity<InputState>,
    time_input: Entity<InputState>,
    timezone_input: Entity<InputState>,
    brain_folder_input: Entity<InputState>,
    /// New Space's repository: one to create, or one the human has.
    choice: RepositoryChoice,
    /// New Space compiles the wiki on this Mac.
    compile_here: bool,
    /// New Space's color.
    new_color: SpaceColor,
    /// The prefix New proposed from the name, which follows the name until the human types.
    prefix_proposed: Option<String>,
    defaults: Option<SpaceFormDefaults>,
    /// Why GitHub turned Protect history down.
    protect_refused: Option<String>,
    bringing_in: Option<BringingIn>,
    /// The form's code repositories, saved with Save.
    repositories: Vec<String>,
    /// The root folders the form follows, saved with Save, and as they read when it was filled.
    followed: Vec<String>,
    followed_read: Vec<String>,
    /// New stores the Space in a GitHub repository rather than on this Mac.
    on_github: bool,
    /// The prefix field is open, to rename a tracker's prefix or set up issues.
    editing_prefix: bool,
    storage: Option<(SpaceId, Result<StorageStatus, String>)>,
    /// The Space whose storage was last asked about, and when.
    storage_asked: Option<(SpaceId, i64)>,
    joining: Option<Joining>,
    /// The repository last checked, as named, and what the check found, shown before anything
    /// is written.
    checked: Option<(String, Result<StorageCheck, String>)>,
    /// A request is waiting for the host.
    busy: bool,
    error: Option<String>,
    /// The local Space whose prefix a join was refused for, which the error offers to open.
    refused_by: Option<SpaceId>,
    /// Import from Linear…, open in the Space it was opened for.
    linear: Option<(SpaceId, Entity<LinearImportPanel>)>,
}
impl EventEmitter<Intent> for SpacesPage {}
impl SpacesPage {
    pub(super) fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let textarea = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 10)
                    .placeholder(placeholder)
            })
        };
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let name = input("Sage", window, cx);
        // New Space proposes the prefix and repository from the name as it is typed.
        cx.subscribe_in(&name, window, |view, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.name_changed(window, cx);
            }
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            shown: None,
            loaded: None,
            name,
            description: textarea("What belongs in this Space, and what doesn’t", window, cx),
            sources: textarea("mcp linear: list_issues, get_issue", window, cx),
            storage_input: input("owner/name or a GitHub URL", window, cx),
            prefix_input: input("2 to 5 capitals, such as SAGE", window, cx),
            folder_input: input("wiffletree/", window, cx),
            time_input: input("06:00", window, cx),
            timezone_input: input("America/Chicago", window, cx),
            brain_folder_input: input(SpaceBrain::DEFAULT_FOLDER, window, cx),
            choice: RepositoryChoice::Create,
            compile_here: true,
            new_color: SpaceColor::Teal,
            prefix_proposed: None,
            defaults: None,
            protect_refused: None,
            bringing_in: None,
            repositories: vec![],
            followed: vec![],
            followed_read: vec![],
            on_github: false,
            editing_prefix: false,
            storage: None,
            storage_asked: None,
            joining: None,
            checked: None,
            busy: false,
            error: None,
            refused_by: None,
            linear: None,
        }
    }

    /// Opens Import from Linear… for the Space, or shows how its import is going.
    fn open_linear_import(&mut self, space: SpaceId, window: &mut Window, cx: &mut Context<Self>) {
        let (bridge, shared) = (self.bridge.clone(), self.shared.clone());
        let id = space.clone();
        let panel = cx.new(|cx| LinearImportPanel::new(bridge, shared, id, window, cx));
        cx.subscribe(&panel, |view, _, _: &linear_import_view::Closed, cx| {
            view.linear = None;
            cx.notify();
        })
        .detach();
        self.linear = Some((space, panel));
        cx.notify();
    }

    /// Opens Move… for the Space, and reads its storage again once it moved.
    fn open_move_storage(&mut self, space: Space, window: &mut Window, cx: &mut Context<Self>) {
        let (bridge, shared) = (self.bridge.clone(), self.shared.clone());
        let sheet = storage_move_view::open(bridge, shared, space, window, cx);
        cx.subscribe_in(
            &sheet,
            window,
            |view, _, _: &storage_move_view::Moved, window, cx| view.read_storage(window, cx),
        )
        .detach();
    }

    /// Redraws while shown, loads the shown Space into the form, and reads its storage again
    /// once the reading is old or is another Space's, as after the shown one is removed, or once
    /// a storage pass for it finished.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let opened = shared.page == Page::Spaces && self.shared.page != Page::Spaces;
        let before = std::mem::replace(&mut self.shared, shared);
        if self.shared.page != Page::Spaces {
            return;
        }
        if moved && let Some((_, panel)) = &self.linear {
            let shared = self.shared.clone();
            panel.update(cx, |panel, cx| panel.share(shared, cx));
        }
        self.load_shown(window, cx);
        let shown = self.shown_space().map(|s| s.id.clone());
        fn trackers(shared: &Shared) -> &[IssueTracker] {
            shared.snapshot().map_or(&[], |s| &s.issue_trackers)
        }
        let again = shown.is_some_and(|shown| {
            reads_storage_again(self.storage_asked.as_ref(), &shown, moved, ui::now())
                || pass_finished(trackers(&before), trackers(&self.shared), &shown)
        });
        if opened || again {
            self.read_storage(window, cx);
        }
        if opened || moved {
            cx.notify();
        }
    }

    fn spaces(&self) -> &[Space] {
        self.shared.snapshot().map_or(&[], |s| &s.spaces)
    }
    fn local(&self, id: &SpaceId) -> Option<&SpaceLocal> {
        self.shared.snapshot()?.space_locals.get(id)
    }
    /// The Space shown, or else the first one, as after the shown one is removed.
    fn shown_space(&self) -> Option<&Space> {
        let spaces = self.spaces();
        match &self.shown {
            Some(Shown::Space(id)) => spaces.iter().find(|s| &s.id == id).or(spaces.first()),
            None => spaces.first(),
            Some(Shown::New | Shown::FromRepository) => None,
        }
    }

    /// Fills the form from the shown Space when it is another than the form holds, or when its
    /// definition changed, by a chip or another Mac, and the form holds no edit. A root removed
    /// meanwhile leaves the form in any case, as there is nothing left to follow.
    fn load_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(space) = self.shown_space().cloned() else {
            return;
        };
        let roots = self
            .shared
            .snapshot()
            .map_or(vec![], |s| s.repository_roots.clone());
        self.followed.retain(|root| roots.contains(root));
        self.followed_read.retain(|root| roots.contains(root));
        if let Some((from, _)) = &self.loaded
            && from.id == space.id
        {
            if *from == space && self.followed_read == self.followed_here(&space.id) {
                return;
            }
            if self.edits(cx) {
                return;
            }
        }
        self.load(&space, window, cx);
    }
    fn followed_here(&self, space: &SpaceId) -> Vec<String> {
        self.local(space)
            .map_or(vec![], |l| l.followed_roots.clone())
    }
    /// The form holds an edit the human has not saved.
    fn edits(&self, cx: &App) -> bool {
        self.loaded.as_ref().is_some_and(|(from, read)| {
            self.edited(from, cx).as_ref() != Ok(read) || self.followed != self.followed_read
        })
    }
    /// The form's repositories or followed roots differ from what it was filled with.
    fn chose_repositories(&self) -> bool {
        self.followed != self.followed_read
            || (self.loaded.as_ref())
                .is_some_and(|(_, read)| self.repositories != read.repositories)
    }
    fn load(&mut self, space: &Space, window: &mut Window, cx: &mut Context<Self>) {
        self.repositories = space.repositories.clone();
        self.followed = self.followed_here(&space.id);
        self.followed_read = self.followed.clone();
        self.editing_prefix = false;
        self.error = None;
        self.refused_by = None;
        let sources = format_sources(&space.sources);
        self.name.update(cx, |input, cx| {
            input.set_value(space.name.clone(), window, cx)
        });
        for (state, value) in [
            (&self.description, space.description.clone()),
            (&self.sources, sources),
        ] {
            state.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let read = self.edited(space, cx).unwrap_or_else(|_| space.clone());
        self.loaded = Some((space.clone(), read));
    }

    /// Shows one Space's page, as a link from another page does.
    pub(super) fn open_space(
        &mut self,
        space: SpaceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(Shown::Space(space), window, cx);
    }

    /// Opens New filled in with what a thread suggested; nothing is created until Save.
    pub(super) fn suggested(
        &mut self,
        name: &str,
        description: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(Shown::New, window, cx);
        self.name
            .update(cx, |input, cx| input.set_value(name.to_owned(), window, cx));
        self.description.update(cx, |input, cx| {
            input.set_value(description.to_owned(), window, cx)
        });
    }
    fn show(&mut self, shown: Shown, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.refused_by = None;
        self.editing_prefix = false;
        self.bringing_in = None;
        self.protect_refused = None;
        if matches!(shown, Shown::New | Shown::FromRepository) {
            self.loaded = None;
            self.joining = None;
            self.checked = None;
            self.on_github = false;
            for input in [&self.name, &self.storage_input, &self.prefix_input] {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
            self.description
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        let new = shown == Shown::New;
        self.shown = Some(shown);
        if new {
            self.open_new(window, cx);
        }
        self.load_shown(window, cx);
        self.read_storage(window, cx);
        cx.notify();
    }

    /// Asks the host what the shown Space's repository holds, for its Repository box.
    fn read_storage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.shown_space().map(|s| s.id.clone()) else {
            return;
        };
        self.storage_asked = Some((id.clone(), ui::now()));
        let asked = id.clone();
        self.ask(
            ask::space_storage(id),
            window,
            cx,
            move |view, status, _, cx| {
                view.storage = Some((asked, status));
                cx.notify();
            },
        );
    }
    fn storage_of(&self, id: &SpaceId) -> Option<&Result<StorageStatus, String>> {
        (self.storage.as_ref())
            .filter(|(shown, _)| shown == id)
            .map(|(_, status)| status)
    }

    /// The Space as the form edits it, or why it cannot be saved.
    fn edited(&self, space: &Space, cx: &App) -> Result<Space, String> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("Give the Space a name".into());
        }
        let sources = parse_sources(&self.sources.read(cx).value(), &space.sources)?;
        Ok(Space {
            name,
            description: self.description.read(cx).value().trim().to_owned(),
            repositories: self.repositories.clone(),
            sources,
            ..space.clone()
        })
    }

    /// Saves the form's edits over the Space as it is now, so a field the form did not change
    /// keeps what a chip or another Mac wrote meanwhile. Followed roots go first, as a root
    /// removed meanwhile can refuse them, which leaves the Space as it was.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(latest), Some((from, read))) = (self.shown_space().cloned(), self.loaded.clone())
        else {
            return;
        };
        let form = match self.edited(&from, cx) {
            Ok(form) => form,
            Err(error) => {
                self.error = Some(error);
                return cx.notify();
            }
        };
        let edited = merged(&read, &form, &latest);
        self.busy = true;
        self.error = None;
        if self.followed == self.followed_read {
            self.save_space(from, form, edited, window, cx);
            return cx.notify();
        }
        self.ask(
            ask::set_followed_roots(from.id.clone(), self.followed.clone()),
            window,
            cx,
            move |view, followed, window, cx| match followed {
                Ok(_) => view.save_space(from, form, edited, window, cx),
                Err(error) => {
                    view.busy = false;
                    view.error = Some(error);
                }
            },
        );
        cx.notify();
    }

    /// Writes `edited`, the merge of `form` over the Space as it is now.
    fn save_space(
        &mut self,
        from: Space,
        form: Space,
        edited: Space,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask(
            ask::update_space(edited),
            window,
            cx,
            move |view, saved, window, cx| {
                view.busy = false;
                // Only the form still holding the draft that was saved takes the reply; edits
                // made since, or another Space's draft, stay as they are.
                let same_draft = (view.loaded.as_ref()).is_some_and(|(shown, _)| {
                    holds_draft(shown, &view.edited(shown, cx), &from, &form)
                });
                match saved {
                    Ok(record) => {
                        if same_draft {
                            view.load(&record.space, window, cx);
                        }
                        toast(format!("Saved {}", record.space.name), window, cx);
                    }
                    Err(error) if same_draft => view.error = Some(error),
                    Err(error) => toast_error(error, window, cx),
                }
            },
        );
    }

    /// Opens Choose repositories… over the form's draft.
    fn choose_repositories(&mut self, space: &Space, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.shared.snapshot() else {
            return;
        };
        let choice = Choice {
            repositories: self.repositories.clone(),
            followed: self.followed.clone(),
        };
        let tree = Tree::new(
            &snapshot.spaces,
            &space.id,
            &snapshot.repositories,
            &snapshot.repository_roots,
            choice,
            home().as_deref(),
        );
        let (spaces, page) = (snapshot.spaces.clone(), cx.weak_entity());
        repository_chooser::open(page, &space.name, tree, spaces, window, cx);
    }

    /// Takes what Done chose into the form's draft; Save writes it.
    pub(super) fn chosen(&mut self, tree: &Tree, cx: &mut Context<Self>) {
        let choice = tree.finish(&self.repositories);
        self.repositories = choice.repositories;
        self.followed = choice.followed;
        cx.notify();
    }

    /// Asks first, then removes the Space; the host refuses the last one and one with an open
    /// project, saying why.
    fn confirm_remove(&mut self, space: &Space, window: &mut Window, cx: &mut Context<Self>) {
        let (id, name) = (space.id.clone(), space.name.clone());
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, id) = (weak.clone(), id.clone());
            dialog
                .title(format!("Remove {name}?"))
                .confirm()
                .ok_text("Remove Space")
                .child(REMOVE_SPACE)
                .on_ok(move |_, window, cx| {
                    let space_id = id.clone();
                    let _ = weak.update(cx, |view, cx| {
                        view.request(Command::RemoveSpace { space_id }, window, cx)
                    });
                    true
                })
        });
    }

    /// Renames the tracker's prefix, or sets up issues in a repository with no tracker, after
    /// asking first.
    fn confirm_prefix(&mut self, space: &Space, window: &mut Window, cx: &mut Context<Self>) {
        let prefix = self.prefix_input.read(cx).value().trim().to_owned();
        if let Err(error) = validate_prefix(&prefix) {
            self.error = Some(error.to_string());
            return cx.notify();
        }
        let current = (self.storage_of(&space.id).and_then(|s| s.as_ref().ok()))
            .and_then(|status| status.prefix.clone());
        let (title, line, ok) = prefix_question(space, current.as_deref(), &prefix);
        let id = space.id.clone();
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (weak, id, prefix) = (weak.clone(), id.clone(), prefix.clone());
            let setting_up = current.is_none();
            dialog
                .title(title.clone())
                .confirm()
                .ok_text(ok)
                .child(line.clone())
                .on_ok(move |_, window, cx| {
                    let (id, prefix) = (id.clone(), prefix.clone());
                    let _ = weak.update(cx, |view, cx| {
                        view.set_prefix(id, prefix, setting_up, window, cx)
                    });
                    true
                })
        });
    }
    fn set_prefix(
        &mut self,
        space: SpaceId,
        prefix: String,
        setting_up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = match setting_up {
            true => ask::set_up_issues(space, prefix),
            false => ask::set_tracker_prefix(space, prefix),
        };
        self.ask(ask, window, cx, |view, set, window, cx| {
            match set {
                Ok(_) => {
                    view.editing_prefix = false;
                    view.error = None;
                }
                Err(error) => view.error = Some(error),
            }
            view.read_storage(window, cx);
        });
    }

    fn list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let shown = self.shown_space().map(|s| s.id.clone());
        let mut list = div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(section("SPACES", p));
        for space in self.spaces() {
            let id = space.id.clone();
            let selected = shown.as_ref() == Some(&space.id) && self.shown != Some(Shown::New);
            list = list.child(
                div()
                    .id(SharedString::from(format!("space-{}", space.id)))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(p.overlay))
                    .hover(|d| d.bg(p.overlay.opacity(0.6)))
                    .child(space_mark(space_color(self.spaces(), &space.id, p)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(space.name.clone()),
                    )
                    .child(hint(
                        if is_shared(space) {
                            "shared"
                        } else {
                            "this Mac"
                        },
                        p,
                    ))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.show(Shown::Space(id.clone()), window, cx)
                    })),
            );
        }
        let weak = cx.weak_entity();
        list.child(
            div().pt_2().child(
                ui::gated(
                    button("add-space")
                        .ghost()
                        .icon(icon("plus"))
                        .label("Add Space")
                        .dropdown_caret(true),
                    &self.shared.host,
                )
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (label, shown) in [
                        ("New", Shown::New),
                        ("From a repository", Shown::FromRepository),
                    ] {
                        let weak = weak.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                let shown = shown.clone();
                                let _ = weak.update(cx, |view, cx| view.show(shown, window, cx));
                            }));
                    }
                    menu
                }),
            ),
        )
    }

    fn space_page(&self, space: &Space, p: Palette, cx: &mut Context<Self>) -> Div {
        let local = self.local(&space.id);
        let status = self.storage_of(&space.id);
        let readers = status.and_then(|s| s.as_ref().ok()).and_then(|s| s.readers);
        let (lead, text) = storage_banner(space, readers);
        let mut page = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                banner(if is_shared(space) { p.yellow } else { p.subtle }).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if is_shared(space) { p.yellow } else { p.text })
                                .child(lead),
                        )
                        .child(text),
                ),
            )
            .child(row("Name", Input::new(&self.name).small(), None, p))
            .child(row(
                "Color",
                self.color_field(space, p, cx),
                Some(COLOR_CAPTION.into()),
                p,
            ))
            .child(row(
                "What belongs here",
                Textarea::new(&self.description).w_full(),
                Some(ROUTER.into()),
                p,
            ))
            .child(row(
                "Code repositories",
                self.repositories_field(space, p, cx),
                Some(repositories_caption(&space.name, self.chose_repositories())),
                p,
            ))
            .child(row(
                "Sources",
                Textarea::new(&self.sources).w_full(),
                Some(format!(
                    "Threads in {} may use these. Jobs list their own. {SOURCES_FORMAT}",
                    space.name
                )),
                p,
            ))
            .child(self.cascade_links(space, local, p, cx))
            .children(self.moved_banner(space, p, cx))
            .child(self.repository_box(space, status, p, cx));
        if let Some(error) = &self.error {
            page = page.child(self.error_line(error, p, cx));
        }
        let removed = space.clone();
        page.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    ui::gated(
                        button("remove-space").ghost().label("Remove Space"),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.confirm_remove(&removed, window, cx)
                    })),
                )
                .child(
                    ui::gated(
                        button("save-space")
                            .primary()
                            .label("Save")
                            .disabled(self.busy),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.save(window, cx))),
                ),
        )
    }

    fn color_field(&self, space: &Space, p: Palette, cx: &mut Context<Self>) -> Div {
        let spaces = self.spaces();
        let at = spaces.iter().position(|s| s.id == space.id).unwrap_or(0);
        let id = space.id.clone();
        let weak = cx.weak_entity();
        swatches(
            "space-color",
            SpaceColor::of(spaces, at),
            p,
            move |color, window, cx| {
                let id = id.clone();
                let _ = weak.update(cx, |view, cx| view.set_color(&id, color, window, cx));
            },
        )
    }

    /// Saves a Space's color at once, over the Space as it is now, as a commit like any edit.
    fn set_color(
        &mut self,
        id: &SpaceId,
        color: SpaceColor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(latest) = self.spaces().iter().find(|s| &s.id == id).cloned() else {
            return;
        };
        if latest.color == Some(color) {
            return;
        }
        let colored = Space {
            color: Some(color),
            ..latest
        };
        self.ask(
            ask::update_space(colored),
            window,
            cx,
            |_, saved, window, cx| {
                if let Err(error) = saved {
                    toast_error(error, window, cx);
                }
            },
        );
    }

    fn error_line(&self, error: &str, p: Palette, cx: &mut Context<Self>) -> Div {
        let open = self.refused_by.clone().and_then(|id| {
            let name = self.spaces().iter().find(|s| s.id == id)?.name.clone();
            Some(
                button("open-refusing-space")
                    .xsmall()
                    .label(format!("Open {name}"))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.show(Shown::Space(id.clone()), window, cx)
                    })),
            )
        });
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(hint(error.to_owned(), p).text_color(p.red))
            .children(open)
    }

    fn repositories_field(&self, space: &Space, p: Palette, cx: &mut Context<Self>) -> Div {
        let here = self.shared.snapshot().map_or(&[][..], |s| &s.repositories);
        let mut field = div().flex().flex_col().gap_1();
        for (at, root) in self.followed.iter().enumerate() {
            let line = following_line(root, &space.name, home().as_deref());
            let stopped = root.clone();
            field = field.child(
                div()
                    .mb_1()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(p.edge)
                    .bg(p.base)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(hint(line, p).text_color(p.text)),
                    )
                    .child(
                        button(SharedString::from(format!("stop-following-{at}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip("Stop following. Its repositories stay; saved with Save")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.followed.retain(|root| *root != stopped);
                                cx.notify();
                            })),
                    ),
            );
        }
        let shown = self.repositories.iter().take(REPOSITORY_ROWS).enumerate();
        for (at, identity) in shown {
            let cloned = here.iter().any(|r| r.is_github(identity));
            field = field.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(identity.clone()))
                    .when(!cloned, |d| {
                        d.child(hint("not cloned on this Mac", p).text_color(p.yellow))
                    })
                    .child(
                        button(SharedString::from(format!("drop-space-repository-{at}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip(format!("Take it out of {}; saved with Save", space.name))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.repositories.remove(at);
                                cx.notify();
                            })),
                    ),
            );
        }
        let hidden = self.repositories.len().saturating_sub(REPOSITORY_ROWS);
        let more = (hidden > 0).then(|| {
            let shown = space.clone();
            button("more-space-repositories")
                .link()
                .xsmall()
                .label(format!("and {hidden} more"))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.choose_repositories(&shown, window, cx)
                }))
        });
        let shown = space.clone();
        field
            .children(more.map(|more| div().flex().child(more)))
            .child(
                div().mt_1().flex().child(
                    button("choose-space-repositories")
                        .label("Choose repositories…")
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.choose_repositories(&shown, window, cx)
                        })),
                ),
            )
    }

    /// Behaviors for ‹Space› → and Models for ‹Space› →, each with what the Space changes, then
    /// what this Mac falls back from.
    fn cascade_links(
        &self,
        space: &Space,
        local: Option<&SpaceLocal>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = space.id.clone();
        let link = |button: Button, changes: String| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(button)
                .child(hint(changes, p))
        };
        let scope = cascade::Scope::Space {
            space_id: space.id.clone(),
        };
        let behaviors = link(
            button("space-behaviors")
                .small()
                .label(format!("Behaviors for {} →", space.name))
                .on_click(
                    cx.listener(move |_, _, _, c| c.emit(Intent::OpenBehaviors(scope.clone()))),
                ),
            counted(behaviors_changed(space), "change"),
        );
        let models = link(
            button("space-models")
                .small()
                .label(format!("Models for {} →", space.name))
                .on_click(
                    cx.listener(move |_, _, _, c| c.emit(Intent::OpenModels(Some(id.clone())))),
                ),
            models_changed(space),
        );
        let fallbacks = self.shared.snapshot().map_or(vec![], |snapshot| {
            let table = models::space_table(
                &snapshot.model_selection,
                Some(space),
                &space.models.overrides(),
                &self.shared.detection,
            );
            models::fallback_lines(&table, &space.name)
        });
        let ignored = models::ignored_line(&space.models.ignored(), &space.name)
            .map(|line| format!("Ignored: {line}"));
        let notes = (local.map_or(&[][..], |l| &l.notes).iter().cloned())
            .chain(fallbacks)
            .chain(ignored);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(behaviors)
                    .child(models),
            )
            .children(notes.map(|note| hint(note, p).text_color(p.yellow)))
    }

    /// Once, until dismissed: what left the Space page for Behaviors and Models.
    fn moved_banner(&self, space: &Space, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        if cx.global::<Preferences>().cascade_moves_seen {
            return None;
        }
        let name = &space.name;
        Some(
            banner(p.yellow)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.yellow)
                                .child("Moved"),
                        )
                        .child(format!(
                            "{name}'s instructions are now “{name} additions” on Behaviors for {name}, and reach every role in {name}. Its thread and job models are now {name}'s Assistant and Job rows on Models for {name}."
                        )),
                )
                .child(
                    button("dismiss-moved")
                        .ghost()
                        .xsmall()
                        .label("Got it")
                        .on_click(|_, _, cx| {
                            let preferences = cx.global_mut::<Preferences>();
                            preferences.cascade_moves_seen = true;
                            if let Err(error) = preferences.save() {
                                eprintln!("Saving preferences: {error}");
                            }
                            cx.refresh_windows();
                        }),
                ),
        )
    }

    fn issues_field(
        &self,
        space: &Space,
        status: &StorageStatus,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let renamable = status.prefix.is_some() && (!is_shared(space) || status.issues == 0);
        let action = match (&status.prefix, renamable) {
            (None, _) => Some("Set up issues"),
            (Some(_), true) => Some("Rename prefix"),
            (Some(_), false) => None,
        };
        let line = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(issues_line(status))
            .when(!self.editing_prefix, |d| {
                d.children(action.map(|label| {
                    ui::gated(
                        button("edit-prefix").xsmall().ghost().label(label),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.editing_prefix = true;
                        view.prefix_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        cx.notify();
                    }))
                }))
            });
        if !self.editing_prefix {
            return line;
        }
        let shown = space.clone();
        div().flex().flex_col().gap_1().child(line).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Input::new(&self.prefix_input).small().w(px(160.)))
                .child(
                    button("save-prefix")
                        .xsmall()
                        .primary()
                        .label(action.unwrap_or("Save"))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.confirm_prefix(&shown, window, cx)
                        })),
                )
                .child(
                    button("cancel-prefix")
                        .xsmall()
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.editing_prefix = false;
                            cx.notify();
                        })),
                )
                .child(hint(PREFIX_RULE, p)),
        )
    }
}

const REMOVE_SPACE: &str = "Its repository stays as it is. Its archived projects move to Personal. A Space with an open project, the last Space, or the last Space in a repository with entries waiting or held here can’t be removed.";
const PREFIX_RULE: &str = "2 to 5 capitals, used by no other repository on this Mac.";

fn reads_storage_again(
    asked: Option<&(SpaceId, i64)>,
    shown: &SpaceId,
    moved: bool,
    now: i64,
) -> bool {
    match asked {
        Some((id, at)) if id == shown => moved && now - at > STORAGE_FRESH_MS,
        _ => true,
    }
}

/// Whether a storage pass for `shown` finished between two snapshots: its tracker's last sync
/// or error changed.
fn pass_finished(before: &[IssueTracker], now: &[IssueTracker], shown: &SpaceId) -> bool {
    let pass = |trackers: &[IssueTracker]| {
        (trackers.iter())
            .find(|t| &t.space_id == shown)
            .map(|t| (t.synced_at, t.error.clone()))
    };
    pass(before) != pass(now)
}

/// Whether the form, showing `shown` and reading `now`, still holds `saved`, the draft Save sent
/// for `from`.
fn holds_draft(shown: &Space, now: &Result<Space, String>, from: &Space, saved: &Space) -> bool {
    shown.id == from.id && now.as_ref() == Ok(saved)
}

/// `latest` with each field the form changed from `read`, how it read when filled, set to
/// the form's value.
fn merged(read: &Space, form: &Space, latest: &Space) -> Space {
    fn pick<T: Clone + PartialEq>(read: &T, form: &T, latest: &T) -> T {
        match form == read {
            true => latest.clone(),
            false => form.clone(),
        }
    }
    Space {
        name: pick(&read.name, &form.name, &latest.name),
        description: pick(&read.description, &form.description, &latest.description),
        repositories: merged_list(&read.repositories, &form.repositories, &latest.repositories),
        sources: pick(&read.sources, &form.sources, &latest.sources),
        ..latest.clone()
    }
}

/// This Mac's home, which paths on the page and in the dialog show as `~`.
pub(super) fn home() -> Option<String> {
    std::env::var("HOME").ok()
}

fn following_line(root: &str, space: &str, home: Option<&str>) -> String {
    let folder = shown_path(root, home);
    format!("Following {folder}: new repositories there join {space}")
}

fn repositories_caption(space: &str, unsaved: bool) -> String {
    let saved = if unsaved {
        " Your changes are kept when you click Save."
    } else {
        ""
    };
    format!("Every project in {space} uses these.{saved}")
}

/// A labelled field of a Space's form, with its caption under it.
fn row(label: &'static str, field: impl IntoElement, caption: Option<String>, p: Palette) -> Div {
    div()
        .flex()
        .items_start()
        .gap_4()
        .child(
            div()
                .w(px(150.))
                .flex_none()
                .pt_1()
                .text_size(px(font::SMALL))
                .text_color(p.subtle)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(field)
                .children(caption.map(|caption| hint(caption, p))),
        )
}

/// One line of the Repository box.
fn fact(label: &'static str, value: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .items_start()
        .gap_3()
        .text_size(px(font::SMALL))
        .child(
            div()
                .w(px(130.))
                .flex_none()
                .text_color(p.subtle)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// How many Behavior choices the Space makes, for itself and for its repositories.
fn behaviors_changed(space: &Space) -> usize {
    let repositories = space.repository_settings.values();
    space.behaviors.len() + repositories.map(|r| r.behaviors.len()).sum::<usize>()
}

/// What the Space sets on Models: its rows that count, and its reviews when it sets any.
fn models_changed(space: &Space) -> String {
    let models = &space.models;
    let rows = counted(models.overrides().len() - models.ignored().len(), "row");
    match space.reviews.is_empty() {
        true => rows,
        false => format!("{rows} · reviews"),
    }
}

pub(super) fn space_mark(color: Hsla) -> Div {
    div().size(px(8.)).flex_none().rounded(px(2.)).bg(color)
}

/// Each Space's color: its own, or else the one its place in the list gives it.
pub(super) fn space_color(spaces: &[Space], id: &SpaceId, p: Palette) -> Hsla {
    let at = spaces.iter().position(|s| &s.id == id).unwrap_or(0);
    palette_color(SpaceColor::of(spaces, at), p)
}

fn palette_color(color: SpaceColor, p: Palette) -> Hsla {
    match color {
        SpaceColor::Teal => p.focus,
        SpaceColor::Coral => p.accent,
        SpaceColor::Lilac => p.magenta,
        SpaceColor::Gold => p.yellow,
        SpaceColor::Sky => p.blue,
        SpaceColor::Sage => p.green,
        SpaceColor::Rose => p.red,
    }
}

/// The seven colors to pick from, `picked` outlined; `pick` runs with the one clicked.
fn swatches(
    id: &'static str,
    picked: SpaceColor,
    p: Palette,
    pick: impl Fn(SpaceColor, &mut Window, &mut App) + 'static,
) -> Div {
    let pick = std::rc::Rc::new(pick);
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(10.))
        .children(SpaceColor::ALL.into_iter().map(|color| {
            let pick = pick.clone();
            div()
                .id(SharedString::from(format!("{id}-{}", color.label())))
                .size(px(22.))
                .rounded(px(6.))
                .cursor_pointer()
                .bg(palette_color(color, p))
                .border_2()
                .border_color(if color == picked { p.text } else { p.base })
                .tooltip(move |window, cx| Tooltip::new(color.label()).build(window, cx))
                .on_click(move |_, window, cx| pick(color, window, cx))
        }))
}

const COLOR_CAPTION: &str = "Marks this Space in the sidebar, the Assistant and the project list.";

/// Stored on GitHub, so other people can read it.
pub(super) fn is_shared(space: &Space) -> bool {
    matches!(space.storage.repository, StorageRepository::GitHub { .. })
}

/// Who `readers` are, as a sentence about `repository`.
pub(super) fn readers_sentence(readers: Readers, repository: &str) -> String {
    let owner = repository_owner(repository);
    match readers {
        Readers::Invited => format!("Only you and people you invite can read {repository}."),
        Readers::EveryMember => format!("Every member of {owner} can read {repository}."),
        Readers::EnterpriseMembers => {
            format!("Every member of {owner}’s enterprise can read {repository}.")
        }
        Readers::MaybeMembers => format!("Members of {owner} may be able to read {repository}."),
    }
}

/// The page's banner: whether the Space is shared, who can read it and who can change it.
fn storage_banner(space: &Space, readers: Option<Readers>) -> (&'static str, String) {
    let StorageRepository::GitHub { .. } = &space.storage.repository else {
        return (
            "Only on this Mac.",
            format!(
                "{}’s storage is a local Git repository with no remote. Nothing in it is pushed anywhere.",
                space.name
            ),
        );
    };
    let repository = space.storage.repository.key();
    let read = readers.map_or(String::new(), |r| {
        format!("{} ", readers_sentence(r, &repository))
    });
    (
        "Shared.",
        format!(
            "{read}Anyone who can push to {repository} can change this Space’s Behaviors, additions and models for everyone in it, and can read and edit its issues. Edits to the Space are commits and follow the settings repository’s sync, pause and compare rules."
        ),
    )
}

fn issues_line(status: &StorageStatus) -> String {
    let Some(prefix) = &status.prefix else {
        return "No tracker yet. Set up issues chooses its prefix and commits wiffletree-issues.yaml."
            .into();
    };
    format!("Prefix {prefix} · {}", counted(status.issues, "issue"))
}

/// Who can read the Space's repository, and so its issues, logs and wiki: as many as GitHub
/// counted, when it let this Mac count them.
pub(super) fn readers_line(
    space: &Space,
    readers: Option<Readers>,
    count: Option<usize>,
) -> String {
    let owner = repository_owner(&space.storage.repository.key()).to_owned();
    match (readers, count) {
        (None, _) => "Only you, on this Mac".into(),
        (Some(Readers::EveryMember), Some(count)) => {
            format!("{} of {owner}", counted(count, "member"))
        }
        (Some(Readers::Invited), Some(1)) => "Only you".into(),
        (Some(Readers::Invited), Some(count)) => format!("{count} people you invited"),
        (Some(Readers::Invited), None) => "Only you and people you invite".into(),
        (Some(Readers::EveryMember), None) => format!("Every member of {owner}"),
        (Some(Readers::EnterpriseMembers), _) => format!("Every member of {owner}’s enterprise"),
        (Some(Readers::MaybeMembers), _) => format!("Maybe every member of {owner}"),
    }
}

/// The other Spaces stored in the same repository, which share its issues and prefix.
fn shared_with(others: &[&str], prefix: Option<&str>) -> Option<String> {
    let (last, rest) = others.split_last()?;
    let (names, verb, share) = match rest {
        [] => (last.to_string(), "also stores", "it shares"),
        rest => (
            format!("{} and {last}", rest.join(", ")),
            "also store",
            "they share",
        ),
    };
    Some(match prefix {
        Some(prefix) => {
            format!("{names} {verb} here, so {share} these issues and the {prefix} prefix.")
        }
        None => format!("{names} {verb} here."),
    })
}

/// "synced 2m ago", or why the latest pass failed; nothing for local storage.
fn sync_line(status: &StorageStatus) -> Option<(String, bool)> {
    if let Some(error) = &status.error {
        return Some((format!("● didn’t sync: {error}"), true));
    }
    let at = status.synced_at?;
    Some((format!("● synced {}", age(at).to_lowercase()), false))
}

/// The Space's brain as New project names it: its repository.
pub(super) fn brain_label(space: &Space) -> String {
    space.storage.repository.key()
}

/// The question before a prefix changes: its title, what happens and the button.
fn prefix_question(
    space: &Space,
    current: Option<&str>,
    prefix: &str,
) -> (String, String, &'static str) {
    let repository = match is_shared(space) {
        true => space.storage.repository.key(),
        false => format!("{}’s local storage", space.name),
    };
    match current {
        None => (
            format!("Set up issues with the prefix {prefix}?"),
            format!(
                "This commits only wiffletree-issues.yaml to {repository} and pushes it. Every Space stored there shares the prefix."
            ),
            "Set up issues",
        ),
        Some(current) if is_shared(space) => (
            format!("Change the prefix {current} to {prefix}?"),
            format!(
                "{repository} has no issue yet, so only wiffletree-issues.yaml changes. Every Space stored there shares the prefix."
            ),
            "Change prefix",
        ),
        Some(current) => (
            format!("Rename the prefix {current} to {prefix}?"),
            format!(
                "Every issue ID in {repository}, its folder and every reference to it in its issues is rewritten. Nothing outside this Mac refers to them."
            ),
            "Rename prefix",
        ),
    }
}

/// The local Space a join was refused for, its tracker using the repository's prefix, so the
/// error can offer to open its page to rename it.
fn join_refused_by(spaces: &[Space], error: &str) -> Option<SpaceId> {
    (spaces.iter())
        .filter(|space| !is_shared(space))
        .find(|space| error.contains(&format!("The {} Space's local tracker", space.name)))
        .map(|space| space.id.clone())
}

/// A Space's sources, one a line, as the Sources field edits them.
fn format_sources(sources: &[Source]) -> String {
    sources
        .iter()
        .map(|source| match source {
            Source::Mcp { server, tools, .. } => format!("mcp {server}: {}", tools.join(", ")),
            Source::Cli {
                command, prefixes, ..
            } => format!("cli {command}: {}", prefixes.join(", ")),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The Sources field read back; a source named as before keeps its sign-in check and the keys
/// this release does not know.
fn parse_sources(text: &str, before: &[Source]) -> Result<Vec<Source>, String> {
    let mut sources = vec![];
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let wrong = || format!("“{line}” isn’t a source. {SOURCES_FORMAT}");
        let (head, list) = line.split_once(':').ok_or_else(wrong)?;
        let (kind, name) = head.trim().split_once(' ').ok_or_else(wrong)?;
        let name = name.trim().to_owned();
        let items: Vec<String> = (list.split(','))
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect();
        if name.is_empty() {
            return Err(wrong());
        }
        sources.push(match kind {
            "mcp" => {
                let extra = (before.iter())
                    .find_map(|s| match s {
                        Source::Mcp { server, extra, .. } if server == &name => Some(extra.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                Source::Mcp {
                    server: name,
                    tools: items,
                    extra,
                }
            }
            "cli" => {
                let (check, extra) = (before.iter())
                    .find_map(|s| match s {
                        Source::Cli {
                            command,
                            check,
                            extra,
                            ..
                        } if command == &name => Some((check.clone(), extra.clone())),
                        _ => None,
                    })
                    .unwrap_or_default();
                Source::Cli {
                    command: name,
                    prefixes: items,
                    check,
                    extra,
                }
            }
            _ => return Err(wrong()),
        });
    }
    Ok(sources)
}

impl Requests for SpacesPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Err(error) => self.error = Some(error),
            Ok(_) if matches!(command, Command::RemoveSpace { .. }) => {
                self.shown = None;
                self.loaded = None;
                self.error = None;
            }
            Ok(_) => self.error = None,
        }
        if matches!(command, Command::SyncNow) {
            self.read_storage(window, cx);
        }
    }
}

impl Render for SpacesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Spaces");
        let p = palette(is_dark(cx));
        let shown = match &self.shown {
            Some(Shown::New) => self.new_page(p, cx),
            Some(Shown::FromRepository) => self.join_page(p, cx),
            _ => match self.shown_space().cloned() {
                Some(space) => self.space_page(&space, p, cx),
                None => hint("No Space yet.", p),
            },
        };
        let body = div()
            .flex()
            .items_start()
            .gap_6()
            .child(self.list(p, cx))
            .child(shown);
        ui::page("Spaces", "Spaces", body, None, &self.shared.host, p, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::repository_box::{
        brings_lines, compile_words, leaves_line, logs_line, not_on_github_line, plan_notes,
        where_line, wiki_line,
    };
    use super::setup::{Mark, check_lines, compiles_now, join_choices, proposed_repository};
    use super::{
        Joining, STORAGE_FRESH_MS, behaviors_changed, brain_label, format_sources, holds_draft,
        issues_line, join_refused_by, merged, models_changed, parse_sources, pass_finished,
        prefix_question, readers_line, reads_storage_again, shared_with, storage_banner, sync_line,
    };
    use crate::wording::counted;
    use workspace_core::*;

    fn space(name: &str, storage: StorageRepository) -> Space {
        Space {
            id: SpaceId(name.to_lowercase()),
            name: name.into(),
            color: None,
            description: String::new(),
            additions: String::new(),
            behaviors: Default::default(),
            repository_settings: Default::default(),
            brain: None,
            storage: SpaceStorage {
                repository: storage,
                folder: name.to_lowercase(),
                root: String::new(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            reviews: Default::default(),
            sources: vec![],
            extra: Extra::new(),
        }
    }
    fn sage() -> Space {
        let storage = StorageRepository::GitHub {
            owner: "Sage-Social".into(),
            name: "wiffletree-storage".into(),
        };
        space("Sage", storage)
    }
    fn personal() -> Space {
        space("Personal", StorageRepository::Local { id: "7k2q".into() })
    }
    fn status(prefix: Option<&str>, issues: usize, readers: Option<Readers>) -> StorageStatus {
        StorageStatus {
            entries: 0,
            waiting: 0,
            wiki_pages: 0,
            compiled: None,
            compile: None,
            compilers: 0,
            compiles_here: false,
            reader_count: None,
            prefix: prefix.map(Into::into),
            issues,
            readers,
            settings_repository: false,
            path: "/data/repos/local-7k2q".into(),
            synced_at: None,
            error: None,
            wiki_behind: None,
        }
    }

    #[test]
    fn the_space_page_links_to_what_the_space_changes_and_saving_it_keeps_models_and_additions() {
        let mut sage = sage();
        assert_eq!(counted(behaviors_changed(&sage), "change"), "0 changes");
        assert_eq!(models_changed(&sage), "0 rows");
        sage.behaviors.insert("review_depth".into(), "full".into());
        let repository = cascade::RepositorySettings {
            behaviors: [("publishing".to_owned(), "always_ask".to_owned())].into(),
            ..Default::default()
        };
        (sage.repository_settings).insert("Sage-Social/sagechat-sms".into(), repository);
        sage.models.threads = Some(SpaceModel::from(ModelProfile {
            provider: Provider::Claude,
            model: "opus".into(),
            effort: "high".into(),
        }));
        assert_eq!(counted(behaviors_changed(&sage), "change"), "2 changes");
        assert_eq!(models_changed(&sage), "1 row");
        sage.reviews.max_cycles = Some(3);
        assert_eq!(models_changed(&sage), "1 row · reviews");

        // The form has no models or additions, so a save keeps what Models and Behaviors set
        // meanwhile, here or on another Mac.
        let read = sage.clone();
        let form = Space {
            description: "Sage Haven work".into(),
            ..read.clone()
        };
        let latest = Space {
            additions: "Never log child PII.".into(),
            models: SpaceModels::default(),
            ..read.clone()
        };
        let saved = merged(&read, &form, &latest);
        assert_eq!(saved.description, "Sage Haven work");
        assert_eq!(saved.additions, "Never log child PII.");
        assert_eq!(saved.models, SpaceModels::default());
    }

    #[test]
    fn a_shared_spaces_page_says_who_reads_it_and_that_pushers_change_its_instructions() {
        let sage = sage();
        let (lead, text) = storage_banner(&sage, Some(Readers::EveryMember));
        assert_eq!(lead, "Shared.");
        assert!(text.starts_with("Every member of Sage-Social can read"));
        assert!(
            text.contains("Every member of Sage-Social can read Sage-Social/wiffletree-storage.")
        );
        assert!(text.contains(
            "Anyone who can push to Sage-Social/wiffletree-storage can change this Space’s Behaviors, additions and models for everyone in it"
        ));
        let status = status(Some("SAGE"), 148, Some(Readers::EveryMember));
        assert_eq!(issues_line(&status), "Prefix SAGE · 148 issues");
        assert_eq!(
            readers_line(&sage, status.readers, None),
            "Every member of Sage-Social"
        );
        assert_eq!(
            where_line(&sage),
            "Sage-Social/wiffletree-storage · the repository’s root"
        );
    }

    #[test]
    fn the_repository_box_says_where_who_reads_and_how_the_wiki_compiles() {
        let mut vyttle = space(
            "Vyttle",
            StorageRepository::GitHub {
                owner: "Vyttle-LLC".into(),
                name: "vyttle-wiffletree-storage".into(),
            },
        );
        vyttle.storage.root = "wiffletree".into();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let six = today.and_hms_opt(6, 4, 0).unwrap();
        let six = six
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp_millis();
        let status = StorageStatus {
            entries: 214,
            waiting: 9,
            wiki_pages: 10,
            compiled: Some((six, "Michael's Mac".into())),
            compile: Some(BrainCompile {
                at: "06:00".into(),
                timezone: "America/Chicago".into(),
                extra: Extra::new(),
            }),
            compilers: 2,
            compiles_here: true,
            reader_count: Some(3),
            ..status(Some("VYT"), 48, Some(Readers::EveryMember))
        };
        assert_eq!(
            where_line(&vyttle),
            "Vyttle-LLC/vyttle-wiffletree-storage · wiffletree/"
        );
        assert_eq!(
            readers_line(&vyttle, status.readers, status.reader_count),
            "3 members of Vyttle-LLC"
        );
        assert_eq!(issues_line(&status), "Prefix VYT · 48 issues");
        assert_eq!(
            logs_line(&status),
            "214 entries · 9 waiting for the next compile"
        );
        assert_eq!(
            wiki_line(&status, today),
            "10 pages · compiled today at 06:04 on Michael's Mac"
        );
        assert_eq!(
            compile_words(&status),
            "Daily at 06:00 · on this Mac and 1 other"
        );
        let nobody = StorageStatus {
            compilers: 0,
            compiles_here: false,
            ..status
        };
        assert_eq!(compile_words(&nobody), "Daily at 06:00 · on no Mac");
    }

    #[test]
    fn a_checked_repository_says_whether_it_can_be_used_in_the_mockups_words() {
        let usable = StorageCheck {
            repository: SettingsRepository {
                name: "Vyttle-LLC/vyttle-wiffletree-storage".into(),
                push_url: "u".into(),
                branch: "main".into(),
                readers: Readers::EveryMember,
            },
            organization: true,
            reader_count: Some(3),
            history_protected: false,
            folder: None,
            spaces: vec![],
            prefix: None,
            compile: None,
            issues: 0,
            wiki_pages: 0,
            compilers: 0,
        };
        assert_eq!(
            check_lines(&Ok(usable.clone())),
            [
                (
                    Mark::Passed,
                    "Private. 3 members of Vyttle-LLC can read it.".to_owned()
                ),
                (Mark::Passed, "Wiffletree can write to main.".to_owned()),
                (
                    Mark::Warning,
                    "History isn’t protected. Anyone with access could force-push over it."
                        .to_owned()
                ),
            ]
        );
        let protected = StorageCheck {
            history_protected: true,
            ..usable
        };
        assert_eq!(check_lines(&Ok(protected)).len(), 2, "no warning");
        let public = "Public. Issues and logs need a private repository.";
        assert_eq!(
            check_lines(&Err(public.into())),
            [(Mark::Refused, public.to_owned())]
        );
        assert_eq!(
            proposed_repository("Vyttle-LLC", "Vyttle"),
            "Vyttle-LLC/vyttle-wiffletree-storage"
        );
        assert_eq!(
            proposed_repository("Sage-Social", "Sage Watch"),
            "Sage-Social/sage-watch-wiffletree-storage"
        );
        assert_eq!(
            compiles_now(1),
            "1 Mac compiles this wiki now. Turning this on would use your model subscription."
        );
    }

    #[test]
    fn bring_in_says_what_it_brings_what_it_leaves_and_what_only_this_mac_has() {
        let plan = BringInPlan {
            pages: 10,
            status: true,
            entries: 191,
            entry_files: 17,
            shortened: 2,
            not_on_github: 17,
            never_committed: 11,
            left_behind: vec!["archive/".into(), "automation/".into(), "raw/".into()],
            ..BringInPlan::default()
        };
        assert_eq!(
            brings_lines(&plan),
            [
                "10 wiki pages and STATUS.md",
                "191 log entries from 17 files, each with its date and repository"
            ]
        );
        assert_eq!(
            leaves_line(&plan),
            "The original folder is left as it is, along with archive/, automation/ and raw/. You can delete it once you’re happy."
        );
        assert_eq!(
            not_on_github_line(&plan).unwrap(),
            "17 of these files aren’t on GitHub yet. They exist only in this Mac’s clone, and 11 have never been committed. Bringing in reads this Mac’s copy, so none of them are lost."
        );
        assert!(plan_notes(&plan)[0].starts_with("2 entries are shortened"));
        let pushed = BringInPlan {
            not_on_github: 0,
            ..plan
        };
        assert_eq!(not_on_github_line(&pushed), None);
    }

    #[test]
    fn a_personal_spaces_page_says_its_storage_is_local_and_only_you_read_its_issues() {
        let personal = personal();
        let (lead, text) = storage_banner(&personal, None);
        assert_eq!(lead, "Only on this Mac.");
        assert!(text.starts_with("Personal’s storage is a local Git repository with no remote."));
        let moved = Space {
            storage: SpaceStorage {
                root: "wiffletree".into(),
                ..personal.storage.clone()
            },
            ..personal.clone()
        };
        assert_eq!(
            where_line(&moved),
            "Only on this Mac · wiffletree/ · no remote"
        );
        assert_eq!(readers_line(&personal, None, None), "Only you, on this Mac");
        assert_eq!(
            issues_line(&status(Some("ME"), 12, None)),
            "Prefix ME · 12 issues"
        );
        assert_eq!(
            sync_line(&status(Some("ME"), 12, None)),
            None,
            "local storage never syncs"
        );
    }

    #[test]
    fn spaces_sharing_a_storage_repository_are_named_with_its_prefix() {
        assert_eq!(
            shared_with(&["Sage Watch"], Some("SAGE")).unwrap(),
            "Sage Watch also stores here, so it shares these issues and the SAGE prefix."
        );
        assert_eq!(
            shared_with(&["Sage Watch", "Ops", "Web"], Some("SAGE")).unwrap(),
            "Sage Watch, Ops and Web also store here, so they share these issues and the SAGE prefix."
        );
        assert_eq!(shared_with(&[], Some("SAGE")), None);
    }

    #[test]
    fn storage_without_a_tracker_offers_set_up_issues_and_asks_before_writing() {
        let sage = sage();
        assert!(
            issues_line(&status(None, 0, Some(Readers::Invited))).starts_with("No tracker yet.")
        );
        let (title, line, ok) = prefix_question(&sage, None, "SAGE");
        assert_eq!(title, "Set up issues with the prefix SAGE?");
        assert!(
            line.contains("commits only wiffletree-issues.yaml to Sage-Social/wiffletree-storage")
        );
        assert_eq!(ok, "Set up issues");
        let (title, line, _) = prefix_question(&personal(), Some("SAGE"), "ME");
        assert_eq!(title, "Rename the prefix SAGE to ME?");
        assert!(line.contains("Every issue ID in Personal’s local storage"));
    }

    #[test]
    fn a_join_refused_for_a_local_prefix_offers_that_spaces_page() {
        let spaces = [personal(), sage()];
        let error = "The Personal Space's local tracker uses the prefix SAGE. Rename its prefix on its Space's page, then try again.";
        assert_eq!(
            join_refused_by(&spaces, error),
            Some(SpaceId("personal".into()))
        );
        assert_eq!(join_refused_by(&spaces, "Sage-Social/x is public"), None);
    }

    #[test]
    fn joining_sends_each_ticked_space_with_its_clones_here() {
        let listing = |folder: &str| SpaceListing {
            folder: folder.into(),
            id: SpaceId(folder.into()),
            name: folder.into(),
            description: String::new(),
            repositories: vec!["Sage-Social/api".into(), "Sage-Social/web".into()],
            added: false,
        };
        let check = StorageCheck {
            repository: SettingsRepository {
                name: "Sage-Social/wiffletree-storage".into(),
                push_url: "u".into(),
                branch: "main".into(),
                readers: Readers::EveryMember,
            },
            organization: true,
            reader_count: None,
            history_protected: true,
            folder: Some("wiffletree".into()),
            spaces: vec![listing("sage"), listing("watch")],
            prefix: Some("SAGE".into()),
            compile: None,
            issues: 0,
            wiki_pages: 0,
            compilers: 1,
        };
        let joining = Joining {
            repository: "Sage-Social/wiffletree-storage".into(),
            check,
            ticked: ["sage".to_owned()].into(),
            compile_here: false,
            clones: [
                (
                    ("sage".to_owned(), "Sage-Social/api".to_owned()),
                    "/dev/api".to_owned(),
                ),
                (
                    ("watch".to_owned(), "Sage-Social/web".to_owned()),
                    "/dev/web".to_owned(),
                ),
            ]
            .into(),
        };
        assert_eq!(
            join_choices(&joining),
            [JoinSpace {
                folder: "sage".into(),
                clones: vec!["/dev/api".into()],
            }],
            "web is left missing, and watch is not ticked"
        );
    }

    #[test]
    fn sources_read_back_as_written_and_keep_what_the_field_does_not_show() {
        let before = vec![Source::Cli {
            command: "gh".into(),
            prefixes: vec!["gh pr view".into()],
            check: Some("gh auth status".into()),
            extra: Extra::new(),
        }];
        let text = "mcp linear: list_issues, get_issue\ncli gh: gh pr view, gh issue list";
        let sources = parse_sources(text, &before).unwrap();
        assert_eq!(format_sources(&sources), text);
        assert!(
            matches!(&sources[1], Source::Cli { check: Some(check), .. } if check == "gh auth status")
        );
        assert!(parse_sources("linear", &[]).is_err());
        assert!(parse_sources("ftp x: y", &[]).is_err());
        assert_eq!(parse_sources("\n  \n", &[]).unwrap(), vec![]);
    }

    #[test]
    fn new_project_names_the_repository_it_will_log_to() {
        assert_eq!(brain_label(&sage()), sage().storage.repository.key());
        assert_eq!(brain_label(&personal()), "local-7k2q");
    }

    #[test]
    fn saving_keeps_what_a_chip_or_another_mac_changed_in_fields_the_form_left_alone() {
        let read = sage();
        let mut latest = read.clone();
        latest.repositories = vec!["Sage-Social/api".into()];
        latest.description = "Pulled from a teammate".into();
        let mut form = read.clone();
        form.name = "Sage Haven".into();
        let saved = merged(&read, &form, &latest);
        assert_eq!(
            saved.repositories,
            ["Sage-Social/api"],
            "the chip's edit stays"
        );
        assert_eq!(saved.description, "Pulled from a teammate");
        assert_eq!(saved.name, "Sage Haven");
        form.repositories = vec![];
        latest.repositories = vec!["Sage-Social/api".into(), "Sage-Social/web".into()];
        let mut read = read;
        read.repositories = vec!["Sage-Social/api".into()];
        assert_eq!(
            merged(&read, &form, &latest).repositories,
            ["Sage-Social/web"],
            "what the form took out goes, and what joined meanwhile stays"
        );
    }

    #[test]
    fn the_space_shown_after_a_removal_reads_its_own_storage() {
        let (removed, next) = (SpaceId("sage".into()), SpaceId("personal".into()));
        let asked = (removed.clone(), 1_000);
        assert!(reads_storage_again(Some(&asked), &next, false, 1_001));
        assert!(!reads_storage_again(Some(&asked), &removed, true, 1_001));
        let stale = 1_001 + STORAGE_FRESH_MS;
        assert!(reads_storage_again(Some(&asked), &removed, true, stale));
        assert!(!reads_storage_again(Some(&asked), &removed, false, stale));
        assert!(reads_storage_again(None, &next, false, 0));
    }

    #[test]
    fn a_finished_storage_pass_reads_the_shown_spaces_storage_again() {
        let tracker = |space: &str, synced_at, error: Option<&str>| IssueTracker {
            space_id: SpaceId(space.into()),
            tracker: format!("github:sage-social/{space}"),
            prefix: Some("SAGE".into()),
            local: false,
            login: Some("mhohlios".into()),
            issues: 0,
            open: 0,
            unread: 0,
            waiting: 0,
            paused: 0,
            synced_at,
            error: error.map(Into::into),
        };
        let sage = SpaceId("sage".into());
        let before = [
            tracker("sage", Some(1_000), None),
            tracker("personal", None, None),
        ];
        assert!(!pass_finished(&before, &before, &sage));
        let synced = [
            tracker("sage", Some(2_000), None),
            tracker("personal", None, None),
        ];
        assert!(pass_finished(&before, &synced, &sage));
        let failed = [tracker("sage", Some(1_000), Some("gh: not signed in"))];
        assert!(pass_finished(&before, &failed, &sage));
        let elsewhere = [
            tracker("sage", Some(1_000), None),
            tracker("personal", Some(5), None),
        ];
        assert!(!pass_finished(&before, &elsewhere, &sage));
    }

    #[test]
    fn a_save_reply_reloads_only_the_draft_it_saved() {
        let sage = sage();
        let mut saved = sage.clone();
        saved.additions = "Treat production as read-only.".into();
        assert!(holds_draft(&sage, &Ok(saved.clone()), &sage, &saved));
        let mut typed_on = saved.clone();
        typed_on.additions.push_str(" Never log names.");
        assert!(
            !holds_draft(&sage, &Ok(typed_on), &sage, &saved),
            "typing after Save keeps the newer text"
        );
        let personal = personal();
        assert!(
            !holds_draft(&personal, &Ok(saved.clone()), &sage, &saved),
            "another Space's draft is left alone"
        );
    }
}
