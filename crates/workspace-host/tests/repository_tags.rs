//! A project's apps and layers come from its Space's tags for its tickets' repositories
//! (OpenSpec change `parent-projects-and-lenses`, slice c).
mod common;
use rusqlite::{Connection, params};
use serde_json::json;
use workspace_core::*;
use workspace_host::Host;

struct Fixture {
    _home: tempfile::TempDir,
    host: Host,
    celadon: SpaceId,
    project: Project,
    coordinator: Session,
    nvim: Repository,
    palette_api: Repository,
}

/// Project "Contrast warnings" in Celadon, which lists `nvim` (app nvim, layer frontend) and
/// `palette-api` (layer backend), both on GitHub under celadon-theme.
fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path().join("home")).unwrap();
    host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let [nvim, palette_api] = ["nvim", "palette-api"].map(|name| {
        let path = home.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        common::git(&path, &["init"]);
        common::git(&path, &["commit", "--allow-empty", "-m", "Initial"]);
        let repository = host.add_repository(path.to_str().unwrap(), "HEAD").unwrap();
        Connection::open(home.path().join("home/workspace.sqlite3"))
            .unwrap()
            .execute(
                "UPDATE repositories SET data=json_set(data,'$.github',json(?2)) WHERE id=?1",
                params![
                    repository.id,
                    json!({"owner": "celadon-theme", "name": name}).to_string()
                ],
            )
            .unwrap();
        host.repository(&repository.id).unwrap()
    });
    let celadon = host.create_space("Celadon", "", "", None).unwrap().space.id;
    let mut space = host.space(&celadon).unwrap().space;
    space.repositories = vec![
        "celadon-theme/nvim".into(),
        "celadon-theme/palette-api".into(),
    ];
    space.set_tag("celadon-theme/nvim", TagKind::App, Some("nvim"));
    space.set_tag("celadon-theme/nvim", TagKind::Layer, Some("frontend"));
    space.set_tag("celadon-theme/palette-api", TagKind::Layer, Some("backend"));
    host.update_space(space).unwrap();
    let project = host
        .create_project_in("Contrast warnings", &celadon)
        .unwrap();
    let coordinator = (host.sessions().unwrap().into_iter())
        .find(|s| s.project_id == project.id && s.role == Role::ProjectOrchestrator)
        .unwrap();
    Fixture {
        _home: home,
        host,
        celadon,
        project,
        coordinator,
        nvim,
        palette_api,
    }
}

impl Fixture {
    fn ticket(&mut self, repository: &Repository, title: &str) -> Ticket {
        (self.host)
            .create_ticket(&self.coordinator.id, &repository.id, title, "Do it")
            .unwrap()
    }
    fn tags(&self) -> ProjectTags {
        let snapshot = self.host.snapshot().unwrap();
        snapshot.project_tags[&self.project.id].clone()
    }
}

#[test]
fn a_project_without_tickets_is_not_started() {
    let f = fixture();
    assert_eq!(f.tags(), ProjectTags::default());
}

#[test]
fn a_project_takes_the_union_of_its_tickets_repositories_in_any_state() {
    let mut f = fixture();
    let (nvim, api) = (f.nvim.clone(), f.palette_api.clone());
    f.ticket(&api, "Palette contrast");
    assert!(f.tags().has_none(TagKind::App));
    assert_eq!(f.tags().layers, ["backend"]);

    let closed = f.ticket(&api, "Palette API");
    f.host.close_ticket(&closed.id).unwrap();
    f.ticket(&nvim, "Highlight groups");
    let expected = ProjectTags {
        started: true,
        apps: vec!["nvim".into()],
        layers: vec!["frontend".into(), "backend".into()],
    };
    assert_eq!(f.tags(), expected);

    // The coordinator reads the same tags on its project.
    let context = f.host.agent_context(&f.coordinator.id).unwrap();
    assert_eq!(
        context["project"]["tags"],
        serde_json::to_value(&expected).unwrap()
    );
}

#[test]
fn retagging_a_repository_shows_in_the_next_snapshot() {
    let mut f = fixture();
    let nvim = f.nvim.clone();
    f.ticket(&nvim, "Highlight groups");
    let mut space = f.host.space(&f.celadon).unwrap().space;
    space.set_tag("celadon-theme/nvim", TagKind::App, Some("editor"));
    f.host.update_space(space).unwrap();
    assert_eq!(f.tags().apps, ["editor"]);
}

#[test]
fn each_space_keeps_its_own_tags_and_a_move_takes_the_new_ones() {
    let mut f = fixture();
    let nvim = f.nvim.clone();
    f.ticket(&nvim, "Highlight groups");
    let vyttle = f
        .host
        .create_space("Vyttle", "", "", None)
        .unwrap()
        .space
        .id;
    let mut space = f.host.space(&vyttle).unwrap().space;
    space.repositories = vec!["celadon-theme/nvim".into()];
    f.host.update_space(space).unwrap();
    assert_eq!(f.tags().layers, ["frontend"]);

    f.host
        .move_project_to_space(&f.project.id, &vyttle)
        .unwrap();
    let moved = f.tags();
    assert!(moved.has_none(TagKind::App) && moved.has_none(TagKind::Layer));
}
