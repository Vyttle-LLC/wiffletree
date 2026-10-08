//! Store schema 6 retires repository coordinators. The fixture is a schema-5 store, as the
//! previous release left it, with an active team and an archived one.
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use workspace_core::*;
use workspace_host::Host;

fn git(path: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

struct Fixture {
    _directory: tempfile::TempDir,
    home: PathBuf,
    main: Session,
    repository: String,
    /// The active repository coordinator and the archived one.
    team: &'static str,
    old_team: &'static str,
    open: Ticket,
    accepted: Ticket,
    closed: Ticket,
    implementer: Session,
    tester: Session,
    old_tester: Session,
}

/// Inserts a repository coordinator row as the previous release stored it.
fn insert_team(db: &Connection, id: &str, main: &Session, repository: &str, archived: bool) {
    let session = Session {
        id: id.into(),
        project_id: main.project_id.clone(),
        parent_id: Some(main.id.clone()),
        repository_id: Some(repository.into()),
        name: format!("team {id}"),
        role: Role::TaskOrchestrator,
        provider: Provider::Codex,
        status: Status::Ready,
        archived,
    };
    db.execute(
        "INSERT INTO sessions VALUES (?1,?2,?3,'task_orchestrator',?4)",
        params![
            id,
            session.project_id,
            main.id,
            serde_json::to_string(&session).unwrap()
        ],
    )
    .unwrap();
}

/// Moves a ticket and its agents under a team, dropping what schema 6 adds.
fn give_to_team(db: &Connection, ticket: &Ticket, agents: &[&Session], team: &str) {
    db.execute(
        "UPDATE tickets SET coordinator_id=?2,data=json_remove(json_set(data,'$.coordinator_id',?2),'$.repository_id') WHERE id=?1",
        params![ticket.id, team],
    )
    .unwrap();
    for agent in agents {
        db.execute(
            "UPDATE sessions SET parent_id=?2,data=json_set(data,'$.parent_id',?2) WHERE id=?1",
            params![agent.id, team],
        )
        .unwrap();
    }
}

fn message(
    db: &Connection,
    id: &str,
    project: &str,
    sender: &str,
    recipient: &str,
    body: &str,
    receipt: &str,
) {
    db.execute(
        "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?4,?5,?6,1)",
        params![id, project, sender, recipient, body, receipt],
    )
    .unwrap();
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("wiffletree");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let home = directory.path().join("home");
    let mut host = Host::open(&home).unwrap();
    host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let project = host.create_project("Flatten").unwrap();
    let main = host.sessions().unwrap().remove(0);
    let repository = host
        .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
        .unwrap()
        .id;
    let mut ticket = |title: &str| {
        host.create_ticket(&main.id, &repository, title, &format!("{title} brief"))
            .unwrap()
    };
    let (open, accepted, closed) = (ticket("Open"), ticket("Accepted"), ticket("Closed"));
    let implementer = host
        .assign_ticket(&open.id, Role::Implementer, Provider::Codex, "Do", None)
        .unwrap();
    let tester = host
        .assign_ticket(&accepted.id, Role::Tester, Provider::Codex, "Test", None)
        .unwrap();
    let old_tester = host
        .assign_ticket(&closed.id, Role::Tester, Provider::Codex, "Test", None)
        .unwrap();
    host.close_ticket(&closed.id).unwrap();
    drop(host);

    let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
    let (team, old_team) = ("team-r", "team-old");
    insert_team(&db, team, &main, &repository, false);
    insert_team(&db, old_team, &main, &repository, true);
    give_to_team(&db, &open, &[&implementer], team);
    give_to_team(&db, &accepted, &[&tester], team);
    give_to_team(&db, &closed, &[&old_tester], old_team);
    db.execute(
        "UPDATE tickets SET data=json_set(data,'$.state','accepted') WHERE id=?1",
        [&accepted.id],
    )
    .unwrap();
    let project_id = main.project_id.as_str();
    // The team's conversation, its unread input and its timer.
    message(
        &db,
        "agent:brief-1",
        project_id,
        &main.id,
        team,
        "Plan the work",
        "completed",
    );
    message(
        &db,
        &format!("output:{team}-1"),
        project_id,
        team,
        team,
        "Planned two tickets",
        "completed",
    );
    message(
        &db,
        &format!("report:{}:ready", implementer.id),
        project_id,
        &implementer.id,
        team,
        "[ready_for_testing] Implementer\nCommitted",
        "queued",
    );
    message(
        &db,
        "agent:brief-2",
        project_id,
        &main.id,
        team,
        "Also add logging",
        "queued",
    );
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail) VALUES ('team-run',?1,'[\"agent:brief-1\"]',1,2,'completed','{}')",
        [team],
    )
    .unwrap();
    db.execute(
        "INSERT INTO schedules VALUES ('timer-1',?1,?2,'Nightly check','Check CI',1,3600000,NULL,9999999999999,1,NULL)",
        params![project_id, team],
    )
    .unwrap();
    let permission = Attention {
        id: "permission-1".into(),
        project_id: project_id.into(),
        session_id: team.into(),
        host: "local".into(),
        operation_id: "permission:1".into(),
        prompt: "team requests Bash".into(),
        options: vec![],
        answer: None,
    };
    db.execute(
        "INSERT INTO attention VALUES (?1,?2,?3,?4,?5)",
        params![
            permission.id,
            project_id,
            team,
            permission.operation_id,
            serde_json::to_string(&permission).unwrap()
        ],
    )
    .unwrap();
    db.execute_batch("ALTER TABLE messages DROP COLUMN quiet; PRAGMA user_version = 5;")
        .unwrap();
    drop(db);
    Fixture {
        _directory: directory,
        home,
        main,
        repository,
        team,
        old_team,
        open,
        accepted,
        closed,
        implementer,
        tester,
        old_tester,
    }
}

fn raw(home: &Path) -> Connection {
    Connection::open(home.join("workspace.sqlite3")).unwrap()
}

fn version(home: &Path) -> i64 {
    raw(home)
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// Everything the migration must leave as it was, in comparable form.
fn kept(home: &Path) -> Value {
    let db = raw(home);
    let rows = |sql: &str| -> Vec<String> {
        db.prepare(sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    json!({
        "tickets": rows("SELECT json_object('id',json_extract(data,'$.id'),'title',json_extract(data,'$.title'),'brief',json_extract(data,'$.brief'),'state',json_extract(data,'$.state'),'worktree',json_extract(data,'$.worktree'),'branch',json_extract(data,'$.branch')) FROM tickets ORDER BY rowid"),
        "sessions": rows("SELECT id FROM sessions ORDER BY rowid"),
        "runtimes": rows("SELECT data FROM runtimes ORDER BY rowid"),
        "runs": rows("SELECT id||session_id||messages FROM provider_runs ORDER BY rowid"),
        "bodies": rows("SELECT id||':'||body FROM messages ORDER BY sequence"),
    })
}

#[test]
fn teams_move_under_their_coordinator_without_losing_anything() {
    let f = fixture();
    let before = kept(&f.home);

    let mut host = Host::open(&f.home).unwrap();

    assert_eq!(version(&f.home), 6);
    assert!(f.home.join("workspace.sqlite3.pre-flatten").exists());
    // Ticket ids, titles, briefs, states, worktrees, branches, sessions, runtimes, runs and
    // message bodies are as they were; only re-sent copies and the notice are new.
    let after = kept(&f.home);
    for key in ["tickets", "sessions", "runtimes", "runs"] {
        assert_eq!(after[key], before[key], "{key}");
    }
    let bodies_before = before["bodies"].as_array().unwrap();
    assert_eq!(
        &after["bodies"].as_array().unwrap()[..bodies_before.len()],
        &bodies_before[..]
    );

    for (ticket, state) in [
        (&f.open, "assigned"),
        (&f.accepted, "accepted"),
        (&f.closed, "closed"),
    ] {
        let migrated = host.ticket(&ticket.id).unwrap();
        assert_eq!(migrated.coordinator_id, f.main.id);
        assert_eq!(migrated.repository_id, f.repository);
        assert_eq!(migrated.state, state);
        assert_eq!(
            (&migrated.worktree, &migrated.branch),
            (&ticket.worktree, &ticket.branch)
        );
    }
    for agent in [&f.implementer, &f.tester, &f.old_tester] {
        let migrated = host.session(&agent.id).unwrap();
        assert_eq!(migrated.parent_id.as_ref(), Some(&f.main.id));
        assert_eq!(migrated.archived, agent.id == f.old_tester.id);
    }
    assert!(
        Path::new(&f.open.worktree).exists(),
        "the open ticket keeps its worktree"
    );

    // Both teams are archived, readable and cannot be restored.
    for team in [f.team, f.old_team] {
        let retired = host.session(team).unwrap();
        assert!(retired.archived);
        let refused = host.set_archived(team, false).unwrap_err().to_string();
        assert!(refused.contains("read-only"), "{refused}");
        assert!(host.session(team).unwrap().archived);
    }
    let transcript = host.messages(f.team, None, 100).unwrap();
    assert!(transcript.iter().any(|m| m.body == "Plan the work"));
    assert!(transcript.iter().any(|m| m.body == "Planned two tickets"));

    // Unread input: the report is re-sent to the coordinator, the instruction only listed.
    let report_id = format!("report:{}:ready", f.implementer.id);
    assert_eq!(
        host.message(&report_id).unwrap().receipt,
        Receipt::Cancelled
    );
    assert_eq!(
        host.message("agent:brief-2").unwrap().receipt,
        Receipt::Cancelled
    );
    let resent = host.message(&format!("migrated:{report_id}")).unwrap();
    assert_eq!(resent.recipient, f.main.id);
    assert_eq!(resent.sender.as_ref(), Some(&f.implementer.id));
    assert_eq!(resent.receipt, Receipt::Queued);
    assert!(resent.body.contains(&report_id) && resent.body.contains("Committed"));
    let copies: i64 = raw(&f.home)
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE body LIKE '%Also add logging%' AND id<>'agent:brief-2' AND recipient<>?1",
            [&f.main.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        copies, 0,
        "no agent receives the coordinator's unread instruction"
    );

    // One quiet notice per team names its tickets, the listed instruction and the timer.
    let notice = host.message(&format!("migration:{}", f.team)).unwrap();
    assert_eq!(notice.recipient, f.main.id);
    let quiet: bool = raw(&f.home)
        .query_row(
            "SELECT quiet FROM messages WHERE id=?1",
            [&notice.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(quiet, "the notice waits for the coordinator's next turn");
    for expected in [
        f.open.id.as_str(),
        &f.accepted.id,
        &f.open.branch,
        &f.open.worktree,
        "agent:brief-2",
        "Also add logging",
        "timer-1",
        &format!("migrated:{report_id}"),
    ] {
        assert!(
            notice.body.contains(expected),
            "{expected}\n{}",
            notice.body
        );
    }
    assert!(host.message(&format!("migration:{}", f.old_team)).is_ok());

    // Its timer stopped and its permission request was denied.
    let stopped: Option<String> = raw(&f.home)
        .query_row(
            "SELECT stopped FROM schedules WHERE id='timer-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stopped.as_deref(), Some("migrated"));
    assert!(host.snapshot().unwrap().attention.is_empty());
    let activity = host.activity(&f.main.project_id, None, 100).unwrap();
    assert_eq!(
        activity
            .iter()
            .filter(|a| a.kind == "team_migrated")
            .count(),
        2
    );

    // A second start changes nothing, the backup included.
    let migrated = kept(&f.home);
    let backup = std::fs::read(f.home.join("workspace.sqlite3.pre-flatten")).unwrap();
    drop(host);
    let host = Host::open(&f.home).unwrap();
    assert_eq!(kept(&f.home), migrated);
    assert_eq!(
        std::fs::read(f.home.join("workspace.sqlite3.pre-flatten")).unwrap(),
        backup
    );
    assert_eq!(version(&f.home), 6);
    assert_eq!(
        host.activity(&f.main.project_id, None, 100)
            .unwrap()
            .iter()
            .filter(|a| a.kind == "team_migrated")
            .count(),
        2
    );
}

#[test]
fn a_failure_mid_migration_leaves_schema_5_for_a_full_retry() {
    let f = fixture();
    let before = kept(&f.home);
    // The second team fails after the first one was already moved.
    raw(&f.home)
        .execute_batch(&format!(
            "CREATE TRIGGER fail_second BEFORE INSERT ON activity WHEN NEW.kind='team_migrated' AND NEW.session_id='{}'
             BEGIN SELECT RAISE(ABORT,'injected'); END;",
            f.old_team
        ))
        .unwrap();

    assert!(Host::open(&f.home).is_err());

    assert_eq!(version(&f.home), 5);
    assert_eq!(kept(&f.home), before);
    let db = raw(&f.home);
    let owner: String = db
        .query_row(
            "SELECT coordinator_id FROM tickets WHERE id=?1",
            [&f.open.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(owner, f.team);
    let quiet: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='quiet'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(quiet, 0);

    db.execute_batch("DROP TRIGGER fail_second").unwrap();
    drop(db);
    let host = Host::open(&f.home).unwrap();
    assert_eq!(version(&f.home), 6);
    assert_eq!(host.ticket(&f.open.id).unwrap().coordinator_id, f.main.id);
    assert_eq!(host.ticket(&f.closed.id).unwrap().coordinator_id, f.main.id);
}

#[test]
fn an_implementer_mid_turn_at_upgrade_resumes_in_its_conversation_and_reports_to_the_coordinator() {
    let f = fixture();
    {
        let db = raw(&f.home);
        let mut runtime: SessionRuntime = serde_json::from_str(
            &db.query_row(
                "SELECT data FROM runtimes WHERE session_id=?1",
                [&f.implementer.id],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
        )
        .unwrap();
        runtime.provider_session_id = Some("conversation-1".into());
        db.execute(
            "UPDATE runtimes SET data=?2 WHERE session_id=?1",
            params![f.implementer.id, serde_json::to_string(&runtime).unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES ('cut-off',?1,'[]',1,'{}')",
            [&f.implementer.id],
        )
        .unwrap();
    }

    let mut host = Host::open(&f.home).unwrap();

    let runtime = host.session_runtime(&f.implementer.id).unwrap();
    assert_eq!(
        runtime.provider_session_id.as_deref(),
        Some("conversation-1")
    );
    assert_eq!(runtime.workdir.as_ref(), Some(&f.open.worktree));
    host.agent_tool(
        &f.implementer.id,
        "report",
        json!({"message_id":"after","kind":"blocked","body":"Need a decision"}),
    )
    .unwrap();
    let report = host
        .message(&format!("report:{}:after", f.implementer.id))
        .unwrap();
    assert_eq!(report.recipient, f.main.id);
}

#[test]
fn a_ticket_migrated_as_passed_is_verified_before_it_can_be_accepted() {
    let f = fixture();
    raw(&f.home)
        .execute(
            "UPDATE tickets SET data=json_set(data,'$.state','passed') WHERE id=?1",
            [&f.open.id],
        )
        .unwrap();
    let mut host = Host::open(&f.home).unwrap();
    let accept = json!({"ticket_id":f.open.id});
    let refused = host
        .agent_tool(&f.main.id, "accept_ticket", accept.clone())
        .unwrap_err();
    assert!(refused.to_string().contains("verify_ticket"), "{refused}");

    let cycle: Ticket = serde_json::from_value(
        host.agent_tool(&f.main.id, "verify_ticket", accept.clone())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(cycle.state, "verifying");
    let run = cycle.verification.unwrap().rounds[0].verifiers[0].clone();
    host.advance_receipt(&run.message_id, Receipt::Delivered)
        .unwrap();
    host.agent_tool(
        &run.session_id,
        "report",
        json!({"message_id":"checked","kind":"passed","body":"Still green"}),
    )
    .unwrap();

    let accepted: Ticket = serde_json::from_value(
        host.agent_tool(&f.main.id, "accept_ticket", accept)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(accepted.state, "accepted");
}
