//! New project (screen 3 of the `inherit-space-repositories` mockup): the project starts in a
//! Space, the one last used, and comes with that Space's repositories, shown as plain rows.
//! Changing the Space swaps them; other workspace repositories are added by hand and stay.
use super::*;
use gpui_component::{
    Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use spaces_view::{brain_label, space_color, space_mark};
use ui::hint;

/// What New project creates once the human confirms.
pub(super) struct NewProject {
    pub(super) name: String,
    pub(super) space_id: SpaceId,
    /// The repositories added to it, in workspace order.
    pub(super) repository_ids: Vec<String>,
    pub(super) first_message: Option<String>,
}

pub(super) struct NewProjectForm {
    bridge: Bridge,
    spaces: Vec<Space>,
    repositories: Vec<Repository>,
    /// The Space chosen and its repositories here, once the host has answered.
    defaults: Option<SpaceRepositories>,
    /// Repositories added by hand, which outlast a change of Space.
    added: BTreeSet<String>,
    /// "Add another repository…" lists the workspace's other repositories.
    others_shown: bool,
    name: Entity<InputState>,
    message: Entity<TextareaState>,
    pub(super) error: String,
}
impl NewProjectForm {
    pub(super) fn new(
        bridge: Bridge,
        snapshot: &Snapshot,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut form = Self {
            bridge,
            spaces: snapshot.spaces.clone(),
            repositories: snapshot.repositories.clone(),
            defaults: None,
            added: BTreeSet::new(),
            others_shown: false,
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(name)
                    .placeholder("Project name")
            }),
            message: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 8)
                    .placeholder("What should the coordinator do?")
            }),
            error: String::new(),
        };
        form.choose_space(None, window, cx);
        form
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Asks for `space`'s repositories here, or the default Space's, in place of the earlier
    /// Space's.
    fn choose_space(
        &mut self,
        space: Option<SpaceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask(
            ask::new_project_defaults(space),
            window,
            cx,
            |form, defaults, _, cx| {
                match defaults {
                    Ok(defaults) => {
                        form.defaults = Some(defaults);
                        form.error.clear();
                    }
                    Err(error) => form.error = error,
                }
                cx.notify();
            },
        );
    }

    /// The project to create, or why it cannot be created yet.
    pub(super) fn chosen(&self, cx: &App) -> Result<NewProject, String> {
        let defaults = self
            .defaults
            .as_ref()
            .ok_or("Wait for the Space’s repositories")?;
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("Give the project a name".into());
        }
        if name.len() > 128 {
            return Err("Use a name of at most 128 bytes".into());
        }
        let message = self.message.read(cx).value().trim().to_owned();
        first_message_fits(&message)?;
        Ok(NewProject {
            name,
            space_id: defaults.space_id.clone(),
            repository_ids: (self.repositories.iter())
                .filter(|r| self.added.contains(&r.id))
                .map(|r| r.id.clone())
                .collect(),
            first_message: (!message.is_empty()).then_some(message),
        })
    }

    fn space_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chosen = self.defaults.as_ref().map(|d| d.space_id.clone());
        let label = (self.spaces.iter())
            .find(|s| Some(&s.id) == chosen.as_ref())
            .map_or("Choose a Space".into(), |s| s.name.clone());
        let spaces: Vec<(SpaceId, String)> = (self.spaces.iter())
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let weak = cx.weak_entity();
        button("new-project-space")
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (id, name) in &spaces {
                    let (weak, id) = (weak.clone(), id.clone());
                    let checked = chosen.as_ref() == Some(&id);
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                        move |_, window, cx| {
                            let id = id.clone();
                            let _ =
                                weak.update(cx, |form, cx| form.choose_space(Some(id), window, cx));
                        },
                    ));
                }
                menu
            })
    }

    fn repositories_field(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(defaults) = &self.defaults else {
            return hint("Reading the Space’s repositories…", p);
        };
        let space = self.spaces.iter().find(|s| s.id == defaults.space_id);
        let mut list = div().flex().flex_col().gap_1();
        let label = |repository: &Repository| {
            (repository.github_identity()).unwrap_or_else(|| repository.name.clone())
        };
        for row in repository_rows(defaults, &self.repositories, &self.added, self.others_shown) {
            list = list.child(match row {
                RepositoryRow::FromSpace(repository) => {
                    div().child(label(repository)).into_any_element()
                }
                RepositoryRow::Other(repository) => {
                    let id = repository.id.clone();
                    let added = self.added.contains(&id);
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Checkbox::new(SharedString::from(format!("new-project-{id}")))
                                .label(label(repository))
                                .checked(added)
                                .on_click(cx.listener(move |form, checked: &bool, _, cx| {
                                    match checked {
                                        true => form.added.insert(id.clone()),
                                        false => form.added.remove(&id),
                                    };
                                    cx.notify();
                                })),
                        )
                        .when(added, |d| d.child(hint("added to this project", p)))
                        .into_any_element()
                }
                RepositoryRow::NotCloned(identity) => div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_color(p.subtle)
                            .child(identity.to_owned()),
                    )
                    .child(hint("not cloned here", p).text_color(p.yellow))
                    .into_any_element(),
            });
        }
        let note = space.map_or_else(String::new, |s| {
            format!("{}’s repositories, kept current.", s.name)
        });
        list.child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(hint(note, p))
                .when(!self.others_shown, |d| {
                    d.child(
                        button("new-project-other-repositories")
                            .ghost()
                            .xsmall()
                            .label("Add another repository…")
                            .on_click(cx.listener(|form, _, _, cx| {
                                form.others_shown = true;
                                cx.notify();
                            })),
                    )
                }),
        )
    }
}

/// Refuses a first message the host would refuse, before the project is created, so the
/// dialog keeps it.
fn first_message_fits(message: &str) -> Result<(), String> {
    match message.len() <= MAX_TEXT_BYTES {
        true => Ok(()),
        false => Err(format!(
            "The first message is {} bytes; at most {MAX_TEXT_BYTES} are allowed",
            message.len()
        )),
    }
}

/// A line of New project's repositories.
enum RepositoryRow<'a> {
    /// It comes with the Space, so there is nothing to tick.
    FromSpace(&'a Repository),
    NotCloned(&'a str),
    /// One the human may add.
    Other(&'a Repository),
}

/// New project's repositories: the Space's here, the ones it lists that are not cloned here,
/// then the workspace's others that are added, or all of them once `others` asks for them.
fn repository_rows<'a>(
    defaults: &'a SpaceRepositories,
    repositories: &'a [Repository],
    added: &BTreeSet<String>,
    others: bool,
) -> Vec<RepositoryRow<'a>> {
    let in_space = |r: &Repository| defaults.repository_ids.contains(&r.id);
    let mut rows: Vec<RepositoryRow> = (repositories.iter())
        .filter(|r| in_space(r))
        .map(RepositoryRow::FromSpace)
        .collect();
    rows.extend(
        defaults
            .not_cloned
            .iter()
            .map(|i| RepositoryRow::NotCloned(i)),
    );
    rows.extend(
        (repositories.iter())
            .filter(|r| !in_space(r) && (others || added.contains(&r.id)))
            .map(RepositoryRow::Other),
    );
    rows
}

impl Requests for NewProjectForm {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

impl Render for NewProjectForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let chosen = self.defaults.as_ref().map(|d| d.space_id.clone());
        let space = self.spaces.iter().find(|s| Some(&s.id) == chosen.as_ref());
        let logs = space.map(|space| format!("Logs to {}.", brain_label(space)));
        let mark = chosen
            .as_ref()
            .map(|id| space_mark(space_color(&self.spaces, id, p)));
        let field = |label: &'static str, control: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap_4()
                .child(
                    div()
                        .w(px(110.))
                        .flex_none()
                        .pt_1()
                        .text_size(px(font::SMALL))
                        .text_color(p.subtle)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(control))
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(field(
                "Space",
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .children(mark)
                            .child(self.space_picker(cx)),
                    )
                    .children(logs.map(|logs| hint(logs, p)))
                    .into_any_element(),
            ))
            .child(field(
                "Name",
                Input::new(&self.name).small().into_any_element(),
            ))
            .child(field(
                "Repositories",
                self.repositories_field(p, cx).into_any_element(),
            ))
            .child(field(
                "First message",
                Textarea::new(&self.message).w_full().into_any_element(),
            ))
            .when(!self.error.is_empty(), |d| {
                d.child(hint(self.error.clone(), p).text_color(p.red))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{RepositoryRow, first_message_fits, repository_rows};
    use std::collections::BTreeSet;
    use workspace_core::*;

    fn repository(id: &str, identity: &str) -> Repository {
        let (owner, name) = identity.split_once('/').unwrap();
        Repository {
            id: id.into(),
            name: name.into(),
            path: format!("/dev/{name}"),
            base: "origin/main".into(),
            github: Some(GitHubRepository {
                owner: owner.into(),
                name: name.into(),
                facts: None,
                problem: None,
            }),
            origin_problem: None,
            checks: None,
            protected_branches: vec![],
        }
    }
    fn shown(rows: &[RepositoryRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                RepositoryRow::FromSpace(r) => r.github_identity().unwrap(),
                RepositoryRow::NotCloned(identity) => format!("{identity} · not cloned here"),
                RepositoryRow::Other(r) => format!("{} · tickable", r.github_identity().unwrap()),
            })
            .collect()
    }

    #[test]
    fn the_spaces_repositories_come_first_with_the_ones_not_cloned_here_and_no_others() {
        let repositories = [
            repository("m", "Sage-Social/mobile-app"),
            repository("d", "MHohlios/dotfiles"),
            repository("w", "Sage-Social/watch-app"),
        ];
        let sage = SpaceRepositories {
            space_id: SpaceId("sage".into()),
            repository_ids: vec!["m".into(), "w".into()],
            not_cloned: vec!["Sage-Social/api".into()],
        };
        let added = BTreeSet::new();
        assert_eq!(
            shown(&repository_rows(&sage, &repositories, &added, false)),
            [
                "Sage-Social/mobile-app",
                "Sage-Social/watch-app",
                "Sage-Social/api · not cloned here"
            ],
            "no personal repository is listed"
        );
        let all = repository_rows(&sage, &repositories, &added, true);
        assert_eq!(shown(&all).last().unwrap(), "MHohlios/dotfiles · tickable");
        let by_hand = BTreeSet::from(["d".to_owned()]);
        assert_eq!(
            shown(&repository_rows(&sage, &repositories, &by_hand, false))
                .last()
                .unwrap(),
            "MHohlios/dotfiles · tickable",
            "a repository added by hand stays listed"
        );
        // Another Space swaps the Space's rows and keeps what was added.
        let personal = SpaceRepositories {
            space_id: SpaceId("personal".into()),
            repository_ids: vec!["w".into()],
            not_cloned: vec![],
        };
        assert_eq!(
            shown(&repository_rows(&personal, &repositories, &by_hand, false)),
            ["Sage-Social/watch-app", "MHohlios/dotfiles · tickable"]
        );
    }

    #[test]
    fn a_first_message_too_long_is_refused_before_the_project_is_created() {
        assert!(first_message_fits(&"a".repeat(MAX_TEXT_BYTES)).is_ok());
        let error = first_message_fits(&"a".repeat(MAX_TEXT_BYTES + 1)).unwrap_err();
        assert!(error.contains("at most 65536"), "{error}");
    }
}
