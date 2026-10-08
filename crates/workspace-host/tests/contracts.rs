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
    for name in [
        "create_ticket",
        "assign_ticket",
        "verify_ticket",
        "accept_ticket",
        "close_ticket",
    ] {
        assert!(names.contains(&name), "{name}");
    }
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
fn bundled_instructions_describe_the_flat_hierarchy() {
    for role in Role::ALL {
        let skill = provider::skill(role).to_lowercase();
        for retired in RETIRED {
            assert!(!skill.contains(retired), "{role:?} mentions {retired}");
        }
    }
    let coordinator = provider::skill(Role::ProjectOrchestrator);
    for tool in [
        "create_ticket",
        "assign_ticket",
        "verify_ticket",
        "accept_ticket",
        "close_ticket",
    ] {
        assert!(coordinator.contains(tool), "{tool}");
    }
}
