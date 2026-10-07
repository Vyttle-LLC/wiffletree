//! The workspace's repositories. Projects use all of them unless they choose their own set.
use super::*;
use gpui_component::{Disableable, Sizable, button::ButtonVariants};
use ui::{empty_state, hint, icon, mono, pill, section};

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
            let teams = snapshot
                .sessions
                .iter()
                .filter(works_here)
                .filter(|s| s.role == Role::TaskOrchestrator && !s.archived)
                .count();
            // Archived teams still point at the repository, so they keep it too.
            let in_use = snapshot.sessions.iter().any(|s| works_here(&s));
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
                    .child(hint(
                        format!(
                            "{projects} {} · {teams} {}",
                            if projects == 1 { "project" } else { "projects" },
                            if teams == 1 { "team" } else { "teams" },
                        ),
                        p,
                    ))
                    .when(missing, |d| d.child(pill("Missing", p.red)))
                    .child(pill(repository.base.clone(), p.subtle))
                    .child(
                        button(SharedString::from(format!("remove-repository-{id}")))
                            .ghost()
                            .xsmall()
                            .icon(icon("close"))
                            .tooltip(if in_use {
                                "Teams work here, so it stays in the workspace"
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
