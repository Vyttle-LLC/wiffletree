use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Host, Session, Session) {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    for args in [
        vec!["init"],
        vec![
            "-c",
            "user.name=Workspace Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Initial",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(repo.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let mut host = Host::open(home.path()).unwrap();
    host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let project = host.create_project("Feature").unwrap();
    let root = host.sessions().unwrap().remove(0);
    let attachment = host
        .attach_repository(&project.id, repo.path().to_str().unwrap(), "HEAD")
        .unwrap();
    let coordinator = host
        .create_session(
            &project.id,
            &root.id,
            Some(&attachment.id),
            "Backend",
            Role::TaskOrchestrator,
            Provider::Codex,
        )
        .unwrap();
    (home, repo, host, root, coordinator)
}

#[test]
fn parent_selects_saved_provider_size_and_cannot_substitute_luna() {
    let (_home, repo, mut host, _root, coordinator) = fixture();
    let profiles: Vec<_> = [Provider::Claude, Provider::Codex]
        .into_iter()
        .map(|provider| {
            let big = ModelProfile {
                provider,
                model: if provider == Provider::Claude {
                    "opus"
                } else {
                    "gpt-6.1-sol"
                }
                .into(),
                effort: "high".into(),
            };
            ProviderProfiles {
                provider,
                small: ModelProfile {
                    effort: "medium".into(),
                    ..big.clone()
                },
                big,
            }
        })
        .collect();
    let mut policy = host.policy(Role::Tester).unwrap();
    policy.mode = RoutingMode::Automatic;
    policy.default = profiles[1].big.clone();
    policy.small = profiles[1].small.clone();
    policy.standard = policy.default.clone();
    policy.complex = policy.default.clone();
    policy.allowed = profiles
        .iter()
        .flat_map(|p| [p.big.clone(), p.small.clone()])
        .collect();
    policy.provider_profiles = profiles.clone();
    host.set_policy(&policy).unwrap();
    let ticket = host
        .create_ticket(&coordinator.id, "Approved models", "Verify routing")
        .unwrap();
    let count = host.sessions().unwrap().len();
    let mut args = json!({"ticket_id":ticket.id,"role":"tester","provider":"codex","size":"small","instruction":"Test"});
    args["profile"] = json!({"provider":"codex","model":"gpt-6-luna","effort":"medium"});
    assert!(
        host.agent_tool(&coordinator.id, "assign_ticket", args.clone())
            .is_err()
    );
    assert_eq!(host.sessions().unwrap().len(), count);
    args.as_object_mut().unwrap().remove("profile");
    let worker: Session = serde_json::from_value(
        host.agent_tool(&coordinator.id, "assign_ticket", args.clone())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        host.session_runtime(&worker.id).unwrap().profile,
        Some(profiles[1].small.clone())
    );
    let retry: Session = serde_json::from_value(
        host.agent_tool(&coordinator.id, "assign_ticket", args)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(retry.id, worker.id);
    let second = host
        .create_ticket(&coordinator.id, "Claude review", "Verify provider routing")
        .unwrap();
    let worker: Session = serde_json::from_value(host.agent_tool(&coordinator.id, "assign_ticket", json!({"ticket_id":second.id,"role":"tester","provider":"claude","size":"big","instruction":"Test"})).unwrap()).unwrap();
    assert_eq!(
        host.session_runtime(&worker.id).unwrap().profile,
        Some(profiles[0].big.clone())
    );
    policy.role = Role::TaskOrchestrator;
    host.set_policy(&policy).unwrap();
    let project = host.create_project("Coordinator profiles").unwrap();
    let root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id)
        .unwrap();
    let attachment = host
        .attach_repository(&project.id, repo.path().to_str().unwrap(), "HEAD")
        .unwrap();
    let child: Session = serde_json::from_value(
        host.agent_tool(
            &root.id,
            "create_repo_coordinator",
            json!({"repository_id":attachment.id,"provider":"codex","size":"small"}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(child.provider, Provider::Codex);
    assert_eq!(
        host.session_runtime(&child.id).unwrap().profile,
        Some(profiles[1].small.clone())
    );
}

#[test]
fn ticket_agents_are_scoped_and_reports_are_immediate_and_idempotent() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            "One ticket",
            "Return the requested result and test it",
        )
        .unwrap();
    assert!(std::path::Path::new(&ticket.worktree).exists());
    assert_eq!(
        host.create_ticket(&coordinator.id, &ticket.title, &ticket.brief)
            .unwrap()
            .id,
        ticket.id
    );
    let implementer = host
        .assign_ticket(
            &ticket.id,
            Role::Implementer,
            Provider::Codex,
            "Implement",
            None,
        )
        .unwrap();
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Verify", None)
        .unwrap();
    assert_eq!(
        host.session_runtime(&implementer.id).unwrap().ticket_id,
        Some(ticket.id.clone())
    );
    assert!(
        host.agent_tool(
            &implementer.id,
            "ask_user",
            json!({"request_id":"q","question":"Skip parent?"})
        )
        .is_err()
    );
    assert!(
        host.agent_tool(
            &implementer.id,
            "send_message",
            json!({"recipient":root.id,"message_id":"bypass","body":"Bypass parent"})
        )
        .is_err()
    );
    assert!(
        host.agent_tool(
            &implementer.id,
            "report",
            json!({"message_id":"bad","kind":"passed","body":"Self approval"})
        )
        .is_err()
    );
    let report =
        json!({"message_id":"result","kind":"ready_for_testing","body":"Commit and checks ready"});
    host.agent_tool(&implementer.id, "report", report.clone())
        .unwrap();
    host.agent_tool(&implementer.id, "report", report).unwrap();
    let messages = host.messages(&coordinator.id, None, 100).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].receipt, Receipt::Queued);
    assert!(
        host.agent_tool(
            &coordinator.id,
            "accept_ticket",
            json!({"ticket_id":ticket.id})
        )
        .is_err()
    );
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"verified","kind":"passed","body":"Independent checks pass"}),
    )
    .unwrap();
    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"verified","kind":"passed","body":"Independent checks pass"}),
    )
    .unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "accepted");
    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    assert!(
        host.assign_ticket(
            &ticket.id,
            Role::Reviewer,
            Provider::Codex,
            "Late assignment",
            None
        )
        .is_err()
    );
}

#[test]
fn progress_reaches_the_parent_without_moving_the_ticket_or_the_reporter() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Progress", "Report as you go")
        .unwrap();
    let implementer = host
        .assign_ticket(
            &ticket.id,
            Role::Implementer,
            Provider::Codex,
            "Implement",
            None,
        )
        .unwrap();
    let progress = json!({"message_id":"halfway","kind":"progress","body":"Parser done"});
    let report: Message = serde_json::from_value(
        host.agent_tool(&implementer.id, "report", progress.clone())
            .unwrap(),
    )
    .unwrap();
    host.agent_tool(&implementer.id, "report", progress)
        .unwrap();
    assert_eq!(report.id, format!("report:{}:halfway", implementer.id));
    assert_eq!(
        report.body,
        format!("[progress] {}\nParser done", implementer.name)
    );
    assert_eq!(
        host.messages(&coordinator.id, None, 100).unwrap(),
        vec![report]
    );
    let progress_rows = host
        .activity(&root.project_id, None, 100)
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "agent_progress")
        .count();
    assert_eq!(progress_rows, 1, "Every progress row has its message");
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "assigned");
    assert_eq!(
        host.session(&implementer.id).unwrap().status,
        implementer.status
    );
    let error = host
        .agent_tool(
            &root.id,
            "report",
            json!({"message_id":"orphan","kind":"progress","body":"No parent"}),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("human in this chat"),
        "{error:#}"
    );
}

#[test]
fn workspace_context_lists_each_childs_recent_reports_newest_first() {
    let (_home, _repo, mut host, _root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "One ticket", "Report twice and be listed")
        .unwrap();
    let implementer = host
        .assign_ticket(
            &ticket.id,
            Role::Implementer,
            Provider::Codex,
            "Implement",
            None,
        )
        .unwrap();
    assert!(
        host.agent_tool(&coordinator.id, "workspace_context", json!({}))
            .unwrap()
            .get("child_reports")
            .is_none(),
        "A child that never reported adds nothing"
    );
    for (id, kind, body) in [
        ("first", "blocked", "Need a decision"),
        ("second", "failed", "Tests fail"),
    ] {
        host.agent_tool(
            &implementer.id,
            "report",
            json!({"message_id":id,"kind":kind,"body":body}),
        )
        .unwrap();
    }
    let first = host
        .message(&format!("report:{}:first", implementer.id))
        .unwrap();
    host.advance_receipt(&first.id, Receipt::Delivered).unwrap();
    let context = host
        .agent_tool(&coordinator.id, "workspace_context", json!({}))
        .unwrap();
    let listed = &context["child_reports"][0];
    assert_eq!(listed["child_id"], implementer.id);
    let reports = listed["reports"].as_array().unwrap();
    let kinds: Vec<_> = reports
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["failed", "blocked"]);
    assert_eq!(reports[0]["summary"], "Tests fail");
    assert_eq!(reports[0]["read"], false);
    assert_eq!(reports[1]["read"], true);
    assert!(
        host.agent_tool(&implementer.id, "workspace_context", json!({}))
            .unwrap()
            .get("child_reports")
            .is_none(),
        "Workers without children see nothing new"
    );
}

#[test]
fn coordinator_closes_questions_the_human_already_settled() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ask = |host: &mut Host, request: &str| {
        host.agent_tool(
            &root.id,
            "ask_user",
            json!({"request_id":request,"question":"Attach the API repository?"}),
        )
        .unwrap()
    };
    ask(&mut host, "repos");
    ask(&mut host, "scope");
    let open = |host: &Host| host.open_questions(&root).unwrap().len();
    assert_eq!(open(&host), 2);
    assert!(
        host.agent_tool(
            &coordinator.id,
            "close_question",
            json!({"request_id":"repos","resolution":"Not mine"})
        )
        .is_err()
    );
    let close = json!({"request_id":"repos","resolution":"Repositories attached"});
    let closed = host
        .agent_tool(&root.id, "close_question", close.clone())
        .unwrap();
    assert_eq!(
        closed["answer"],
        "Closed by coordinator: Repositories attached"
    );
    assert_eq!(
        host.agent_tool(&root.id, "close_question", close).unwrap(),
        closed
    );
    assert_eq!(open(&host), 1);
    assert_eq!(host.snapshot().unwrap().attention.len(), 1);
}

#[test]
fn coordinator_offers_choices_with_a_question() {
    let (_home, _repo, mut host, root, _coordinator) = fixture();
    let mut ask = |options: Value| {
        host.agent_tool(
            &root.id,
            "ask_user",
            json!({"request_id":"waive","question":"Waive the P1?","options":options}),
        )
    };
    assert!(ask(json!("Waive it")).is_err());
    assert!(ask(json!(["Waive it", ""])).is_err());
    ask(json!(["Waive it", "Fix it now"])).unwrap();
    assert_eq!(
        host.snapshot().unwrap().attention[0].options,
        ["Waive it", "Fix it now"]
    );
}

#[test]
fn human_dismisses_questions_but_decides_permissions() {
    let (_home, _repo, mut host, root, _coordinator) = fixture();
    let question = host
        .request_attention(&root.id, "local", "question", "Ship it?", &[])
        .unwrap();
    let permission = host
        .request_attention(&root.id, "local", "permission:1", "Run rm?", &[])
        .unwrap();
    assert!(host.dismiss_attention(&permission.id).is_err());
    host.dismiss_attention(&question.id).unwrap();
    assert!(host.resolve_attention(&question.id, "Late answer").is_err());
    let open: Vec<_> = host.snapshot().unwrap().attention;
    assert_eq!(open, vec![permission]);
}

#[test]
fn coordinator_ownership_and_runtime_recovery_are_enforced() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let other = host.create_project("Other").unwrap();
    let other_root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == other.id)
        .unwrap();
    assert!(
        host.agent_tool(
            &other_root.id,
            "create_ticket",
            json!({"title":"No","brief":"Not a repo coordinator"})
        )
        .is_err()
    );
    assert!(
        host.agent_tool(
            &coordinator.id,
            "send_message",
            json!({"recipient":other_root.id,"message_id":"leak","body":"Cross project"})
        )
        .is_err()
    );
    host.send("uncertain", None, &root.id, "Work").unwrap();
    host.advance_receipt("uncertain", Receipt::Delivered)
        .unwrap();
    let home = host.home.clone();
    drop(host);
    let mut host = Host::open(home).unwrap();
    assert_eq!(host.message("uncertain").unwrap().receipt, Receipt::Held);
    host.reconcile_session(&root.id, false).unwrap();
    assert_eq!(
        host.message("uncertain").unwrap().receipt,
        Receipt::Cancelled
    );
}

#[test]
fn agent_assignment_enforces_and_freezes_the_bounded_model_choice() {
    let (_home, _repo, mut host, _root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Routing", "Check bounded routing")
        .unwrap();
    let mut policy = host.policy(Role::Implementer).unwrap();
    policy.mode = RoutingMode::Automatic;
    host.set_policy(&policy).unwrap();
    let assignment = json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Implement","complexity":"complex"});
    let mut disallowed = assignment.clone();
    disallowed["profile"] = json!({"provider":"codex","model":"unlisted-model","effort":"high"});
    assert!(
        host.agent_tool(&coordinator.id, "assign_ticket", disallowed)
            .is_err()
    );
    assert!(host.runtimes().unwrap().is_empty());
    let worker: Session = serde_json::from_value(
        host.agent_tool(&coordinator.id, "assign_ticket", assignment.clone())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        host.session_runtime(&worker.id).unwrap().profile,
        Some(policy.complex.clone())
    );
    policy.mode = RoutingMode::Fixed;
    host.set_policy(&policy).unwrap();
    host.agent_tool(&coordinator.id, "assign_ticket", assignment)
        .unwrap();
    assert_eq!(
        host.session_runtime(&worker.id).unwrap().profile,
        Some(policy.complex)
    );
    let second = host
        .create_ticket(
            &coordinator.id,
            "Fixed",
            "Respect tester-style fixed choice",
        )
        .unwrap();
    let fixed: Session = serde_json::from_value(host.agent_tool(&coordinator.id, "assign_ticket", json!({"ticket_id":second.id,"role":"implementer","instruction":"Implement","complexity":"complex"})).unwrap()).unwrap();
    assert_eq!(
        host.session_runtime(&fixed.id).unwrap().profile,
        Some(policy.default)
    );
}

#[test]
fn reviewers_with_distinct_focuses_share_their_ticket() {
    let (_home, _repo, mut host, _root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Fix calls", "Fix inbound calls")
        .unwrap();
    let mut review = |provider, focus| {
        host.assign_ticket(&ticket.id, Role::Reviewer, provider, "Review", Some(focus))
            .unwrap()
    };
    let claude = review(Provider::Claude, "Claude correctness");
    let codex = review(Provider::Codex, "Codex correctness");
    let retry = review(Provider::Codex, "Codex correctness");
    assert_ne!(claude.id, codex.id);
    assert_eq!(retry.id, codex.id);
    assert_eq!(codex.name, "Reviewer · Codex correctness · Fix calls");
    for reviewer in [&claude, &codex] {
        let runtime = host.session_runtime(&reviewer.id).unwrap();
        assert_eq!(runtime.ticket_id.as_ref(), Some(&ticket.id));
        assert_eq!(runtime.workdir.as_ref(), Some(&ticket.worktree));
    }
    assert_eq!(host.tickets().unwrap().len(), 1);
}

#[test]
fn closing_a_finished_review_ticket_archives_its_agents() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Review round 1", "Review the fix")
        .unwrap();
    let reviewer = host
        .assign_ticket(&ticket.id, Role::Reviewer, Provider::Claude, "Review", None)
        .unwrap();
    let close = json!({"ticket_id":ticket.id});
    assert!(
        host.agent_tool(&root.id, "close_ticket", close.clone())
            .is_err()
    );
    host.set_status(&reviewer.id, Status::Working).unwrap();
    assert!(
        host.agent_tool(&coordinator.id, "close_ticket", close.clone())
            .is_err()
    );
    host.set_status(&reviewer.id, Status::Done).unwrap();
    host.agent_tool(&coordinator.id, "close_ticket", close.clone())
        .unwrap();
    host.agent_tool(&coordinator.id, "close_ticket", close)
        .unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "closed");
    assert!(host.session(&reviewer.id).unwrap().archived);
    assert!(!host.session(&coordinator.id).unwrap().archived);
    assert!(
        host.assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Late", None)
            .is_err()
    );
}

fn accepted_ticket(host: &mut Host, coordinator: &Session, title: &str) -> (Ticket, Session) {
    let ticket = host
        .create_ticket(&coordinator.id, title, "Ship it")
        .unwrap();
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
        .unwrap();
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"done","kind":"passed","body":"Green"}),
    )
    .unwrap();
    host.agent_tool(
        &coordinator.id,
        "accept_ticket",
        json!({"ticket_id":ticket.id}),
    )
    .unwrap();
    (host.ticket(&ticket.id).unwrap(), tester)
}

#[test]
fn only_the_main_coordinator_archives_its_own_repository_teams() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Toolbar", "Style it")
        .unwrap();
    let worker = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    let other = host.create_project("Elsewhere").unwrap();
    let other_root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == other.id)
        .unwrap();
    let not_main = "Only the main coordinator archives repository teams";
    let not_own = "Archive only your own repository coordinators";
    for (caller, target, expected) in [
        (&coordinator.id, &coordinator.id, not_main),
        (&root.id, &root.id, not_own),
        (&root.id, &worker.id, not_own),
        (&other_root.id, &coordinator.id, not_own),
    ] {
        let refused = host
            .agent_tool(caller, "archive_team", json!({"session_id":target}))
            .unwrap_err();
        assert!(refused.to_string().contains(expected), "{refused}");
    }
    assert!(host.sessions().unwrap().iter().all(|s| !s.archived));
}

#[test]
fn archiving_a_team_names_every_blocker_then_closes_accepted_work() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let (accepted, tester) = accepted_ticket(&mut host, &coordinator, "Toolbar");
    let open = host
        .create_ticket(&coordinator.id, "Sidebar", "Style it")
        .unwrap();
    let worker = host
        .assign_ticket(&open.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    host.set_status(&worker.id, Status::Working).unwrap();
    let archive = json!({"session_id":coordinator.id});

    let refused = host
        .agent_tool(&root.id, "archive_team", archive.clone())
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains(&format!("{} is working", worker.name)),
        "{refused}"
    );
    assert!(refused.contains("\"Sidebar\""), "{refused}");
    assert!(!host.session(&coordinator.id).unwrap().archived);
    assert_eq!(host.ticket(&accepted.id).unwrap().state, "accepted");

    host.set_status(&worker.id, Status::Done).unwrap();
    host.close_ticket(&open.id).unwrap();
    let timer = json!({"label":"Check","prompt":"Check the build","every":"30m"});
    host.agent_tool(&coordinator.id, "schedule", timer).unwrap();
    let team: Vec<Session> = serde_json::from_value(
        host.agent_tool(&root.id, "archive_team", archive.clone())
            .unwrap(),
    )
    .unwrap();

    let ids: Vec<_> = team.iter().map(|s| s.id.as_str()).collect();
    for member in [&coordinator, &tester, &worker] {
        assert!(ids.contains(&member.id.as_str()));
        assert!(host.session(&member.id).unwrap().archived);
    }
    assert!(!host.session(&root.id).unwrap().archived);
    assert_eq!(host.ticket(&accepted.id).unwrap().state, "closed");
    assert!(
        host.snapshot().unwrap().schedules.is_empty(),
        "Archiving a team stops its timers"
    );
    let archived_events = |host: &Host| {
        host.activity(&root.project_id, None, 100)
            .unwrap()
            .into_iter()
            .filter(|a| a.kind == "team_archived")
            .count()
    };
    assert_eq!(archived_events(&host), 1);
    assert!(host.send("late", None, &tester.id, "One more").is_err());
    host.agent_tool(&root.id, "archive_team", archive).unwrap();
    assert_eq!(archived_events(&host), 1);

    host.set_archived(&coordinator.id, false).unwrap();
    assert!(!host.session(&coordinator.id).unwrap().archived);
    host.send("back", None, &tester.id, "Welcome back").unwrap();
}

#[test]
fn main_coordinator_context_lists_each_team_with_its_tickets() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Toolbar", "Style it")
        .unwrap();

    let context = host
        .agent_tool(&root.id, "workspace_context", json!({}))
        .unwrap();
    assert_eq!(
        context["teams"],
        json!([{"id":coordinator.id,"name":"Backend","status":coordinator.status,"archived":false,
            "tickets":[{"id":ticket.id,"title":"Toolbar","state":"planned"}]}])
    );
    let context = host
        .agent_tool(&coordinator.id, "workspace_context", json!({}))
        .unwrap();
    assert!(context.get("teams").is_none());
}

#[test]
fn a_failed_team_archive_leaves_the_team_active_and_a_retry_records_it() {
    let (home, _repo, mut host, root, coordinator) = fixture();
    accepted_ticket(&mut host, &coordinator, "Toolbar");
    let db = rusqlite::Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_team_event BEFORE INSERT ON activity WHEN NEW.kind='team_archived'
         BEGIN SELECT RAISE(ABORT,'injected'); END;",
    )
    .unwrap();
    let archive = json!({"session_id":coordinator.id});

    assert!(
        host.agent_tool(&root.id, "archive_team", archive.clone())
            .is_err()
    );
    assert!(!host.session(&coordinator.id).unwrap().archived);

    db.execute_batch("DROP TRIGGER fail_team_event").unwrap();
    host.agent_tool(&root.id, "archive_team", archive).unwrap();
    assert!(host.session(&coordinator.id).unwrap().archived);
    let events = host.activity(&root.project_id, None, 100).unwrap();
    assert_eq!(
        events.iter().filter(|a| a.kind == "team_archived").count(),
        1
    );
}

#[test]
fn send_message_tells_the_sender_when_the_recipient_is_mid_turn() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let send = |host: &mut Host, id: &str| {
        host.agent_tool(
            &root.id,
            "send_message",
            json!({"recipient":coordinator.id,"message_id":id,"body":"Next step"}),
        )
        .unwrap()
    };
    let idle = send(&mut host, "idle");
    assert_eq!(idle["receipt"], "queued");
    assert!(idle.get("recipient_turn_started_at").is_none());

    host.set_status(&coordinator.id, Status::Working).unwrap();
    let mut runtime = host.session_runtime(&coordinator.id).unwrap();
    runtime.last_started_at = Some(1_791_403_377_633);
    host.save_runtime(&runtime).unwrap();
    let busy = send(&mut host, "busy");
    assert_eq!(busy["receipt"], "queued_behind_turn");
    assert_eq!(busy["recipient_turn_started_at"], 1_791_403_377_633_i64);
    assert_eq!(
        host.message(&format!("agent:{}:busy", root.id))
            .unwrap()
            .receipt,
        Receipt::Queued
    );
}
