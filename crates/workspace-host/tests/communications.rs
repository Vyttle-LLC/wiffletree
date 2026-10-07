use serde_json::json;
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
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Implement")
        .unwrap();
    let tester = host
        .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Verify")
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
            "Late assignment"
        )
        .is_err()
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
fn human_dismisses_questions_but_decides_permissions() {
    let (_home, _repo, mut host, root, _coordinator) = fixture();
    let question = host
        .request_attention(&root.id, "local", "question", "Ship it?")
        .unwrap();
    let permission = host
        .request_attention(&root.id, "local", "permission:1", "Run rm?")
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
fn closing_a_finished_review_ticket_archives_its_agents() {
    let (_home, _repo, mut host, root, coordinator) = fixture();
    let ticket = host
        .create_ticket(&coordinator.id, "Review round 1", "Review the fix")
        .unwrap();
    let reviewer = host
        .assign_ticket(&ticket.id, Role::Reviewer, Provider::Claude, "Review")
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
        host.assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Late")
            .is_err()
    );
}
