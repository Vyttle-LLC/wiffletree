//! A project uses its Space's repositories, those added to it and those of its open tickets,
//! and store schema 10 moves earlier projects onto that.
mod common;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::Path;
use workspace_core::*;
use workspace_host::Host;

struct Fixture {
    home: tempfile::TempDir,
    host: Host,
    space: SpaceId,
    project: Project,
    coordinator: Session,
    web: Repository,
    api: Repository,
    tool: Repository,
}

fn store(home: &Path) -> Connection {
    Connection::open(home.join("home/workspace.sqlite3")).unwrap()
}

/// Project "Calls" in Personal, which lists nothing, and three repositories on GitHub.
fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path().join("home")).unwrap();
    host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let [web, api, tool] = [
        ("web", "Sage-Social"),
        ("api", "Sage-Social"),
        ("tool", "octo"),
    ]
    .map(|(name, owner)| {
        let path = home.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        common::git(&path, &["init"]);
        common::git(&path, &["commit", "--allow-empty", "-m", "Initial"]);
        let repository = host.add_repository(path.to_str().unwrap(), "HEAD").unwrap();
        store(home.path())
            .execute(
                "UPDATE repositories SET data=json_set(data,'$.github',json(?2)) WHERE id=?1",
                params![
                    repository.id,
                    json!({"owner": owner, "name": name}).to_string()
                ],
            )
            .unwrap();
        host.repository(&repository.id).unwrap()
    });
    let project = host.create_project("Calls").unwrap();
    let space = project.space.clone();
    let coordinator = coordinator_of(&host, &project.id);
    Fixture {
        home,
        host,
        space,
        project,
        coordinator,
        web,
        api,
        tool,
    }
}

fn coordinator_of(host: &Host, project: &str) -> Session {
    (host.sessions().unwrap().into_iter())
        .find(|s| s.project_id == project && s.role == Role::ProjectOrchestrator)
        .unwrap()
}

fn list(host: &mut Host, space: &SpaceId, repositories: &[&str]) {
    let mut definition = host.space(space).unwrap().space;
    definition.repositories = repositories.iter().map(|r| r.to_string()).collect();
    host.update_space(definition).unwrap();
}

fn used(host: &Host, project: &str) -> Vec<String> {
    (host.project_repositories(project).unwrap().into_iter())
        .map(|r| r.name)
        .collect()
}

impl Fixture {
    fn ticket(&mut self, repository: &Repository, title: &str) -> anyhow::Result<Ticket> {
        (self.host).create_ticket(&self.coordinator.id, &repository.id, title, "Do it")
    }
    fn raw_project(&self) -> String {
        store(self.home.path())
            .query_row(
                "SELECT data FROM projects WHERE id=?1",
                [&self.project.id],
                |r| r.get(0),
            )
            .unwrap()
    }
}

#[test]
fn a_repository_joining_the_space_works_at_once_with_no_write_to_the_project() {
    let mut f = fixture();
    assert!(
        used(&f.host, &f.project.id).is_empty(),
        "lists nothing here"
    );
    let before = f.raw_project();
    list(&mut f.host, &f.space.clone(), &["sage-social/WEB"]);
    assert_eq!(used(&f.host, &f.project.id), ["web"]);
    f.ticket(&f.web.clone(), "Work").unwrap();
    assert_eq!(f.raw_project(), before);
    assert!(
        f.host
            .project(&f.project.id)
            .unwrap()
            .added_repositories
            .is_empty()
    );
}

#[test]
fn a_refusal_names_the_space_and_the_way_forward() {
    let mut f = fixture();
    let renamed = Space {
        name: "Vyttle".into(),
        ..f.host.space(&f.space).unwrap().space
    };
    f.host.update_space(renamed).unwrap();
    let refused = f.ticket(&f.web.clone(), "Work").unwrap_err();
    assert_eq!(
        format!("{refused:#}"),
        "Project Calls does not use repository web: Vyttle does not list it and it was not added to this project. A coordinator asks for it with request_repository, and the human answers in the Inbox."
    );
}

#[test]
fn an_added_repository_outside_the_space_is_usable_and_a_move_keeps_it() {
    let mut f = fixture();
    let (project, tool) = (f.project.id.clone(), f.tool.clone());
    f.host
        .set_added_repositories(&project, vec![tool.id.clone()])
        .unwrap();
    let sage = f.host.create_space("Sage", "", "", None).unwrap().space.id;
    list(&mut f.host, &sage, &["Sage-Social/api"]);

    let moved = f.host.move_project_to_space(&project, &sage).unwrap();

    assert_eq!(moved.added_repositories, std::slice::from_ref(&tool.id));
    assert_eq!(used(&f.host, &project), ["api", "tool"]);
    f.ticket(&tool, "Keep going").unwrap();
}

#[test]
fn an_addition_the_space_later_lists_outlasts_it_after_an_unrelated_edit() {
    let mut f = fixture();
    let (project, tool, api) = (f.project.id.clone(), f.tool.clone(), f.api.clone());
    f.host
        .set_added_repositories(&project, vec![tool.id.clone()])
        .unwrap();
    list(&mut f.host, &f.space.clone(), &["octo/tool"]);

    (f.host)
        .set_added_repositories(&project, vec![tool.id.clone(), api.id.clone()])
        .unwrap();
    list(&mut f.host, &f.space.clone(), &[]);

    assert_eq!(used(&f.host, &project), ["api", "tool"]);
}

#[test]
fn the_list_holds_nothing_the_space_lists_and_reports_why_each_is_used() {
    let mut f = fixture();
    list(&mut f.host, &f.space.clone(), &["Sage-Social/web"]);
    let (web, api, tool, project) = (
        f.web.clone(),
        f.api.clone(),
        f.tool.clone(),
        f.project.id.clone(),
    );
    let added = (f.host)
        .set_added_repositories(&project, vec![web.id.clone(), tool.id.clone()])
        .unwrap();
    assert_eq!(added.added_repositories, std::slice::from_ref(&tool.id));
    list(
        &mut f.host,
        &f.space.clone(),
        &["Sage-Social/web", "Sage-Social/api"],
    );
    f.ticket(&api, "Stays").unwrap();
    list(&mut f.host, &f.space.clone(), &["Sage-Social/web"]);

    let snapshot = f.host.snapshot().unwrap();
    let project = (snapshot.projects.iter())
        .find(|p| p.id == project)
        .unwrap();
    assert_eq!(
        snapshot.repository_use(project, &web),
        Some(RepositoryUse::Space)
    );
    assert_eq!(
        snapshot.repository_use(project, &tool),
        Some(RepositoryUse::Added)
    );
    assert_eq!(
        snapshot.repository_use(project, &api),
        Some(RepositoryUse::OpenTicket)
    );
    assert_eq!(
        snapshot.project_repositories(&project.id).len(),
        f.host.project_repositories(&project.id).unwrap().len(),
        "the client and the host agree"
    );
}

#[test]
fn an_open_ticket_keeps_its_repository_until_the_last_one_closes() {
    let mut f = fixture();
    list(&mut f.host, &f.space.clone(), &["Sage-Social/web"]);
    let web = f.web.clone();
    let first = f.ticket(&web, "First").unwrap();

    list(&mut f.host, &f.space.clone(), &[]);

    f.host
        .assign_ticket(&first.id, Role::Implementer, Provider::Codex, "Fix", None)
        .unwrap();
    let verifier = (f.host)
        .assign_ticket(&first.id, Role::Tester, Provider::Codex, "Check", None)
        .unwrap();
    assert_eq!(verifier.repository_id.as_deref(), Some(web.id.as_str()));
    let second = f.ticket(&web, "Second").unwrap();

    f.host.close_ticket(&first.id).unwrap();
    assert_eq!(used(&f.host, &f.project.id), ["web"]);
    f.host.close_ticket(&second.id).unwrap();
    assert!(used(&f.host, &f.project.id).is_empty());
    assert!(f.ticket(&web, "Third").is_err());
}

#[test]
fn a_move_leaves_open_tickets_working_and_an_archived_projects_open_ticket_still_counts() {
    let mut f = fixture();
    list(&mut f.host, &f.space.clone(), &["Sage-Social/web"]);
    let (web, project) = (f.web.clone(), f.project.id.clone());
    let ticket = f.ticket(&web, "Work").unwrap();
    let sage = f.host.create_space("Sage", "", "", None).unwrap().space.id;

    f.host.move_project_to_space(&project, &sage).unwrap();
    assert_eq!(used(&f.host, &project), ["web"]);
    (f.host)
        .assign_ticket(&ticket.id, Role::Tester, Provider::Codex, "Check", None)
        .unwrap();

    store(f.home.path())
        .execute(
            "UPDATE sessions SET data=json_set(data,'$.archived',json('true')) WHERE id=?1",
            [&f.coordinator.id],
        )
        .unwrap();
    assert_eq!(used(&f.host, &project), ["web"]);
    let snapshot = f.host.snapshot().unwrap();
    assert!(!snapshot.sessions.iter().any(|s| s.id == f.coordinator.id));
    assert_eq!(snapshot.project_repositories(&project).len(), 1);
}

/// Puts a project back as schema 9 stored it.
fn with_selection(home: &Path, project: &str, repositories: Value) {
    store(home)
        .execute(
            "UPDATE projects SET data=json_set(json_remove(data,'$.added_repositories'),'$.repositories',json(?2)) WHERE id=?1",
            params![project, repositories.to_string()],
        )
        .unwrap();
}

fn reopen(home: &Path) -> Host {
    Host::open_set_up(home.join("home")).unwrap()
}

fn user_version(home: &Path) -> i64 {
    store(home)
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn project_named(host: &Host, name: &str) -> Project {
    (host.projects().unwrap().into_iter())
        .find(|p| p.name == name)
        .unwrap()
}

#[test]
fn the_upgrade_keeps_what_a_project_named_or_works_in_beyond_its_space() {
    let mut f = fixture();
    list(&mut f.host, &f.space.clone(), &["Sage-Social/web"]);
    let space = f.space.clone();
    let (web, api, tool) = (f.web.clone(), f.api.clone(), f.tool.clone());

    // Calls used every repository and has open tickets in and beyond the Space, and a closed one.
    (f.host)
        .set_added_repositories(&f.project.id.clone(), vec![api.id.clone(), tool.id.clone()])
        .unwrap();
    f.ticket(&web, "Inside").unwrap();
    f.ticket(&api, "Beyond").unwrap();
    f.ticket(&api, "Beyond too").unwrap();
    let closed = f.ticket(&tool, "Done").unwrap();
    f.host.close_ticket(&closed.id).unwrap();
    // Idle used every repository and works in none.
    let idle = f.host.create_project_in("Idle", &space).unwrap();
    // Empty chose none.
    let empty = f.host.create_project_in("Empty", &space).unwrap();
    // Overlap named one the Space lists, one it does not and one twice over.
    let overlap = f.host.create_project_in("Overlap", &space).unwrap();
    // Archived used every repository and still has an open ticket beyond the Space.
    let archived = f.host.create_project_in("Archived", &space).unwrap();
    f.host
        .set_added_repositories(&archived.id, vec![api.id.clone()])
        .unwrap();
    let archived_root = coordinator_of(&f.host, &archived.id);
    f.host
        .create_ticket(&archived_root.id, &api.id, "Left open", "Do it")
        .unwrap();

    let home = f.home;
    let spaces = f.host.spaces().unwrap();
    let keep_home = home.path().to_owned();
    drop(f.host);
    with_selection(&keep_home, &f.project.id, Value::Null);
    with_selection(&keep_home, &idle.id, Value::Null);
    with_selection(&keep_home, &empty.id, json!([]));
    with_selection(&keep_home, &overlap.id, json!([web.id, tool.id, tool.id]));
    with_selection(&keep_home, &archived.id, Value::Null);
    store(&keep_home)
        .execute_batch("PRAGMA user_version = 9;")
        .unwrap();
    store(&keep_home)
        .execute(
            "UPDATE sessions SET data=json_set(data,'$.archived',json('true')) WHERE id=?1",
            [&archived_root.id],
        )
        .unwrap();

    let mut host = reopen(&keep_home);

    let added = |host: &Host, name: &str| project_named(host, name).added_repositories;
    assert_eq!(
        added(&host, "Calls"),
        std::slice::from_ref(&api.id),
        "no closed ticket"
    );
    assert!(added(&host, "Idle").is_empty());
    assert!(added(&host, "Empty").is_empty());
    assert_eq!(added(&host, "Overlap"), std::slice::from_ref(&tool.id));
    assert_eq!(added(&host, "Archived"), std::slice::from_ref(&api.id));
    assert_eq!(used(&host, &idle.id), ["web"]);
    assert_eq!(host.spaces().unwrap(), spaces, "no Space's list changes");
    assert_eq!(user_version(&keep_home), 10);
    let leftovers: i64 = store(&keep_home)
        .query_row(
            "SELECT count(*) FROM projects WHERE json_type(data,'$.repositories') IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leftovers, 0);

    // A second open, even from a version an earlier migration put back, changes nothing.
    host.set_added_repositories(&empty.id, vec![tool.id.clone()])
        .unwrap();
    host.set_added_repositories(&overlap.id, vec![]).unwrap();
    drop(host);
    store(&keep_home)
        .execute_batch("PRAGMA user_version = 9;")
        .unwrap();
    let host = reopen(&keep_home);
    assert_eq!(added(&host, "Empty"), std::slice::from_ref(&tool.id));
    assert!(added(&host, "Overlap").is_empty());
    assert_eq!(user_version(&keep_home), 10);
}
