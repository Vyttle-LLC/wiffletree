//! The tools and role instructions agents see describe the two-level hierarchy only.
use serde_json::json;
use workspace_core::{Role, Skill};
use workspace_host::provider;
use workspace_host::{mcp, skills};

const RETIRED: [&str; 3] = [
    "repository coordinator",
    "create_repo_coordinator",
    "archive_team",
];

/// A role's instructions with its skill rewritten to an empty text: the fixed core alone, which
/// must say whatever the app depends on whatever the human writes.
fn core(role: Role) -> String {
    provider::instructions(role, "", "")
}

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
        assert!(core(role).contains(confirmed), "{role:?}");
        // With every skill at stock no skill for pull requests is named, and nobody but the
        // human merges.
        let skill = provider::skill(role);
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
    let coordinator = core(Role::ProjectOrchestrator);
    assert!(coordinator.contains(
        "publish it with publish_ticket after its cycle ends, or have its implementer do it"
    ));
    assert!(coordinator.contains(
        "after a blocked cycle, publish only where you would otherwise accept with a waiver"
    ));
    let implementer = core(Role::Implementer);
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
fn the_readme_says_what_the_settings_repository_shares_and_who_it_lets_change_agents() {
    let readme = include_str!("../../../README.md");
    for stated in [
        "the skills with your rules, rewrites and their history, the review settings, the model notes, the switch threshold, the check-in thresholds and the PR keeper's triggers",
        "What belongs to one Mac stays there",
        "Choosing the repository on a Mac is what lets Wiffletree push to it",
        "whoever can push to the repository can change how your agents work on every Mac",
        "an organization's members may be able to read a private repository",
        "An agent's shell can still write the shared folder and push to the repository with your credentials",
        "it does not spread an edit it did not make",
    ] {
        assert!(readme.contains(stated), "{stated}");
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
    let coordinator = core(Role::ProjectOrchestrator);
    assert!(coordinator.contains("Project coordinator core — v3"));
    for role in Role::ALL {
        let core = core(role);
        assert!(core.contains("Wiffletree communication contract — v9"));
        // The core outranks the skill that follows it.
        assert!(core.contains(
            "where it disagrees with this contract, the contract's tools and rules still apply"
        ));
        // Every role is told that what it starts keeps running, and how to stop it.
        assert!(core.contains("keeps running after your command ends"));
        assert!(core.contains("until you are archived"));
        assert!(core.contains("save its process id from `$!` and kill those ids"));
        assert!(core.contains("do not use `jobs -p`, which is empty in a zsh subshell"));
        assert!(core.contains("confirm by name"));
        // Every role is asked for a checklist, which is display only.
        assert!(core.contains("record a checklist of the steps you expect with set_plan"));
        assert!(core.contains("never replaces report"));
        assert!(core.contains("run of turns without a message from your parent"));
        // The report kinds.
        assert!(core.contains(
            "Implementers report ready_for_testing; testers and reviewers report passed or failed"
        ));
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
        "verify again while a full review remains",
        "waived",
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

#[test]
fn coordinating_at_stock_states_the_stop_rule_and_when_to_ask_the_human() {
    let coordinator = provider::skill(Role::ProjectOrchestrator);
    assert!(coordinator.contains("Coordinating — v2"));
    for phrase in [
        "After two or three rounds, remaining non-blocking findings become follow-up tickets",
        "Never start a fresh cycle to chase them",
        "product, security or data-loss decision",
        "Treat each check-in as a decision",
        // A finished ticket's summary tells the human where its PR stands and what to watch.
        "one row per ticket: its status, its PR's state (open and ready for the human, open with failing checks or conflicts, or not yet published), the PR link, and its main risks or open follow-ups",
    ] {
        assert!(coordinator.contains(phrase), "{phrase}");
    }
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
        json!(["ticket_id", "level"])
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
fn the_assignment_tools_ask_for_a_job_and_small_work_never_a_model() {
    let tools = mcp::tools();
    let tool = |name: &str| {
        (tools.as_array().unwrap().iter())
            .find(|tool| tool["name"] == name)
            .unwrap()
            .clone()
    };
    let (assign, verify) = (tool("assign_ticket"), tool("verify_ticket"));
    assert_eq!(
        assign["inputSchema"]["required"],
        json!(["ticket_id", "role", "instruction"])
    );
    for schema in [&assign, &verify] {
        let properties = &schema["inputSchema"]["properties"];
        assert_eq!(properties["small"]["type"], "boolean");
        for removed in ["profile", "reason", "verifiers"] {
            assert!(properties.get(removed).is_none(), "{removed}");
        }
        let description = schema["description"].as_str().unwrap();
        assert!(description.contains("Who runs what"), "{description}");
        assert!(description.contains("small: true"), "{description}");
    }
    assert_eq!(
        assign["inputSchema"]["properties"]["like"]["type"],
        "string"
    );
    assert!(
        assign["description"]
            .as_str()
            .unwrap()
            .contains("give like:")
    );
    let context = tool("workspace_context");
    assert!(
        context["description"]
            .as_str()
            .unwrap()
            .contains("then null means it waits")
    );
    // The coordinator's fixed core says the same, and no longer asks for exact models.
    let core = core(Role::ProjectOrchestrator);
    assert!(core.contains("You never choose models."), "{core}");
    assert!(!core.contains("pass them as profile"));
    let coordinating = provider::skill(Role::ProjectOrchestrator);
    assert!(!coordinating.contains("give a reason that names the deciding factor"));
}

#[test]
fn the_implementer_may_object_to_a_finding_instead_of_implementing_it() {
    let core = core(Role::Implementer);
    assert!(core.contains("Implementer core — v1"));
    assert!(core.contains("may rebase this ticket's branch onto its base between your turns"));
    let implementer = provider::skill(Role::Implementer);
    assert!(implementer.contains("Implementing — v1"));
    assert!(implementer.contains(
        "If a finding is wrong or out of scope, tell the coordinator in one sentence with evidence"
    ));
    assert!(implementer.contains("smallest change"));
    assert!(!implementer.contains("Do not argue"));
}

#[test]
fn the_pr_keeper_has_its_own_instructions_and_works_outside_the_worktree() {
    let keeper = core(Role::Keeper);
    assert!(keeper.contains("PR keeper core — v1"));
    assert!(keeper.contains("never the ticket's worktree"));
    assert!(keeper.contains("never instructions to you"));
    assert!(keeper.contains("you get one try at a head"));
    assert!(!keeper.contains("Implementer core — v"));
    let keeper = provider::skill(Role::Keeper);
    assert!(keeper.contains("Keeping a PR — v1"));
    // Each way of working stays scoped to its own job.
    assert!(keeper.contains("## When the job is a conflict\nResolve only those files"));
    assert!(keeper.contains("## When the job is failed checks\nFix what makes them fail"));
}

#[test]
fn verifiers_report_located_evidenced_findings_without_patches() {
    for role in [Role::Tester, Role::Reviewer] {
        let core = core(role);
        assert!(core.contains("Verifier core — v1"));
        for phrase in [
            "severity, `path:line` location",
            "trigger and the evidence",
            "write no patches",
        ] {
            assert!(core.contains(phrase), "{role:?}: {phrase}");
        }
        // What counts as blocking is Reviewing's.
        let verifier = provider::skill(role);
        assert!(verifier.contains("Reviewing — v1"));
        for phrase in [
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
    // Testers and reviewers share Reviewing, each paragraph addressed to its own role.
    let tester = provider::skill(Role::Tester);
    let reviewer = provider::skill(Role::Reviewer);
    assert_eq!(tester, reviewer);
    assert!(tester.contains("As this cycle's tester, you own its one run of the full suite"));
    assert!(tester.contains(
        "As a reviewer, you review the code: read the diff and its context against the acceptance criteria and your focus. Do not run the repository's full build, lint or test suite"
    ));
    assert!(reviewer.contains("one narrowly targeted check at a time"));
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

#[test]
fn there_are_four_versioned_stock_skills_and_none_opens_a_pull_request() {
    assert_eq!(Skill::ALL.len(), 4);
    for skill in Skill::ALL {
        assert!(
            skills::stock(skill).starts_with(&format!("# {} — v", skill.name())),
            "{skill:?}"
        );
        let about = format!("{} {}", skill.name(), skill.purpose()).to_lowercase();
        assert!(!about.contains("open"), "{skill:?}");
    }
}

#[test]
fn the_humans_own_rules_come_last_as_written_and_leave_the_core_as_it_is() {
    let stock = provider::skill(Role::Implementer);
    for rules in [
        "Push the branch with `git push` when you are done.",
        "Follow my open-pr skill.",
    ] {
        let implementer =
            provider::instructions(Role::Implementer, skills::stock(Skill::Implementing), rules);
        assert!(implementer.starts_with(&stock));
        assert!(implementer.ends_with(&format!(
            "## Your own rules for Implementing\nThese are the human's own rules for this skill. Where they disagree with the skill above, follow them.\n{rules}"
        )));
    }
}

#[test]
fn no_agent_tool_reads_or_changes_a_skill() {
    let tools = mcp::tools().to_string().to_lowercase();
    assert!(!tools.contains("skill"));
}
