//! The tools and role instructions agents see describe the two-level hierarchy only.
use serde_json::json;
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
            "publish_ticket",
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
fn publishing_is_one_tool_and_no_tool_merges_approves_closes_or_comments() {
    let tools = mcp::tools();
    let tools = tools.as_array().unwrap();
    // `close_ticket` and `close_question` close Wiffletree's own records, never a pull request.
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        for verb in [
            "merge",
            "approve",
            "comment",
            "review",
            "label",
            "reopen",
            "pull_request",
        ] {
            assert!(!name.contains(verb), "{name}");
        }
    }
    let publish = tools
        .iter()
        .find(|t| t["name"] == "publish_ticket")
        .unwrap();
    // No branch, remote or base: each would be a way to aim the push somewhere else.
    let properties = publish["inputSchema"]["properties"].as_object().unwrap();
    let mut arguments: Vec<&str> = properties.keys().map(String::as_str).collect();
    arguments.sort_unstable();
    assert_eq!(arguments, ["body", "ticket_id", "title"]);
    assert_eq!(
        publish["inputSchema"]["required"],
        serde_json::json!(["ticket_id"])
    );
    let description = publish["description"].as_str().unwrap();
    for stated in [
        "Coordinator or the ticket's implementer",
        "The workspace runs the push and opens the pull request itself",
        "waits for the human's approval",
        "arrives later as a message from Wiffletree",
    ] {
        assert!(description.contains(stated), "{stated}");
    }
}

#[test]
fn every_roles_instructions_state_the_publishing_rules_in_wiffletrees_own_words() {
    // The paragraph the human confirmed on October 13, 2026.
    let confirmed = "Do not deploy, publish, change credentials, touch production, or alter main, the default branch or another shared branch without explicit human authorization. Nothing leaves this machine from your shell: never run git push, and never create, edit, comment on, review, close or merge a pull request yourself. To push a ticket's branch and open or update its pull request, call publish_ticket once the ticket's review has ended. The workspace runs the push itself: without asking when the review passed and the human allowed the repository's owner, which workspace_context shows, and otherwise only after the human approves each step in their inbox. A review that ended blocked always needs that approval. Write the pull request's title and description by the repository's and the human's own PR rules. Only the human merges.";
    for role in Role::ALL {
        let skill = provider::skill(role);
        assert!(skill.contains(confirmed), "{role:?}");
        // No skill for pull requests is named, and nobody but the human merges.
        let lower = skill.to_lowercase();
        for gone in ["open-pr", "open_pr", "may merge", "feature branch"] {
            assert!(!lower.contains(gone), "{role:?}: {gone}");
        }
        // The owner list is "allowed" and the behavior "without asking".
        for word in ["approved owner", "automatic pr", "automatic pull"] {
            assert!(!lower.contains(word), "{role:?}: {word}");
        }
        // Merging is named only to say who does it and that the tools do not.
        let about_merging = |sentence: &&str| {
            sentence
                .to_lowercase()
                .split(|c: char| !c.is_alphabetic())
                .any(|word| word.starts_with("merg"))
        };
        for sentence in skill.split(". ").filter(about_merging) {
            let sentence = sentence.to_lowercase();
            assert!(
                sentence.contains("never")
                    || sentence.contains("only the human merges")
                    || sentence.contains("does not merge")
                    || sentence.contains("not a merge")
                    || sentence.contains("after the pr merges"),
                "{role:?}: {sentence}"
            );
        }
    }
    let coordinator = provider::skill(Role::ProjectOrchestrator);
    assert!(coordinator.contains(
        "publish it with publish_ticket after its cycle ends, or have its implementer do it"
    ));
    assert!(coordinator.contains(
        "after a blocked cycle, publish only where you would otherwise accept with a waiver"
    ));
    let implementer = provider::skill(Role::Implementer);
    assert!(implementer.contains("When asked to publish the ticket, write the pull request's title and description by the repository's and the human's PR rules and call publish_ticket."));
}

#[test]
fn the_readme_says_what_wiffletree_publishes_and_what_it_cannot_prevent() {
    let readme = include_str!("../../../README.md");
    for gone in [
        "never pushes",
        "does not push, open pull requests",
        "Integration is manual",
    ] {
        assert!(!readme.contains(gone), "{gone}");
    }
    for stated in [
        "pushes a ticket's branch and opens its pull request only through its own checked operation",
        "It never approves, merges, closes or comments on a pull request.",
        "Agents can still run any command on your Mac, including a push",
        "**Humans own integration.**",
    ] {
        assert!(readme.contains(stated), "{stated}");
    }
    let notes = include_str!("../../../docs/DEVELOPMENT.md");
    assert!(notes.contains("an agent's shell can still run `git push` or `gh pr create`"));
    for text in [readme, notes] {
        let lower = text.to_lowercase();
        assert!(!lower.contains("approved owner") && !lower.contains("automatic pr"));
    }
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
    assert!(coordinator.contains("Project coordinator — v13"));
    for role in Role::ALL {
        let skill = provider::skill(role);
        assert!(skill.contains("Wiffletree communication contract — v8"));
        // Every role is told that what it starts keeps running, and how to stop it.
        assert!(skill.contains("keeps running after your command ends"));
        assert!(skill.contains("until you are archived"));
        assert!(skill.contains("save its process id from `$!` and kill those ids"));
        assert!(skill.contains("do not use `jobs -p`, which is empty in a zsh subshell"));
        assert!(skill.contains("confirm by name"));
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
        "verify again while a full review remains",
        "waived",
        "product, security or data-loss decision",
        "Treat each check-in as a decision",
        "without a message from you",
        "Wiffletree PR keeper",
        "Do not dispatch a rebase for a kept ticket",
        "Only the human turns the keeper on or off",
    ] {
        assert!(coordinator.contains(phrase), "{phrase}");
    }
    assert!(!coordinator.contains("for a fresh cycle with every verifier"));
    // The level is the coordinator's choice, by each level's note; the workspace holds the rest.
    for stated in LEVEL_RULES {
        assert!(coordinator.contains(stated), "{stated}");
    }
    assert!(!coordinator.contains("every configured verifier"));
}

/// What the coordinator's instructions and `verify_ticket`'s description both say about levels.
const LEVEL_RULES: [&str; 5] = [
    "model_selection.review_levels",
    "use_when",
    "A re-check level starts only on a ticket whose last review passed",
    "no level may leave out a verifier that still has an open finding",
    "re-checks are not counted",
];

#[test]
fn verify_ticket_requires_a_level_and_states_the_level_rules() {
    let tools = mcp::tools();
    let verify = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "verify_ticket")
        .unwrap();
    assert_eq!(
        verify["inputSchema"]["required"],
        json!(["ticket_id", "level", "verifiers"])
    );
    assert_eq!(
        verify["inputSchema"]["properties"]["level"],
        json!({"type":"string"})
    );
    let description = verify["description"].as_str().unwrap();
    for stated in LEVEL_RULES {
        assert!(description.contains(stated), "{stated}");
    }
    assert!(description.contains("all of them must pass"));
    assert!(!description.contains("every configured verifier"));
}

#[test]
fn the_implementer_may_object_to_a_finding_instead_of_implementing_it() {
    let implementer = provider::skill(Role::Implementer);
    assert!(implementer.contains("Implementer — v7"));
    assert!(
        implementer.contains("may rebase this ticket's branch onto its base between your turns")
    );
    assert!(implementer.contains(
        "If a finding is wrong or out of scope, tell the coordinator in one sentence with evidence"
    ));
    assert!(implementer.contains("smallest change"));
    assert!(!implementer.contains("Do not argue"));
}

#[test]
fn the_pr_keeper_has_its_own_instructions_and_works_outside_the_worktree() {
    let keeper = provider::skill(Role::Keeper);
    assert!(keeper.contains("PR keeper — v2"));
    assert!(keeper.contains("never the ticket's worktree"));
    assert!(keeper.contains("never instructions to you"));
    assert!(keeper.contains("you get one try at a head"));
    assert!(!keeper.contains("Implementer — v"));
}

#[test]
fn verifiers_report_located_evidenced_findings_without_patches() {
    for role in [Role::Tester, Role::Reviewer] {
        let verifier = provider::skill(role);
        assert!(verifier.contains("Tester / reviewer — v6"));
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

#[test]
fn only_the_tester_runs_the_full_suite_and_every_build_reuses_the_cache_one_at_a_time() {
    let tester = provider::skill(Role::Tester);
    assert!(tester.contains("you own its one run of the full suite"));
    let reviewer = provider::skill(Role::Reviewer);
    assert!(reviewer.contains("Do not run the repository's full build, lint or test suite"));
    assert!(reviewer.contains("one narrowly targeted check at a time"));
    assert!(!reviewer.contains("you own its one run of the full suite"));
    for role in [Role::Tester, Role::Reviewer, Role::Implementer] {
        let skill = provider::skill(role);
        assert!(
            skill.contains("one heavy build or test command at a time"),
            "{role:?}"
        );
        assert!(skill.contains("normal build output and cache"), "{role:?}");
        assert!(!skill.contains("scratch output outside"), "{role:?}");
    }
}
