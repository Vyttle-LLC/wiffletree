//! Filing a project under a parent: what Move under… offers and what its confirmation says.
//! The host checks every link again, so these only keep the menu from offering a refusal.
use super::project_repositories::names_label;
use workspace_core::{Project, Role, Snapshot};

pub(super) const A_PARENT_STAYS_LOOSE: &str = "A project with children cannot have a parent";

/// What Move under… offers for one project.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum MoveUnder {
    /// Disabled, with the reason for its tooltip.
    Disabled(&'static str),
    /// The possible parents as `(id, name)`, and whether "No parent" is offered.
    Choices {
        parents: Vec<(String, String)>,
        no_parent: bool,
    },
}

/// The other unarchived loose projects of `project`'s Space, and "No parent" for a child.
pub(super) fn move_under(snapshot: &Snapshot, project: &Project) -> MoveUnder {
    if snapshot.child_projects(&project.id).next().is_some() {
        return MoveUnder::Disabled(A_PARENT_STAYS_LOOSE);
    }
    let parents = (snapshot.projects.iter())
        .filter(|p| p.id != project.id && p.space == project.space)
        .filter(|p| p.parent_id.is_none() && is_open(snapshot, &p.id))
        .filter(|p| project.parent_id.as_ref() != Some(&p.id))
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    MoveUnder::Choices {
        parents,
        no_parent: project.parent_id.is_some(),
    }
}

/// The confirmation's title and line for filing `project` under `parent`, or making it loose.
pub(super) fn confirmation(project: &str, parent: Option<&str>) -> (String, String) {
    match parent {
        Some(parent) => (
            format!("Move “{project}” under “{parent}”?"),
            format!("“{project}” becomes a child of “{parent}”."),
        ),
        None => (
            format!("Make “{project}” loose?"),
            format!("“{project}” leaves its parent and has none."),
        ),
    }
}

/// What moving `project` to another Space does to its links, since a parent never spans Spaces;
/// none for a loose project. `from` names its Space.
pub(super) fn space_move_line(
    snapshot: &Snapshot,
    project: &Project,
    from: &str,
) -> Option<String> {
    let children: Vec<String> = (snapshot.child_projects(&project.id))
        .map(|child| format!("“{}”", child.name))
        .collect();
    if !children.is_empty() {
        let (these, verb, stay) = match children.len() {
            1 => ("Its child project", "becomes", "stays"),
            _ => ("Its child projects", "become", "stay"),
        };
        return Some(format!(
            "{these} {} {verb} loose and {stay} in {from}.",
            names_label(&children)
        ));
    }
    let parent = (snapshot.projects.iter()).find(|p| Some(&p.id) == project.parent_id.as_ref())?;
    Some(format!(
        "It leaves its parent “{}”, which stays in {from}, and becomes loose.",
        parent.name
    ))
}

/// A project is open while its coordinator is unarchived; the snapshot leaves archived ones out.
fn is_open(snapshot: &Snapshot, project: &str) -> bool {
    (snapshot.sessions.iter()).any(|s| {
        s.project_id == project
            && s.role == Role::ProjectOrchestrator
            && s.parent_id.is_none()
            && !s.archived
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn project(id: &str, space: &str, parent: Option<&str>) -> Value {
        json!({"id": id, "name": id, "space": space, "parent_id": parent})
    }

    /// Projects with an unarchived coordinator each, apart from `archived`.
    fn snapshot(projects: Vec<Value>, archived: &[&str]) -> Snapshot {
        let mut snapshot = Snapshot::default();
        for project in projects {
            let project: Project = serde_json::from_value(project).unwrap();
            if !archived.contains(&project.id.as_str()) {
                snapshot.sessions.push(
                    serde_json::from_value(json!({
                        "id": format!("c-{}", project.id), "project_id": project.id,
                        "parent_id": null, "repository_id": null, "name": project.name,
                        "role": "project_orchestrator", "provider": "claude",
                        "status": "ready", "archived": false,
                    }))
                    .unwrap(),
                );
            }
            snapshot.projects.push(project);
        }
        snapshot
    }

    fn parents(snapshot: &Snapshot, id: &str) -> MoveUnder {
        let project = snapshot.projects.iter().find(|p| p.id == id).unwrap();
        move_under(snapshot, project)
    }

    fn choices(names: &[&str], no_parent: bool) -> MoveUnder {
        MoveUnder::Choices {
            parents: names
                .iter()
                .map(|n| (n.to_string(), n.to_string()))
                .collect(),
            no_parent,
        }
    }

    #[test]
    fn offers_the_other_open_loose_projects_of_its_space() {
        let snapshot = snapshot(
            vec![
                project("Redo sponsor page", "celadon", None),
                project("Sponsorships", "celadon", None),
                project("Accessible contrast", "celadon", None),
                project("Contrast warnings", "celadon", Some("Accessible contrast")),
                project("Old work", "celadon", None),
                project("Vyttle site", "vyttle", None),
            ],
            &["Old work"],
        );
        assert_eq!(
            parents(&snapshot, "Redo sponsor page"),
            choices(&["Sponsorships", "Accessible contrast"], false)
        );
        // A child is offered the other parents and "No parent", never its own parent.
        assert_eq!(
            parents(&snapshot, "Contrast warnings"),
            choices(&["Redo sponsor page", "Sponsorships"], true)
        );
    }

    #[test]
    fn a_parent_cannot_be_filed() {
        let snapshot = snapshot(
            vec![
                project("Accessible contrast", "celadon", None),
                project("Contrast warnings", "celadon", Some("Accessible contrast")),
                project("Sponsorships", "celadon", None),
            ],
            &[],
        );
        assert_eq!(
            parents(&snapshot, "Accessible contrast"),
            MoveUnder::Disabled(A_PARENT_STAYS_LOOSE)
        );
    }

    #[test]
    fn the_space_move_names_the_links_it_clears() {
        let snapshot = snapshot(
            vec![
                project("Light themes", "celadon", None),
                project("Daylight", "celadon", Some("Light themes")),
                project("Palette check", "celadon", Some("Light themes")),
                project("Loose", "celadon", None),
            ],
            &[],
        );
        let line = |id: &str| {
            let project = snapshot.projects.iter().find(|p| p.id == id).unwrap();
            space_move_line(&snapshot, project, "Celadon")
        };
        assert_eq!(
            line("Light themes").as_deref(),
            Some(
                "Its child projects “Daylight”, “Palette check” become loose and stay in Celadon."
            )
        );
        assert_eq!(
            line("Daylight").as_deref(),
            Some("It leaves its parent “Light themes”, which stays in Celadon, and becomes loose.")
        );
        assert_eq!(line("Loose"), None);
    }

    #[test]
    fn the_confirmation_names_the_project_and_its_new_parent() {
        let (title, line) = confirmation("Redo sponsor page", Some("Sponsorships"));
        assert_eq!(title, "Move “Redo sponsor page” under “Sponsorships”?");
        assert!(line.contains("child of “Sponsorships”"));
        let (title, _) = confirmation("Contrast warnings", None);
        assert_eq!(title, "Make “Contrast warnings” loose?");
    }
}
