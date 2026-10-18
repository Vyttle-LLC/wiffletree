//! The workspace's repositories. Projects use all of them unless they choose their own set.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
};
use ui::{banner, empty_state, hint, icon, mono, pill, section};
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
const FIXES: &str = "Try once to fix a conflict or failed checks";
const NO_CHECKS: &str = "none entered · rebases and comment sorting work; a conflict or a failed check goes to the coordinator";
const OWNER_PLACEHOLDER: &str = "An owner, or owner/name for one fork";

/// A repository's checks command being edited on its row, with the limit chosen so far.
pub(super) struct ChecksEdit {
    repository_id: String,
    minutes: u32,
}

/// One of the PR keeper's triggers: its tick box beside its name and what it does.
fn trigger(checkbox: Checkbox, name: &'static str, details: Vec<AnyElement>) -> Div {
    div().flex().items_start().gap_3().child(checkbox).child(
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
    /// The owner being added to those allowed to have PRs opened without asking.
    owner_input: Entity<InputState>,
    /// The saved review levels, which the keeper's review level picks from.
    verifiers: Entity<verifiers::VerifierEditor>,
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
        Self {
            verifiers,
            bridge,
            shared: Shared::default(),
            checks_command: cx.new(|cx| {
                InputState::new(window, cx).placeholder("The command that runs its checks")
            }),
            checks_edit: None,
            owner_input: cx.new(|cx| InputState::new(window, cx).placeholder(OWNER_PLACEHOLDER)),
        }
    }
    /// Redraws while shown, as the repositories it lists change.
    pub(super) fn share(&mut self, shared: Shared, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let opened = shared.page == Page::Repositories && self.shared.page != Page::Repositories;
        self.shared = shared;
        if opened || (moved && self.shared.page == Page::Repositories) {
            cx.notify();
        }
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
                .child(self.keeper_block(&snapshot.keeper, &snapshot.keeper_held, p, cx))
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
                button("add-workspace-repositories")
                    .primary()
                    .icon(icon("plus"))
                    .label("Add repositories…")
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
        let mut list = div().flex().flex_col();
        for repository in &snapshot.repositories {
            let projects = snapshot
                .projects
                .iter()
                .filter(|project| project.uses(&repository.id))
                .count();
            let in_use = in_use.contains(repository.id.as_str());
            let id = repository.id.clone();
            let missing = snapshot.missing_repositories.contains(&repository.id);
            let kept = kept_label(repository, &snapshot.allowed_owners, &snapshot.keeper);
            let checks = self.checks_field(repository, kept == Some("kept"), p, cx);
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
            list = list.child(
                div()
                    .py_2()
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::HAIRLINE))
                    .child(row)
                    .child(checks.pl(px(28.))),
            );
        }
        body.child(list)
            .child(self.repository_roots(snapshot, p, cx))
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
                button("add-owner")
                    .small()
                    .icon(icon("plus"))
                    .label("Add")
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
                // Not `disabled`: its tick is half-faded on a half-faded box, unreadable in both
                // themes. With no click handler it still cannot change.
                Checkbox::new("keeper-watch").checked(true),
                "Watch it",
                vec![hint(WATCH, p).into_any_element()],
            ))
            .child(trigger(
                Checkbox::new("keeper-rebase")
                    .checked(keeper.rebase)
                    .on_click(rebase),
                "Keep it rebased",
                vec![period.into_any_element()],
            ))
            .child(trigger(
                Checkbox::new("keeper-fixes")
                    .checked(fixes_are_on(keeper))
                    .on_click(fixes),
                FIXES,
                fix_details,
            ))
            .child(trigger(
                Checkbox::new("keeper-comments")
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
            button("remove-checks")
                .ghost()
                .xsmall()
                .label("Remove")
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
                button("save-checks")
                    .primary()
                    .xsmall()
                    .label("Save")
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

/// Rewords a refusal from the host in the Repositories page's words.
fn page_words(error: &str) -> String {
    if error.contains("needs a PR keeper model: choose") {
        return "Choose a PR keeper model in Models first.".into();
    }
    card_names(error)
}

/// The held triggers' lines in the page's words; the one trigger for tries is held once.
fn held_lines(held: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = held.iter().map(|line| held_words(line)).collect();
    lines.dedup();
    lines
}

/// Rewords a held trigger's line from the host in the Repositories page's words.
fn held_words(line: &str) -> String {
    card_names(line).replace(
        " needs a PR keeper model: choose one in Models first",
        ". Choose a PR keeper model in Models first.",
    )
}

/// `text` with the old trigger names replaced by the names of the card's rows.
fn card_names(text: &str) -> String {
    text.replace("Try to resolve conflicts", FIXES)
        .replace("Try to fix failed checks", FIXES)
        .replace("Rebase when the base branch moves", "Keep it rebased")
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
        match reply {
            Err(error) => toast_error(page_words(&error), window, cx),
            Ok(_) if matches!(command, Command::SetRepositoryChecks { .. }) => {
                self.checks_edit = None
            }
            Ok(_) if matches!(command, Command::SetOwnerAllowed { allowed: true, .. }) => self
                .owner_input
                .update(cx, |input, cx| input.set_value("", window, cx)),
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
            p,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FIXES, NEVER, UNTIL_GITHUB_ANSWERS, WITHOUT_ASKING, checks_summary, fixes_are_on,
        held_lines, held_words, page_words, partly_on_hint, pr_check_status, repository_counts,
        with_fixes,
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
    fn refusals_use_the_names_on_the_page() {
        assert_eq!(
            page_words(
                "\"Try to resolve conflicts\" needs a PR keeper model: choose one in Models first"
            ),
            "Choose a PR keeper model in Models first."
        );
        assert_eq!(
            page_words(
                "Sorting comments needs a PR keeper model: choose a Claude model in Models first"
            ),
            "Choose a PR keeper model in Models first."
        );
        assert_eq!(
            page_words("\"Try to fix failed checks\" needs \"Rebase when the base branch moves\""),
            format!("\"{FIXES}\" needs \"Keep it rebased\"")
        );
    }

    #[test]
    fn held_lines_use_the_names_on_the_card() {
        assert_eq!(
            held_words(
                "On from another machine, off on this Mac: \"Try to resolve conflicts\" needs a PR keeper model: choose one in Models first"
            ),
            format!(
                "On from another machine, off on this Mac: \"{FIXES}\". Choose a PR keeper model in Models first."
            )
        );
        let held = |name: &str| {
            format!(
                "On from another machine, off on this Mac: \"{name}\" needs a re-check level to review with: add one in Models → Reviews"
            )
        };
        // One trigger on the card writes both, so it is held once.
        assert_eq!(
            held_lines(&[
                held("Try to resolve conflicts"),
                held("Try to fix failed checks")
            ]),
            [format!(
                "On from another machine, off on this Mac: \"{FIXES}\" needs a re-check level to review with: add one in Models → Reviews"
            )]
        );
        assert_eq!(
            held_words(
                "On from another machine, off on this Mac: \"Try to fix failed checks\" needs a re-check level to review with: add one in Models → Reviews"
            ),
            format!(
                "On from another machine, off on this Mac: \"{FIXES}\" needs a re-check level to review with: add one in Models → Reviews"
            )
        );
    }
}
