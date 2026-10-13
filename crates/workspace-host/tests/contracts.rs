//! The tools and role instructions agents see describe the two-level hierarchy only.
use workspace_core::Role;
use workspace_host::{mcp, provider};

const RETIRED: [&str; 3] = [
    "repository coordinator",
    "create_repo_coordinator",
    "archive_team",
];

#[test]
fn the_tool_list_offers_ticket_tools_and_no_repository_coordinator_tools() {
    let tools = mcp::tools();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    // The whole list, in the order it is offered, so a tool cannot be added unnoticed.
    assert_eq!(
        names,
        [
            "workspace_context",
            "create_ticket",
            "assign_ticket",
            "verify_ticket",
            "send_message",
            "report",
            "set_plan",
            "stop_agents",
            "resume_agent",
            "schedule",
            "unschedule",
            "list_schedules",
            "triage_findings",
            "accept_ticket",
            "close_ticket",
            "archive_agent",
            "ask_user",
            "close_question",
            "request_permission",
        ]
    );
    let schema = |name: &str| {
        tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["inputSchema"]
            .clone()
    };
    assert_eq!(
        schema("report")["properties"]["findings"]["items"]["properties"]["severity"]["enum"],
        serde_json::json!(["blocking", "non_blocking", "pre_existing"])
    );
    assert!(schema("accept_ticket")["properties"]["waived"].is_object());
    let plan = schema("set_plan");
    assert_eq!(plan["required"], serde_json::json!(["items"]));
    let items = &plan["properties"]["items"];
    assert_eq!(items["maxItems"], workspace_core::MAX_PLAN_ITEMS);
    assert_eq!(
        items["items"]["required"],
        serde_json::json!(["text", "done"])
    );
    assert_eq!(
        items["items"]["properties"]["text"]["maxLength"],
        workspace_core::MAX_STEP_TITLE
    );
    assert_eq!(
        schema("triage_findings")["properties"]["decisions"]["items"]["properties"]["decision"]["enum"],
        serde_json::json!(["fix_now", "follow_up", "wont_fix"])
    );
    assert!(
        !names.contains(&"stop_turn"),
        "stop_agents replaces stop_turn"
    );
    let text = tools.to_string().to_lowercase();
    for retired in RETIRED.into_iter().chain(["main coordinator"]) {
        assert!(!text.contains(retired), "{retired}");
    }
    let create = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "create_ticket")
        .unwrap();
    assert_eq!(
        create["inputSchema"]["required"],
        serde_json::json!(["repository_id", "title", "brief"])
    );
}

#[test]
fn no_agent_tool_requests_cancels_or_runs_a_compaction() {
    // Compacting an agent's context is the human's action, on the Context card only.
    let tools = mcp::tools().to_string().to_lowercase();
    assert!(!tools.contains("compact"), "{tools}");
}

#[test]
fn bundled_instructions_describe_the_flat_hierarchy() {
    for role in Role::ALL {
        let skill = provider::skill(role).to_lowercase();
        for retired in RETIRED {
            assert!(!skill.contains(retired), "{role:?} mentions {retired}");
        }
    }
    let coordinator = provider::skill(Role::ProjectOrchestrator);
    assert!(coordinator.contains("Project coordinator — v10"));
    for role in Role::ALL {
        let skill = provider::skill(role);
        assert!(skill.contains("Wiffletree communication contract — v6"));
        // Every role is asked for a checklist, which is display only.
        assert!(skill.contains("record a checklist of the steps you expect with set_plan"));
        assert!(skill.contains("never replaces report"));
        assert!(skill.contains("run of turns without a message from your parent"));
    }
    for tool in [
        "create_ticket",
        "assign_ticket",
        "verify_ticket",
        "triage_findings",
        "accept_ticket",
        "close_ticket",
        "archive_agent",
        "stop_agents",
        "resume_agent",
    ] {
        assert!(coordinator.contains(tool), "{tool}");
    }
    assert!(!coordinator.contains("stop_turn"));
    for phrase in [
        "Never start a fresh cycle to chase them",
        "waived",
        "product, security or data-loss decision",
        "Treat each check-in as a decision",
        "without a message from you",
    ] {
        assert!(coordinator.contains(phrase), "{phrase}");
    }
    assert!(!coordinator.contains("for a fresh cycle with every verifier"));
}

#[test]
fn the_implementer_may_object_to_a_finding_instead_of_implementing_it() {
    let implementer = provider::skill(Role::Implementer);
    assert!(implementer.contains("Implementer — v4"));
    assert!(implementer.contains(
        "If a finding is wrong or out of scope, tell the coordinator in one sentence with evidence"
    ));
    assert!(implementer.contains("smallest change"));
    assert!(!implementer.contains("Do not argue"));
}

#[test]
fn verifiers_report_located_evidenced_findings_without_patches() {
    for role in [Role::Tester, Role::Reviewer] {
        let verifier = provider::skill(role);
        assert!(verifier.contains("Tester / reviewer — v5"));
        for phrase in [
            "severity, `path:line` location",
            "trigger and the evidence",
            "write no patches",
            "A finding is blocking only if it breaks an acceptance criterion",
            "step-by-step trace citing `file:line`",
            "pre_existing",
        ] {
            assert!(verifier.contains(phrase), "{role:?}: {phrase}");
        }
    }
}
