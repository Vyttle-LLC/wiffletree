//! Settings › Spaces (OpenSpec change `spaces`, screens 1 and 2 of its mockup): the Spaces down
//! the left, and the shown Space's page with its Storage and issues box, or the forms that add a
//! Space, new or from a repository. Only the human edits a Space; no agent tool can.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::BTreeMap;
use ui::{banner, card, hint, icon, mono, section};
use wording::{age, counted};

const SCANS: &str = "Before pushing issues/ or spaces/, Wiffletree runs the brain gate’s personal-data and secret scans on this Mac and holds a file that fails, instead of pushing it.";
const ROUTER: &str = "The router reads this to decide where a message goes.";
const ALSO_SETTINGS: &str =
    "Your settings repository. Settings, the Space and issues each sync only their own folders.";
const SOURCES_FORMAT: &str =
    "One a line: mcp ‹server›: ‹tool›, ‹tool› or cli ‹command›: ‹prefix›, ‹prefix›.";
/// How long a storage reading stands before a change to the workspace asks again.
const STORAGE_FRESH_MS: i64 = 10_000;

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
    /// By folder, where each Space's brain is on this Mac.
    brains: BTreeMap<String, String>,
    /// By folder and `owner/name`, where each of a Space's code repositories is cloned here.
    clones: BTreeMap<(String, String), String>,
}

pub(super) struct SpacesPage {
    bridge: Bridge,
    shared: Shared,
    shown: Option<Shown>,
    /// The definition the form was filled from, and the form as it read then.
    loaded: Option<(Space, Space)>,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    instructions: Entity<TextareaState>,
    sources: Entity<TextareaState>,
    repository_input: Entity<InputState>,
    /// The storage repository for New, or the repository to add Spaces from.
    storage_input: Entity<InputState>,
    prefix_input: Entity<InputState>,
    /// The form's code repositories and default models, saved with Save.
    repositories: Vec<String>,
    models: SpaceModels,
    /// New stores the Space in a GitHub repository rather than on this Mac.
    on_github: bool,
    /// The prefix field is open, to rename a tracker's prefix or set up issues.
    editing_prefix: bool,
    storage: Option<(SpaceId, Result<StorageStatus, String>)>,
    storage_asked_at: i64,
    joining: Option<Joining>,
    /// The repository New was asked to store the Space in, as named, and who can read it,
    /// shown before anything is written.
    checked: Option<(String, StorageCheck)>,
    /// A request is waiting for the host.
    busy: bool,
    error: Option<String>,
    /// The local Space whose prefix a join was refused for, which the error offers to open.
    refused_by: Option<SpaceId>,
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
        Self {
            bridge,
            shared: Shared::default(),
            shown: None,
            loaded: None,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Sage")),
            description: textarea("What belongs in this Space, and what doesn’t", window, cx),
            instructions: textarea("Rules for every thread and job in this Space", window, cx),
            sources: textarea("mcp linear: list_issues, get_issue", window, cx),
            repository_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("owner/name, such as Sage-Social/api")
            }),
            storage_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("owner/name or a GitHub URL")),
            prefix_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("2 to 5 capitals, such as SAGE")),
            repositories: vec![],
            models: SpaceModels::default(),
            on_github: false,
            editing_prefix: false,
            storage: None,
            storage_asked_at: 0,
            joining: None,
            checked: None,
            busy: false,
            error: None,
            refused_by: None,
        }
    }

    /// Redraws while shown, loads the shown Space into the form, and reads its storage again
    /// once the reading is old.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let opened = shared.page == Page::Spaces && self.shared.page != Page::Spaces;
        self.shared = shared;
        if self.shared.page != Page::Spaces {
            return;
        }
        self.load_shown(window, cx);
        let stale = ui::now() - self.storage_asked_at > STORAGE_FRESH_MS;
        if opened || (moved && stale) {
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
    /// definition changed, by a chip or another Mac, and the form holds no edit.
    fn load_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(space) = self.shown_space().cloned() else {
            return;
        };
        if let Some((from, _)) = &self.loaded
            && from.id == space.id
            && (*from == space || self.edits(cx))
        {
            return;
        }
        self.load(&space, window, cx);
    }
    /// The form holds an edit the human has not saved.
    fn edits(&self, cx: &App) -> bool {
        self.loaded
            .as_ref()
            .is_some_and(|(from, read)| self.edited(from, cx).as_ref() != Ok(read))
    }
    fn load(&mut self, space: &Space, window: &mut Window, cx: &mut Context<Self>) {
        self.repositories = space.repositories.clone();
        self.models = space.models.clone();
        self.editing_prefix = false;
        self.error = None;
        self.refused_by = None;
        let sources = format_sources(&space.sources);
        self.name.update(cx, |input, cx| {
            input.set_value(space.name.clone(), window, cx)
        });
        for (state, value) in [
            (&self.description, space.description.clone()),
            (&self.instructions, space.instructions.clone()),
            (&self.sources, sources),
        ] {
            state.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let read = self.edited(space, cx).unwrap_or_else(|_| space.clone());
        self.loaded = Some((space.clone(), read));
    }

    fn show(&mut self, shown: Shown, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.refused_by = None;
        self.editing_prefix = false;
        if matches!(shown, Shown::New | Shown::FromRepository) {
            self.loaded = None;
            self.joining = None;
            self.checked = None;
            self.on_github = false;
            for input in [&self.name, &self.storage_input, &self.prefix_input] {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
            for state in [&self.description, &self.instructions] {
                state.update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
        self.shown = Some(shown);
        self.load_shown(window, cx);
        self.read_storage(window, cx);
        cx.notify();
    }

    /// Asks the host what the shown Space's storage holds, for its Storage and issues box.
    fn read_storage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.shown_space().map(|s| s.id.clone()) else {
            return;
        };
        self.storage_asked_at = ui::now();
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
            instructions: self.instructions.read(cx).value().trim().to_owned(),
            repositories: self.repositories.clone(),
            models: self.models.clone(),
            sources,
            ..space.clone()
        })
    }

    /// Saves the form's edits over the Space as it is now, so a field the form did not change
    /// keeps what a chip or another Mac wrote meanwhile.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(latest), Some((from, read))) = (self.shown_space().cloned(), self.loaded.clone())
        else {
            return;
        };
        let edited = match self.edited(&from, cx) {
            Ok(form) => merged(&read, &form, &latest),
            Err(error) => {
                self.error = Some(error);
                return cx.notify();
            }
        };
        self.busy = true;
        self.error = None;
        self.ask(
            ask::update_space(edited),
            window,
            cx,
            |view, saved, window, cx| {
                view.busy = false;
                match saved {
                    Ok(record) => {
                        view.load(&record.space, window, cx);
                        toast(format!("Saved {}", record.space.name), window, cx);
                    }
                    Err(error) => view.error = Some(error),
                }
            },
        );
        cx.notify();
    }

    fn add_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let identity = self.repository_input.read(cx).value().trim().to_owned();
        if identity.is_empty() {
            return;
        }
        if let Err(error) = validate_repository_identity(&identity) {
            self.error = Some(error.to_string());
            return cx.notify();
        }
        if !(self.repositories.iter()).any(|r| r.eq_ignore_ascii_case(&identity)) {
            self.repositories.push(identity);
        }
        self.error = None;
        self.repository_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn choose_brain(&mut self, space: SpaceId, window: &mut Window, cx: &mut Context<Self>) {
        let paths = folders::choose(&self.bridge, "Choose the brain’s folder", false, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let chosen = paths.recv().await.ok().and_then(|p| p.into_iter().next());
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |view, cx| {
                    if let Some(path) = chosen {
                        let path = path.to_string_lossy().into_owned();
                        let command = Command::SetSpaceBrain {
                            space_id: space,
                            path,
                        };
                        view.request(command, window, cx);
                    }
                });
            });
        })
        .detach();
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

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.error = Some("Give the Space a name".into());
            return cx.notify();
        }
        let storage = self.storage_input.read(cx).value().trim().to_owned();
        if self.on_github && storage.is_empty() {
            self.error = Some("Name the GitHub repository to store the Space in".into());
            return cx.notify();
        }
        if self.on_github && self.checked_for(&storage).is_none() {
            self.error = Some("Check the repository first, to see who can read it".into());
            return cx.notify();
        }
        let prefix = self.prefix_input.read(cx).value().trim().to_owned();
        let ask = ask::create_space(
            name,
            self.description.read(cx).value().trim().to_owned(),
            self.instructions.read(cx).value().trim().to_owned(),
            (!prefix.is_empty()).then_some(prefix),
            self.on_github.then_some(storage),
        );
        self.busy = true;
        self.error = None;
        self.ask(ask, window, cx, |view, created, window, cx| {
            view.busy = false;
            match created {
                Ok(record) => view.show(Shown::Space(record.space.id), window, cx),
                Err(error) => view.error = Some(error),
            }
        });
        cx.notify();
    }

    /// What checking `repository`, as named, found, once New has checked it.
    fn checked_for(&self, repository: &str) -> Option<&StorageCheck> {
        (self.checked.as_ref())
            .filter(|(named, _)| named == repository)
            .map(|(_, check)| check)
    }

    /// Checks New's GitHub repository, writing nothing, so the form says who can read it
    /// before Create Space writes anything.
    fn check_new_storage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repository = self.storage_input.read(cx).value().trim().to_owned();
        if repository.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        let asked = repository.clone();
        self.ask(
            ask::check_storage_repository(repository),
            window,
            cx,
            move |view, check, _, _| {
                view.busy = false;
                match check {
                    Ok(check) => view.checked = Some((asked, check)),
                    Err(error) => view.error = Some(error),
                }
            },
        );
        cx.notify();
    }

    fn check_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repository = self.storage_input.read(cx).value().trim().to_owned();
        if repository.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.refused_by = None;
        self.joining = None;
        let asked = repository.clone();
        self.ask(
            ask::check_storage_repository(repository),
            window,
            cx,
            move |view, check, _, _| {
                view.busy = false;
                match check {
                    Ok(check) => {
                        let ticked = (check.spaces.iter())
                            .filter(|listing| !listing.added)
                            .map(|listing| listing.folder.clone())
                            .collect();
                        view.joining = Some(Joining {
                            repository: asked,
                            check,
                            ticked,
                            brains: BTreeMap::new(),
                            clones: BTreeMap::new(),
                        });
                    }
                    Err(error) => view.error = Some(error),
                }
            },
        );
        cx.notify();
    }

    /// Asks where a ticked Space's brain, or one of its code repositories, is on this Mac.
    fn choose_place(
        &mut self,
        folder: String,
        repository: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = match &repository {
            Some(identity) => format!("Where is {identity} cloned?"),
            None => "Where is the brain on this Mac?".into(),
        };
        let paths = folders::choose(&self.bridge, &prompt, false, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let chosen = paths.recv().await.ok().and_then(|p| p.into_iter().next());
            let _ = this.update(cx, |view, cx| {
                let (Some(path), Some(joining)) = (chosen, view.joining.as_mut()) else {
                    return;
                };
                let path = path.to_string_lossy().into_owned();
                match repository {
                    Some(identity) => drop(joining.clones.insert((folder, identity), path)),
                    None => drop(joining.brains.insert(folder, path)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(joining) = &self.joining else {
            return;
        };
        let spaces = join_choices(joining);
        if spaces.is_empty() {
            self.error = Some("Tick at least one Space to add".into());
            return cx.notify();
        }
        self.busy = true;
        self.error = None;
        self.refused_by = None;
        let repository = joining.repository.clone();
        self.ask(
            ask::join_spaces(repository, spaces),
            window,
            cx,
            |view, joined, window, cx| {
                view.busy = false;
                match joined {
                    Ok(records) => {
                        if let Some(first) = records.into_iter().next() {
                            view.show(Shown::Space(first.space.id), window, cx);
                        }
                    }
                    Err(error) => {
                        view.refused_by = join_refused_by(view.spaces(), &error);
                        view.error = Some(error);
                    }
                }
            },
        );
        cx.notify();
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
                "What belongs here",
                Textarea::new(&self.description).w_full(),
                Some(ROUTER.into()),
                p,
            ))
            .child(row(
                "Instructions",
                Textarea::new(&self.instructions).w_full(),
                Some(format!("Added to every thread and job in {}.", space.name)),
                p,
            ))
            .child(row("Brain", self.brain_field(space, local, p, cx), None, p))
            .child(row(
                "Code repositories",
                self.repositories_field(space, p, cx),
                Some(format!("New projects in {} start with these.", space.name)),
                p,
            ))
            .child(row(
                "Default models",
                self.models_field(local, p, cx),
                None,
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
            .child(self.storage_box(space, status, p, cx));
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

    fn brain_field(
        &self,
        space: &Space,
        local: Option<&SpaceLocal>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = space.id.clone();
        let choose = ui::gated(
            button("choose-brain")
                .xsmall()
                .ghost()
                .label("Choose a brain…"),
            &self.shared.host,
        )
        .on_click(
            cx.listener(move |view, _, window, cx| view.choose_brain(id.clone(), window, cx)),
        );
        let field = div().flex().flex_col().gap_1();
        match &space.brain {
            Some(SpaceBrain {
                repository: BrainRepository::GitHub { owner, name, .. },
                folder,
                ..
            }) => field
                .child(mono(format!("{owner}/{name} · folder {folder}/"), p))
                .child(hint("Changes reach main through gated pull requests.", p)),
            Some(SpaceBrain { folder, .. }) => field
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(mono(
                            format!("{} · folder {folder}/", brain_label(space, local)),
                            p,
                        ))
                        .child(choose),
                )
                .child(hint("A folder on this Mac.", p)),
            None => field.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(hint("None", p))
                    .child(choose),
            ),
        }
    }

    fn repositories_field(&self, space: &Space, p: Palette, cx: &mut Context<Self>) -> Div {
        let here = self.shared.snapshot().map_or(&[][..], |s| &s.repositories);
        let mut field = div().flex().flex_col().gap_1();
        for (at, identity) in self.repositories.iter().enumerate() {
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
        field.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Input::new(&self.repository_input)
                        .small()
                        .flex_1()
                        .min_w_0(),
                )
                .child(
                    button("add-space-repository")
                        .xsmall()
                        .icon(icon("plus"))
                        .label("Add")
                        .on_click(
                            cx.listener(|view, _, window, cx| view.add_repository(window, cx)),
                        ),
                ),
        )
    }

    fn models_field(&self, local: Option<&SpaceLocal>, p: Palette, cx: &mut Context<Self>) -> Div {
        let notes = local.map_or(&[][..], |l| &l.notes);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child("Threads:")
                    .child(self.model_picker(ModelFor::Threads, cx))
                    .child("Jobs:")
                    .child(self.model_picker(ModelFor::Jobs, cx)),
            )
            .children(
                notes
                    .iter()
                    .map(|note| hint(note.clone(), p).text_color(p.yellow)),
            )
    }

    fn model_picker(&self, which: ModelFor, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current = match which {
            ModelFor::Threads => self.models.threads.clone(),
            ModelFor::Jobs => self.models.jobs.clone(),
        };
        let label = current
            .as_ref()
            .map_or("Who runs what".into(), |model| model.profile().short());
        let options: Vec<ModelProfile> = (self.shared.catalog.iter())
            .flat_map(|option| {
                option.efforts.iter().map(|effort| ModelProfile {
                    provider: option.provider,
                    model: option.model.clone(),
                    effort: effort.clone(),
                })
            })
            .collect();
        let weak = cx.weak_entity();
        let id = match which {
            ModelFor::Threads => "space-threads-model",
            ModelFor::Jobs => "space-jobs-model",
        };
        button(id)
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let set = |model: Option<SpaceModel>, weak: WeakEntity<SpacesPage>| {
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = weak.update(cx, |view, cx| {
                            match which {
                                ModelFor::Threads => view.models.threads = model.clone(),
                                ModelFor::Jobs => view.models.jobs = model.clone(),
                            }
                            cx.notify();
                        });
                    }
                };
                menu = menu.item(
                    PopupMenuItem::new("Who runs what")
                        .checked(current.is_none())
                        .on_click(set(None, weak.clone())),
                );
                for profile in &options {
                    let checked = current.as_ref().is_some_and(|c| &c.profile() == profile);
                    let model = Some(SpaceModel::from(profile.clone()));
                    menu = menu.item(
                        PopupMenuItem::new(profile.short())
                            .checked(checked)
                            .on_click(set(model, weak.clone())),
                    );
                }
                menu
            })
    }

    fn storage_box(
        &self,
        space: &Space,
        status: Option<&Result<StorageStatus, String>>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut head = div().flex().items_center().gap_2().child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Storage and issues"),
        );
        let status = match status {
            Some(Ok(status)) => status,
            Some(Err(error)) => {
                return card(p)
                    .child(head)
                    .child(hint(error.clone(), p).text_color(p.red));
            }
            None => return card(p).child(head).child(hint("Reading…", p)),
        };
        if let Some((line, failed)) = sync_line(status) {
            head = head
                .child(div().flex_1())
                .child(hint(line, p).text_color(if failed { p.red } else { p.green }));
        }
        let others: Vec<&str> = (self.spaces().iter())
            .filter(|s| s.id != space.id && s.storage.repository == space.storage.repository)
            .map(|s| s.name.as_str())
            .collect();
        let mut facts = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(fact("Repository", repository_line(space), p))
            .child(fact("Issues", self.issues_field(space, status, p, cx), p))
            .child(fact(
                "Who can read them",
                hint(readers_line(space, status.readers), p).text_color(
                    if status.readers.is_some_and(|r| r != Readers::Invited) {
                        p.yellow
                    } else {
                        p.text
                    },
                ),
                p,
            ));
        if let Some(line) = shared_with(&others, status.prefix.as_deref()) {
            facts = facts.child(fact("Shared with", line, p));
        }
        if status.settings_repository {
            facts = facts.child(fact("Also used for", ALSO_SETTINGS, p));
        }
        facts = facts.child(fact("On this Mac", mono(status.path.clone(), p), p));
        let mut box_ = card(p).child(head).child(facts);
        if is_shared(space) {
            box_ =
                box_.child(hint(SCANS, p)).child(
                    div().flex().justify_end().child(
                        ui::gated(
                            button("space-sync-now").label("Sync now"),
                            &self.shared.host,
                        )
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.request(Command::SyncNow, window, cx)
                        })),
                    ),
                );
        }
        box_
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
            .child(issues_line(space, status))
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

    fn new_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut page = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("New Space"))
            .child(row("Name", Input::new(&self.name).small(), None, p))
            .child(row(
                "What belongs here",
                Textarea::new(&self.description).w_full(),
                Some(ROUTER.into()),
                p,
            ))
            .child(row(
                "Instructions",
                Textarea::new(&self.instructions).w_full(),
                Some("Added to every thread and job in this Space.".into()),
                p,
            ))
            .child(row(
                "Storage",
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                ui::segment("storage-local", "Only on this Mac", !self.on_github)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.on_github = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                ui::segment("storage-github", "A GitHub repository", self.on_github)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.on_github = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(self.on_github, |d| {
                        let named = self.storage_input.read(cx).value().trim().to_owned();
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(Input::new(&self.storage_input).small().flex_1().min_w_0())
                                .child(
                                    ui::gated(
                                        button("check-new-storage").label("Check").disabled(self.busy),
                                        &self.shared.host,
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.check_new_storage(window, cx)
                                    })),
                                ),
                        )
                        .children(self.checked_for(&named).map(|check| {
                            hint(new_storage_line(check), p).text_color(p.yellow)
                        }))
                    }),
                Some(
                    if self.on_github {
                        "A private repository. Its issues and its Spaces’ instructions are shared with everyone who can read it."
                    } else {
                        "A local Git repository with no remote. Nothing in it is pushed anywhere."
                    }
                    .into(),
                ),
                p,
            ))
            .child(row(
                "Issue prefix",
                Input::new(&self.prefix_input).small().w(px(160.)),
                Some(format!("{PREFIX_RULE} Left empty, it is proposed from the name; a repository that is already a tracker keeps its own.")),
                p,
            ));
        if let Some(error) = &self.error {
            page = page.child(self.error_line(error, p, cx));
        }
        page.child(
            div().flex().justify_end().child(
                ui::gated(
                    button("create-space")
                        .primary()
                        .label("Create Space")
                        .disabled(self.busy),
                    &self.shared.host,
                )
                .on_click(cx.listener(|view, _, window, cx| view.create(window, cx))),
            ),
        )
    }

    fn join_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut page = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Add Space from a repository"))
            .child(hint(
                "A private GitHub repository whose spaces/ folder holds Spaces your team shares. Nothing is written until you add them.",
                p,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Input::new(&self.storage_input).small().flex_1().min_w_0())
                    .child(
                        ui::gated(
                            button("check-storage").label("Check").disabled(self.busy),
                            &self.shared.host,
                        )
                        .on_click(cx.listener(|view, _, window, cx| view.check_repository(window, cx))),
                    ),
            );
        if let Some(joining) = &self.joining {
            page = page.child(self.join_choices(joining, p, cx));
        }
        if let Some(error) = &self.error {
            page = page.child(self.error_line(error, p, cx));
        }
        if self.joining.is_some() {
            page = page.child(
                div().flex().justify_end().child(
                    ui::gated(
                        button("join-spaces")
                            .primary()
                            .label("Add Spaces")
                            .disabled(self.busy),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.join(window, cx))),
                ),
            );
        }
        page
    }

    fn join_choices(&self, joining: &Joining, p: Palette, cx: &mut Context<Self>) -> Div {
        let check = &joining.check;
        let here = self.shared.snapshot().map_or(&[][..], |s| &s.repositories);
        let mut list = div().flex().flex_col().gap_3().child(hint(
            format!(
                "{} · {} · {}",
                check.repository.name,
                readers_sentence(check.repository.readers, &check.repository.name),
                match &check.prefix {
                    Some(prefix) => format!("issues use the prefix {prefix}"),
                    None => "no tracker yet; its Space pages offer Set up issues".into(),
                }
            ),
            p,
        ));
        if check.spaces.is_empty() {
            return list.child(hint("It holds no Space yet.", p));
        }
        for listing in &check.spaces {
            let folder = listing.folder.clone();
            let ticked = joining.ticked.contains(&listing.folder);
            let mut entry = div().flex().flex_col().gap_1().child(
                Checkbox::new(SharedString::from(format!("join-{}", listing.folder)))
                    .label(if listing.added {
                        format!("{} · already on this Mac", listing.name)
                    } else {
                        listing.name.clone()
                    })
                    .checked(ticked)
                    .disabled(listing.added)
                    .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                        if let Some(joining) = &mut view.joining {
                            match checked {
                                true => joining.ticked.insert(folder.clone()),
                                false => joining.ticked.remove(&folder),
                            };
                        }
                        cx.notify();
                    })),
            );
            if !listing.description.is_empty() {
                entry = entry.child(hint(listing.description.clone(), p).pl_6());
            }
            if ticked {
                entry = entry.child(self.places(joining, listing, here, p, cx).pl_6());
            }
            list = list.child(entry);
        }
        list
    }

    /// Where a ticked Space's brain and code repositories are on this Mac, each with Choose…;
    /// a repository already in the workspace needs nothing, and one left alone stays missing.
    fn places(
        &self,
        joining: &Joining,
        listing: &SpaceListing,
        here: &[Repository],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let choose =
            |id: String, folder: String, repository: Option<String>, cx: &mut Context<Self>| {
                button(SharedString::from(id))
                    .xsmall()
                    .ghost()
                    .label("Choose…")
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.choose_place(folder.clone(), repository.clone(), window, cx)
                    }))
            };
        let mut places = div().flex().flex_col().gap_1();
        if let Some(brain) = &listing.brain {
            let named = match &brain.repository {
                BrainRepository::GitHub { owner, name, .. } => format!("Brain {owner}/{name}"),
                BrainRepository::Local { .. } => "Brain, a folder".into(),
            };
            let at = joining.brains.get(&listing.folder);
            places = places.child(place(
                named,
                at.map_or("missing on this Mac".into(), Clone::clone),
                choose(
                    format!("brain-{}", listing.folder),
                    listing.folder.clone(),
                    None,
                    cx,
                ),
                p,
            ));
        }
        for (at, identity) in listing.repositories.iter().enumerate() {
            if let Some(cloned) = here.iter().find(|r| r.is_github(identity)) {
                places = places.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_1().child(identity.clone()))
                        .child(hint(format!("cloned at {}", cloned.path), p)),
                );
                continue;
            }
            let chosen = joining
                .clones
                .get(&(listing.folder.clone(), identity.clone()));
            places = places.child(place(
                identity.clone(),
                chosen.map_or("missing on this Mac".into(), Clone::clone),
                choose(
                    format!("clone-{}-{at}", listing.folder),
                    listing.folder.clone(),
                    Some(identity.clone()),
                    cx,
                ),
                p,
            ));
        }
        places
    }
}

#[derive(Clone, Copy)]
enum ModelFor {
    Threads,
    Jobs,
}

const REMOVE_SPACE: &str = "Its storage and brain repositories stay as they are. Its archived projects move to Personal. A Space with an open project, or the last Space, can’t be removed.";
const PREFIX_RULE: &str = "2 to 5 capitals, used by no other storage repository on this Mac.";

/// Who can read the repository New will store the Space in, and its tracker, said before
/// anything is written.
fn new_storage_line(check: &StorageCheck) -> String {
    let repository = &check.repository.name;
    let tracker = match &check.prefix {
        Some(prefix) => format!(" It is already a tracker; the Space shares its prefix {prefix}."),
        None => String::new(),
    };
    format!(
        "{} They can read this Space and its issues, and anyone who can push to it can change its instructions.{tracker}",
        readers_sentence(check.repository.readers, repository)
    )
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
        instructions: pick(&read.instructions, &form.instructions, &latest.instructions),
        repositories: pick(&read.repositories, &form.repositories, &latest.repositories),
        models: pick(&read.models, &form.models, &latest.models),
        sources: pick(&read.sources, &form.sources, &latest.sources),
        ..latest.clone()
    }
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

/// One line of the Storage and issues box.
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

/// Where a joined Space's brain or repository is on this Mac, with Choose….
fn place(named: String, at: String, choose: Button, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().min_w_0().child(named))
        .child(hint(at, p))
        .child(choose)
}

/// A Space's square, in its color.
pub(super) fn space_mark(color: Hsla) -> Div {
    div().size(px(8.)).flex_none().rounded(px(2.)).bg(color)
}

/// Each Space's color, by its place in the list.
pub(super) fn space_color(spaces: &[Space], id: &SpaceId, p: Palette) -> Hsla {
    let colors = [p.focus, p.blue, p.magenta, p.green, p.yellow, p.accent];
    let at = spaces.iter().position(|s| &s.id == id).unwrap_or(0);
    colors[at % colors.len()]
}

/// Stored on GitHub, so other people can read it.
pub(super) fn is_shared(space: &Space) -> bool {
    matches!(space.storage.repository, StorageRepository::GitHub { .. })
}

/// Who `readers` are, as a sentence about `repository`.
fn readers_sentence(readers: Readers, repository: &str) -> String {
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
                " {}’s storage is a local Git repository with no remote. Nothing in it is pushed anywhere.",
                space.name
            ),
        );
    };
    let repository = space.storage.repository.key();
    let read = readers.map_or(String::new(), |r| {
        format!(" {}", readers_sentence(r, &repository))
    });
    (
        "Shared.",
        format!(
            "{read} Anyone who can push to {repository} can change these instructions for everyone in this Space, and can read and edit its issues. Edits to the Space are commits and follow the settings repository’s sync, pause and compare rules."
        ),
    )
}

fn repository_line(space: &Space) -> String {
    match &space.storage.repository {
        StorageRepository::Local { .. } => "Local, no remote".into(),
        repository => format!(
            "{} · this Space’s folder spaces/{}/",
            repository.key(),
            space.storage.folder
        ),
    }
}

fn issues_line(space: &Space, status: &StorageStatus) -> String {
    let Some(prefix) = &status.prefix else {
        return "No tracker yet. Set up issues chooses its prefix and commits wiffletree-issues.yaml."
            .into();
    };
    let count = counted(status.issues, "issue");
    match is_shared(space) {
        true => format!(
            "Prefix {prefix} · {count} · issues/ and wiffletree-issues.yaml at the repository’s root"
        ),
        false => format!("Prefix {prefix} · {count}"),
    }
}

/// Who can read the Space's storage, and so its issues.
fn readers_line(space: &Space, readers: Option<Readers>) -> String {
    let owner = repository_owner(&space.storage.repository.key()).to_owned();
    match readers {
        None => "Only you, on this Mac".into(),
        Some(Readers::Invited) => "Only you and people you invite".into(),
        Some(Readers::EveryMember) => format!("Every member of {owner}"),
        Some(Readers::EnterpriseMembers) => format!("Every member of {owner}’s enterprise"),
        Some(Readers::MaybeMembers) => format!("Maybe every member of {owner}"),
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

/// The Space's brain as New project and the Space page name it.
pub(super) fn brain_label(space: &Space, local: Option<&SpaceLocal>) -> String {
    match &space.brain {
        None => "no brain".into(),
        Some(SpaceBrain {
            repository: BrainRepository::GitHub { owner, name, .. },
            ..
        }) => format!("{owner}/{name}"),
        Some(_) => (local.and_then(|l| l.brain_path.clone()))
            .unwrap_or_else(|| "a folder not chosen on this Mac".into()),
    }
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

/// The ticked Spaces with where their brain and repositories are on this Mac.
fn join_choices(joining: &Joining) -> Vec<JoinSpace> {
    (joining.check.spaces.iter())
        .filter(|listing| joining.ticked.contains(&listing.folder))
        .map(|listing| JoinSpace {
            folder: listing.folder.clone(),
            brain_path: joining.brains.get(&listing.folder).cloned(),
            clones: (joining.clones.iter())
                .filter(|((folder, _), _)| folder == &listing.folder)
                .map(|(_, path)| path.clone())
                .collect(),
        })
        .collect()
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
pub(super) fn format_sources(sources: &[Source]) -> String {
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
pub(super) fn parse_sources(text: &str, before: &[Source]) -> Result<Vec<Source>, String> {
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
        if matches!(command, Command::SyncNow | Command::SetSpaceBrain { .. }) {
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
    use super::{
        Joining, brain_label, format_sources, issues_line, join_choices, join_refused_by, merged,
        new_storage_line, parse_sources, prefix_question, readers_line, repository_line,
        shared_with, storage_banner, sync_line,
    };
    use workspace_core::*;

    fn space(name: &str, storage: StorageRepository) -> Space {
        Space {
            id: SpaceId(name.to_lowercase()),
            name: name.into(),
            description: String::new(),
            instructions: String::new(),
            brain: None,
            storage: SpaceStorage {
                repository: storage,
                folder: name.to_lowercase(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
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
            prefix: prefix.map(Into::into),
            issues,
            readers,
            settings_repository: false,
            path: "/data/repos/local-7k2q".into(),
            synced_at: None,
            error: None,
        }
    }

    #[test]
    fn a_shared_spaces_page_says_who_reads_it_and_that_pushers_change_its_instructions() {
        let sage = sage();
        let (lead, text) = storage_banner(&sage, Some(Readers::EveryMember));
        assert_eq!(lead, "Shared.");
        assert!(
            text.contains("Every member of Sage-Social can read Sage-Social/wiffletree-storage.")
        );
        assert!(text.contains(
            "Anyone who can push to Sage-Social/wiffletree-storage can change these instructions for everyone in this Space"
        ));
        let status = status(Some("SAGE"), 148, Some(Readers::EveryMember));
        assert_eq!(
            issues_line(&sage, &status),
            "Prefix SAGE · 148 issues · issues/ and wiffletree-issues.yaml at the repository’s root"
        );
        assert_eq!(
            readers_line(&sage, status.readers),
            "Every member of Sage-Social"
        );
        assert_eq!(
            repository_line(&sage),
            "Sage-Social/wiffletree-storage · this Space’s folder spaces/sage/"
        );
    }

    #[test]
    fn a_personal_spaces_page_says_its_storage_is_local_and_only_you_read_its_issues() {
        let personal = personal();
        let (lead, text) = storage_banner(&personal, None);
        assert_eq!(lead, "Only on this Mac.");
        assert!(text.contains("Personal’s storage is a local Git repository with no remote."));
        assert_eq!(repository_line(&personal), "Local, no remote");
        assert_eq!(readers_line(&personal, None), "Only you, on this Mac");
        assert_eq!(
            issues_line(&personal, &status(Some("ME"), 12, None)),
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
            issues_line(&sage, &status(None, 0, Some(Readers::Invited)))
                .starts_with("No tracker yet.")
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
    fn joining_sends_each_ticked_space_with_its_brain_and_clones_here() {
        let listing = |folder: &str| SpaceListing {
            folder: folder.into(),
            id: SpaceId(folder.into()),
            name: folder.into(),
            description: String::new(),
            brain: None,
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
            spaces: vec![listing("sage"), listing("watch")],
            prefix: Some("SAGE".into()),
        };
        let joining = Joining {
            repository: "Sage-Social/wiffletree-storage".into(),
            check,
            ticked: ["sage".to_owned()].into(),
            brains: [("sage".to_owned(), "/dev/brain".to_owned())].into(),
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
                brain_path: Some("/dev/brain".into()),
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
    fn new_project_names_the_brain_it_will_log_to() {
        let mut sage = sage();
        assert_eq!(brain_label(&sage, None), "no brain");
        sage.brain = Some(SpaceBrain {
            repository: BrainRepository::GitHub {
                owner: "Sage-Social".into(),
                name: "sagehaven-docs".into(),
                extra: Extra::new(),
            },
            folder: "second-brain".into(),
            extra: Extra::new(),
        });
        assert_eq!(brain_label(&sage, None), "Sage-Social/sagehaven-docs");
        let mut personal = personal();
        personal.brain = Some(SpaceBrain {
            repository: BrainRepository::local(),
            folder: "second-brain".into(),
            extra: Extra::new(),
        });
        let local = SpaceLocal {
            brain_path: Some("/Users/m/dev/brain".into()),
            ..SpaceLocal::default()
        };
        assert_eq!(brain_label(&personal, Some(&local)), "/Users/m/dev/brain");
    }

    #[test]
    fn saving_keeps_what_a_chip_or_another_mac_changed_in_fields_the_form_left_alone() {
        let read = sage();
        let mut latest = read.clone();
        latest.repositories = vec!["Sage-Social/api".into()];
        latest.description = "Pulled from a teammate".into();
        let mut form = read.clone();
        form.instructions = "Treat production as read-only.".into();
        let saved = merged(&read, &form, &latest);
        assert_eq!(
            saved.repositories,
            ["Sage-Social/api"],
            "the chip's edit stays"
        );
        assert_eq!(saved.description, "Pulled from a teammate");
        assert_eq!(saved.instructions, "Treat production as read-only.");
        form.repositories = vec![];
        latest.repositories = vec!["Sage-Social/api".into(), "Sage-Social/web".into()];
        let mut read = read;
        read.repositories = vec!["Sage-Social/api".into()];
        assert!(
            merged(&read, &form, &latest).repositories.is_empty(),
            "a list the form changed is saved as the form has it"
        );
    }

    #[test]
    fn a_new_shared_space_says_who_can_read_its_repository_before_it_is_written() {
        let check = StorageCheck {
            repository: SettingsRepository {
                name: "Sage-Social/wiffletree-storage".into(),
                push_url: "u".into(),
                branch: "main".into(),
                readers: Readers::EveryMember,
            },
            organization: true,
            spaces: vec![],
            prefix: Some("SAGE".into()),
        };
        let line = new_storage_line(&check);
        assert!(
            line.starts_with(
                "Every member of Sage-Social can read Sage-Social/wiffletree-storage."
            )
        );
        assert!(line.contains("anyone who can push to it can change its instructions"));
        assert!(line.ends_with("shares its prefix SAGE."));
    }
}
