//! The workspace's repositories. Projects use all of them unless they choose their own set.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
};
use ui::{age, banner, empty_state, from_now, hint, icon, mono, pill, section};

const WITHOUT_ASKING: &str = "In an allowed owner’s repositories, agents push a ticket’s branch and open and update its pull request once its review has passed, without asking you. Everywhere else, each push and each pull request waits for your approval. This list stays on this Mac and is never synced.";
const NEVER: &str = "Wiffletree never approves, merges, closes or comments on a pull request.";
const UNTIL_GITHUB_ANSWERS: &str = "Owners are still listed, because they come from each repository’s remote on disk. Until GitHub has answered for a repository, it asks first, whatever is ticked. Wiffletree asks GitHub again the next time you come back.";
const FORKS: &str = "FORKS · ALLOWED ONE AT A TIME, NEVER BY THEIR OWNER’S TICK";
const NOT_ON_GITHUB: &str = "NOT ON GITHUB · NOTHING TO PUSH TO";
const KEEP: &str = "Only in the repositories ticked above · all off until you tick them.";
const REBASE: &str = "Plain Git, no model and no review. Once the base has been quiet, replaces the branch on GitHub with the rebased one (a force-push that refuses to overwrite anything Wiffletree has not seen).";
const CONFLICTS: &str = "The PR keeper’s model tries once. Its result must change only the conflicted files, pass the repository’s checks and pass a review at the level below before anything is pushed. If not, the try is thrown away and the coordinator is told. A rebase that changes the patch is checked and reviewed the same way. Needs the rebase trigger above and a checks command in the repository.";
const CHECKS: &str = "Only when the checks also fail on this machine. One try, the same gate and review, and only in files the pull request already changes. Needs the rebase trigger above and a checks command in the repository.";
const NO_CHECKS: &str = "none entered · rebases and comment sorting work; a conflict or a failed check goes to the coordinator";
const SORT: &str = "Reads each new comment and sorts it: needs a change, a question, or no action. Tells the coordinator about the first two. Never replies, resolves or reacts.";
const NEVER_KEEPER: &str = "Wiffletree never merges, approves, closes or comments on a pull request. The model is chosen in Models → PR keeper.";

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

impl Workspace {
    pub(super) fn repositories_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(snapshot) = &self.snapshot else {
            return div();
        };
        let mut body = div().flex().flex_col().gap_3();
        if !snapshot.repositories.is_empty() {
            body = body
                .child(self.owners_block(snapshot, p, cx))
                .child(self.keeper_block(&snapshot.keeper, &snapshot.keeper_held, p, cx));
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
                    .on_click(
                        cx.listener(|v, _, w, c| v.open_creation(Creation::Import, None, w, c)),
                    ),
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
            let works_here = |s: &&Session| s.repository_id.as_ref() == Some(&repository.id);
            // Finished tickets and archived agents still point at the repository, so they keep it.
            let in_use = snapshot.sessions.iter().any(|s| works_here(&s))
                || snapshot
                    .tickets
                    .iter()
                    .any(|t| t.repository_id == repository.id);
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
                                        .text_size(px(10.5))
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
                    .border_color(p.edge.opacity(0.25))
                    .child(row)
                    .child(checks.pl(px(28.))),
            );
        }
        body.child(list)
            .child(self.repository_roots(snapshot, p, cx))
    }

    /// The owners the human allows to have pull requests opened without asking: one tick box per
    /// organization or user and per fork, and the repositories with nowhere to push.
    fn owners_block(&self, snapshot: &Snapshot, p: Palette, cx: &mut Context<Self>) -> Div {
        let entries = owner_entries(&snapshot.repositories);
        let allowed = &snapshot.allowed_owners;
        let mut block = div()
            .mb_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                section(owners_heading(&entries, allowed), p)
                    .child(pill("This Mac only", p.subtle)),
            )
            .child(hint(WITHOUT_ASKING, p))
            .child(div().text_size(px(12.)).child(NEVER));
        if let Some(error) = &snapshot.github_error {
            block = block.child(
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
        let mut rows = div().flex().flex_col();
        for owner in &entries.owners {
            rows = rows.child(self.owner_row(
                Some(&owner.login),
                &owner.login,
                owner.kind(),
                owner.summary(),
                allowed,
                p,
                cx,
            ));
        }
        if !entries.forks.is_empty() {
            rows = rows.child(section(FORKS, p).mt_2());
        }
        for fork in &entries.forks {
            let detail = format!("{} · {}", fork.summary(), fork.detail());
            rows = rows.child(self.owner_row(
                Some(&fork.entry),
                &fork.entry,
                None,
                detail,
                allowed,
                p,
                cx,
            ));
        }
        if !entries.not_on_github.is_empty() {
            rows = rows.child(section(NOT_ON_GITHUB, p).mt_2());
        }
        for local in &entries.not_on_github {
            let detail = format!("{} · cannot be allowed", local.reason);
            rows = rows.child(self.owner_row(None, &local.name, None, detail, allowed, p, cx));
        }
        block.child(rows)
    }

    /// The PR keeper's triggers, for the whole workspace: the rebase trigger and its quiet
    /// period, the conflicts trigger and its review level, the checks trigger, and comment
    /// sorting, with a line for each trigger on from another machine that this one cannot run.
    fn keeper_block(
        &self,
        keeper: &KeeperSettings,
        held: &[String],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let picker = self.review_level_picker(keeper, cx);
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
        let conflicts = cx.listener(move |v, checked: &bool, w, c| {
            let keeper = KeeperSettings {
                conflicts: *checked,
                ..current.clone()
            };
            v.request(Command::SetKeeper { keeper }, w, c)
        });
        let current = keeper.clone();
        let checks = cx.listener(move |v, checked: &bool, w, c| {
            let keeper = KeeperSettings {
                checks: *checked,
                ..current.clone()
            };
            v.request(Command::SetKeeper { keeper }, w, c)
        });
        let level = div()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(12.))
            .text_color(p.subtle)
            .child("Reviewed with")
            .child(picker)
            .child("a re-check level from Models → Review");
        let period = div()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(12.))
            .text_color(p.subtle)
            .child("Waits until the base has been quiet for")
            .child(step("less", "−", minutes - 1).on_click(save(quiet(minutes - 1))))
            .child(
                div()
                    .text_color(p.text)
                    .font_weight(FontWeight::MEDIUM)
                    .child(minutes.to_string()),
            )
            .child(step("more", "+", minutes + 1).on_click(save(quiet(minutes + 1))))
            .child("minutes");
        div()
            .mb_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("KEEP PULL REQUESTS UP TO DATE", p))
            .child(hint(KEEP, p))
            .children(
                held.iter()
                    .map(|line| hint(line.clone(), p).text_color(p.yellow)),
            )
            .child(trigger(
                Checkbox::new("keeper-rebase")
                    .checked(keeper.rebase)
                    .on_click(rebase),
                "Rebase when the base branch moves",
                vec![
                    hint(REBASE, p).into_any_element(),
                    period.into_any_element(),
                ],
            ))
            .child(trigger(
                Checkbox::new("keeper-conflicts")
                    .checked(keeper.conflicts)
                    .on_click(conflicts),
                "Try to resolve conflicts",
                vec![
                    hint(CONFLICTS, p).into_any_element(),
                    level.into_any_element(),
                ],
            ))
            .child(trigger(
                Checkbox::new("keeper-checks")
                    .checked(keeper.checks)
                    .on_click(checks),
                "Try to fix failed checks",
                vec![hint(CHECKS, p).into_any_element()],
            ))
            .child(trigger(
                Checkbox::new("keeper-comments")
                    .checked(keeper.comments)
                    .on_click(comments),
                "Sort new comments",
                vec![hint(SORT, p).into_any_element()],
            ))
            .child(div().text_size(px(12.)).child(NEVER_KEEPER))
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
            .text_size(px(11.5))
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
                    .text_size(px(11.))
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
            .snapshot
            .iter()
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

    /// One row of the owners block. `entry` is what its tick box allows; a repository that is
    /// not on GitHub has none.
    #[allow(clippy::too_many_arguments)]
    fn owner_row(
        &self,
        entry: Option<&str>,
        name: &str,
        kind: Option<&'static str>,
        detail: String,
        allowed: &[String],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let tick = entry.map(|entry| {
            let entry = entry.to_owned();
            Checkbox::new(SharedString::from(format!("allow-owner-{entry}")))
                .checked(is_entry_allowed(&entry, allowed))
                .on_click(cx.listener(move |v, checked: &bool, w, c| {
                    v.request(
                        Command::SetOwnerAllowed {
                            entry: entry.clone(),
                            allowed: *checked,
                        },
                        w,
                        c,
                    )
                }))
        });
        div()
            .py_1p5()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(p.edge.opacity(0.25))
            // A row without a box keeps its text in line with the others.
            .child(div().w(px(16.)).flex_none().children(tick))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .child(name.to_owned()),
            )
            .children(kind.map(|kind| pill(kind, p.subtle)))
            .child(div().flex_1().min_w_0().text_right().child(hint(detail, p)))
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

/// "OPEN PULL REQUESTS WITHOUT ASKING · 3 of 4 owners allowed", counting forks as owners to
/// allow.
fn owners_heading(entries: &OwnerEntries, allowed: &[String]) -> String {
    let all = entries
        .owners
        .iter()
        .map(|owner| &owner.login)
        .chain(entries.forks.iter().map(|fork| &fork.entry));
    let (mut total, mut ticked) = (0, 0);
    for entry in all {
        total += 1;
        ticked += usize::from(is_entry_allowed(entry, allowed));
    }
    let owners = if total == 1 { "owner" } else { "owners" };
    format!("OPEN PULL REQUESTS WITHOUT ASKING · {ticked} of {total} {owners} allowed")
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
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let tickets = open_tickets(snapshot, repository).count();
    let mut counts = format!(
        "{} · {}",
        plural(projects, "project", "projects"),
        plural(tickets, "open ticket", "open tickets")
    );
    let prs = open_tickets(snapshot, repository)
        .filter(|t| {
            t.pull_request
                .as_ref()
                .is_some_and(|pr| pr.state == PrState::Open)
        })
        .count();
    if tickets > 0 {
        counts.push_str(&format!(" · {}", plural(prs, "open PR", "open PRs")));
    }
    counts
}

/// When the repository's PRs were last checked, or why not, and whether the check failed. A
/// repository is not watched, and shows nothing, without an open ticket whose coordinator is
/// not archived.
fn pr_check_status(snapshot: &Snapshot, repository: &Repository) -> Option<(String, bool)> {
    let archived = |id: &str| snapshot.sessions.iter().any(|s| s.id == id && s.archived);
    open_tickets(snapshot, repository).find(|t| !archived(&t.coordinator_id))?;
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

#[cfg(test)]
mod tests {
    use super::{
        FORKS, NEVER, NOT_ON_GITHUB, UNTIL_GITHUB_ANSWERS, WITHOUT_ASKING, checks_summary,
        owners_heading, pr_check_status, repository_counts,
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
        let snapshot = serde_json::from_value(serde_json::json!({
            "projects": [], "repositories": [repository], "sessions": [], "attention": [],
            "tickets": tickets
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
    fn the_owners_block_lists_owners_with_counts_and_a_fork_on_its_own() {
        let mut repositories: Vec<_> = (0..29)
            .map(|i| on_github(&format!("Sage-Social/r{i}"), false, false))
            .collect();
        repositories.push(on_github("Vyttle-LLC/wiffletree", false, true));
        repositories.push(on_github("MHohlios/dotfiles", false, false));
        repositories.push(on_github("MHohlios/gpui-component", true, true));
        repositories.push(
            serde_json::from_value(serde_json::json!({
                "id": "s", "name": "scratch-prototypes", "path": "/s", "base": "main"
            }))
            .unwrap(),
        );
        let entries = owner_entries(&repositories);
        let allowed = vec![
            "sage-social".to_owned(),
            "MHohlios/gpui-component".to_owned(),
        ];
        assert_eq!(
            owners_heading(&entries, &allowed),
            "OPEN PULL REQUESTS WITHOUT ASKING · 2 of 4 owners allowed"
        );
        assert_eq!(
            owners_heading(&owner_entries(&repositories[..1]), &[]),
            "OPEN PULL REQUESTS WITHOUT ASKING · 0 of 1 owner allowed"
        );
        let owners: Vec<_> = entries
            .owners
            .iter()
            .map(|o| format!("{} · {} · {}", o.login, o.kind().unwrap(), o.summary()))
            .collect();
        assert_eq!(
            owners,
            [
                "Sage-Social · Organization · 29 repositories · all private",
                "Vyttle-LLC · Organization · 1 repository · public",
                "MHohlios · User · 1 repository · private",
            ]
        );
        // The fork has its own entry and is not counted under MHohlios.
        let fork = &entries.forks[0];
        assert_eq!(
            format!("{} · {} · {}", fork.entry, fork.summary(), fork.detail()),
            "MHohlios/gpui-component · Fork of longbridge/gpui-component · public · pull requests stay inside the fork"
        );
        assert_eq!(entries.not_on_github[0].reason, "No GitHub remote");
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
        let entries = owner_entries(std::slice::from_ref(&fork));
        let mut copy = vec![
            WITHOUT_ASKING.to_owned(),
            NEVER.to_owned(),
            UNTIL_GITHUB_ANSWERS.to_owned(),
            FORKS.to_owned(),
            NOT_ON_GITHUB.to_owned(),
            owners_heading(&entries, &[]),
            rule_label(&fork, &["MHohlios/gpui-component".to_owned()]),
            rule_label(&fork, &[]),
            entries.forks[0].detail(),
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
        assert!(WITHOUT_ASKING.contains("stays on this Mac"));
    }
}
