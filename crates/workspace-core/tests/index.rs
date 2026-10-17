use serde_json::json;
use std::rc::Rc;
use workspace_core::*;

fn session(id: &str, parent: Option<&str>, status: &str, archived: bool) -> Session {
    serde_json::from_value(json!({
        "id":id,"project_id":"p","parent_id":parent,"repository_id":null,"name":id,
        "role":if parent.is_some() {"implementer"} else {"project_orchestrator"},
        "provider":"claude","status":status,"archived":archived
    }))
    .unwrap()
}

fn runtime(session: &str, ticket: Option<&str>) -> SessionRuntime {
    serde_json::from_value(json!({
        "session_id":session,"provider_session_id":null,"profile":null,"ticket_id":ticket,
        "workdir":null,"last_error":null,"last_started_at":null,"last_finished_at":null
    }))
    .unwrap()
}

fn ticket(id: &str, state: &str) -> Ticket {
    serde_json::from_value(json!({
        "id":id,"coordinator_id":"root","title":id,"brief":"Do","worktree":"/w",
        "branch":"wiffletree/t","state":state
    }))
    .unwrap()
}

fn ids<'a>(sessions: impl Iterator<Item = &'a Session>) -> Vec<&'a str> {
    sessions.map(|s| s.id.as_str()).collect()
}

/// A coordinator with an archived worker on a closed ticket and a working one on an open
/// ticket, created in that order.
fn workspace() -> (Snapshot, Archive) {
    let snapshot = Snapshot {
        sessions: vec![
            session("root", None, "ready", false),
            session("now", Some("root"), "working", false),
        ],
        tickets: vec![ticket("open", "assigned")],
        runtimes: vec![runtime("root", None), runtime("now", Some("open"))],
        archive_stamp: 7,
        ..Default::default()
    };
    let archive = Archive {
        sessions: vec![session("before", Some("root"), "done", true)],
        tickets: vec![ticket("finished", "closed")],
        runtimes: vec![runtime("before", Some("finished"))],
        session_order: vec!["root".into(), "before".into(), "now".into()],
        ticket_order: vec!["finished".into(), "open".into()],
        stamp: 7,
    };
    (snapshot, archive)
}

#[test]
fn the_index_joins_live_and_archived_work_in_creation_order() {
    let (snapshot, archive) = workspace();
    let index = WorkspaceIndex::new(snapshot, Rc::new(archive));

    assert_eq!(ids(index.sessions()), ["root", "before", "now"]);
    assert!(index.session("before").unwrap().archived);
    assert_eq!(index.archived_sessions(), 1);
    assert_eq!(ids(index.children("root")), ["before", "now"]);
    assert_eq!(ids(index.roots("p")), ["root"]);
    assert_eq!(ids(index.agents("finished")), ["before"]);
    assert_eq!(ids(index.agents("open")), ["now"]);
    assert_eq!(
        index.runtime("before").unwrap().ticket_id.as_deref(),
        Some("finished")
    );
    assert_eq!(index.ticket_of("now").unwrap().id, "open");
    assert!(index.ticket_of("root").is_none());
    let owned: Vec<&str> = index
        .tickets_owned_by("root")
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(owned, ["finished", "open"]);
    assert_eq!(index.ticket("finished").unwrap().state, TicketState::Closed);
    assert!(index.session("missing").is_none() && index.ticket("missing").is_none());
    assert!(!index.archive_is_stale());
}

#[test]
fn work_restored_or_created_since_the_archive_was_read_shows_live_in_its_place() {
    let (mut snapshot, archive) = workspace();
    let mut restored = archive.sessions[0].clone();
    restored.archived = false;
    snapshot.sessions.insert(1, restored);
    snapshot.runtimes.push(runtime("before", None));
    snapshot
        .sessions
        .push(session("newest", Some("root"), "ready", false));
    snapshot.archive_stamp = 8;

    let index = WorkspaceIndex::new(snapshot, Rc::new(archive));

    assert_eq!(ids(index.sessions()), ["root", "before", "now", "newest"]);
    assert!(!index.session("before").unwrap().archived);
    assert_eq!(index.archived_sessions(), 0);
    assert!(index.runtime("before").unwrap().ticket_id.is_none());
    assert!(index.archive_is_stale());
}

#[test]
fn a_team_works_while_an_unarchived_session_below_it_works_at_any_depth() {
    let snapshot = Snapshot {
        sessions: vec![
            session("root", None, "ready", false),
            session("lead", Some("root"), "done", false),
            session("deep", Some("lead"), "working", false),
            session("other", None, "ready", false),
        ],
        ..Default::default()
    };
    let archive = Archive {
        sessions: vec![session("gone", Some("other"), "working", true)],
        ..Default::default()
    };
    let index = WorkspaceIndex::new(snapshot, Rc::new(archive));

    assert!(index.team_working("root") && index.team_working("lead"));
    assert!(!index.team_working("deep"));
    assert!(
        !index.team_working("other"),
        "an archived worker does not count"
    );
}
