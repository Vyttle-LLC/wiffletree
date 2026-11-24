//! Parent projects: the link and its rules, children made by `create_project`, messages and
//! reports between parent and child coordinators, the close guard and Space moves.
use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

struct Fixture {
    home: tempfile::TempDir,
    host: Host,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        Self { home, host }
    }
    fn project(&mut self, name: &str) -> Project {
        self.host.create_project(name).unwrap()
    }
    fn coordinator(&self, project: &str) -> Session {
        (self.host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project && s.role == Role::ProjectOrchestrator)
            .unwrap()
    }
    fn file(&mut self, project: &Project, parent: &Project) {
        self.host
            .set_project_parent(&project.id, Some(&parent.id))
            .unwrap();
    }
    fn parent_of(&self, project: &Project) -> Option<String> {
        self.host.project(&project.id).unwrap().parent_id
    }
    fn tool(&mut self, caller: &Session, name: &str, args: Value) -> anyhow::Result<Value> {
        self.host.agent_tool(&caller.id, name, args)
    }
    fn create(&mut self, caller: &Session, name: &str) -> Project {
        let args = json!({"name": name, "brief": "Do"});
        serde_json::from_value(self.tool(caller, "create_project", args).unwrap()).unwrap()
    }
    fn message(&mut self, from: &Session, to: &Session, id: &str) -> anyhow::Result<Value> {
        let args = json!({"recipient": to.id, "message_id": id, "body": "Hello"});
        self.tool(from, "send_message", args)
    }
    fn received(&self, session: &Session) -> Vec<Message> {
        self.host.messages(&session.id, None, 100).unwrap()
    }
    fn activity(&self, project: &Project, kind: &str) -> Vec<String> {
        (self
            .host
            .activity(&project.id, None, 100)
            .unwrap()
            .into_iter())
        .filter(|a| a.kind == kind)
        .map(|a| a.detail)
        .collect()
    }
}

#[test]
fn filing_a_project_survives_a_restart_and_is_recorded_on_both() {
    let mut f = Fixture::new();
    let parent = f.project("Accessible contrast");
    let child = f.project("Contrast warnings");
    f.file(&child, &parent);

    assert_eq!(
        f.activity(&child, "project_moved_under"),
        ["Accessible contrast"]
    );
    assert_eq!(
        f.activity(&parent, "project_moved_under"),
        ["Contrast warnings"]
    );

    let Fixture { home, host } = f;
    drop(host);
    let host = Host::open_set_up(home.path().join("home")).unwrap();
    let projects = host.snapshot().unwrap().projects;
    let shown = |id: &str| projects.iter().find(|p| p.id == id).unwrap();
    assert_eq!(shown(&child.id).parent_id.as_ref(), Some(&parent.id));
    assert_eq!(shown(&parent.id).parent_id, None);
}

#[test]
fn unfiling_makes_a_project_loose_and_is_recorded_on_both() {
    let mut f = Fixture::new();
    let parent = f.project("Sponsorships");
    let child = f.project("Redo sponsor page");
    f.file(&child, &parent);
    f.host.set_project_parent(&child.id, None).unwrap();
    assert_eq!(f.parent_of(&child), None);
    assert_eq!(f.activity(&child, "project_moved_out"), ["Sponsorships"]);
    assert_eq!(
        f.activity(&parent, "project_moved_out"),
        ["Redo sponsor page"]
    );
}

#[test]
fn each_refused_link_says_its_rule_and_changes_nothing() {
    let mut f = Fixture::new();
    let parent = f.project("Accessible contrast");
    let child = f.project("Contrast warnings");
    let loose = f.project("Pickers");
    let closed = f.project("Old work");
    f.file(&child, &parent);
    let vyttle = f.host.create_space("Vyttle", "", "", None).unwrap().space;
    let elsewhere = f.project("Vyttle site");
    f.host
        .move_project_to_space(&elsewhere.id, &vyttle.id)
        .unwrap();
    let closed_coordinator = f.coordinator(&closed.id);
    f.host.set_archived(&closed_coordinator.id, true).unwrap();
    let before = f.host.projects().unwrap();

    for (project, parent, rule) in [
        (&loose, &loose, "cannot be its own parent"),
        (&loose, &child, "a child cannot have children"),
        (
            &parent,
            &loose,
            "a project with children cannot have a parent",
        ),
        (&loose, &elsewhere, "must be in the same Space"),
        (&loose, &closed, "archived"),
    ] {
        let refused = (f.host.set_project_parent(&project.id, Some(&parent.id)))
            .unwrap_err()
            .to_string();
        assert!(refused.contains(rule), "{rule}: {refused}");
    }
    assert_eq!(f.host.projects().unwrap(), before);
}

#[test]
fn an_older_store_opens_with_every_project_loose_and_its_creator_kept() {
    let mut f = Fixture::new();
    let calls = f.project("Calls");
    let creator = f.coordinator(&calls.id);
    let pickers = f.create(&creator, "Pickers");
    // A store from before parent links has neither field.
    let db = rusqlite::Connection::open(f.home.path().join("home/workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE projects SET data=json_remove(data,'$.parent_id','$.standing')",
        [],
    )
    .unwrap();
    drop(db);

    let Fixture { home, host } = f;
    drop(host);
    let mut host = Host::open_set_up(home.path().join("home")).unwrap();
    let opened = host.project(&pickers.id).unwrap();
    assert_eq!(opened.parent_id, None);
    assert!(!opened.standing);
    assert_eq!(
        opened.created_by.map(|by| by.session_id),
        Some(creator.id.clone())
    );
    let context = host
        .agent_tool(&creator.id, "workspace_context", json!({}))
        .unwrap();
    assert_eq!(context["created_projects"][0]["id"], pickers.id);
    assert!(context.get("child_projects").is_none());
}

#[test]
fn empty_links_and_flags_stay_out_of_the_store() {
    let mut f = Fixture::new();
    let project = f.project("Loose");
    let db = rusqlite::Connection::open(f.home.path().join("home/workspace.sqlite3")).unwrap();
    let data: String = db
        .query_row(
            "SELECT data FROM projects WHERE id=?1",
            [&project.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!data.contains("parent_id"), "{data}");
    assert!(!data.contains("standing"), "{data}");
}

#[test]
fn a_loose_coordinator_creates_a_child_and_a_child_creates_a_sibling() {
    let mut f = Fixture::new();
    let parent = f.project("Accessible contrast");
    let parent_coordinator = f.coordinator(&parent.id);
    let child = f.create(&parent_coordinator, "Contrast warnings");
    assert_eq!(child.parent_id.as_ref(), Some(&parent.id));
    assert_eq!(child.space, parent.space);
    // Its own creation sends the parent no notice.
    assert!(f.received(&parent_coordinator).is_empty());

    let child_coordinator = f.coordinator(&child.id);
    let brief = f.received(&child_coordinator).remove(0);
    assert!(brief.body.contains("You can message that coordinator"));

    // The human has spoken to the child, so it may create projects.
    f.host
        .execute(Command::Send {
            id: "human-1".into(),
            sender: None,
            recipient: child_coordinator.id.clone(),
            body: "Go ahead".into(),
            attachments: vec![],
            uploads: vec![],
        })
        .unwrap();
    let sibling = f.create(&child_coordinator, "Warning popup copy");
    assert_eq!(sibling.parent_id.as_ref(), Some(&parent.id));
    assert_eq!(
        sibling.created_by.map(|by| by.session_id),
        Some(child_coordinator.id.clone())
    );
    let notices = f.received(&parent_coordinator);
    assert_eq!(notices.len(), 1);
    assert_eq!(host_notice(&notices[0].id), Some("Wiffletree"));
    assert!(notices[0].body.contains("\"Contrast warnings\""));
    assert!(notices[0].body.contains("\"Warning popup copy\""));
    let sibling_brief = f.received(&f.coordinator(&sibling.id)).remove(0);
    assert!(
        sibling_brief.body.contains(
            "you can message the coordinator of your parent project \"Accessible contrast\""
        ),
        "{}",
        sibling_brief.body
    );
}

#[test]
fn a_parents_message_is_not_the_humans() {
    let mut f = Fixture::new();
    let parent = f.project("Accessible contrast");
    let parent_coordinator = f.coordinator(&parent.id);
    let child = f.create(&parent_coordinator, "Contrast warnings");
    let child_coordinator = f.coordinator(&child.id);
    f.message(&parent_coordinator, &child_coordinator, "go")
        .unwrap();

    let refused = f
        .tool(
            &child_coordinator,
            "create_project",
            json!({"name": "Chained", "brief": "Do"}),
        )
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("once the human has messaged you"),
        "{refused}"
    );
    assert_eq!(f.host.projects().unwrap().len(), 2);
}

#[test]
fn parent_and_child_coordinators_message_each_other_and_siblings_do_not() {
    let mut f = Fixture::new();
    let parent = f.project("Accessible contrast");
    let warnings = f.project("Contrast warnings");
    let pickers = f.project("Pickers: restore highlights");
    let unrelated = f.project("Unrelated");
    f.file(&warnings, &parent);
    f.file(&pickers, &parent);
    let [parent, warnings, pickers, unrelated] =
        [&parent, &warnings, &pickers, &unrelated].map(|p| f.coordinator(&p.id));

    f.message(&parent, &warnings, "down").unwrap();
    f.message(&warnings, &parent, "up").unwrap();
    let delivered = f.received(&warnings).remove(0);
    assert_eq!(delivered.sender.as_ref(), Some(&parent.id));

    let sibling = f
        .message(&warnings, &pickers, "across")
        .unwrap_err()
        .to_string();
    assert!(
        sibling.contains("your parent project's coordinator and your own agents"),
        "{sibling}"
    );
    let stranger = f
        .message(&parent, &unrelated, "out")
        .unwrap_err()
        .to_string();
    assert!(
        stranger.contains("your child projects' coordinators"),
        "{stranger}"
    );
    assert!(f.received(&pickers).is_empty());
    assert!(f.received(&unrelated).is_empty());

    // Unfiled, the two coordinators can no longer reach each other.
    f.host
        .set_project_parent(&warnings.project_id, None)
        .unwrap();
    assert!(f.message(&parent, &warnings, "again").is_err());
}

#[test]
fn a_child_coordinators_report_reaches_its_parents_coordinator() {
    let mut f = Fixture::new();
    let parent = f.project("Accessible contrast");
    let child = f.project("Pickers: restore highlights");
    let loose = f.project("Loose");
    f.file(&child, &parent);
    let [parent, child, loose] = [&parent, &child, &loose].map(|p| f.coordinator(&p.id));

    let report = json!({"message_id": "r1", "kind": "progress", "body": "fix ready for review"});
    f.tool(&child, "report", report.clone()).unwrap();
    let received = f.received(&parent);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].sender.as_ref(), Some(&child.id));
    assert!(received[0].body.contains("fix ready for review"));

    let refused = f.tool(&loose, "report", report).unwrap_err().to_string();
    assert!(refused.contains("no parent to report to"), "{refused}");
}

#[test]
fn workspace_context_names_the_parent_and_lists_the_children() {
    let mut f = Fixture::new();
    let calls = f.project("Calls");
    let calls_coordinator = f.coordinator(&calls.id);
    let billing = f.create(&calls_coordinator, "Billing");
    let billing_coordinator = f.coordinator(&billing.id);
    f.host
        .execute(Command::Send {
            id: "human-1".into(),
            sender: None,
            recipient: billing_coordinator.id.clone(),
            body: "Go ahead".into(),
            attachments: vec![],
            uploads: vec![],
        })
        .unwrap();
    let refunds = f.create(&billing_coordinator, "Refunds");

    let context = f
        .tool(&calls_coordinator, "workspace_context", json!({}))
        .unwrap();
    let children: Vec<&str> = (context["child_projects"].as_array().unwrap().iter())
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(children.len(), 2);
    assert!(children.contains(&"Billing") && children.contains(&"Refunds"));
    let entry = &context["child_projects"].as_array().unwrap()[0];
    assert!(entry["coordinator_id"].is_string());
    assert_eq!(entry["standing"], false);
    assert!(entry["tickets"].is_array());
    // A child is not listed twice.
    assert!(context.get("created_projects").is_none(), "{context}");
    assert!(context.get("parent_project").is_none());

    let context = f
        .tool(&billing_coordinator, "workspace_context", json!({}))
        .unwrap();
    assert_eq!(context["parent_project"]["id"], calls.id);
    assert_eq!(context["parent_project"]["name"], "Calls");
    assert_eq!(
        context["parent_project"]["coordinator_id"],
        calls_coordinator.id
    );
    // The sibling it created is followed under created_projects.
    assert_eq!(context["created_projects"][0]["id"], refunds.id);
    assert!(context.get("child_projects").is_none());
}

#[test]
fn a_parent_stays_open_while_a_child_is_open() {
    let mut f = Fixture::new();
    let parent = f.project("Light themes");
    let child = f.project("Generated palette check");
    f.file(&child, &parent);
    let (parent_coordinator, child_coordinator) =
        (f.coordinator(&parent.id), f.coordinator(&child.id));

    let refused = f
        .host
        .set_archived(&parent_coordinator.id, true)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("\"Generated palette check\""), "{refused}");
    assert!(!f.coordinator(&parent.id).archived);

    f.host.set_archived(&child_coordinator.id, true).unwrap();
    f.host.set_archived(&parent_coordinator.id, true).unwrap();

    let refused = f
        .host
        .set_archived(&child_coordinator.id, false)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("restore \"Light themes\" first"),
        "{refused}"
    );
    assert!(f.coordinator(&child.id).archived);

    f.host.set_archived(&parent_coordinator.id, false).unwrap();
    f.host.set_archived(&child_coordinator.id, false).unwrap();
}

#[test]
fn moving_a_parent_to_another_space_leaves_its_children_loose_behind() {
    let mut f = Fixture::new();
    let parent = f.project("Light themes");
    let first = f.project("Daylight variant");
    let second = f.project("Generated palette check");
    f.file(&first, &parent);
    f.file(&second, &parent);
    let celadon = parent.space.clone();
    let vyttle = f.host.create_space("Vyttle", "", "", None).unwrap().space;

    let moved = f
        .host
        .move_project_to_space(&parent.id, &vyttle.id)
        .unwrap();
    assert_eq!(moved.space, vyttle.id);
    for child in [&first, &second] {
        let child = f.host.project(&child.id).unwrap();
        assert_eq!(child.parent_id, None);
        assert_eq!(child.space, celadon);
        assert_eq!(f.activity(&child, "project_moved_out"), ["Light themes"]);
    }
}

#[test]
fn moving_a_child_to_another_space_makes_it_loose() {
    let mut f = Fixture::new();
    let parent = f.project("Light themes");
    let child = f.project("Daylight variant");
    f.file(&child, &parent);
    let vyttle = f.host.create_space("Vyttle", "", "", None).unwrap().space;
    let moved = f.host.move_project_to_space(&child.id, &vyttle.id).unwrap();
    assert_eq!(moved.parent_id, None);
    assert_eq!(f.host.project(&child.id).unwrap().parent_id, None);
}

#[test]
fn standing_is_the_humans_alone() {
    let mut f = Fixture::new();
    let platform = f.project("Platform");
    let standing = f.host.set_project_standing(&platform.id, true).unwrap();
    assert!(standing.standing);
    assert!(f.host.snapshot().unwrap().projects[0].standing);

    for command in [
        Command::SetProjectStanding {
            project_id: platform.id.clone(),
            standing: false,
        },
        Command::SetProjectParent {
            project_id: platform.id.clone(),
            parent_id: None,
        },
    ] {
        assert!(command.is_human_only());
    }
    let coordinator = f.coordinator(&platform.id);
    for tool in ["set_project_standing", "set_standing", "set_project_parent"] {
        assert!(f.tool(&coordinator, tool, json!({})).is_err(), "{tool}");
    }
    let names: Vec<String> = (workspace_host::mcp::tools_for(Some(Role::ProjectOrchestrator))
        .as_array()
        .unwrap()
        .iter())
    .map(|t| t["name"].as_str().unwrap().to_owned())
    .collect();
    assert!(
        !names
            .iter()
            .any(|n| n.contains("standing") || n.contains("parent"))
    );
}

#[test]
fn a_project_name_is_not_checked_against_the_naming_rules() {
    let mut f = Fixture::new();
    let calls = f.project("Calls");
    let coordinator = f.coordinator(&calls.id);
    let created = f.create(&coordinator, "Backend: palette API");
    assert_eq!(created.name, "Backend: palette API");
}
