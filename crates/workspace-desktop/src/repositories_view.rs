//! The workspace's repositories. Projects use all of them unless they choose their own set.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Escape, Input, InputEvent},
    menu::{DropdownMenu, PopupMenuItem},
};
use ui::{banner, card, empty_state, eyebrow, hint, icon, mono, pill, section};
use wording::{age, counted, from_now};

const WITHOUT_ASKING: &str =
    "For repositories owned by these accounts. Everything else asks you first.";
const NEVER: &str = "Wiffletree never approves, merges, closes or comments on a pull request.";
const HELD: &str = "A trigger held here can be turned off only on a Mac that runs it.";
const UNTIL_GITHUB_ANSWERS: &str = "Owners are still listed, because they come from each repository’s remote on disk. Until GitHub has answered for a repository, it asks first, whatever is ticked. Wiffletree asks GitHub again the next time you come back.";
const KEEP: &str =
    "Only in repositories whose owner is allowed below · all off until you allow one.";
const WATCH: &str =
    "Always on. Tells the coordinator when it merges, closes, fails checks or conflicts.";
const REBASE_AFTER: &str = "After the base has been quiet for";
const REBASE_PUSH: &str = "minutes. Pushes only when the change is the same as the reviewed one.";
const SORT: &str = "Tells the coordinator which need a change or an answer.";
/// The name of the one trigger for tries at conflicts and failed checks.
const NO_CHECKS: &str = "none entered · rebases and comment sorting work; a conflict or a failed check goes to the coordinator";
const OWNER_PLACEHOLDER: &str = "An owner, or owner/name for one fork";
const BASE_HINT: &str = "Changes the base used for tickets and publishing. Existing branches and worktrees are not rebased.";

/// A repository's checks command being edited on its row, with the limit chosen so far.
pub(super) struct ChecksEdit {
    repository_id: String,
    minutes: u32,
}

/// A repository's base being edited under its row, with why the host refused the last save.
pub(super) struct BaseEdit {
    repository_id: String,
    error: Option<String>,
    /// The base a save sent and is waiting for; only the reply to it applies here.
    pending: Option<String>,
}

/// One of the PR keeper's triggers: its tick box beside its name and what it does.
fn trigger(mark: impl IntoElement, name: &'static str, details: Vec<AnyElement>) -> Div {
    div().flex().items_start().gap_3().child(mark).child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().font_weight(FontWeight::MEDIUM).child(name))
            .children(details),
    )
}

/// The Repositories page: the workspace's repositories, the owners allowed to publish without
/// asking, the PR keeper's triggers and each repository's checks command.
pub(super) struct RepositoriesPage {
    bridge: Bridge,
    shared: Shared,
    checks_command: Entity<InputState>,
    checks_edit: Option<ChecksEdit>,
    base_input: Entity<InputState>,
    base_edit: Option<BaseEdit>,
    /// The owner being added to those allowed to have PRs opened without asking.
    owner_input: Entity<InputState>,
    /// The saved review levels, which the keeper's review level picks from.
    verifiers: Entity<verifiers::VerifierEditor>,
    /// The upgrade's suggested Spaces with the human's picks, until confirmed; asked for again
    /// while the page shows, as GitHub names and the `gh` login arrive after start.
    suggestions: Vec<(RepositorySuggestion, Pick)>,
    suggestions_asking: bool,
    /// The workspace repositories known since the first snapshot; one that arrives later is
    /// being added, and is suggested a Space once its GitHub name is known.
    seen: Option<BTreeSet<String>>,
    arriving: BTreeSet<String>,
    adding: Vec<Adding>,
}

/// The Space chosen for a repository: one of them, Personal before it exists, or none.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pick {
    Space(SpaceId),
    NewPersonal,
    NoSpace,
}

/// A repository just added, with the Space its owner suggests and the one the human picked.
struct Adding {
    repository: String,
    suggested: Option<SpaceId>,
    pick: Option<SpaceId>,
}
impl EventEmitter<Intent> for RepositoriesPage {}
impl RepositoriesPage {
    pub(super) fn new(
        bridge: Bridge,
        verifiers: Entity<verifiers::VerifierEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&verifiers, |_, _, cx| cx.notify()).detach();
        let base_input = cx.new(|cx| InputState::new(window, cx).placeholder("origin/main"));
        cx.subscribe_in(&base_input, window, |v, _, event, w, c| {
            if let InputEvent::PressEnter { .. } = event {
                v.save_base(w, c)
            }
        })
        .detach();
        Self {
            verifiers,
            bridge,
            shared: Shared::default(),
            checks_command: cx.new(|cx| {
                InputState::new(window, cx).placeholder("The command that runs its checks")
            }),
            checks_edit: None,
            base_input,
            base_edit: None,
            owner_input: cx.new(|cx| InputState::new(window, cx).placeholder(OWNER_PLACEHOLDER)),
            suggestions: vec![],
            suggestions_asking: false,
            seen: None,
            arriving: BTreeSet::new(),
            adding: vec![],
        }
    }
    /// Redraws while shown, as the repositories it lists change; asks for the upgrade's
    /// suggestions at the first open, and a Space for each repository as it is added.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let opened = shared.page == Page::Repositories && self.shared.page != Page::Repositories;
        // Another host's repositories are not being added, and its suggestions are its own.
        if shared.host.name() != self.shared.host.name() {
            self.seen = None;
            self.arriving.clear();
            self.adding.clear();
            self.suggestions.clear();
        }
        self.shared = shared;
        if moved {
            self.notice_added(window, cx);
        }
        let shown = self.shared.page == Page::Repositories;
        if (opened || (moved && shown)) && !self.suggestions_asking {
            self.suggestions_asking = true;
            self.ask(
                ask::repository_suggestions(),
                window,
                cx,
                |view, suggestions, window, cx| {
                    view.suggestions_asking = false;
                    match suggestions {
                        Ok(suggestions) => {
                            view.suggestions = with_picks(&view.suggestions, suggestions)
                        }
                        Err(error) => toast_error(error, window, cx),
                    }
                },
            );
        }
        if opened || (moved && self.shared.page == Page::Repositories) {
            cx.notify();
        }
    }

    /// Finds the repositories added since the last snapshot and asks which Space their owner
    /// suggests, once each has a GitHub name.
    fn notice_added(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.shared.snapshot() else {
            return;
        };
        let ids = snapshot.repositories.iter().map(|r| r.id.clone());
        let Some(seen) = &mut self.seen else {
            self.seen = Some(ids.collect());
            return;
        };
        for id in ids {
            if seen.insert(id.clone()) {
                self.arriving.insert(id);
            }
        }
        // Its GitHub name arrives with a later read; until then it waits.
        let named: Vec<(String, String)> = (snapshot.repositories.iter())
            .filter(|r| self.arriving.contains(&r.id))
            .filter_map(|r| Some((r.id.clone(), r.github_identity()?)))
            .collect();
        for (id, _) in &named {
            self.arriving.remove(id);
        }
        let unlisted = named
            .into_iter()
            .map(|(_, identity)| identity)
            .filter(|identity| spaces_of(&snapshot.spaces, identity).next().is_none())
            .collect::<Vec<_>>();
        for identity in unlisted {
            let asked = identity.clone();
            self.ask(
                ask::suggest_repository_space(identity),
                window,
                cx,
                move |view, suggested, _, _| {
                    let suggested = suggested.ok().flatten();
                    view.adding.push(Adding {
                        repository: asked,
                        pick: suggested.clone(),
                        suggested,
                    });
                },
            );
        }
    }

    /// Puts `repository` in exactly `space_ids`, editing each Space's repositories.
    fn set_spaces(
        &mut self,
        repository: String,
        space_ids: Vec<SpaceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = Command::SetRepositorySpaces {
            repository,
            space_ids,
        };
        self.request(command, window, cx);
    }

    /// One chip per Space the repository is in, or "Space" for none; each opens the Spaces to
    /// add or remove. `at` keeps a repository drawn in several sections apart.
    fn space_chips(
        &self,
        at: usize,
        repository: &Repository,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let chips = div().flex().flex_wrap().items_center().gap_1();
        let Some(identity) = repository.github_identity() else {
            return chips.child(hint("Not on GitHub, so in no Space", p));
        };
        let member: Vec<SpaceId> = spaces_of(&snapshot.spaces, &identity)
            .map(|s| s.id.clone())
            .collect();
        let spaces: Vec<(SpaceId, String)> = (snapshot.spaces.iter())
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let weak = cx.weak_entity();
        let chip = |id: String, label: String, mark: Option<Hsla>| {
            let (member, spaces, weak, identity) = (
                member.clone(),
                spaces.clone(),
                weak.clone(),
                identity.clone(),
            );
            ui::gated(
                button(SharedString::from(id))
                    .xsmall()
                    .outline()
                    .when(mark.is_none(), |b| b.icon(icon("plus")))
                    .label(label)
                    .when_some(mark, |b, color| b.child(spaces_view::space_mark(color)))
                    .dropdown_caret(true),
                &self.shared.host,
            )
            .dropdown_menu(move |mut menu, _, _| {
                for (space, name) in &spaces {
                    let checked = member.contains(space);
                    let chosen = toggled(&member, space);
                    let (weak, identity) = (weak.clone(), identity.clone());
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                        move |_, window, cx| {
                            let (identity, chosen) = (identity.clone(), chosen.clone());
                            let _ = weak.update(cx, |view, cx| {
                                view.set_spaces(identity, chosen, window, cx)
                            });
                        },
                    ));
                }
                menu
            })
        };
        if member.is_empty() {
            let id = format!("space-chip-{at}-{}-none", repository.id);
            return chips.child(chip(id, "Space".into(), None));
        }
        let mut chips = chips;
        for space in spaces_of(&snapshot.spaces, &identity) {
            let id = format!("space-chip-{at}-{}-{}", repository.id, space.id);
            let color = spaces_view::space_color(&snapshot.spaces, &space.id, p);
            chips = chips.child(chip(id, space.name.clone(), Some(color)));
        }
        chips
    }

    /// The upgrade's suggestions, each with a picker, and Confirm, which writes them all.
    fn suggestions_card(
        &self,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if self.suggestions.is_empty() {
            return None;
        }
        let mut card = card(p)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Suggested Spaces"),
            )
            .child(hint(SUGGESTED, p));
        for (at, (suggestion, pick)) in self.suggestions.iter().enumerate() {
            let label = pick_label(pick, &snapshot.spaces);
            let mut options: Vec<(Pick, String)> = (snapshot.spaces.iter())
                .map(|s| (Pick::Space(s.id.clone()), s.name.clone()))
                .collect();
            if suggestion.space == Some(SuggestedSpace::NewPersonal) {
                options.push((Pick::NewPersonal, pick_label(&Pick::NewPersonal, &[])));
            }
            options.push((Pick::NoSpace, "No Space".into()));
            let (weak, current) = (cx.weak_entity(), pick.clone());
            let picker = button(SharedString::from(format!("suggested-{at}")))
                .xsmall()
                .outline()
                .label(label)
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    for (pick, name) in &options {
                        let (weak, pick) = (weak.clone(), pick.clone());
                        let checked = pick == current;
                        menu =
                            menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                                move |_, _, cx| {
                                    let pick = pick.clone();
                                    let _ = weak.update(cx, |view, cx| {
                                        view.suggestions[at].1 = pick;
                                        cx.notify();
                                    });
                                },
                            ));
                    }
                    menu
                });
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(suggestion.repository.clone()),
                    )
                    .child(picker),
            );
        }
        Some(
            card.child(
                div().flex().justify_end().child(
                    ui::gated(
                        button("confirm-suggestions").primary().label("Confirm"),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| {
                        let spaces = view.shared.snapshot().map_or(&[][..], |s| &s.spaces);
                        let choices = confirmed(&view.suggestions, spaces);
                        view.request(
                            Command::ConfirmRepositorySuggestions { choices },
                            window,
                            cx,
                        )
                    })),
                ),
            ),
        )
    }

    /// A repository just added, with its owner's suggested Space to confirm, change or refuse.
    fn adding_card(
        &self,
        adding: &Adding,
        snapshot: &Snapshot,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let repository = adding.repository.clone();
        let owner = repository_owner(&repository).to_owned();
        let picked =
            (adding.pick.as_ref()).and_then(|id| snapshot.spaces.iter().find(|s| &s.id == id));
        let why = suggestion_reason(&snapshot.spaces, adding.suggested.as_ref(), &owner);
        let spaces: Vec<(SpaceId, String)> = (snapshot.spaces.iter())
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let (weak, key) = (cx.weak_entity(), repository.clone());
        let picker = button(SharedString::from(format!("adding-{repository}")))
            .xsmall()
            .outline()
            .label(picked.map_or("No Space".into(), |s| s.name.clone()))
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (id, name) in &spaces {
                    let (weak, id, key) = (weak.clone(), id.clone(), key.clone());
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        let (id, key) = (id.clone(), key.clone());
                        let _ = weak.update(cx, |view, cx| {
                            if let Some(adding) =
                                view.adding.iter_mut().find(|a| a.repository == key)
                            {
                                adding.pick = Some(id);
                            }
                            cx.notify();
                        });
                    }));
                }
                menu
            });
        let dismissed = repository.clone();
        let added = (repository.clone(), adding.pick.clone());
        card(p)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Adding {repository}")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(hint("Suggested Space:", p))
                    .child(picker)
                    .child(hint(why, p)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button(SharedString::from(format!("adding-none-{repository}")))
                            .ghost()
                            .label("No Space")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.adding.retain(|a| a.repository != dismissed);
                                cx.notify();
                            })),
                    )
                    .children(picked.map(|space| {
                        let (repository, pick) = added.clone();
                        ui::gated(
                            button(SharedString::from(format!("adding-to-{repository}")))
                                .primary()
                                .label(format!("Add to {}", space.name)),
                            &self.shared.host,
                        )
                        .on_click(cx.listener(
                            move |view, _, window, cx| {
                                view.adding.retain(|a| a.repository != repository);
                                let spaces = pick.clone().into_iter().collect();
                                view.set_spaces(repository.clone(), spaces, window, cx);
                            },
                        ))
                    })),
            )
    }

    fn body(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(index) = &self.shared.index else {
            return div();
        };
        let snapshot = index.snapshot();
        // Finished tickets and archived agents still point at their repository, so they keep it.
        let in_use: std::collections::HashSet<&str> = (index.sessions())
            .filter_map(|s| s.repository_id.as_deref())
            .chain(index.tickets().map(|t| t.repository_id.as_str()))
            .collect();
        let mut body = div().flex().flex_col().gap_3();
        if !snapshot.repositories.is_empty() {
            body = body
                .child(ui::locked(
                    self.keeper_block(&snapshot.keeper, &snapshot.keeper_held, p, cx),
                    &self.shared.host,
                    p,
                ))
                .child(self.owners_block(snapshot, p, cx));
        }
        body = body.child(
            section(
                format!("IN THE WORKSPACE · {}", snapshot.repositories.len()),
                p,
            )
            .child(
                button("leftover-worktrees")
                    .ghost()
                    .label("Leftover worktrees…")
                    .on_click(cx.listener(|v, _, w, c| v.open_leftovers(w, c))),
            )
            .child(
                ui::gated(
                    button("add-workspace-repositories")
                        .primary()
                        .icon(icon("plus"))
                        .label("Add repositories…"),
                    &self.shared.host,
                )
                .on_click(cx.listener(|_, _, _, c| {
                    c.emit(Intent::Create {
                        kind: Creation::Import,
                        ticket: None,
                    })
                })),
            ),
        );
        if snapshot.repositories.is_empty() {
            body = body.child(empty_state(
                "folder",
                "No repositories yet",
                "Add a repository, or parent folders such as ~/dev/* and ~/dev/company/*.",
                p,
            ));
            return body.child(self.repository_roots(snapshot, p, cx));
        }
        body = body.child(hint(
            "Every project uses all of these unless it chooses its own set in its Overview.",
            p,
        ));
        if let Some(card) = self.suggestions_card(snapshot, p, cx) {
            body = body.child(card);
        }
        for adding in &self.adding {
            body = body.child(self.adding_card(adding, snapshot, p, cx));
        }
        let mut shown = std::collections::HashSet::new();
        for (at, (space, repositories)) in grouped(&snapshot.spaces, &snapshot.repositories)
            .into_iter()
            .enumerate()
        {
            let label = match space {
                Some(space) => div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(spaces_view::space_mark(spaces_view::space_color(
                        &snapshot.spaces,
                        &space.id,
                        p,
                    )))
                    .child(eyebrow(space.name.to_uppercase(), p)),
                None => eyebrow("NO SPACE", p),
            };
            let mut list = div().flex().flex_col().child(div().pt_2().child(label));
            for repository in repositories {
                let chips = self.space_chips(at, repository, snapshot, p, cx);
                // A repository in several Spaces is drawn in full under the first.
                list = list.child(match shown.insert(repository.id.as_str()) {
                    true => {
                        let in_use = in_use.contains(repository.id.as_str());
                        self.full_row(snapshot, repository, in_use, chips, p, cx)
                    }
                    false => compact_row(snapshot, repository, space, chips, p),
                });
            }
            body = body.child(list);
        }
        body.child(self.repository_roots(snapshot, p, cx))
    }

    /// A repository's row: its name, path, counts, rules, base and checks, with `chips` for its
    /// Spaces.
    fn full_row(
        &self,
        snapshot: &Snapshot,
        repository: &Repository,
        in_use: bool,
        chips: Div,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let projects = snapshot
            .projects
            .iter()
            .filter(|project| project.uses(&repository.id))
            .count();
        let id = repository.id.clone();
        let missing = snapshot.missing_repositories.contains(&repository.id);
        let kept = kept_label(repository, &snapshot.allowed_owners, &snapshot.keeper);
        let checks = self.checks_field(repository, kept == Some("kept"), p, cx);
        let base_editor = self.base_editor(repository, p, cx);
        let edit_id = id.clone();
        let row = div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("folder").text_color(p.subtle))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_ellipsis()
                                    .child(repository.name.clone()),
                            )
                            .child(mono(repository.path.clone(), p)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .child(hint(repository_counts(snapshot, repository, projects), p))
                            .children(pr_check_status(snapshot, repository).map(
                                |(status, failed)| {
                                    div()
                                        .text_size(px(font::META))
                                        .text_color(if failed { p.red } else { p.subtle })
                                        .child(status)
                                },
                            )),
                    )
                    .when(missing, |d| d.child(pill("Missing", p.red)))
                    .child(pill(
                        rule_label(repository, &snapshot.allowed_owners),
                        if is_allowed(repository, &snapshot.allowed_owners) {
                            p.green
                        } else {
                            p.subtle
                        },
                    ))
                    .children(kept.map(|label| {
                        pill(label, if label == "kept" { p.focus } else { p.subtle })
                    }))
                    .child(pill(repository.base.clone(), p.subtle))
                    .child(
                        button(SharedString::from(format!("edit-base-{id}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("pencil"))
                            .label("Edit")
                            .tooltip("Change the base")
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.begin_base_edit(&edit_id, w, c)
                            })),
                    )
                    .child(
                        button(SharedString::from(format!("remove-repository-{id}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip(if in_use {
                                "Tickets work here, so it stays in the workspace"
                            } else {
                                "Remove from the workspace; root folders won't add it back. Files on disk are untouched."
                            })
                            .disabled(in_use)
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.request(
                                    Command::RemoveRepository {
                                        repository_id: id.clone(),
                                    },
                                    w,
                                    c,
                                )
                            })),
                    );
        // The checks line takes the row's whole width, in line with the name.
        div()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::HAIRLINE))
            .child(row)
            .children(base_editor)
            .child(checks.pl(px(28.)))
            .child(chips.pl(px(28.)).pt_1())
    }

    /// The owners the human allows to have pull requests opened without asking: one chip per
    /// organization or user, and per single fork, with a field to add another.
    fn owners_block(&self, snapshot: &Snapshot, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut chips = div().flex().flex_wrap().items_center().gap_2();
        for entry in &snapshot.allowed_owners {
            chips = chips.child(self.owner_chip(entry, p, cx));
        }
        let add = div()
            .flex()
            .items_center()
            .gap_2()
            .child(Input::new(&self.owner_input).small().flex_1().min_w_0())
            .child(
                ui::gated(
                    button("add-owner").small().icon(icon("plus")).label("Add"),
                    &self.shared.host,
                )
                .on_click(cx.listener(|v, _, w, c| v.add_owner(w, c))),
            );
        let mut card = div()
            .mb_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("OPENING PRS WITHOUT ASKING", p).child(pill("This Mac only", p.subtle)))
            .child(hint(WITHOUT_ASKING, p));
        if let Some(error) = &snapshot.github_error {
            card = card.child(
                banner(p.yellow).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(format!("Couldn’t check GitHub: {error}"))
                        .child(hint(UNTIL_GITHUB_ANSWERS, p)),
                ),
            );
        }
        card.child(chips).child(add)
    }

    /// One allowed owner, with a button that stops allowing it.
    fn owner_chip(&self, entry: &str, p: Palette, cx: &mut Context<Self>) -> Div {
        let id = entry.to_owned();
        div()
            .flex()
            .items_center()
            .gap_1()
            .pl_2()
            .rounded_md()
            .border_1()
            .border_color(p.edge)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(entry.to_owned()),
            )
            .child(
                button(SharedString::from(format!("remove-owner-{entry}")))
                    .ghost()
                    .xsmall()
                    .icon(icon("close"))
                    .tooltip("Stop opening PRs without asking here")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(cx.listener(move |v, _, w, c| {
                        let command = Command::SetOwnerAllowed {
                            entry: id.clone(),
                            allowed: false,
                        };
                        v.request(command, w, c)
                    })),
            )
    }

    /// Allows the owner as typed; the host refuses a name GitHub could not have.
    fn add_owner(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self.owner_input.read(cx).value().trim().to_owned();
        if entry.is_empty() {
            return;
        }
        let command = Command::SetOwnerAllowed {
            entry,
            allowed: true,
        };
        self.request(command, window, cx);
    }

    /// The PR keeper's triggers, for the whole workspace, in one card: the watch that is always
    /// on, the rebase trigger and its quiet period, the one trigger for tries at conflicts and
    /// failed checks and its review level, and comment sorting, with a line for each trigger on
    /// from another machine that this one cannot run.
    fn keeper_block(
        &self,
        keeper: &KeeperSettings,
        held: &[String],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let refused = self.shared.host.changes_refused().is_some();
        let save = |keeper: KeeperSettings| {
            cx.listener(move |v, _, w, c| {
                v.request(
                    Command::SetKeeper {
                        keeper: keeper.clone(),
                    },
                    w,
                    c,
                )
            })
        };
        let quiet = |minutes: u32| KeeperSettings {
            quiet_minutes: minutes,
            ..keeper.clone()
        };
        let minutes = keeper.quiet_minutes;
        let step = |id: &str, glyph: &str, minutes: u32| {
            button(SharedString::from(format!("keeper-quiet-{id}")))
                .ghost()
                .xsmall()
                .label(glyph.to_owned())
                .disabled(!KEEPER_QUIET_MINUTES.contains(&minutes))
        };
        let current = keeper.clone();
        let rebase = cx.listener(move |v, checked: &bool, w, c| {
            let keeper = KeeperSettings {
                rebase: *checked,
                ..current.clone()
            };
            v.request(Command::SetKeeper { keeper }, w, c)
        });
        let current = keeper.clone();
        let comments = cx.listener(move |v, checked: &bool, w, c| {
            let keeper = KeeperSettings {
                comments: *checked,
                ..current.clone()
            };
            v.request(Command::SetKeeper { keeper }, w, c)
        });
        let current = keeper.clone();
        let fixes = cx.listener(move |v, checked: &bool, w, c| {
            let keeper = with_fixes(&current, *checked);
            v.request(Command::SetKeeper { keeper }, w, c)
        });
        let inline = || {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .text_size(px(font::SMALL))
                .text_color(p.subtle)
        };
        let period = inline()
            .child(REBASE_AFTER)
            .child(step("less", "−", minutes - 1).on_click(save(quiet(minutes - 1))))
            .child(
                div()
                    .text_color(p.text)
                    .font_weight(FontWeight::MEDIUM)
                    .child(minutes.to_string()),
            )
            .child(step("more", "+", minutes + 1).on_click(save(quiet(minutes + 1))))
            .child(REBASE_PUSH);
        let level = inline()
            .child("Then runs the")
            .child(self.review_level_picker(keeper, cx))
            .child("review before pushing.");
        let mut fix_details = vec![level.into_any_element()];
        if let Some(partly) = partly_on_hint(keeper) {
            fix_details.push(hint(partly, p).text_color(p.yellow).into_any_element());
        }
        div()
            .mb_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(section("WHEN A TICKET’S PR IS OPEN", p))
            .child(hint(KEEP, p))
            .children(
                held_lines(held)
                    .into_iter()
                    .map(|line| hint(line, p).text_color(p.yellow)),
            )
            .when(!held.is_empty(), |d| d.child(hint(HELD, p)))
            .child(trigger(
                // A plain tick: a disabled checkbox fades its tick into its fill, unreadable in
                // both themes, and an enabled one would look clickable.
                icon("check").text_color(p.accent),
                "Watch it",
                vec![hint(WATCH, p).into_any_element()],
            ))
            .child(trigger(
                Checkbox::new("keeper-rebase")
                    .disabled(refused)
                    .checked(keeper.rebase)
                    .on_click(rebase),
                KEEPER_REBASE,
                vec![period.into_any_element()],
            ))
            .child(trigger(
                Checkbox::new("keeper-fixes")
                    .disabled(refused)
                    .checked(fixes_are_on(keeper))
                    .on_click(fixes),
                KEEPER_TRIES,
                fix_details,
            ))
            .child(trigger(
                Checkbox::new("keeper-comments")
                    .disabled(refused)
                    .checked(keeper.comments)
                    .on_click(comments),
                "Sort new comments",
                vec![hint(SORT, p).into_any_element()],
            ))
            .child(div().text_size(px(font::SMALL)).child(NEVER))
    }

    /// The repository's checks command and its limit, or its editor while the human edits it.
    fn checks_field(
        &self,
        repository: &Repository,
        kept: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let row = div()
            .mt_1()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(font::DETAIL))
            .text_color(p.subtle)
            .child("Checks");
        let id = repository.id.clone();
        let Some(edit) = self
            .checks_edit
            .as_ref()
            .filter(|edit| edit.repository_id == repository.id)
        else {
            let (command, note) = checks_summary(repository.checks.as_ref(), kept);
            // A long command wraps, with its limit under it.
            let command = command.map(|command| {
                div()
                    .font_family(".SystemMonoFont")
                    .text_size(px(font::CAPTION))
                    .text_color(p.text)
                    .child(command)
            });
            return row
                .items_start()
                .child(div().flex_1().min_w_0().children(command).child(note))
                .child(
                    button(SharedString::from(format!("edit-checks-{id}")))
                        .ghost()
                        .xsmall()
                        .label("Edit")
                        .on_click(cx.listener(move |v, _, w, c| v.begin_checks_edit(&id, w, c))),
                );
        };
        let minutes = edit.minutes;
        let step = |glyph: &'static str, minutes: u32| {
            button(SharedString::from(format!("checks-minutes-{glyph}")))
                .ghost()
                .xsmall()
                .label(glyph)
                .disabled(!CHECKS_MINUTES.contains(&minutes))
                .on_click(cx.listener(move |v, _, _, c| {
                    if let Some(edit) = &mut v.checks_edit {
                        edit.minutes = minutes;
                        c.notify();
                    }
                }))
        };
        let remove = repository.checks.is_some().then(|| {
            let id = id.clone();
            ui::gated(
                button("remove-checks").ghost().xsmall().label("Remove"),
                &self.shared.host,
            )
            .on_click(cx.listener(move |v, _, w, c| {
                let command = Command::SetRepositoryChecks {
                    repository_id: id.clone(),
                    checks: None,
                };
                v.request(command, w, c)
            }))
        });
        row.child(Input::new(&self.checks_command).xsmall().flex_1().min_w_0())
            .child("· stop after")
            .child(step("−", minutes - 1))
            .child(div().text_color(p.text).child(minutes.to_string()))
            .child(step("+", minutes + 1))
            .child("min")
            .child(
                ui::gated(
                    button("save-checks").primary().xsmall().label("Save"),
                    &self.shared.host,
                )
                .on_click(cx.listener(|v, _, w, c| v.save_checks(w, c))),
            )
            .children(remove)
            .child(
                button("cancel-checks")
                    .ghost()
                    .xsmall()
                    .label("Cancel")
                    .on_click(cx.listener(|v, _, _, c| {
                        v.checks_edit = None;
                        c.notify();
                    })),
            )
    }

    fn begin_checks_edit(
        &mut self,
        repository_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let checks = self
            .shared
            .snapshot()
            .into_iter()
            .flat_map(|s| &s.repositories)
            .find(|r| r.id == repository_id)
            .and_then(|r| r.checks.clone());
        self.base_edit = None;
        let command = checks
            .as_ref()
            .map_or("", |c| c.command.as_str())
            .to_owned();
        self.checks_command
            .update(cx, |input, cx| input.set_value(command, window, cx));
        self.checks_edit = Some(ChecksEdit {
            repository_id: repository_id.to_owned(),
            minutes: checks.map_or(RepositoryChecks::DEFAULT_MINUTES, |c| c.minutes),
        });
        cx.notify();
    }

    /// Saves the command as typed; the host refuses an empty one or a limit out of range.
    fn save_checks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.checks_edit else {
            return;
        };
        let checks = RepositoryChecks {
            command: self.checks_command.read(cx).value().trim().to_owned(),
            minutes: edit.minutes,
        };
        let command = Command::SetRepositoryChecks {
            repository_id: edit.repository_id.clone(),
            checks: Some(checks),
        };
        self.request(command, window, cx);
    }

    fn base_editor(
        &self,
        repository: &Repository,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let edit = (self.base_edit.as_ref()).filter(|e| e.repository_id == repository.id)?;
        let field = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Input::new(&self.base_input)
                    .xsmall()
                    .flex_1()
                    .min_w_0()
                    .disabled(edit.pending.is_some()),
            )
            .child(
                button("cancel-base")
                    .ghost()
                    .xsmall()
                    .label("Cancel")
                    .on_click(cx.listener(|v, _, _, c| v.cancel_base_edit(c))),
            )
            .child(
                ui::gated(
                    button("save-base")
                        .primary()
                        .xsmall()
                        .label("Save")
                        .disabled(edit.pending.is_some()),
                    &self.shared.host,
                )
                .on_click(cx.listener(|v, _, w, c| v.save_base(w, c))),
            );
        Some(
            card(p)
                .mt_2()
                .ml(px(28.))
                .w_auto()
                .gap_1()
                .on_action(cx.listener(|v, _: &Escape, _, c| {
                    v.cancel_base_edit(c);
                    c.stop_propagation();
                }))
                .child(hint(
                    format!("{} · {}", repository.name, repository.path),
                    p,
                ))
                .child(
                    div()
                        .text_size(px(font::DETAIL))
                        .text_color(p.subtle)
                        .child("Base"),
                )
                .child(field)
                .children(
                    edit.error
                        .clone()
                        .map(|error| hint(error, p).text_color(p.red)),
                )
                .child(hint(BASE_HINT, p))
                .child(
                    div()
                        .text_size(px(font::CAPTION))
                        .text_color(p.subtle)
                        .child("Enter saves · Esc cancels"),
                ),
        )
    }

    /// Opens the base editor under one row, closing any other editor, with the saved base.
    fn begin_base_edit(
        &mut self,
        repository_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(base) = (self.shared.snapshot().into_iter())
            .flat_map(|s| &s.repositories)
            .find(|r| r.id == repository_id)
            .map(|r| r.base.clone())
        else {
            return;
        };
        self.base_input
            .update(cx, |input, cx| input.set_value(base, window, cx));
        self.checks_edit = None;
        self.base_edit = Some(BaseEdit {
            repository_id: repository_id.to_owned(),
            error: None,
            pending: None,
        });
        // Focus once the input is in the next frame's dispatch tree.
        cx.on_next_frame(window, |v, window, cx| {
            if v.base_edit.is_some() {
                v.base_input.update(cx, |input, cx| input.focus(window, cx));
            }
        });
        cx.notify();
    }

    fn cancel_base_edit(&mut self, cx: &mut Context<Self>) {
        self.base_edit = None;
        cx.notify();
    }

    /// Saves the base as typed; the host refuses one that doesn't resolve to a commit.
    fn save_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Enter reaches here too, so it checks what gates Save.
        if self.shared.host.changes_refused().is_some() {
            return;
        }
        let base = self.base_input.read(cx).value().trim().to_owned();
        let Some(edit) = self.base_edit.as_mut().filter(|e| e.pending.is_none()) else {
            return;
        };
        edit.pending = Some(base.clone());
        let command = Command::SetRepositoryBase {
            repository_id: edit.repository_id.clone(),
            base,
        };
        self.request(command, window, cx);
        cx.notify();
    }

    /// The re-check levels the keeper's reviews may run, from the saved verification setting;
    /// the host refuses a choice the conflicts trigger cannot use.
    fn review_level_picker(
        &self,
        keeper: &KeeperSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let rechecks: Vec<String> = self
            .verifiers
            .read(cx)
            .verification()
            .into_iter()
            .flat_map(|v| &v.levels)
            .filter(|level| level.kind == LevelKind::Recheck)
            .map(|level| level.name.clone())
            .collect();
        let label = keeper
            .review_level
            .clone()
            .unwrap_or_else(|| "No level chosen".into());
        let current = keeper.clone();
        let weak = cx.weak_entity();
        button("keeper-review-level")
            .xsmall()
            .label(label)
            .dropdown_caret(true)
            .disabled(rechecks.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                for name in &rechecks {
                    let keeper = KeeperSettings {
                        review_level: Some(name.clone()),
                        ..current.clone()
                    };
                    let checked = current.review_level.as_ref() == Some(name);
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                        move |_, w, c| {
                            let keeper = keeper.clone();
                            let _ = weak
                                .update(c, |v, c| v.request(Command::SetKeeper { keeper }, w, c));
                        },
                    ));
                }
                menu
            })
    }

    /// Lists leftover worktrees in a sheet; removal happens there only after confirmation.
    fn open_leftovers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        let sheet = cx.new(|cx| leftovers::LeftoverSheet::new(bridge, cx));
        window.open_alert_dialog(cx, move |dialog, window, _| {
            dialog
                .title("Leftover worktrees")
                .width(px(640.).min(window.viewport_size().width - px(48.)))
                .max_h(window.viewport_size().height - px(100.))
                .ok_text("Close")
                .child(sheet.clone())
        });
    }

    fn repository_roots(&self, snapshot: &Snapshot, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut roots = div().mt_4().flex().flex_col().gap_2().child(section(
            format!("ROOT FOLDERS · {}", snapshot.repository_roots.len()),
            p,
        ));
        if snapshot.repository_roots.is_empty() {
            return roots.child(hint(
                "Parent folders you add are remembered here and checked for new repositories.",
                p,
            ));
        }
        roots = roots.child(hint(
            "Checked for new repositories when you come back to Wiffletree. Linked worktrees and repositories you removed are skipped.",
            p,
        ));
        for path in &snapshot.repository_roots {
            let remove = path.clone();
            roots = roots.child(
                div()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("folder").text_color(p.subtle))
                    .child(div().flex_1().min_w_0().child(mono(path.clone(), p)))
                    .child(
                        button(SharedString::from(format!("remove-root-{path}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip("Stop checking this folder. Its repositories stay.")
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.request(
                                    Command::RemoveRepositoryRoot {
                                        path: remove.clone(),
                                    },
                                    w,
                                    c,
                                )
                            })),
                    ),
            );
        }
        roots
    }
}

const SUGGESTED: &str = "After the update, Wiffletree suggests a Space for each repository: the Space whose projects use its owner’s repositories most, or Personal for your own. Nothing joins a Space until you confirm.";

/// The Spaces that list the repository named `identity`, in their order.
fn spaces_of<'a>(spaces: &'a [Space], identity: &str) -> impl Iterator<Item = &'a Space> {
    let identity = identity.to_owned();
    (spaces.iter()).filter(move |s| {
        s.repositories
            .iter()
            .any(|i| i.eq_ignore_ascii_case(&identity))
    })
}

/// The page's sections: each Space with the workspace repositories it lists, in the Spaces'
/// order and leaving out a Space with none here, then No Space with the rest.
fn grouped<'a>(
    spaces: &'a [Space],
    repositories: &'a [Repository],
) -> Vec<(Option<&'a Space>, Vec<&'a Repository>)> {
    let listed = |space: &Space, r: &Repository| space.repositories.iter().any(|i| r.is_github(i));
    let mut sections: Vec<(Option<&Space>, Vec<&Repository>)> = (spaces.iter())
        .map(|space| {
            (
                Some(space),
                repositories.iter().filter(|r| listed(space, r)).collect(),
            )
        })
        .filter(|(_, listed): &(_, Vec<_>)| !listed.is_empty())
        .collect();
    let none: Vec<&Repository> = (repositories.iter())
        .filter(|r| !spaces.iter().any(|space| listed(space, r)))
        .collect();
    if !none.is_empty() {
        sections.push((None, none));
    }
    sections
}

/// The Spaces a repository is in with `space` added, or taken out when it is in it.
fn toggled(member: &[SpaceId], space: &SpaceId) -> Vec<SpaceId> {
    match member.contains(space) {
        true => member.iter().filter(|m| *m != space).cloned().collect(),
        false => member.iter().chain([space]).cloned().collect(),
    }
}

/// The pick the host's suggestion makes.
fn suggested_pick(suggestion: &RepositorySuggestion) -> Pick {
    match &suggestion.space {
        Some(SuggestedSpace::Space(id)) => Pick::Space(id.clone()),
        Some(SuggestedSpace::NewPersonal) => Pick::NewPersonal,
        None => Pick::NoSpace,
    }
}

/// The host's latest suggestions, each picked as suggested unless the human picked otherwise
/// for that repository before.
fn with_picks(
    before: &[(RepositorySuggestion, Pick)],
    latest: Vec<RepositorySuggestion>,
) -> Vec<(RepositorySuggestion, Pick)> {
    (latest.into_iter())
        .map(|suggestion| {
            let chosen = (before.iter())
                .find(|(old, _)| old.repository_id == suggestion.repository_id)
                .filter(|(old, pick)| *pick != suggested_pick(old))
                .map(|(_, pick)| pick.clone());
            let pick = chosen.unwrap_or_else(|| suggested_pick(&suggestion));
            (suggestion, pick)
        })
        .collect()
}

/// The upgrade's suggestions as the human picked them, to confirm: each adds its pick to the
/// Spaces the repository is in now, so a chip edited meanwhile stays.
fn confirmed(
    suggestions: &[(RepositorySuggestion, Pick)],
    spaces: &[Space],
) -> Vec<RepositorySpaces> {
    (suggestions.iter())
        .map(|(suggestion, pick)| {
            let mut space_ids: Vec<SpaceId> = spaces_of(spaces, &suggestion.repository)
                .map(|s| s.id.clone())
                .collect();
            if let Pick::Space(id) = pick
                && !space_ids.contains(id)
            {
                space_ids.push(id.clone());
            }
            RepositorySpaces {
                repository: suggestion.repository.clone(),
                space_ids,
                new_personal: *pick == Pick::NewPersonal,
            }
        })
        .collect()
}

fn pick_label(pick: &Pick, spaces: &[Space]) -> String {
    match pick {
        Pick::Space(id) => (spaces.iter())
            .find(|s| &s.id == id)
            .map_or("No Space".into(), |s| s.name.clone()),
        Pick::NewPersonal => format!("{} (new)", workspace_core::PERSONAL),
        Pick::NoSpace => "No Space".into(),
    }
}

/// Why the owner suggests what it does; it only ever suggests.
fn suggestion_reason(spaces: &[Space], suggested: Option<&SpaceId>, owner: &str) -> String {
    match suggested.and_then(|id| spaces.iter().find(|s| &s.id == id)) {
        Some(space) => format!(
            "because {}’s repositories are owned by {owner}. The owner only suggests; nothing joins a Space until you confirm.",
            space.name
        ),
        None => format!("No Space lists a repository owned by {owner}, so none is suggested."),
    }
}

/// A repository drawn in full under another Space: its name, where else it is, and its chips.
fn compact_row(
    snapshot: &Snapshot,
    repository: &Repository,
    space: Option<&Space>,
    chips: Div,
    p: Palette,
) -> Div {
    let identity = repository.github_identity().unwrap_or_default();
    let elsewhere: Vec<&str> = spaces_of(&snapshot.spaces, &identity)
        .filter(|s| Some(&s.id) != space.map(|s| &s.id))
        .map(|s| s.name.as_str())
        .collect();
    div()
        .py_2()
        .flex()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(p.edge.opacity(edge::HAIRLINE))
        .child(icon("folder").text_color(p.subtle))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(repository.name.clone()),
        )
        .child(hint(format!("also in {}", elsewhere.join(", ")), p))
        .child(div().flex_1())
        .child(chips)
}

fn fixes_are_on(keeper: &KeeperSettings) -> bool {
    keeper.conflicts && keeper.checks
}

fn with_fixes(keeper: &KeeperSettings, on: bool) -> KeeperSettings {
    KeeperSettings {
        conflicts: on,
        checks: on,
        ..keeper.clone()
    }
}

/// What the one trigger says while saved settings, such as ones from another machine, have
/// only one of the two on. It shows as off, and turning it on sets both.
fn partly_on_hint(keeper: &KeeperSettings) -> Option<&'static str> {
    match (keeper.conflicts, keeper.checks) {
        (true, false) => Some("Partly on: only conflicts. Turning it on sets both."),
        (false, true) => Some("Partly on: only failed checks. Turning it on sets both."),
        _ => None,
    }
}

/// The held triggers' lines; the one trigger for tries is held once.
fn held_lines(held: &[String]) -> Vec<String> {
    let mut lines = held.to_vec();
    lines.dedup();
    lines
}

/// The repository's checks command, if it has one, and the line that goes with it: its limit,
/// or that none was entered and, in a kept repository, where a conflict or a failed check goes.
fn checks_summary(checks: Option<&RepositoryChecks>, kept: bool) -> (Option<String>, String) {
    match checks {
        Some(checks) => (
            Some(checks.command.clone()),
            format!("stop after {} min", checks.minutes),
        ),
        None if kept => (None, NO_CHECKS.into()),
        None => (None, "none entered".into()),
    }
}

/// Why a base was not saved, and the base that stands.
fn refused_base(error: &str, saved: Option<&str>) -> String {
    match saved {
        Some(saved) => format!("{error}. The base is still {saved}."),
        None => error.to_owned(),
    }
}

fn open_tickets<'a>(
    snapshot: &'a Snapshot,
    repository: &'a Repository,
) -> impl Iterator<Item = &'a Ticket> {
    snapshot
        .tickets
        .iter()
        .filter(|t| t.repository_id == repository.id && t.is_open())
}

/// "1 project · 6 open tickets · 5 open PRs", counting the open PRs of open tickets.
fn repository_counts(snapshot: &Snapshot, repository: &Repository, projects: usize) -> String {
    let tickets = open_tickets(snapshot, repository).count();
    let mut counts = format!(
        "{} · {}",
        counted(projects, "project"),
        counted(tickets, "open ticket")
    );
    let prs = open_tickets(snapshot, repository)
        .filter(|t| {
            t.pull_request
                .as_ref()
                .is_some_and(|pr| pr.state == PrState::Open)
        })
        .count();
    if tickets > 0 {
        counts.push_str(&format!(" · {}", counted(prs, "open PR")));
    }
    counts
}

/// When the repository's PRs were last checked, or why not, and whether the check failed. A
/// repository is not watched, and shows nothing, without an open ticket whose coordinator is
/// not archived, which the snapshot lists.
fn pr_check_status(snapshot: &Snapshot, repository: &Repository) -> Option<(String, bool)> {
    let listed = |id: &str| snapshot.sessions.iter().any(|s| s.id == id && !s.archived);
    open_tickets(snapshot, repository).find(|t| listed(&t.coordinator_id))?;
    let check = snapshot
        .pull_request_checks
        .iter()
        .find(|c| c.repository_id == repository.id);
    Some(match check {
        None => ("PRs not checked yet".into(), false),
        Some(check) if !check.watched => ("Not on GitHub; PRs not watched".into(), false),
        Some(PullRequestCheck {
            error: Some(error),
            next_at,
            ..
        }) => (
            format!("PR check failed: {error} · retrying {}", from_now(*next_at)),
            true,
        ),
        Some(check) => match check.checked_at {
            Some(at) => (format!("PRs checked {}", age(at).to_lowercase()), false),
            None => ("PRs not checked yet".into(), false),
        },
    })
}

impl Requests for RepositoriesPage {
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
        if let Command::SetRepositoryBase {
            repository_id,
            base,
        } = &command
        {
            let saved = self.shared.snapshot().and_then(|s| {
                let repository = s.repositories.iter().find(|r| r.id == *repository_id)?;
                Some(repository.base.clone())
            });
            // Only the editor waiting for this save takes its reply, not one opened since.
            let editing = (self.base_edit.as_mut())
                .filter(|e| e.repository_id == *repository_id && e.pending.as_ref() == Some(base));
            match (editing, reply) {
                (Some(_), Ok(_)) => self.base_edit = None,
                (Some(edit), Err(error)) => {
                    edit.pending = None;
                    edit.error = Some(refused_base(&error, saved.as_deref()))
                }
                (None, Err(error)) => toast_error(error, window, cx),
                (None, Ok(_)) => {}
            }
            return;
        }
        match reply {
            Err(error) => toast_error(error, window, cx),
            Ok(_) if matches!(command, Command::SetRepositoryChecks { .. }) => {
                self.checks_edit = None
            }
            Ok(_) if matches!(command, Command::SetOwnerAllowed { allowed: true, .. }) => self
                .owner_input
                .update(cx, |input, cx| input.set_value("", window, cx)),
            Ok(_) if matches!(command, Command::ConfirmRepositorySuggestions { .. }) => {
                self.suggestions.clear()
            }
            Ok(_) => {}
        }
    }
}
impl Render for RepositoriesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Repositories");
        let p = palette(is_dark(cx));
        ui::page(
            "Repositories",
            "Repositories every project can use",
            self.body(p, cx),
            None,
            &self.shared.host,
            p,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BASE_HINT, NEVER, Pick, UNTIL_GITHUB_ANSWERS, WITHOUT_ASKING, checks_summary, confirmed,
        fixes_are_on, grouped, held_lines, partly_on_hint, pr_check_status, refused_base,
        repository_counts, suggestion_reason, toggled, with_fixes, with_picks,
    };
    use crate::ui;
    use workspace_core::*;

    fn snapshot(open_prs: usize, open_tickets: usize) -> (Snapshot, Repository) {
        let repository: Repository = serde_json::from_value(serde_json::json!({
            "id": "r", "name": "wiffletree", "path": "/w", "base": "origin/main"
        }))
        .unwrap();
        let pr = |state: &str| {
            serde_json::json!({"number": 1, "url": "u", "state": state, "draft": false,
                "base": "main", "head": "h", "merge_state": "clean", "checks": "success",
                "unresolved_threads": 0, "comments": {}})
        };
        let ticket = |i: usize, state: &str, pr: Option<serde_json::Value>| {
            let mut ticket = serde_json::json!({"id": format!("t{i}"), "coordinator_id": "c",
                "repository_id": "r", "title": "T", "brief": "B", "worktree": "/w",
                "branch": "b", "state": state});
            if let Some(pr) = pr {
                ticket["pull_request"] = pr;
            }
            ticket
        };
        // Open tickets with open PRs, then one with a merged PR and the rest without one, and
        // a closed ticket whose open PR is not counted.
        let mut tickets: Vec<_> = (0..open_tickets)
            .map(|i| match i {
                _ if i < open_prs => ticket(i, "assigned", Some(pr("open"))),
                _ if i == open_prs => ticket(i, "passed", Some(pr("merged"))),
                _ => ticket(i, "assigned", None),
            })
            .collect();
        tickets.push(ticket(open_tickets, "closed", Some(pr("open"))));
        let coordinator = serde_json::json!({"id": "c", "project_id": "p", "parent_id": null,
            "repository_id": null, "name": "C", "role": "project_orchestrator",
            "provider": "claude", "status": "ready"});
        let snapshot = serde_json::from_value(serde_json::json!({
            "projects": [], "repositories": [repository], "sessions": [coordinator],
            "attention": [], "tickets": tickets
        }))
        .unwrap();
        (snapshot, repository)
    }
    fn check(checked_at: Option<i64>, error: Option<&str>, next_at: i64) -> PullRequestCheck {
        PullRequestCheck {
            repository_id: "r".into(),
            checked_at,
            error: error.map(Into::into),
            watched: true,
            next_at,
        }
    }

    #[test]
    fn a_repository_row_counts_open_prs_and_says_when_they_were_checked() {
        let (mut snapshot, repository) = snapshot(5, 6);
        assert_eq!(
            repository_counts(&snapshot, &repository, 1),
            "1 project · 6 open tickets · 5 open PRs"
        );
        assert_eq!(
            pr_check_status(&snapshot, &repository),
            Some(("PRs not checked yet".into(), false))
        );
        snapshot.pull_request_checks = vec![check(Some(ui::now() - 40_000), None, 0)];
        assert_eq!(
            pr_check_status(&snapshot, &repository),
            Some(("PRs checked just now".into(), false))
        );
        snapshot.pull_request_checks[0].checked_at = Some(ui::now() - 3 * 60_000);
        assert_eq!(
            pr_check_status(&snapshot, &repository).unwrap().0,
            "PRs checked 3m ago"
        );
        snapshot.pull_request_checks[0].watched = false;
        assert_eq!(
            pr_check_status(&snapshot, &repository).unwrap().0,
            "Not on GitHub; PRs not watched"
        );
    }

    #[test]
    fn a_repository_whose_coordinator_is_archived_shows_no_check() {
        let (mut snapshot, repository) = snapshot(1, 1);
        snapshot.pull_request_checks = vec![check(Some(ui::now() - 3 * 60_000), None, 0)];
        snapshot.sessions = vec![
            serde_json::from_value(serde_json::json!({"id": "c", "project_id": "p",
                "parent_id": null, "repository_id": null, "name": "Main",
                "role": "project_orchestrator", "provider": "claude", "status": "ready",
                "archived": true}))
            .unwrap(),
        ];
        assert_eq!(pr_check_status(&snapshot, &repository), None);
    }

    #[test]
    fn a_failed_check_shows_why_and_when_it_retries() {
        let (mut snapshot, repository) = snapshot(1, 1);
        assert_eq!(
            repository_counts(&snapshot, &repository, 2),
            "2 projects · 1 open ticket · 1 open PR"
        );
        snapshot.pull_request_checks = vec![check(
            Some(ui::now() - 10 * 60_000),
            Some("gh failed: To get started, run: gh auth login"),
            ui::now() + 4 * 60_000,
        )];
        assert_eq!(
            pr_check_status(&snapshot, &repository),
            Some((
                "PR check failed: gh failed: To get started, run: gh auth login · retrying in 4m"
                    .into(),
                true
            ))
        );
    }

    #[test]
    fn a_repository_without_open_tickets_shows_no_pr_status() {
        let (mut snapshot, repository) = snapshot(0, 0);
        snapshot.pull_request_checks = vec![check(Some(0), None, 0)];
        assert_eq!(
            repository_counts(&snapshot, &repository, 1),
            "1 project · 0 open tickets"
        );
        assert_eq!(pr_check_status(&snapshot, &repository), None);
    }

    #[test]
    fn a_repository_row_shows_its_checks_or_where_problems_go_without_them() {
        let checks = RepositoryChecks {
            command: "cargo test --locked".into(),
            minutes: 20,
        };
        assert_eq!(
            checks_summary(Some(&checks), true),
            (
                Some("cargo test --locked".into()),
                "stop after 20 min".into()
            )
        );
        assert_eq!(
            checks_summary(None, true),
            (
                None,
                "none entered · rebases and comment sorting work; a conflict or a failed check goes to the coordinator".into()
            )
        );
        assert_eq!(checks_summary(None, false), (None, "none entered".into()));
    }

    fn on_github(name_with_owner: &str, fork: bool, public: bool) -> Repository {
        let (owner, name) = name_with_owner.split_once('/').unwrap();
        serde_json::from_value(serde_json::json!({
            "id": name, "name": name, "path": format!("/code/{name}"), "base": "origin/main",
            "github": {"owner": owner, "name": name, "facts": {
                "name_with_owner": name_with_owner, "organization": owner != "MHohlios",
                "fork": fork, "parent": fork.then_some("longbridge/gpui-component"),
                "public": public, "checked_at": 1}}
        }))
        .unwrap()
    }

    #[test]
    fn each_repository_row_says_which_rule_applies() {
        let allowed = vec!["Vyttle-LLC".to_owned()];
        let label = |repository: &Repository| rule_label(repository, &allowed);
        assert_eq!(
            label(&on_github("Vyttle-LLC/wiffletree", false, true)),
            "Vyttle-LLC · without asking"
        );
        assert_eq!(
            label(&on_github("celadon-theme/celadon-website", false, true)),
            "celadon-theme · asks first"
        );
        assert_eq!(
            label(&on_github("Vyttle-LLC/gpui-component", true, true)),
            "Fork · asks first"
        );
        let local: Repository = serde_json::from_value(serde_json::json!({
            "id": "s", "name": "scratch", "path": "/s", "base": "main"
        }))
        .unwrap();
        assert_eq!(label(&local), "Not on GitHub");
    }

    #[test]
    fn the_page_says_allowed_and_without_asking_and_never_approved_or_automatic() {
        let fork = on_github("MHohlios/gpui-component", true, true);
        let mut copy = vec![
            WITHOUT_ASKING.to_owned(),
            NEVER.to_owned(),
            UNTIL_GITHUB_ANSWERS.to_owned(),
            rule_label(&fork, &["MHohlios/gpui-component".to_owned()]),
            rule_label(&fork, &[]),
            owner_entries(std::slice::from_ref(&fork)).forks[0].detail(),
        ];
        copy.push(rule_label(
            &on_github("Vyttle-LLC/wiffletree", false, true),
            &[],
        ));
        for text in copy {
            let text = text.to_lowercase();
            assert!(
                !text.contains("approved") && !text.contains("automatic"),
                "{text}"
            );
        }
        assert_eq!(
            NEVER,
            "Wiffletree never approves, merges, closes or comments on a pull request."
        );
        assert_eq!(
            WITHOUT_ASKING,
            "For repositories owned by these accounts. Everything else asks you first."
        );
    }

    #[test]
    fn a_refused_base_names_the_ref_and_the_base_that_stands() {
        assert_eq!(
            refused_base(
                "“origin/mian” doesn’t resolve to a commit in this repository",
                Some("main")
            ),
            "“origin/mian” doesn’t resolve to a commit in this repository. The base is still main."
        );
        assert_eq!(
            BASE_HINT,
            "Changes the base used for tickets and publishing. Existing branches and worktrees are not rebased."
        );
    }

    fn keeper(conflicts: bool, checks: bool) -> KeeperSettings {
        KeeperSettings {
            rebase: true,
            quiet_minutes: 5,
            comments: true,
            conflicts,
            checks,
            review_level: Some("Light".into()),
        }
    }

    #[test]
    fn the_one_trigger_for_tries_shows_on_only_when_both_are_on() {
        assert!(fixes_are_on(&keeper(true, true)));
        assert!(!fixes_are_on(&keeper(false, false)));
        assert!(!fixes_are_on(&keeper(true, false)));
        assert!(!fixes_are_on(&keeper(false, true)));
        assert_eq!(partly_on_hint(&keeper(true, true)), None);
        assert_eq!(partly_on_hint(&keeper(false, false)), None);
        assert!(
            partly_on_hint(&keeper(true, false))
                .unwrap()
                .contains("only conflicts")
        );
        assert!(
            partly_on_hint(&keeper(false, true))
                .unwrap()
                .contains("only failed checks")
        );
    }

    #[test]
    fn turning_the_one_trigger_on_or_off_sets_both_and_changes_nothing_else() {
        for from in [(false, false), (true, false), (false, true), (true, true)] {
            let before = keeper(from.0, from.1);
            assert_eq!(with_fixes(&before, true), keeper(true, true));
            assert_eq!(with_fixes(&before, false), keeper(false, false));
        }
    }

    #[test]
    fn a_partly_on_pair_shows_off_and_turning_it_on_sets_both() {
        let saved = keeper(true, false);
        assert!(!fixes_are_on(&saved));
        // The box shows off, so a click asks for on.
        assert_eq!(with_fixes(&saved, true), keeper(true, true));
    }

    #[test]
    fn a_settings_file_saved_before_the_regrouping_loads_and_displays() {
        let saved: KeeperSettings = serde_json::from_str(
            r#"{"rebase": true, "quiet_minutes": 3, "conflicts": true, "review_level": "Light"}"#,
        )
        .unwrap();
        assert!(!fixes_are_on(&saved));
        assert!(partly_on_hint(&saved).is_some());
        assert_eq!(
            serde_json::to_value(with_fixes(&saved, true)).unwrap(),
            serde_json::json!({"rebase": true, "quiet_minutes": 3, "comments": false,
                "conflicts": true, "checks": true, "review_level": "Light"})
        );
    }

    #[test]
    fn the_one_trigger_for_tries_is_held_once() {
        let held = |name: &str| format!("On another machine: \"{name}\" needs a PR keeper model.");
        assert_eq!(
            held_lines(&[held(KEEPER_TRIES), held(KEEPER_TRIES), "Another.".into()]),
            [held(KEEPER_TRIES), "Another.".into()]
        );
    }

    fn space(name: &str, repositories: &[&str]) -> Space {
        Space {
            id: SpaceId(name.to_lowercase()),
            name: name.into(),
            description: String::new(),
            instructions: String::new(),
            brain: None,
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: name.into() },
                folder: name.to_lowercase(),
            },
            repositories: repositories.iter().map(|r| (*r).to_owned()).collect(),
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        }
    }

    #[test]
    fn repositories_are_grouped_by_space_with_one_in_two_spaces_under_each() {
        let repositories = [
            on_github("Sage-Social/api", false, false),
            on_github("Vyttle-LLC/site", false, false),
            on_github("octo/scratch", false, true),
        ];
        let spaces = [
            space("Sage", &["sage-social/API"]),
            space("Sage Watch", &["Sage-Social/api"]),
            space("Vyttle", &["Vyttle-LLC/site"]),
            space("Personal", &["MHohlios/dotfiles"]),
        ];
        let sections: Vec<(Option<&str>, Vec<&str>)> = grouped(&spaces, &repositories)
            .into_iter()
            .map(|(space, listed)| {
                let names = listed.iter().map(|r| r.name.as_str()).collect();
                (space.map(|s| s.name.as_str()), names)
            })
            .collect();
        assert_eq!(
            sections,
            [
                (Some("Sage"), vec!["api"]),
                (Some("Sage Watch"), vec!["api"]),
                (Some("Vyttle"), vec!["site"]),
                (None, vec!["scratch"]),
            ],
            "Personal lists nothing cloned here, so it has no section"
        );
    }

    #[test]
    fn a_chip_adds_or_removes_one_space_and_keeps_the_others() {
        let (sage, watch) = (SpaceId("sage".into()), SpaceId("watch".into()));
        let both = [sage.clone(), watch.clone()];
        assert_eq!(toggled(&both, &sage), both[1..]);
        assert_eq!(toggled(&both[1..], &sage), [watch, sage.clone()]);
        assert_eq!(
            toggled(&both[..1], &sage),
            Vec::<SpaceId>::new(),
            "No Space"
        );
    }

    #[test]
    fn confirming_the_upgrades_suggestions_sends_each_pick() {
        let suggestion = |repository: &str, space| RepositorySuggestion {
            repository_id: repository.into(),
            repository: repository.into(),
            space,
        };
        let sage = SpaceId("sage".into());
        let picks = [
            (
                suggestion("Sage-Social/api", Some(SuggestedSpace::Space(sage.clone()))),
                Pick::Space(sage.clone()),
            ),
            (
                suggestion("MHohlios/dotfiles", Some(SuggestedSpace::NewPersonal)),
                Pick::NewPersonal,
            ),
            (
                suggestion("Sage-Social/web", Some(SuggestedSpace::Space(sage.clone()))),
                Pick::NoSpace,
            ),
        ];
        let choices = confirmed(&picks, &[]);
        assert_eq!(choices[0].space_ids, [sage]);
        assert!(choices[1].new_personal && choices[1].space_ids.is_empty());
        assert!(!choices[2].new_personal && choices[2].space_ids.is_empty());
    }

    #[test]
    fn a_chip_edit_before_confirm_stays() {
        let suggestion = RepositorySuggestion {
            repository_id: "api".into(),
            repository: "Sage-Social/api".into(),
            space: Some(SuggestedSpace::Space(SpaceId("sage".into()))),
        };
        // The human put api in Vyttle with a chip, and leaves the suggestion at No Space.
        let spaces = [space("Sage", &[]), space("Vyttle", &["Sage-Social/api"])];
        let kept = confirmed(&[(suggestion.clone(), Pick::NoSpace)], &spaces);
        assert_eq!(kept[0].space_ids, [SpaceId("vyttle".into())]);
        let both = confirmed(
            &[(suggestion, Pick::Space(SpaceId("sage".into())))],
            &spaces,
        );
        assert_eq!(
            both[0].space_ids,
            [SpaceId("vyttle".into()), SpaceId("sage".into())]
        );
    }

    #[test]
    fn the_owner_only_suggests_a_space() {
        let spaces = [space("Sage", &["Sage-Social/api"])];
        assert_eq!(
            suggestion_reason(&spaces, Some(&SpaceId("sage".into())), "Sage-Social"),
            "because Sage’s repositories are owned by Sage-Social. The owner only suggests; nothing joins a Space until you confirm."
        );
        assert_eq!(
            suggestion_reason(&spaces, None, "octo"),
            "No Space lists a repository owned by octo, so none is suggested."
        );
    }

    #[test]
    fn suggestions_asked_again_follow_the_host_but_keep_the_humans_picks() {
        let suggestion = |id: &str, space| RepositorySuggestion {
            repository_id: id.into(),
            repository: format!("o/{id}"),
            space,
        };
        let sage = SpaceId("sage".into());
        let before = [
            (suggestion("dotfiles", None), Pick::NoSpace),
            (suggestion("api", None), Pick::Space(sage.clone())),
        ];
        let latest = vec![
            suggestion("dotfiles", Some(SuggestedSpace::NewPersonal)),
            suggestion("api", Some(SuggestedSpace::NewPersonal)),
            suggestion("web", Some(SuggestedSpace::Space(sage.clone()))),
        ];
        let picks: Vec<Pick> = with_picks(&before, latest)
            .into_iter()
            .map(|(_, p)| p)
            .collect();
        assert_eq!(
            picks,
            [
                Pick::NewPersonal,
                Pick::Space(sage.clone()),
                Pick::Space(sage)
            ],
            "the login's Personal arrives, and the human's pick for api stays"
        );
    }
}
