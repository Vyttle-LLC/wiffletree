//! The workspace's repositories. Projects use all of them unless they choose their own set.
use super::*;
use gpui_component::{Disableable, Sizable, button::ButtonVariants};
use ui::{age, empty_state, from_now, hint, icon, mono, pill, section};

impl Workspace {
    pub(super) fn repositories_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(snapshot) = &self.snapshot else {
            return div();
        };
        let mut body = div().flex().flex_col().gap_3().child(
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
            list = list.child(
                div()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.edge.opacity(0.25))
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
                    ),
            );
        }
        body.child(list)
            .child(self.repository_roots(snapshot, p, cx))
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
    use super::{pr_check_status, repository_counts};
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
}
