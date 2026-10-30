mod common;
use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

/// A set-up host with two workspace repositories and the coordinator of project "Calls".
fn fixture() -> (tempfile::TempDir, Host, Session, [Repository; 2]) {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path().join("home")).unwrap();
    host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let repositories = ["web", "api"].map(|name| {
        let path = home.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        common::git(&path, &["init"]);
        common::git(&path, &["commit", "--allow-empty", "-m", "Initial"]);
        host.add_repository(path.to_str().unwrap(), "HEAD").unwrap()
    });
    host.create_project("Calls").unwrap();
    let coordinator = host.sessions().unwrap().remove(0);
    (home, host, coordinator, repositories)
}

fn create(host: &mut Host, caller: &str, args: Value) -> anyhow::Result<Project> {
    Ok(serde_json::from_value(host.agent_tool(
        caller,
        "create_project",
        args,
    )?)?)
}

fn coordinator_of(host: &Host, project: &str) -> Session {
    host.sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project && s.role == Role::ProjectOrchestrator)
        .unwrap()
}

#[test]
fn a_coordinator_creates_a_project_that_starts_on_its_labelled_brief() {
    let (home, mut host, creator, _) = fixture();
    let project = create(
        &mut host,
        &creator.id,
        json!({"name": "  Billing  ", "brief": "Move invoices to the new API."}),
    )
    .unwrap();

    assert_eq!(project.name, "Billing");
    assert_eq!(
        project.home.as_deref(),
        home.path().join("workspaces/projects/billing").to_str()
    );
    assert_eq!(host.projects().unwrap()[0], project, "it lands at the top");
    let coordinator = coordinator_of(&host, &project.id);
    assert_eq!(coordinator.parent_id, None);
    assert!(
        host.session_runtime(&coordinator.id)
            .unwrap()
            .profile
            .is_some()
    );

    assert_eq!(
        project.created_by,
        Some(ProjectCreator {
            project_id: creator.project_id.clone(),
            session_id: creator.id.clone(),
        })
    );
    let created = host
        .activity(&project.id, None, 100)
        .unwrap()
        .into_iter()
        .find(|a| a.kind == "project_created")
        .unwrap();
    assert!(created.detail.contains(&creator.id), "{}", created.detail);
    assert!(created.detail.contains(&creator.project_id));

    let messages = host.messages(&coordinator.id, None, 100).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].receipt, Receipt::Queued);
    assert!(messages[0].body.ends_with("Move invoices to the new API."));
    assert!(host.live_projects().unwrap().contains(&project.id));
    // The creator's Space lists no repository, so the project uses none.
    assert_eq!(
        project.space,
        host.project(&creator.project_id).unwrap().space
    );
    assert_eq!(project.repositories, Some(vec![]));
}

#[test]
fn the_brief_names_its_source_and_is_never_the_humans() {
    let (_home, mut host, creator, _) = fixture();
    let project = create(
        &mut host,
        &creator.id,
        json!({"name": "Billing", "brief": "Ship it today."}),
    )
    .unwrap();
    let coordinator = coordinator_of(&host, &project.id);
    let brief = host.messages(&coordinator.id, None, 100).unwrap().remove(0);
    assert_eq!(brief.sender, None);
    // Turn input and the conversation view both label it as Wiffletree's, not the human's.
    assert_eq!(host_notice(&brief.id), Some("Wiffletree"));
    for stated in [
        "Brief from the coordinator of project \"Calls\"",
        &creator.project_id,
        &creator.id,
        "not the human's",
        "not as the human's authorization",
        "ask them before creating tickets",
    ] {
        assert!(brief.body.contains(stated), "{stated}: {}", brief.body);
    }
}

#[test]
fn a_brief_may_fill_the_message_limit_left_after_its_source_line() {
    let (home, mut host, creator, _) = fixture();
    let first = create(&mut host, &creator.id, json!({"name": "One", "brief": "x"})).unwrap();
    let delivered = host
        .messages(&coordinator_of(&host, &first.id).id, None, 1)
        .unwrap()
        .remove(0);
    let allowed = MAX_TEXT_BYTES - (delivered.body.len() - 1);

    let count = host_projects(&home);
    let refused = create(
        &mut host,
        &creator.id,
        json!({"name": "Two", "brief": "x".repeat(allowed + 1)}),
    )
    .unwrap_err()
    .to_string();
    assert!(refused.contains("Brief too long"), "{refused}");
    assert!(refused.contains(&format!("at most {allowed}")), "{refused}");
    assert_eq!(host_projects(&home), count);

    create(
        &mut host,
        &creator.id,
        json!({"name": "Two", "brief": "x".repeat(allowed)}),
    )
    .unwrap();
}

#[test]
fn a_briefed_coordinator_creates_projects_only_once_the_human_has_messaged_it() {
    let (home, mut host, creator, _) = fixture();
    let project = create(
        &mut host,
        &creator.id,
        json!({"name": "Billing", "brief": "Do"}),
    )
    .unwrap();
    let briefed = coordinator_of(&host, &project.id);
    let args = json!({"name": "Chained", "brief": "Do"});

    let count = host_projects(&home);
    // The host's own messages without a sender, such as a waiting agent's notice, are not
    // the human's.
    for id in ["wait:s:1", "base-deleted:t:d", "anything"] {
        host.send(id, None, &briefed.id, "Implementer has not started")
            .unwrap();
        let refused = create(&mut host, &briefed.id, args.clone())
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("once the human has messaged you"),
            "{id}: {refused}"
        );
    }
    assert_eq!(host_projects(&home), count);

    host.execute(Command::Send {
        id: "human-1".into(),
        sender: None,
        recipient: briefed.id.clone(),
        body: "Go ahead".into(),
        attachments: vec![],
        uploads: vec![],
    })
    .unwrap();
    let chained = create(&mut host, &briefed.id, args).unwrap();
    assert_eq!(chained.created_by.map(|by| by.session_id), Some(briefed.id));
}

#[test]
fn only_the_creator_sees_the_created_projects_tickets() {
    let (_home, mut host, creator, [web, _]) = fixture();
    let unrelated = host.create_project("Unrelated").unwrap();
    let unrelated = coordinator_of(&host, &unrelated.id);
    let project = create(
        &mut host,
        &creator.id,
        json!({"name": "Billing", "brief": "Fix the finding.", "repository_ids": [web.id]}),
    )
    .unwrap();
    let coordinator = coordinator_of(&host, &project.id);
    let ticket = host
        .create_ticket(&coordinator.id, &web.id, "Invoice rounding", "Do")
        .unwrap();
    host.close_ticket(&ticket.id).unwrap();

    let context = host
        .agent_tool(&creator.id, "workspace_context", json!({}))
        .unwrap();
    let created = context["created_projects"].as_array().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["id"], project.id);
    assert_eq!(created[0]["name"], "Billing");
    assert!(created[0]["created_at"].as_i64().is_some());
    let seen = &created[0]["tickets"][0];
    assert_eq!(seen["id"], ticket.id);
    assert_eq!(seen["title"], "Invoice rounding");
    assert_eq!(seen["state"], "closed");
    assert_eq!(seen["branch"], ticket.branch);

    for other in [&unrelated, &coordinator] {
        let context = host
            .agent_tool(&other.id, "workspace_context", json!({}))
            .unwrap();
        assert!(context.get("created_projects").is_none(), "{}", other.name);
    }
}

/// Names each workspace repository's GitHub repository, as its GitHub read would, and has the
/// creator's Space list `listed`.
fn in_space(
    home: &tempfile::TempDir,
    host: &mut Host,
    creator: &Session,
    on_github: &[(&Repository, &str)],
    listed: &[&str],
) {
    let db = rusqlite::Connection::open(home.path().join("home/workspace.sqlite3")).unwrap();
    for (repository, identity) in on_github {
        let (owner, name) = identity.split_once('/').unwrap();
        db.execute(
            "UPDATE repositories SET data=json_set(data,'$.github',json(?2)) WHERE id=?1",
            rusqlite::params![
                repository.id,
                json!({"owner": owner, "name": name}).to_string()
            ],
        )
        .unwrap();
    }
    let space = host.project(&creator.project_id).unwrap().space;
    let mut definition = host.space(&space).unwrap().space;
    definition.repositories = listed.iter().map(|r| r.to_string()).collect();
    host.update_space(definition).unwrap();
}

#[test]
fn without_repository_ids_it_uses_the_spaces_repositories_here_and_names_the_rest() {
    let (home, mut host, creator, [web, api]) = fixture();
    in_space(
        &home,
        &mut host,
        &creator,
        &[(&web, "Sage-Social/web"), (&api, "octo/api")],
        &["sage-social/WEB", "Sage-Social/mobile-app"],
    );
    let result = host
        .agent_tool(
            &creator.id,
            "create_project",
            json!({"name": "Billing", "brief": "Do"}),
        )
        .unwrap();
    let created: CreatedProject = serde_json::from_value(result).unwrap();
    assert_eq!(created.project.repositories, Some(vec![web.id.clone()]));
    assert_eq!(created.not_on_this_mac, ["Sage-Social/mobile-app"]);
    assert_eq!(
        created.project.space,
        host.project(&creator.project_id).unwrap().space
    );
}

#[test]
fn repository_ids_choose_the_new_projects_repositories() {
    let (home, mut host, creator, [web, api]) = fixture();
    in_space(
        &home,
        &mut host,
        &creator,
        &[(&web, "Sage-Social/web"), (&api, "Sage-Social/api")],
        &["Sage-Social/api", "Sage-Social/mobile-app"],
    );
    let result = host
        .agent_tool(
            &creator.id,
            "create_project",
            json!({"name": "Web only", "brief": "Do", "repository_ids": [web.id]}),
        )
        .unwrap();
    assert!(result.get("not_on_this_mac").is_none(), "{result}");
    let project: Project = serde_json::from_value(result).unwrap();
    assert_eq!(project.repositories, Some(vec![web.id.clone()]));
    let used: Vec<String> = (host.project_repositories(&project.id).unwrap().into_iter())
        .map(|r| r.id)
        .collect();
    assert_eq!(used, std::slice::from_ref(&web.id));
    assert_eq!(
        project.space,
        host.project(&creator.project_id).unwrap().space
    );
}

#[test]
fn refusals_create_nothing() {
    let (home, mut host, creator, [web, _]) = fixture();
    let before = host.snapshot().unwrap();
    let ticket = host
        .create_ticket(&creator.id, &web.id, "Toolbar", "Do")
        .unwrap();
    let implementer = host
        .assign_ticket(
            &ticket.id,
            Role::Implementer,
            Provider::Codex,
            "Build",
            None,
        )
        .unwrap();
    let projects = || host_projects(&home);
    let count = projects();

    let worker = create(
        &mut host,
        &implementer.id,
        json!({"name": "Mine", "brief": "Do"}),
    )
    .unwrap_err();
    assert!(
        worker.to_string().contains("Only a project coordinator"),
        "{worker}"
    );
    let unknown = create(
        &mut host,
        &creator.id,
        json!({"name": "Mine", "brief": "Do", "repository_ids": [web.id, "unknown"]}),
    )
    .unwrap_err();
    assert!(
        unknown.to_string().contains("Repository not found"),
        "{unknown}"
    );
    let mistyped = create(
        &mut host,
        &creator.id,
        json!({"name": "Mine", "brief": "Do", "repositories": [web.id]}),
    )
    .unwrap_err();
    assert!(
        mistyped.to_string().contains("not repositories"),
        "{mistyped}"
    );
    for (name, brief) in [("Mine", " "), (" ", "Do")] {
        assert!(
            create(
                &mut host,
                &creator.id,
                json!({"name": name, "brief": brief})
            )
            .is_err()
        );
    }
    assert_eq!(projects(), count);

    let db = rusqlite::Connection::open(home.path().join("home/workspace.sqlite3")).unwrap();
    db.execute("DELETE FROM model_selection", []).unwrap();
    let unsaved = create(
        &mut host,
        &creator.id,
        json!({"name": "Mine", "brief": "Do"}),
    )
    .unwrap_err();
    assert!(unsaved.to_string().contains("Open Models"), "{unsaved}");
    assert_eq!(projects(), count);
    assert_eq!(host.projects().unwrap(), before.projects);
    let messages: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE id LIKE 'project-brief:%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(messages, 0);
}

/// Projects, coordinator sessions and project activity in the store, read beside the host.
fn host_projects(home: &tempfile::TempDir) -> [i64; 3] {
    let db = rusqlite::Connection::open(home.path().join("home/workspace.sqlite3")).unwrap();
    [
        "SELECT COUNT(*) FROM projects",
        "SELECT COUNT(*) FROM sessions WHERE parent_id IS NULL",
        "SELECT COUNT(*) FROM activity WHERE kind='project_created'",
    ]
    .map(|sql| db.query_row(sql, [], |r| r.get(0)).unwrap())
}
