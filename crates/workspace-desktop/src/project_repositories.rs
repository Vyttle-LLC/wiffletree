//! Where a project's repositories come from: Overview's groups and what a Move would change.
use workspace_core::{Project, Repository, RepositoryUse, Snapshot, Space};

/// A project's repositories as Overview groups them, each in the first group that applies.
pub(super) struct RepositoryGroups<'a> {
    pub(super) from_space: Vec<&'a Repository>,
    /// What the Space lists that is not cloned here, as `owner/name`.
    pub(super) not_cloned: Vec<&'a str>,
    pub(super) added: Vec<&'a Repository>,
    /// With the titles of the open tickets there.
    pub(super) open_tickets: Vec<(&'a Repository, Vec<&'a str>)>,
}
impl RepositoryGroups<'_> {
    /// Whether the project uses nothing here, whatever its Space lists that is not cloned.
    pub(super) fn uses_nothing(&self) -> bool {
        self.from_space.is_empty() && self.added.is_empty() && self.open_tickets.is_empty()
    }
}

pub(super) fn groups<'a>(snapshot: &'a Snapshot, project: &Project) -> RepositoryGroups<'a> {
    let space = snapshot.space_of(project);
    let mut groups = RepositoryGroups {
        from_space: vec![],
        not_cloned: (space.iter())
            .flat_map(|s| &s.repositories)
            .filter(|i| !snapshot.repositories.iter().any(|r| r.is_github(i)))
            .map(String::as_str)
            .collect(),
        added: vec![],
        open_tickets: vec![],
    };
    for repository in &snapshot.repositories {
        match snapshot.repository_use(project, repository) {
            Some(RepositoryUse::Space) => groups.from_space.push(repository),
            Some(RepositoryUse::Added) => groups.added.push(repository),
            Some(RepositoryUse::OpenTicket) => groups
                .open_tickets
                .push((repository, ticket_titles(snapshot, project, repository))),
            None => {}
        }
    }
    groups
}

fn ticket_titles<'a>(
    snapshot: &'a Snapshot,
    project: &Project,
    repository: &Repository,
) -> Vec<&'a str> {
    (snapshot.open_ticket_repositories.iter())
        .filter(|t| t.project_id == project.id && t.repository_id == repository.id)
        .map(|t| t.title.as_str())
        .collect()
}

/// “Title”, or “Title” and 2 more, for an open-ticket row.
pub(super) fn tickets_label(titles: &[&str]) -> String {
    let first = titles.first().copied().unwrap_or_default();
    match titles.len() {
        0 | 1 => format!("“{first}”"),
        n => format!("“{first}” and {} more", n - 1),
    }
}

/// What moving a project to another Space would change, by repository name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct MovePreview {
    pub(super) gains: Vec<String>,
    pub(super) loses: Vec<String>,
    /// With why each stays.
    pub(super) keeps: Vec<(String, String)>,
}

pub(super) fn move_preview(snapshot: &Snapshot, project: &Project, target: &Space) -> MovePreview {
    let open_tickets = &snapshot.open_ticket_repositories;
    let mut preview = MovePreview::default();
    for repository in &snapshot.repositories {
        let before = snapshot.repository_use(project, repository);
        let after = project.repository_use(repository, Some(target), open_tickets);
        let name = repository.name.clone();
        match (before, after) {
            (None, Some(RepositoryUse::Space)) => preview.gains.push(name),
            (Some(_), None) => preview.loses.push(name),
            (_, Some(RepositoryUse::Added)) => {
                preview.keeps.push((name, "added to this project".into()))
            }
            (_, Some(RepositoryUse::OpenTicket)) => {
                let titles = ticket_titles(snapshot, project, repository);
                let why = format!("{} · until it closes", tickets_label(&titles));
                preview.keeps.push((name, why));
            }
            _ => {}
        }
    }
    preview
}

/// Up to four names in full; a longer list as its first three and how many more.
pub(super) fn names_label(names: &[String]) -> String {
    match names.len() {
        0..=4 => names.join(", "),
        count => format!("{} and {} more", names[..3].join(", "), count - 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_core::*;

    fn repository(name: &str) -> Repository {
        Repository {
            id: name.into(),
            name: name.into(),
            path: format!("/dev/{name}"),
            base: "origin/main".into(),
            github: Some(GitHubRepository {
                owner: "o".into(),
                name: name.into(),
                facts: None,
                problem: None,
            }),
            origin_problem: None,
            checks: None,
            protected_branches: vec![],
        }
    }
    fn space(id: &str, repositories: &[&str]) -> Space {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "repositories": repositories,
            "storage": {"repository": {"kind": "local", "id": id}, "folder": id},
        }))
        .unwrap()
    }
    fn project(space: &str, added: &[&str]) -> Project {
        Project {
            id: "p".into(),
            name: "Pica onboarding".into(),
            space: SpaceId(space.into()),
            added_repositories: added.iter().map(|a| a.to_string()).collect(),
            home: None,
            slug: None,
            created_by: None,
        }
    }
    fn snapshot(project: Project, spaces: Vec<Space>, open: &[(&str, &str)]) -> Snapshot {
        Snapshot {
            projects: vec![project],
            spaces,
            repositories: ["web", "api", "tool", "ops", "extra"]
                .map(repository)
                .to_vec(),
            open_ticket_repositories: (open.iter())
                .map(|(repository, title)| OpenTicketRepository {
                    project_id: "p".into(),
                    repository_id: (*repository).into(),
                    title: (*title).into(),
                })
                .collect(),
            ..Snapshot::default()
        }
    }
    fn names(repositories: &[&Repository]) -> Vec<String> {
        repositories.iter().map(|r| r.name.clone()).collect()
    }

    #[test]
    fn each_repository_shows_once_in_the_first_group_that_applies() {
        let personal = space("personal", &["o/web", "o/gone"]);
        let snapshot = snapshot(
            project("personal", &["web", "tool"]),
            vec![personal],
            &[
                ("web", "Inside"),
                ("ops", "Fix login retry"),
                ("ops", "Tidy"),
            ],
        );
        let groups = groups(&snapshot, &snapshot.projects[0]);
        assert_eq!(names(&groups.from_space), ["web"]);
        assert_eq!(groups.not_cloned, ["o/gone"]);
        assert_eq!(names(&groups.added), ["tool"]);
        assert_eq!(groups.open_tickets[0].0.name, "ops");
        assert_eq!(
            tickets_label(&groups.open_tickets[0].1),
            "“Fix login retry” and 1 more"
        );
    }

    #[test]
    fn a_space_with_nothing_cloned_here_gives_the_project_nothing() {
        for listed in [&[][..], &["o/gone"][..]] {
            let snapshot = snapshot(
                project("personal", &[]),
                vec![space("personal", listed)],
                &[],
            );
            assert!(groups(&snapshot, &snapshot.projects[0]).uses_nothing());
        }
    }

    #[test]
    fn a_move_lists_gains_losses_and_what_it_keeps() {
        let snapshot = snapshot(
            project("personal", &["tool"]),
            vec![
                space("personal", &["o/web", "o/api"]),
                space("vyttle", &["o/api", "o/extra"]),
            ],
            &[("ops", "Fix login retry")],
        );
        let preview = move_preview(&snapshot, &snapshot.projects[0], &snapshot.spaces[1]);
        assert_eq!(preview.gains, ["extra"]);
        assert_eq!(preview.loses, ["web"], "api is in both");
        assert_eq!(
            preview.keeps,
            [
                ("tool".to_owned(), "added to this project".to_owned()),
                (
                    "ops".to_owned(),
                    "“Fix login retry” · until it closes".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn four_names_show_in_full_and_a_longer_list_is_cut_to_three() {
        let names = |n: usize| -> Vec<String> { (0..n).map(|i| format!("r{i}")).collect() };
        assert_eq!(names_label(&names(4)), "r0, r1, r2, r3");
        assert_eq!(names_label(&names(5)), "r0, r1, r2 and 2 more");
        assert_eq!(names_label(&[]), "");
    }
}
