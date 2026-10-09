mod common;
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

/// Reports the implementer ready, verifies with a single tester and passes it.
fn verified(host: &mut Host, ticket: &Ticket, implementer: &Session) -> Session {
    common::one_tester(host);
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
            common::verify_args(host, &ticket.id),
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

fn claude(model: &str, effort: &str) -> ModelProfile {
    ModelProfile {
        provider: Provider::Claude,
        model: model.into(),
        effort: effort.into(),
    }
}
fn codex(model: &str, effort: &str) -> ModelProfile {
    ModelProfile {
        provider: Provider::Codex,
        model: model.into(),
        effort: effort.into(),
    }
}
fn access(models: &[(&str, &str)]) -> ProviderAccess {
    ProviderAccess {
        enabled: true,
        models: models
            .iter()
            .map(|(model, effort)| AllowedModel {
                model: (*model).into(),
                effort: (*effort).into(),
            })
            .collect(),
    }
}
/// Both subscriptions, with Claude for every role.
fn configure(host: &mut Host) -> ModelSelection {
    let mut selection = host.model_selection().unwrap();
    selection.providers = [
        (
            Provider::Claude,
            access(&[("opus", "high"), ("opus", "medium"), ("sonnet", "high")]),
        ),
        (
            Provider::Codex,
            access(&[("gpt-6.1-sol", "high"), ("gpt-6.1-sol", "medium")]),
        ),
    ]
    .into();
    selection.role_providers = PROVIDER_ROLES
        .into_iter()
        .map(|role| (role, [Provider::Claude].into()))
        .collect();
    host.set_model_selection(&selection).unwrap()
}
fn events(host: &Host, project: &str, kind: &str) -> Vec<String> {
    host.activity(project, None, 100)
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == kind)
        .map(|a| a.detail)
        .collect()
}
#[test]
fn every_coordinator_assignment_records_its_exact_model_and_reason() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let configured = configure(&mut host);
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Add --json",
            "Add a --json flag",
        )
        .unwrap();
    let reason = "Small, contained CLI flag; Sonnet is enough";
    let result = host
        .agent_tool(
            &coordinator.id,
            "assign_ticket",
            json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Implement",
                "profile":claude("sonnet","high"),"reason":format!("  {reason} ")}),
        )
        .unwrap();
    assert_eq!(result["profile"], json!(claude("sonnet", "high")));
    assert_eq!(result["selection"]["reason"], reason);
    let worker: Session = serde_json::from_value(result).unwrap();
    assert_eq!(worker.provider, Provider::Claude);
    let runtime = host.session_runtime(&worker.id).unwrap();
    assert_eq!(runtime.profile, Some(claude("sonnet", "high")));
    let selection = runtime.selection.unwrap();
    assert_eq!(
        selection.chosen_by,
        Chooser::Coordinator {
            session_id: coordinator.id.clone()
        }
    );
    assert_eq!(selection.reason, reason);
    assert_eq!(selection.revision, configured.revision);
    assert_eq!(
        events(&host, &coordinator.project_id, "model_selected"),
        [format!("claude · sonnet · high. {reason}")]
    );
    // Repeating the assignment returns the same agent without switching its model.
    let retry: Session = serde_json::from_value(
        host.agent_tool(
            &coordinator.id,
            "assign_ticket",
            json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Implement",
                "profile":claude("opus","high"),"reason":"Bigger"}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(retry.id, worker.id);
    assert_eq!(
        host.session_runtime(&worker.id).unwrap().profile,
        Some(claude("sonnet", "high"))
    );
}

#[test]
fn off_allowlist_and_wrong_provider_choices_are_refused_without_fallback() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    configure(&mut host);
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Refusals",
            "Nothing should be created",
        )
        .unwrap();
    let sessions = host.sessions().unwrap().len();
    let runtimes = host.runtimes().unwrap().len();
    let assign = |extra: Value| {
        let mut args = json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Implement",
            "profile":claude("sonnet","high"),"reason":"Small change"});
        for (key, value) in extra.as_object().unwrap() {
            if value.is_null() {
                args.as_object_mut().unwrap().remove(key);
            } else {
                args[key] = value.clone();
            }
        }
        args
    };
    let cases = [
        (
            assign(json!({"profile":codex("gpt-6.1-sol","high")})),
            "Codex is not configured for Implementer on this machine. Implementer uses: Claude.",
        ),
        (
            assign(json!({"profile":claude("opus","max")})),
            "claude · opus · max is not allowed on this machine. Allowed Claude models: opus · high, opus · medium, sonnet · high.",
        ),
        (assign(json!({"size":"big"})), "`size` was removed"),
        (
            assign(json!({"complexity":"complex"})),
            "`complexity` was removed",
        ),
        (
            assign(json!({"provider":"claude"})),
            "`provider` was removed",
        ),
        (assign(json!({"reason":null})), "Missing reason"),
        (assign(json!({"reason":"one\ntwo"})), "one line"),
        (assign(json!({"profile":null})), "Missing profile"),
    ];
    for (args, message) in cases {
        let error = host
            .agent_tool(&coordinator.id, "assign_ticket", args)
            .unwrap_err();
        assert!(format!("{error:#}").contains(message), "{error:#}");
    }
    assert_eq!(host.sessions().unwrap().len(), sessions, "nothing created");
    assert_eq!(host.runtimes().unwrap().len(), runtimes, "nothing pinned");
    let rejected = events(&host, &coordinator.project_id, "model_rejected");
    assert_eq!(rejected.len(), 2, "{rejected:?}");
    assert!(
        rejected
            .iter()
            .all(|d| d.ends_with("No agent was created."))
    );
}

#[test]
fn reviewers_may_use_any_enabled_provider_but_only_allowed_models() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let mut selection = configure(&mut host);
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Review",
            "Review the change",
        )
        .unwrap();
    let review = |profile: ModelProfile, focus: &str| {
        json!({"ticket_id":ticket.id,"role":"reviewer","focus":focus,
            "instruction":"Run reviso:style on this ticket's branch and report findings.",
            "profile":profile,"reason":"Custom review from the review instructions"})
    };
    let custom: Session = serde_json::from_value(
        host.agent_tool(
            &coordinator.id,
            "assign_ticket",
            review(codex("gpt-6.1-sol", "medium"), "Codex · reviso:style"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(custom.provider, Provider::Codex);
    assert!(
        host.agent_tool(
            &coordinator.id,
            "assign_ticket",
            review(codex("gpt-6-luna", "medium"), "Luna")
        )
        .is_err()
    );
    selection
        .providers
        .get_mut(&Provider::Codex)
        .unwrap()
        .enabled = false;
    host.set_model_selection(&selection).unwrap();
    let error = host
        .agent_tool(
            &coordinator.id,
            "assign_ticket",
            review(codex("gpt-6.1-sol", "high"), "Codex"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("Codex is disabled"), "{error}");
}

#[test]
fn the_humans_choice_is_never_limited() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    configure(&mut host);
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, "Human", "The human picks")
        .unwrap();
    let assign = |host: &mut Host, role: Role, profile: ModelProfile| {
        let response = host.respond(Request {
            version: PROTOCOL_VERSION,
            id: new_id(),
            command: Command::AssignTicket {
                ticket_id: ticket.id.clone(),
                role,
                profile,
                instruction: "Go".into(),
                focus: None,
            },
        });
        assert_eq!(response.error, None);
        let session: Session = serde_json::from_value(response.result.unwrap()).unwrap();
        host.session_runtime(&session.id).unwrap()
    };
    // Codex is not configured for testers and Luna is not allowed at all.
    let tester = assign(&mut host, Role::Tester, codex("gpt-6-luna", "medium"));
    assert_eq!(tester.profile, Some(codex("gpt-6-luna", "medium")));
    assert_eq!(tester.selection.unwrap().chosen_by, Chooser::Human);
    let prefill = host
        .model_selection()
        .unwrap()
        .prefill(Role::Implementer)
        .unwrap();
    let implementer = assign(&mut host, Role::Implementer, prefill);
    let selection = implementer.selection.unwrap();
    assert_eq!(selection.chosen_by, Chooser::Default);
    assert_eq!(selection.reason, "Role default");
    // Re-assigning an existing agent never replaces a pinned model, but gives an unpinned one,
    // such as a pre-upgrade provider mismatch, the human's pick.
    let reassign = |host: &mut Host, focus: &str, profile: ModelProfile| {
        let response = host.respond(Request {
            version: PROTOCOL_VERSION,
            id: new_id(),
            command: Command::AssignTicket {
                ticket_id: ticket.id.clone(),
                role: Role::Reviewer,
                profile,
                instruction: "Go".into(),
                focus: Some(focus.into()),
            },
        });
        assert_eq!(response.error, None);
    };
    let pinned = host
        .assign_ticket(
            &ticket.id,
            Role::Reviewer,
            Provider::Claude,
            "Review",
            Some("Pinned"),
        )
        .unwrap();
    host.configure_session(&pinned.id, claude("sonnet", "high"))
        .unwrap();
    reassign(&mut host, "Pinned", claude("opus", "high"));
    assert_eq!(
        host.session_runtime(&pinned.id).unwrap().profile,
        Some(claude("sonnet", "high"))
    );
    let unpinned = host
        .assign_ticket(
            &ticket.id,
            Role::Reviewer,
            Provider::Claude,
            "Review",
            Some("Old"),
        )
        .unwrap();
    reassign(&mut host, "Old", claude("opus", "medium"));
    let runtime = host.session_runtime(&unpinned.id).unwrap();
    assert_eq!(runtime.profile, Some(claude("opus", "medium")));
    assert_eq!(runtime.selection.unwrap().chosen_by, Chooser::Human);
}

#[test]
fn a_closed_ticket_takes_no_coordinator_assignment_even_for_an_existing_agent() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    configure(&mut host);
    let ticket = host
        .create_ticket(&coordinator.id, &repository.id, "Closed", "Finished work")
        .unwrap();
    let worker = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    host.agent_tool(
        &worker.id,
        "report",
        json!({"message_id":"done","kind":"completed","body":"Done"}),
    )
    .unwrap();
    host.close_ticket(&ticket.id).unwrap();
    let error = host
        .agent_tool(
            &coordinator.id,
            "assign_ticket",
            json!({"ticket_id":ticket.id,"role":"implementer","instruction":"Again",
                "profile":claude("sonnet","high"),"reason":"Small change"}),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("Ticket is already closed"),
        "{error}"
    );
}

#[test]
fn the_coordinator_receives_the_model_configuration_and_workers_do_not() {
    let (_home, _repo, mut host, coordinator, repository) = fixture();
    let mut selection = configure(&mut host);
    let context = host.agent_context(&coordinator.id).unwrap();
    let model = &context["model_selection"];
    assert_eq!(model["revision"], selection.revision);
    assert_eq!(model["providers"].as_array().unwrap().len(), 2);
    assert_eq!(
        model["verifiers"],
        json!(host.settings().verification.verifiers)
    );
    selection.guide = "Raise effort for migrations.".into();
    selection.role_providers.insert(
        Role::Implementer,
        [Provider::Claude, Provider::Codex].into(),
    );
    let saved = host.set_model_selection(&selection).unwrap();
    let model = host.agent_context(&coordinator.id).unwrap()["model_selection"].clone();
    assert_eq!(model["revision"], saved.revision);
    assert_eq!(model["guide"], "Raise effort for migrations.");
    assert_eq!(
        model["role_providers"]["implementer"],
        json!(["codex", "claude"])
    );
    let ticket = host
        .create_ticket(
            &coordinator.id,
            &repository.id,
            "Worker",
            "No guide for workers",
        )
        .unwrap();
    let worker = host
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    assert!(
        host.agent_context(&worker.id)
            .unwrap()
            .get("model_selection")
            .is_none()
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
    common::one_tester(&mut host);
    let cycle: Ticket = serde_json::from_value(
        host.agent_tool(
            &coordinator.id,
            "verify_ticket",
            common::verify_args(&host, &ticket.id),
        )
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
            ("verify_ticket", common::verify_args(&host, &ticket.id)),
            (
                "triage_findings",
                json!({"ticket_id":ticket.id,"decisions":[]}),
            ),
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
