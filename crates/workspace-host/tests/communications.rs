use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

/// A project using one repository. The project coordinator `root` owns tickets directly.
fn fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Host,
    Session,
    Repository,
) {
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
    let repository = host
        .attach_repository(&project.id, repo.path().to_str().unwrap(), "HEAD")
        .unwrap();
    (home, repo, host, root, repository)
}

fn implementer(host: &mut Host, ticket: &Ticket) -> Session {
    host.assign_ticket(
        &ticket.id,
        Role::Implementer,
        Provider::Codex,
        "Implement",
        None,
    )
    .unwrap()
}

/// What a turn does first: takes every queued message as its input.
fn take_input(host: &mut Host, session: &str) {
    for message in host.messages(session, None, 100).unwrap() {
        if message.receipt == Receipt::Queued {
            host.advance_receipt(&message.id, Receipt::Delivered)
                .unwrap();
        }
    }
}

/// Reports the implementer ready, verifies with the default single tester and passes it.
fn verified(host: &mut Host, ticket: &Ticket, implementer: &Session) -> Session {
    host.agent_tool(
        &implementer.id,
        "report",
        json!({"message_id":"ready","kind":"ready_for_testing","body":"Committed"}),
    )
    .unwrap();
    let cycle: Ticket = serde_json::from_value(
        host.agent_tool(
            &ticket.coordinator_id,
            "verify_ticket",
            json!({"ticket_id":ticket.id}),
        )
        .unwrap(),
    )
    .unwrap();
    let verifier = host
        .session(&cycle.verification.unwrap().rounds[0].verifiers[0].session_id)
        .unwrap();
    take_input(host, &verifier.id);
    host.agent_tool(
        &verifier.id,
        "report",
        json!({"message_id":"verified","kind":"passed","body":"Independent checks pass"}),
    )
    .unwrap();
    verifier
}

#[test]
fn parent_selects_saved_provider_size_and_cannot_substitute_luna() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
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
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Approved models",
            "Verify routing",
        )
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
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Claude review",
            "Verify provider routing",
        )
        .unwrap();
    let worker: Session = serde_json::from_value(host.agent_tool(&coordinator.id, "assign_ticket", json!({"ticket_id":second.id,"role":"tester","provider":"claude","size":"big","instruction":"Test"})).unwrap()).unwrap();
    assert_eq!(
        host.session_runtime(&worker.id).unwrap().profile,
        Some(profiles[0].big.clone())
    );
}

#[test]
fn ticket_agents_are_scoped_and_reports_are_immediate_and_idempotent() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "One ticket",
            "Return the requested result and test it",
        )
        .unwrap();
    assert!(std::path::Path::new(&ticket.worktree).exists());
    assert_eq!(ticket.repository_id, repository.id);
    assert_eq!(
        host.create_ticket(
            &coordinator.id,
            &repository.id,
            &ticket.title,
            &ticket.brief
        )
        .unwrap()
        .id,
        ticket.id
    );
    let implementer = implementer(&mut host, &ticket);
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Verify", None)
        .unwrap();
    assert_eq!(
        implementer.parent_id.as_deref(),
        Some(coordinator.id.as_str())
    );
    assert_eq!(
        implementer.repository_id.as_deref(),
        Some(repository.id.as_str())
    );
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
            json!({"recipient":tester.id,"message_id":"sideways","body":"Peer to peer"})
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
    let accept = json!({"ticket_id":ticket.id});
    assert!(
        host.agent_tool(&coordinator.id, "accept_ticket", accept.clone())
            .is_err()
    );
    // An ad-hoc tester's pass is evidence, but acceptance needs a verified commit.
    host.agent_tool(
        &tester.id,
        "report",
        json!({"message_id":"verified","kind":"passed","body":"Independent checks pass"}),
    )
    .unwrap();
    let refused = host
        .agent_tool(&coordinator.id, "accept_ticket", accept.clone())
        .unwrap_err();
    assert!(refused.to_string().contains("verify_ticket"), "{refused}");
    let db = rusqlite::Connection::open(host.home.join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE tickets SET data=json_set(data,'$.state','ready_for_testing') WHERE id=?1",
        [&ticket.id],
    )
    .unwrap();
    let cycle: Ticket = serde_json::from_value(
        host.agent_tool(&coordinator.id, "verify_ticket", accept.clone())
            .unwrap(),
    )
    .unwrap();
    let verifier = &cycle.verification.unwrap().rounds[0].verifiers[0].session_id;
    take_input(&mut host, verifier);
    host.agent_tool(
        verifier,
        "report",
        json!({"message_id":"verified","kind":"passed","body":"Independent checks pass"}),
    )
    .unwrap();
    host.agent_tool(&coordinator.id, "accept_ticket", accept.clone())
        .unwrap();
    host.agent_tool(
        verifier,
        "report",
        json!({"message_id":"verified","kind":"passed","body":"Independent checks pass"}),
    )
    .unwrap();
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "accepted");
    host.agent_tool(&coordinator.id, "accept_ticket", accept)
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
    let (_home, _repo, mut host, root, repository) = fixture();
    let coordinator = root.clone();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Progress",
            "Report as you go",
        )
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
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "One ticket",
            "Report twice and be listed",
        )
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
    let (_home, _repo, mut host, root, repository) = fixture();
    let ticket = host
        .create_ticket(&root.id, &repository.id, "Ask", "Ask the human")
        .unwrap();
    let worker = implementer(&mut host, &ticket);
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
            &worker.id,
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
    let (_home, _repo, mut host, root, _repository) = fixture();
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
    let (_home, _repo, mut host, root, _repository) = fixture();
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
    let (_home, _repo, mut host, root, repository) = fixture();
    let other = host.create_project("Other").unwrap();
    let other_root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == other.id)
        .unwrap();
    host.set_project_repositories(&other.id, Some(vec![]))
        .unwrap();
    let refused = host
        .agent_tool(
            &other_root.id,
            "create_ticket",
            json!({"repository_id":repository.id,"title":"No","brief":"Not this project's repository"}),
        )
        .unwrap_err();
    assert!(refused.to_string().contains(&repository.name), "{refused}");
    assert!(host.tickets().unwrap().is_empty());
    let ticket = host
        .create_ticket(&root.id, &repository.id, "Mine", "Ours")
        .unwrap();
    let refused = host
        .agent_tool(
            &other_root.id,
            "assign_ticket",
            json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Take over"}),
        )
        .unwrap_err();
    assert!(
        refused.to_string().contains("project coordinator"),
        "{refused}"
    );
    assert!(
        host.agent_tool(
            &root.id,
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
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Routing",
            "Check bounded routing",
        )
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
            &repository.id,
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
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Fix calls",
            "Fix inbound calls",
        )
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
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Review round 1",
            "Review the fix",
        )
        .unwrap();
    let reviewer = host
        .assign_ticket(&ticket.id, Role::Reviewer, Provider::Claude, "Review", None)
        .unwrap();
    let close = json!({"ticket_id":ticket.id});
    assert!(
        host.agent_tool(&reviewer.id, "close_ticket", close.clone())
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

#[test]
fn accepting_archives_the_tickets_agents_and_keeps_their_conversations() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, "Toolbar", "Ship it")
        .unwrap();
    let worker = implementer(&mut host, &ticket);
    let verifier = verified(&mut host, &ticket, &worker);
    let accept = json!({"ticket_id":ticket.id});
    let accepted: Ticket = serde_json::from_value(
        host.agent_tool(&coordinator.id, "accept_ticket", accept.clone())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(accepted.state, "accepted");
    for agent in [&worker, &verifier] {
        assert!(host.session(&agent.id).unwrap().archived);
        assert!(!host.messages(&agent.id, None, 100).unwrap().is_empty());
    }
    assert!(!host.session(&coordinator.id).unwrap().archived);
    assert!(!std::path::Path::new(&ticket.worktree).exists());
    host.agent_tool(&coordinator.id, "accept_ticket", accept)
        .unwrap();
}

#[test]
fn only_the_project_coordinator_manages_tickets() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, "Toolbar", "Ship it")
        .unwrap();
    let worker = implementer(&mut host, &ticket);
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
        .unwrap();
    let reviewer = host
        .assign_ticket(&ticket.id, Role::Reviewer, Provider::Claude, "Review", None)
        .unwrap();
    let before = (
        host.tickets().unwrap().len(),
        host.sessions().unwrap().len(),
    );
    for agent in [&worker, &tester, &reviewer] {
        for (tool, args) in [
            (
                "create_ticket",
                json!({"repository_id":repository.id,"title":"Mine","brief":"No"}),
            ),
            (
                "assign_ticket",
                json!({"ticket_id":ticket.id,"role":"tester","instruction":"Test"}),
            ),
            ("verify_ticket", json!({"ticket_id":ticket.id})),
            ("accept_ticket", json!({"ticket_id":ticket.id})),
            ("close_ticket", json!({"ticket_id":ticket.id})),
        ] {
            assert!(
                host.agent_tool(&agent.id, tool, args).is_err(),
                "{tool} by {}",
                agent.name
            );
        }
    }
    assert_eq!(
        (
            host.tickets().unwrap().len(),
            host.sessions().unwrap().len()
        ),
        before
    );
    assert_eq!(host.ticket(&ticket.id).unwrap().state, "assigned");
    for removed in ["create_repo_coordinator", "archive_team"] {
        assert!(
            host.agent_tool(&coordinator.id, removed, json!({}))
                .is_err()
        );
    }
}

#[test]
fn coordinator_context_lists_tickets_with_repository_agents_and_verification() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, "Toolbar", "Style it")
        .unwrap();
    let worker = implementer(&mut host, &ticket);

    let context = host
        .agent_tool(&coordinator.id, "workspace_context", json!({}))
        .unwrap();
    assert!(context.get("teams").is_none());
    let listed = &context["tickets"][0];
    assert_eq!(listed["id"], ticket.id);
    assert_eq!(listed["repository_id"], repository.id);
    assert_eq!(listed["repository"], repository.name);
    assert_eq!(listed["state"], "assigned");
    assert_eq!(listed["agents"][0]["id"], worker.id);
    assert_eq!(listed["agents"][0]["role"], "implementer");
    assert!(listed.get("verification").is_none());

    verified(&mut host, &ticket, &worker);
    let context = host
        .agent_tool(&coordinator.id, "workspace_context", json!({}))
        .unwrap();
    let verification = &context["tickets"][0]["verification"];
    assert_eq!(verification["outcome"], "passed");
    assert_eq!(
        verification["rounds"][0]["verifiers"][0]["result"],
        "passed"
    );
}

#[test]
fn send_message_tells_the_sender_when_the_recipient_is_mid_turn() {
    let (_home, _repo, mut host, root, repository) = fixture();
    let ticket = host
        .create_ticket(&root.id, &repository.id, "Toolbar", "Style it")
        .unwrap();
    let worker = implementer(&mut host, &ticket);
    let send = |host: &mut Host, id: &str| {
        host.agent_tool(
            &root.id,
            "send_message",
            json!({"recipient":worker.id,"message_id":id,"body":"Next step"}),
        )
        .unwrap()
    };
    let idle = send(&mut host, "idle");
    assert_eq!(idle["receipt"], "queued");
    assert!(idle.get("recipient_turn_started_at").is_none());

    host.set_status(&worker.id, Status::Working).unwrap();
    let mut runtime = host.session_runtime(&worker.id).unwrap();
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
