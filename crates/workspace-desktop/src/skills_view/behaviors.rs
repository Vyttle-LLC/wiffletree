//! The Behaviors page's front (OpenSpec change `settings-cascade`): a scope bar of Workspace,
//! a Space and one of its repositories; each role's Behaviors at that scope, with the layer each
//! value came from and a Reset to what the scope inherits; and the scope's additions. Every
//! value is read and saved through `layer_values` and `set_layer_values`, and resolved here as
//! the host resolves it, so a choice the host would refuse is not offered.
use super::*;
use gpui_component::{
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
};
use workspace_core::cascade::{self, Behavior, Choices, Layer, LayerValues, Resolved, Scope};

const WORKSPACE_NOTE: &str = "How your agents work. Stock defaults, changed where you need them.";
const ADVANCED: &str =
    "Rewrite a role's stock text, its history and merges. Rewrites are Workspace-only.";

/// What the Behaviors list shows and holds: the scope, the catalog and what each layer through
/// the scope sets, as last read or saved.
pub(in crate::skills_view) struct Behaviors {
    scope: Scope,
    catalog: Vec<Behavior>,
    values: Layers,
    /// The Space's or repository's additions as the human is editing them.
    additions: Entity<TextareaState>,
    /// The host's refusal of the last save, beside the Behavior it names or the additions.
    refusal: Option<(Refused, String)>,
    advanced: bool,
}

/// What each layer through the scope sets, as last read or saved, and how many saves wait for
/// the host. A save is shown at once, so a change made before the host answers is sent with
/// it; each save sends everything the scope sets, so only the last one's reply is kept.
#[derive(Debug, Default)]
struct Layers {
    /// By scope; a scope the list no longer resolves through is dropped.
    values: Vec<(Scope, LayerValues)>,
    saving: usize,
}
impl Layers {
    fn get(&self, scope: &Scope) -> Option<&LayerValues> {
        (self.values.iter()).find_map(|(s, values)| (s == scope).then_some(values))
    }
    fn put(&mut self, scope: Scope, values: LayerValues) {
        self.values.retain(|(s, _)| *s != scope);
        self.values.push((scope, values));
    }
    fn keep(&mut self, chain: &[Scope]) {
        self.values.retain(|(scope, _)| chain.contains(scope));
    }
    /// What the additions editor holds when `scope` opens: its own additions, or nothing until
    /// they are read.
    fn additions(&self, scope: &Scope) -> String {
        self.get(scope)
            .map_or(String::new(), |values| values.additions.clone())
    }
    /// Shows a save before the host answers it, and returns what it replaced.
    fn send(&mut self, scope: Scope, values: LayerValues) -> Option<LayerValues> {
        let previous = self.get(&scope).cloned();
        self.put(scope, values);
        self.saving += 1;
        previous
    }
    /// The host answered a save: whether it was the last one waiting.
    fn answered(&mut self) -> bool {
        self.saving -= 1;
        self.saving == 0
    }
}

/// What a save that the host refused was changing.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Refused {
    Behavior(String),
    Additions,
}

impl Behaviors {
    pub(in crate::skills_view) fn new(
        window: &mut Window,
        cx: &mut Context<BehaviorsPage>,
    ) -> Self {
        let additions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 16)
                .placeholder("Nothing added.")
        });
        // Save and Discard follow the draft.
        cx.subscribe(&additions, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            scope: Scope::Workspace,
            catalog: vec![],
            values: Layers::default(),
            additions,
            refusal: None,
            advanced: false,
        }
    }
    fn saved(&self, scope: &Scope) -> Option<&LayerValues> {
        self.values.get(scope)
    }
    /// What each layer through the scope sets, once every one has been read.
    fn chain(&self) -> Option<Vec<(Layer, &Choices)>> {
        let chain = chain(&self.scope).into_iter();
        chain
            .map(|scope| Some((scope.layer(), &self.saved(&scope)?.behaviors)))
            .collect()
    }
}

/// The scopes `scope` resolves through, Workspace first and `scope` last.
fn chain(scope: &Scope) -> Vec<Scope> {
    match scope {
        Scope::Workspace => vec![Scope::Workspace],
        Scope::Space { space_id } => vec![Scope::Workspace, scope_of(space_id, None)],
        Scope::Repository {
            space_id,
            repository,
        } => vec![
            Scope::Workspace,
            scope_of(space_id, None),
            scope_of(space_id, Some(repository)),
        ],
    }
}

/// The Space's scope, or that of one of its repositories.
fn scope_of(space_id: &SpaceId, repository: Option<&str>) -> Scope {
    let space_id = space_id.clone();
    match repository {
        None => Scope::Space { space_id },
        Some(repository) => Scope::Repository {
            space_id,
            repository: repository.into(),
        },
    }
}

fn space_id(scope: &Scope) -> Option<&SpaceId> {
    match scope {
        Scope::Workspace => None,
        Scope::Space { space_id } | Scope::Repository { space_id, .. } => Some(space_id),
    }
}

/// `owner/name`'s name, as a repository's chip and additions call it.
fn short_name(repository: &str) -> &str {
    repository.rsplit('/').next().unwrap_or(repository)
}

/// A layer as the page names it at `scope`: the Space by its name and a repository by its own.
fn layer_name(layer: Layer, scope: &Scope, space: &str) -> String {
    match (layer, scope) {
        (Layer::Space, _) => space.into(),
        (Layer::Repository, Scope::Repository { repository, .. }) => short_name(repository).into(),
        _ => layer.name().into(),
    }
}

/// One Behavior as a scope shows it.
#[derive(Debug)]
struct Row<'a> {
    behavior: &'a Behavior,
    /// Its value at the scope, and the layer that came from.
    resolved: Resolved<String>,
    /// Its value without the scope's own, which Reset goes back to.
    inherited: Resolved<String>,
    /// The scope sets a value itself, so Reset is offered.
    set_here: bool,
    /// The choices the scope may make: none for a layer the Behavior does not allow, and for a
    /// safety Behavior at a Space or repository only those at least as strict as it inherits.
    offered: Vec<&'a cascade::Choice>,
}
impl Row<'_> {
    fn label(&self) -> &str {
        (self.behavior.choice(&self.resolved.value)).map_or(&self.resolved.value, |c| &c.label)
    }
    /// An on/off Behavior draws as a switch, any other as a pick.
    fn is_toggle(&self) -> bool {
        let values = self.behavior.choices.iter().map(|c| c.value.as_str());
        values.eq(["on", "off"])
    }
}

/// Every Behavior of `catalog` at the scope whose layer is `layer`, given what each layer
/// through it sets, `layer` last.
fn rows<'a>(catalog: &'a [Behavior], layer: Layer, chain: &[(Layer, &Choices)]) -> Vec<Row<'a>> {
    catalog
        .iter()
        .map(|behavior| {
            let set: Vec<(Layer, String)> = (chain.iter())
                .filter_map(|(l, choices)| Some((*l, choices.get(&behavior.id)?.clone())))
                .collect();
            let below: Vec<_> = set.iter().filter(|(l, _)| *l != layer).cloned().collect();
            let inherited = cascade::resolve(behavior, &below);
            let offered = (behavior.choices.iter())
                .filter(|c| cascade::check(behavior, layer, &inherited.value, &c.value).is_ok())
                .collect();
            Row {
                behavior,
                resolved: cascade::resolve(behavior, &set),
                set_here: set.len() > below.len(),
                inherited,
                offered,
            }
        })
        .collect()
}

/// The chip beside a row: where its value came from, or "Set here" for the scope's own.
fn chip(row: &Row<'_>, scope: &Scope, space: &str) -> (String, Layer) {
    let from = row.resolved.from;
    let name = match from == scope.layer() {
        true => "Set here".into(),
        false => layer_name(from, scope, space),
    };
    (name, from)
}

fn reset_label(row: &Row<'_>, scope: &Scope, space: &str) -> String {
    format!("Reset to {}", layer_name(row.inherited.from, scope, space))
}

/// Why a row has no control: the scope's layer may not set it.
fn read_only_note(layer: Layer) -> String {
    format!("A {} can't set this", layer.name().to_lowercase())
}

/// What a Space or repository is told about its safety Behaviors: that they only tighten, and
/// which looser choices it is not offered.
fn tightening_note(rows: &[Row<'_>], layer: Layer) -> Option<String> {
    if !layer.is_shared_with_a_space() {
        return None;
    }
    let safety: Vec<_> = (rows.iter())
        .filter(|row| row.behavior.tightens && !row.offered.is_empty())
        .collect();
    let names: Vec<_> = safety
        .iter()
        .map(|row| row.behavior.name.as_str())
        .collect();
    let names = match names.as_slice() {
        [] => return None,
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let withheld: Vec<_> = (safety.iter())
        .flat_map(|row| {
            let offered = |c: &&cascade::Choice| row.offered.iter().any(|o| o.value == c.value);
            row.behavior.choices.iter().filter(move |c| !offered(c))
        })
        .map(|choice| format!("“{}”", choice.label))
        .collect();
    let withheld = match withheld.as_slice() {
        [] => String::new(),
        [one] => format!(" {one} is not offered here."),
        [rest @ .., last] => format!(" {} and {last} are not offered here.", rest.join(", ")),
    };
    Some(format!(
        "A Space or repository may only make {names} stricter.{withheld}"
    ))
}

/// `values` with the Behavior `id` set to `choice`, or with none to inherit it.
fn with_choice(values: &LayerValues, id: &str, choice: Option<&str>) -> LayerValues {
    let mut values = values.clone();
    match choice {
        Some(choice) => values.behaviors.insert(id.into(), choice.into()),
        None => values.behaviors.remove(id),
    };
    values
}

/// A role's heading, and who its Workspace additions reach.
fn role(skill: Skill) -> (&'static str, &'static str) {
    match skill {
        Skill::Coordinating => ("Coordinator", "every coordinator"),
        Skill::Implementing => ("Implementer", "every implementer"),
        Skill::Reviewing => ("Reviewers", "every tester and reviewer"),
        Skill::KeepingAPr => ("PR keeper", "the PR keeper"),
        Skill::Assisting => ("Assistant", "every assistant thread"),
    }
}

/// The Workspace's additions for a skill, as its role's line says: its rules, or that a rewrite
/// takes their place.
fn workspace_additions(state: &SkillState) -> String {
    if state.rewrite.is_some() {
        return "Rewritten; see Advanced".into();
    }
    match rule_count(&state.rules) {
        0 => "Nothing added".into(),
        count => counted(count, "rule"),
    }
}

/// Advanced holds the rewrites, which are the Workspace's alone.
fn shows_advanced(scope: &Scope) -> bool {
    *scope == Scope::Workspace
}

impl BehaviorsPage {
    /// Opens the page at `scope`, as a Space's page does for its own. The additions editor
    /// takes the new scope's own additions, so another scope's text is never saved to it.
    pub(crate) fn open_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if self.behaviors.scope != scope {
            self.behaviors.scope = scope;
            self.behaviors.refusal = None;
            (self.behaviors.values).keep(&chain(&self.behaviors.scope));
            let text = (self.behaviors.values).additions(&self.behaviors.scope);
            (self.behaviors.additions).update(cx, |input, cx| input.set_value(text, window, cx));
        }
        self.open_skill(SkillView::List, window, cx);
    }

    /// Reads the catalog once, and what each layer through the scope sets.
    pub(in crate::skills_view) fn read_behaviors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.behaviors.catalog.is_empty() {
            self.ask(
                ask::behavior_catalog(),
                window,
                cx,
                |view, catalog, window, cx| match catalog {
                    Ok(catalog) => view.behaviors.catalog = catalog,
                    Err(error) => toast_error(error, window, cx),
                },
            );
        }
        for scope in chain(&self.behaviors.scope) {
            let read = scope.clone();
            self.ask(
                ask::layer_values(scope),
                window,
                cx,
                move |view, values, window, cx| match values {
                    // A save still waiting holds newer values than this read.
                    Ok(_) if view.behaviors.values.saving > 0 => {}
                    Ok(values) => view.store(read, values, window, cx),
                    Err(error) => toast_error(error, window, cx),
                },
            );
        }
    }

    /// The additions editor follows the scope's own additions unless it holds unsaved edits.
    fn store(
        &mut self,
        scope: Scope,
        values: LayerValues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let behaviors = &mut self.behaviors;
        if !chain(&behaviors.scope).contains(&scope) {
            return;
        }
        if scope == behaviors.scope {
            let draft = behaviors.additions.read(cx).value().to_string();
            let unsaved = (behaviors.saved(&scope)).is_some_and(|saved| saved.additions != draft);
            if !unsaved {
                let text = values.additions.clone();
                (behaviors.additions).update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
        behaviors.values.put(scope, values);
    }

    /// Saves what the scope sets, its Behaviors with one changed or with the additions edited,
    /// showing it before the host answers.
    fn save_layer(
        &mut self,
        values: LayerValues,
        saving: Refused,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scope = self.behaviors.scope.clone();
        let saved = scope.clone();
        let previous = self.behaviors.values.send(scope.clone(), values.clone());
        self.ask(
            ask::set_layer_values(scope, values),
            window,
            cx,
            move |view, values, window, cx| {
                let last = view.behaviors.values.answered();
                match values {
                    Ok(values) if last => {
                        view.behaviors.refusal = None;
                        if saving == Refused::Additions && view.behaviors.scope == saved {
                            let text = values.additions.clone();
                            (view.behaviors.additions)
                                .update(cx, |input, cx| input.set_value(text, window, cx));
                        }
                        view.store(saved, values, window, cx);
                    }
                    // A later save, sent with this one's values, answers for both.
                    Ok(_) => {}
                    Err(error) => {
                        view.behaviors.refusal = Some((saving, error));
                        // The values from before this save come back before the read, so
                        // refused additions stay in the editor as unsaved.
                        if !last {
                            return;
                        }
                        let values = &mut view.behaviors.values;
                        if let Some(previous) = previous.filter(|_| values.get(&saved).is_some()) {
                            values.put(saved, previous);
                        }
                        view.read_behaviors(window, cx);
                    }
                }
            },
        );
    }

    fn choose(
        &mut self,
        id: String,
        choice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(saved) = self.behaviors.saved(&self.behaviors.scope) else {
            return;
        };
        let values = with_choice(saved, &id, choice.as_deref());
        self.save_layer(values, Refused::Behavior(id), window, cx);
    }

    fn space(&self) -> Option<&Space> {
        let id = space_id(&self.behaviors.scope)?;
        (self.shared.snapshot()?.spaces.iter()).find(|space| &space.id == id)
    }

    /// The list at the scope: the scope bar, each role's Behaviors and additions, and, at the
    /// Workspace, Advanced.
    pub(in crate::skills_view) fn behaviors_list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let scope = self.behaviors.scope.clone();
        let space = self
            .space()
            .map_or("", |space| space.name.as_str())
            .to_owned();
        let note = match self.space() {
            Some(space) if spaces_view::is_shared(space) => {
                format!("Shared with {}'s members · {APPLIES}", space.name)
            }
            Some(_) => APPLIES.into(),
            None => format!("{WORKSPACE_NOTE} {APPLIES}"),
        };
        let page = (div().flex().flex_col().gap_2())
            .child(hint(note, p))
            .child(self.scope_bar(p, cx));
        let Some(chain) = self
            .behaviors
            .chain()
            .filter(|_| !self.behaviors.catalog.is_empty())
        else {
            return page.child(hint("Loading…", p));
        };
        let rows = rows(&self.behaviors.catalog, scope.layer(), &chain);
        let skills = Skill::ALL.into_iter().filter(|skill| {
            shows_advanced(&scope) || rows.iter().any(|row| row.behavior.skill == *skill)
        });
        let sections = skills.map(|skill| {
            let rows = rows.iter().filter(|row| row.behavior.skill == skill);
            let rows = rows.map(|row| self.behavior_row(row, &space, p, cx));
            let section = (div().flex().flex_col())
                .child(Self::role_heading(role(skill).0, p))
                .children(rows);
            match shows_advanced(&scope) {
                true => section.children(self.workspace_additions_line(skill, p, cx)),
                false => section,
            }
        });
        let page = page
            .child(ui::locked(
                div().flex().flex_col().gap_1().children(sections),
                &self.shared.host,
                p,
            ))
            .children(tightening_note(&rows, scope.layer()).map(|note| hint(note, p).pt_1()));
        match &scope {
            Scope::Workspace => page.child(self.advanced(p, cx)),
            Scope::Space { .. } => page.child(self.additions_editor(
                format!("{space} additions"),
                format!("every role in {space}; was “Space instructions”"),
                p,
                cx,
            )),
            Scope::Repository { repository, .. } => page.child(self.additions_editor(
                format!("{} additions", short_name(repository)),
                format!("in {space} only"),
                p,
                cx,
            )),
        }
    }

    /// `Workspace · Space ▾ · Repository ▾`: the Space menu lists every Space, and the
    /// Repository menu the chosen Space's repositories, off until a Space is chosen.
    fn scope_bar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let scope = self.behaviors.scope.clone();
        let spaces: Vec<(SpaceId, String)> = (self.shared.snapshot().into_iter())
            .flat_map(|s| &s.spaces)
            .map(|space| (space.id.clone(), space.name.clone()))
            .collect();
        let repositories = self.space().map_or(vec![], |s| s.repositories.clone());
        let weak = cx.weak_entity();
        let open =
            move |weak: &WeakEntity<Self>, scope: Scope, window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |view, cx| view.open_scope(scope, window, cx));
            };
        let workspace = ui::segment("scope-workspace", "Workspace", scope == Scope::Workspace)
            .ghost()
            .on_click(cx.listener(|v, _, w, c| v.open_scope(Scope::Workspace, w, c)));
        let space_label = self
            .space()
            .map_or("Space".into(), |s| format!("Space · {}", s.name));
        let chosen_space = space_id(&scope).cloned();
        let space = ui::segment(
            "scope-space",
            space_label,
            matches!(scope, Scope::Space { .. }),
        )
        .ghost()
        .dropdown_caret(true)
        .disabled(spaces.is_empty())
        .dropdown_menu({
            let weak = weak.clone();
            move |mut menu, _, _| {
                for (id, name) in &spaces {
                    let (weak, scope) = (weak.clone(), scope_of(id, None));
                    let checked = chosen_space.as_ref() == Some(id);
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(checked)
                            .on_click(move |_, window, cx| open(&weak, scope.clone(), window, cx)),
                    );
                }
                menu
            }
        });
        let (repository_label, chosen) = match &scope {
            Scope::Repository { repository, .. } => {
                (short_name(repository).to_owned(), Some(repository.clone()))
            }
            _ => ("Repository".to_owned(), None),
        };
        let space_of_scope = space_id(&scope).cloned();
        let repository = ui::segment("scope-repository", repository_label, chosen.is_some())
            .ghost()
            .dropdown_caret(true)
            .disabled(space_of_scope.is_none() || repositories.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                let Some(space_id) = &space_of_scope else {
                    return menu;
                };
                for repository in &repositories {
                    let (weak, scope) = (weak.clone(), scope_of(space_id, Some(repository)));
                    let checked = chosen.as_ref() == Some(repository);
                    menu = menu.item(
                        PopupMenuItem::new(repository.clone())
                            .checked(checked)
                            .on_click(move |_, window, cx| open(&weak, scope.clone(), window, cx)),
                    );
                }
                menu
            });
        div().flex().child(
            (div().flex().items_center().gap_1().p(px(3.)))
                .rounded_lg()
                .border_1()
                .border_color(p.edge)
                .bg(p.base)
                .child(workspace)
                .child(space)
                .child(repository),
        )
    }

    fn role_heading(name: &'static str, p: Palette) -> Div {
        eyebrow(name.to_uppercase(), p)
            .pt_3()
            .pb_1()
            .border_b_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
    }

    /// A Behavior's name and what agents are told, its control, its chip and Reset.
    fn behavior_row(&self, row: &Row<'_>, space: &str, p: Palette, cx: &mut Context<Self>) -> Div {
        let scope = &self.behaviors.scope;
        let behavior = row.behavior;
        let id = behavior.id.clone();
        let told = (behavior.choice(&row.resolved.value)).map_or("", |c| c.sentence.as_str());
        let stricter_only = (behavior.tightens && *scope == Scope::Workspace)
            .then(|| hint("Spaces and repositories may only make this stricter.", p));
        let refusal = (self.behaviors.refusal.as_ref())
            .filter(|(refused, _)| *refused == Refused::Behavior(id.clone()))
            .map(|(_, error)| hint(error.clone(), p).text_color(p.red));
        let notes = row.resolved.notes.iter().map(|note| hint(note.clone(), p));
        let about = (div().flex_1().min_w_0().flex().flex_col())
            .child(behavior.name.clone())
            .when(!told.is_empty(), |d| {
                d.child(hint(told.to_owned(), p).text_size(px(font::CAPTION)))
            })
            .children(stricter_only)
            .children(notes)
            .children(refusal);
        let (chip_name, from) = chip(row, scope, space);
        let reset = match row.set_here {
            true => div()
                .id(SharedString::from(format!("reset-{id}")))
                .text_color(p.accent)
                .cursor_pointer()
                .on_click(cx.listener({
                    let id = id.clone();
                    move |v, _, w, c| v.choose(id.clone(), None, w, c)
                }))
                .child(reset_label(row, scope, space)),
            false => div().id(SharedString::from(format!("reset-{id}"))),
        };
        (div().py_2().flex().items_center().gap_3())
            .border_b_1()
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .child(about)
            .child(self.control(row, p, cx))
            .child(source_chip(chip_name, from, from == scope.layer(), p))
            .child(
                (div().w(px(124.)).flex().justify_end())
                    .text_size(px(font::CAPTION))
                    .child(reset),
            )
    }

    /// A switch or a pick of the choices the scope may make, or the value read-only.
    fn control(&self, row: &Row<'_>, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let id = row.behavior.id.clone();
        if row.offered.is_empty() {
            let layer = self.behaviors.scope.layer();
            return (div().flex().flex_col().items_end())
                .child(row.label().to_owned())
                .child(hint(read_only_note(layer), p).text_size(px(font::CAPTION)))
                .into_any_element();
        }
        if row.is_toggle() {
            let on = row.resolved.value == "on";
            return Switch::new(SharedString::from(format!("behavior-{id}")))
                .checked(on)
                .disabled(row.offered.len() < 2)
                .on_click(cx.listener(move |v, on: &bool, w, c| {
                    let choice = if *on { "on" } else { "off" };
                    v.choose(id.clone(), Some(choice.into()), w, c)
                }))
                .into_any_element();
        }
        let current = row.resolved.value.clone();
        let offered: Vec<(String, String)> = (row.offered.iter())
            .map(|c| (c.value.clone(), c.label.clone()))
            .collect();
        let weak = cx.weak_entity();
        button(SharedString::from(format!("behavior-{id}")))
            .small()
            .outline()
            .label(row.label().to_owned())
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in &offered {
                    let (weak, id, value) = (weak.clone(), id.clone(), value.clone());
                    let checked = *value == current;
                    menu = menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                        move |_, window, cx| {
                            if checked {
                                return;
                            }
                            let (id, value) = (id.clone(), Some(value.clone()));
                            let _ = weak.update(cx, |v, cx| v.choose(id, value, window, cx));
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// At the Workspace, a role's additions are its skill's own rules, edited on its page.
    fn workspace_additions_line(
        &self,
        skill: Skill,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let state = self.skill_state(skill)?;
        let line = format!(
            "Additions for {} · {}",
            role(skill).1,
            workspace_additions(state)
        );
        let view = match state.rewrite {
            Some(_) => SkillView::List,
            None => SkillView::Skill(skill),
        };
        Some(
            (div().py_2().flex().items_center().gap_3())
                .child(hint(line, p).flex_1())
                .child(
                    div()
                        .id(SharedString::from(format!("additions-{}", skill.folder())))
                        .text_size(px(font::SMALL))
                        .text_color(p.focus)
                        .cursor_pointer()
                        .on_click(cx.listener(move |v, _, w, c| {
                            v.behaviors.advanced |= view == SkillView::List;
                            v.open_skill(view, w, c)
                        }))
                        .child(match view {
                            SkillView::List => "Open Advanced",
                            _ => "Edit",
                        }),
                ),
        )
    }

    /// The Space's or repository's additions, with Save and Discard and the host's refusal.
    fn additions_editor(
        &self,
        title: String,
        reaches: String,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let draft = self.behaviors.additions.read(cx).value().to_string();
        let saved = (self.behaviors.saved(&self.behaviors.scope)).cloned();
        let saved_text = saved
            .as_ref()
            .map_or(String::new(), |s| s.additions.clone());
        let dirty = draft != saved_text;
        let refusal = (self.behaviors.refusal.as_ref())
            .filter(|(refused, _)| *refused == Refused::Additions)
            .map(|(_, error)| hint(error.clone(), p).text_color(p.red));
        let discard = cx.listener(move |v, _, w, c| {
            let text = saved_text.clone();
            (v.behaviors.additions).update(c, |input, c| input.set_value(text, w, c))
        });
        let save = cx.listener(move |v, _, w, c| {
            let Some(saved) = saved.clone() else {
                return;
            };
            let additions = v.behaviors.additions.read(c).value().to_string();
            let values = LayerValues { additions, ..saved };
            v.save_layer(values, Refused::Additions, w, c)
        });
        (div().pt_3().flex().flex_col().gap_2())
            .child(
                (div().flex().items_baseline().gap_2())
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(hint(format!("· {reaches}"), p)),
            )
            .child(Textarea::new(&self.behaviors.additions).w_full())
            .children(refusal)
            .child(
                (div().flex().items_center().gap_2())
                    .child(
                        hint(
                            "Free text, after the Behaviors. It can't widen an agent's authority.",
                            p,
                        )
                        .flex_1(),
                    )
                    .when(dirty, |d| {
                        d.child(
                            button("discard-additions")
                                .ghost()
                                .label("Discard")
                                .on_click(discard),
                        )
                    })
                    .child(
                        ui::gated(
                            button("save-additions")
                                .primary()
                                .label("Save")
                                .disabled(!dirty),
                            &self.shared.host,
                        )
                        .on_click(save),
                    ),
            )
    }

    /// The disclosure over today's skills list, where rewrites, history and merges live.
    fn advanced(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let open = self.behaviors.advanced;
        let waiting = a_rewrite_is_out_of_date(&self.shared);
        let head = div()
            .id("advanced")
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(p.edge)
            .text_size(px(font::SMALL))
            .text_color(p.subtle)
            .cursor_pointer()
            .on_click(cx.listener(|v, _, _, c| {
                v.behaviors.advanced = !v.behaviors.advanced;
                c.notify();
            }))
            .child(if open { "▾" } else { "▸" })
            .child(
                div()
                    .text_color(p.text)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Advanced"),
            )
            .child(div().flex_1().min_w_0().child(format!("· {ADVANCED}")))
            .when(waiting, |d| {
                d.child(pill("A rewrite is out of date", p.yellow))
            });
        (div().pt_3().flex().flex_col().gap_2())
            .child(head)
            .when(open, |d| d.child(self.skills_list(p, cx)))
    }
}

/// "Stock" dashed, the Workspace blue, a Space teal and a repository violet; bold when it is
/// the scope's own value.
fn source_chip(name: String, from: Layer, here: bool, p: Palette) -> Div {
    let chip = (div().flex_none().px_2().rounded_md())
        .text_size(px(font::CAPTION))
        .whitespace_nowrap()
        .when(here, |d| d.font_weight(FontWeight::SEMIBOLD))
        .child(name);
    let color = match from {
        Layer::Stock => {
            return chip
                .border_1()
                .border_dashed()
                .border_color(p.edge)
                .text_color(p.subtle);
        }
        // No Behavior comes from This Mac, a layer of the Who runs what table only.
        Layer::Workspace | Layer::ThisMac => p.blue,
        Layer::Space => p.focus,
        Layer::Repository => p.magenta,
    };
    chip.text_color(color).bg(color.opacity(0.13))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        Layers, Row, chain, chip, read_only_note, reset_label, rows, scope_of, shows_advanced,
        tightening_note, with_choice,
    };
    use workspace_core::{
        Command, SpaceId,
        cascade::{Behavior, Layer, LayerValues, Scope},
    };
    use workspace_host::Host;

    const SMS: &str = "Sage-Social/sagechat-sms";

    /// A host with the Space Sage listing sagechat-sms.
    fn sage(home: &tempfile::TempDir) -> (Host, SpaceId) {
        let mut host = Host::open(home.path()).unwrap();
        let mut space = host.create_space("Sage", "", "", None).unwrap().space;
        space.repositories.push(SMS.into());
        let id = space.id.clone();
        host.update_space(space).unwrap();
        (host, id)
    }
    fn catalog(host: &mut Host) -> Vec<Behavior> {
        serde_json::from_value(host.execute(Command::BehaviorCatalog).unwrap()).unwrap()
    }
    fn read(host: &mut Host, scope: Scope) -> LayerValues {
        serde_json::from_value(host.execute(Command::LayerValues { scope }).unwrap()).unwrap()
    }
    fn save(host: &mut Host, scope: Scope, values: LayerValues) -> Result<LayerValues, String> {
        let reply = host.execute(Command::SetLayerValues { scope, values });
        // As the host sends a refusal to the app.
        reply
            .map(|values| serde_json::from_value(values).unwrap())
            .map_err(|error| format!("{error:#}"))
    }
    /// What the page reads for `scope`: each layer through it, as the scope bar chose it.
    fn layers(host: &mut Host, scope: &Scope) -> Vec<(Layer, LayerValues)> {
        (chain(scope).into_iter())
            .map(|scope| (scope.layer(), read(host, scope)))
            .collect()
    }
    /// The page's row for `id` at `scope`, and what it shows.
    fn shown<T>(host: &mut Host, scope: &Scope, id: &str, then: impl FnOnce(&Row<'_>) -> T) -> T {
        let catalog = catalog(host);
        let layers = layers(host, scope);
        let chain: Vec<_> = layers.iter().map(|(l, v)| (*l, &v.behaviors)).collect();
        let rows = rows(&catalog, scope.layer(), &chain);
        then(rows.iter().find(|row| row.behavior.id == id).unwrap())
    }
    fn chip_of(host: &mut Host, scope: &Scope, id: &str) -> (String, Layer, Option<String>) {
        shown(host, scope, id, |row| {
            let (name, from) = chip(row, scope, "Sage");
            let reset = row.set_here.then(|| reset_label(row, scope, "Sage"));
            (name, from, reset)
        })
    }
    fn space(id: &SpaceId) -> Scope {
        scope_of(id, None)
    }
    fn sms(id: &SpaceId) -> Scope {
        scope_of(id, Some(SMS))
    }

    #[test]
    fn the_scope_bar_resolves_each_scope_through_the_layers_before_it() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = sage(&home);
        let values = |id: &str, value: &str| LayerValues {
            behaviors: [(id.to_owned(), value.to_owned())].into(),
            additions: String::new(),
        };
        save(&mut host, Scope::Workspace, values("wip_commits", "off")).unwrap();
        let mut space_values = values("optional_findings", "3");
        space_values.additions = "Never log child PII.".into();
        save(&mut host, space(&sage), space_values).unwrap();

        assert_eq!(chain(&Scope::Workspace), [Scope::Workspace]);
        assert_eq!(
            chain(&sms(&sage)),
            [Scope::Workspace, space(&sage), sms(&sage)]
        );
        let layers = layers(&mut host, &sms(&sage));
        assert_eq!(layers[1].1.additions, "Never log child PII.");
        assert_eq!(layers[2].1, LayerValues::default());

        // The Workspace's choice reaches every scope; the Space's only the Space and its
        // repositories.
        let at = |host: &mut Host, scope: &Scope, id: &str| {
            shown(host, scope, id, |row| {
                (row.label().to_owned(), row.resolved.from)
            })
        };
        let workspace = Scope::Workspace;
        assert_eq!(
            at(&mut host, &workspace, "wip_commits"),
            ("Off".into(), Layer::Workspace)
        );
        assert_eq!(
            at(&mut host, &workspace, "optional_findings"),
            ("5".into(), Layer::Stock)
        );
        assert_eq!(
            at(&mut host, &space(&sage), "optional_findings"),
            ("3".into(), Layer::Space)
        );
        assert_eq!(
            at(&mut host, &sms(&sage), "optional_findings"),
            ("3".into(), Layer::Space)
        );
        assert_eq!(
            at(&mut host, &sms(&sage), "wip_commits"),
            ("Off".into(), Layer::Workspace)
        );
    }

    #[test]
    fn a_value_set_at_a_space_shows_its_chip_and_reset_restores_what_it_inherits() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = sage(&home);
        let set = |host: &mut Host, scope: &Scope, id: &str, choice: Option<&str>| {
            let values = read(host, scope.clone());
            save(host, scope.clone(), with_choice(&values, id, choice)).unwrap()
        };
        set(
            &mut host,
            &Scope::Workspace,
            "status_table",
            Some("every_update"),
        );
        set(&mut host, &space(&sage), "status_table", Some("on_finish"));
        assert_eq!(
            chip_of(&mut host, &space(&sage), "status_table"),
            (
                "Set here".into(),
                Layer::Space,
                Some("Reset to Workspace".into())
            )
        );
        // Below the Space, the chip names it, and the repository has nothing to reset.
        set(&mut host, &space(&sage), "wip_commits", Some("off"));
        assert_eq!(
            chip_of(&mut host, &sms(&sage), "wip_commits"),
            ("Sage".into(), Layer::Space, None)
        );
        set(&mut host, &sms(&sage), "wip_commits", Some("on"));
        assert_eq!(
            chip_of(&mut host, &sms(&sage), "wip_commits"),
            (
                "Set here".into(),
                Layer::Repository,
                Some("Reset to Sage".into())
            )
        );

        // Reset saves the scope without its own value, keeping the rest.
        let reset = set(&mut host, &space(&sage), "status_table", None);
        assert_eq!(
            reset.behaviors.get("wip_commits").map(String::as_str),
            Some("off")
        );
        assert_eq!(
            shown(&mut host, &space(&sage), "status_table", |row| (
                row.label().to_owned(),
                row.resolved.from,
                row.set_here
            )),
            ("With every update".into(), Layer::Workspace, false)
        );
        set(&mut host, &Scope::Workspace, "status_table", None);
        assert_eq!(
            chip_of(&mut host, &space(&sage), "status_table"),
            ("Stock".into(), Layer::Stock, None)
        );
    }

    #[test]
    fn a_safety_behavior_at_a_space_offers_only_stricter_choices() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = sage(&home);
        let offered = |host: &mut Host, scope: &Scope| {
            shown(host, scope, "publishing", |row| {
                (row.offered.iter())
                    .map(|c| c.value.clone())
                    .collect::<Vec<_>>()
            })
        };
        // The Workspace is the human's own, so it is offered both.
        assert_eq!(
            offered(&mut host, &Scope::Workspace),
            ["allowed_owners", "always_ask"]
        );
        assert_eq!(
            offered(&mut host, &space(&sage)),
            ["allowed_owners", "always_ask"]
        );

        let ask = |values: LayerValues| with_choice(&values, "publishing", Some("always_ask"));
        let workspace_values = read(&mut host, Scope::Workspace);
        save(&mut host, Scope::Workspace, ask(workspace_values)).unwrap();
        assert_eq!(offered(&mut host, &space(&sage)), ["always_ask"]);
        assert_eq!(offered(&mut host, &sms(&sage)), ["always_ask"]);
        let catalog = catalog(&mut host);
        let layers = layers(&mut host, &space(&sage));
        let chain: Vec<_> = layers.iter().map(|(l, v)| (*l, &v.behaviors)).collect();
        let shown = rows(&catalog, Layer::Space, &chain);
        assert_eq!(
            tightening_note(&shown, Layer::Space).as_deref(),
            Some(
                "A Space or repository may only make Review depth and Publishing stricter. “As allowed owners say” is not offered here."
            )
        );
        assert_eq!(tightening_note(&shown, Layer::Workspace), None);

        // What the page leaves out, the host refuses, and the refusal is what the row shows.
        let looser = with_choice(
            &LayerValues::default(),
            "publishing",
            Some("allowed_owners"),
        );
        let refused = save(&mut host, space(&sage), looser).unwrap_err();
        assert!(
            refused.contains("may only make Publishing stricter than “Always ask first”"),
            "{refused}"
        );
    }

    #[test]
    fn a_behavior_a_layer_cannot_set_is_shown_read_only() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = sage(&home);
        let values = read(&mut host, space(&sage));
        let values = with_choice(&values, "status_table", Some("every_update"));
        save(&mut host, space(&sage), values).unwrap();
        let at_sms = shown(&mut host, &sms(&sage), "status_table", |row| {
            (
                row.offered.is_empty(),
                row.label().to_owned(),
                row.resolved.from,
            )
        });
        assert_eq!(at_sms, (true, "With every update".into(), Layer::Space));
        assert_eq!(
            read_only_note(Layer::Repository),
            "A repository can't set this"
        );
        let editable = shown(&mut host, &space(&sage), "status_table", |row| {
            row.offered.len()
        });
        assert_eq!(editable, 2);
    }

    #[test]
    fn going_back_from_a_repository_to_its_space_edits_the_spaces_own_additions() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, sage) = sage(&home);
        let additions = |text: &str| LayerValues {
            additions: text.into(),
            ..LayerValues::default()
        };
        save(&mut host, space(&sage), additions("Never log child PII.")).unwrap();
        save(
            &mut host,
            sms(&sage),
            additions("Verify Telnyx signatures."),
        )
        .unwrap();
        // The scope bar goes Sage → sagechat-sms → Sage, keeping the layers both read.
        let mut layers = Layers::default();
        for scope in chain(&sms(&sage)) {
            let values = read(&mut host, scope.clone());
            layers.put(scope, values);
        }
        assert_eq!(layers.additions(&sms(&sage)), "Verify Telnyx signatures.");
        layers.keep(&chain(&space(&sage)));
        assert_eq!(layers.additions(&space(&sage)), "Never log child PII.");
        assert_eq!(layers.additions(&sms(&sage)), "", "not read again yet");
    }

    #[test]
    fn a_second_choice_before_the_host_answers_the_first_keeps_both() {
        let home = tempfile::tempdir().unwrap();
        let (mut host, _) = sage(&home);
        let workspace = Scope::Workspace;
        let mut layers = Layers::default();
        layers.put(workspace.clone(), read(&mut host, workspace.clone()));
        // Both saves are sent before either reply, each from what the page shows.
        let mut sent = vec![];
        for id in ["mockup_before_ui_spec", "fetch_before_landing_order"] {
            let values = with_choice(layers.get(&workspace).unwrap(), id, Some("off"));
            layers.send(workspace.clone(), values.clone());
            sent.push(values);
        }
        let replies: Vec<_> = (sent.into_iter())
            .map(|values| save(&mut host, workspace.clone(), values).unwrap())
            .collect();
        assert_eq!([layers.answered(), layers.answered()], [false, true]);
        let off = |values: &LayerValues| values.behaviors.len();
        assert_eq!(off(&replies[1]), 2, "the last reply is the one kept");
        assert_eq!(read(&mut host, workspace).behaviors, replies[1].behaviors);
    }

    #[test]
    fn advanced_holds_the_rewrites_at_the_workspace_only() {
        let sage = SpaceId("sage".into());
        assert!(shows_advanced(&Scope::Workspace));
        assert!(!shows_advanced(&space(&sage)));
        assert!(!shows_advanced(&sms(&sage)));
    }
}
