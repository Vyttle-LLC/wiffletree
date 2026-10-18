//! The human's rules for a skill reach a running agent from its next turn, through the real
//! Service with the fake provider standing in for Claude, and the context probe sizes the same
//! instructions. Kept in its own test binary because it sets the provider environment.
mod common;
use common::{git, request, snapshot, until};
use std::path::Path;
use workspace_core::*;
use workspace_host::{Host, context, provider, service::Service};

#[test]
fn a_running_implementer_gets_saved_rules_from_its_next_turn() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let implementer = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Skills").unwrap();
        let main = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let ticket = host
            .create_ticket(&main.id, &repository.id, "Gated", "Work")
            .unwrap();
        // Its first turn waits for the gate, so the rules are saved while it runs.
        let implementer = host
            .assign_ticket(
                &ticket.id,
                Role::Implementer,
                Provider::Claude,
                "JUST_REPLY WAIT_FOR_GATE",
                None,
            )
            .unwrap();
        let profile = ModelProfile {
            provider: Provider::Claude,
            model: "opus".into(),
            effort: "medium".into(),
        };
        host.configure_session(&implementer.id, profile).unwrap();
        host.set_live(&project.id, true).unwrap();
        implementer
    };
    let prompts = scratch.path().join("prompts");
    std::fs::create_dir(&prompts).unwrap();
    // SAFETY: this binary has one test and owns its provider environment.
    let service = unsafe {
        common::ServiceFixture::new(home.path())
            .fake_provider(scratch.path())
            .env("WORKSPACE_TEST_PROMPT_DIR", &prompts)
            .start()
    };
    let log = prompts.join(format!("{}.instructions.txt", implementer.id));
    let turns = || {
        let log = std::fs::read_to_string(&log).unwrap_or_default();
        log.split("\n=====\n")
            .filter(|turn| !turn.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    until("the first turn", || turns().len() == 1);

    let rules = "Write PR descriptions with a Summary and a Testing section";
    request(
        &service,
        Command::SaveSkillRules {
            skill: Skill::Implementing,
            rules: rules.into(),
        },
    );
    std::fs::write(scratch.path().join("gate-turn"), "").unwrap();
    let runtime = |service: &Service| {
        snapshot(service)
            .runtimes
            .into_iter()
            .find(|r| r.session_id == implementer.id)
            .unwrap()
    };
    until("the first turn to finish", || {
        runtime(&service).last_finished_at.is_some()
    });
    request(
        &service,
        Command::Send {
            id: "next".into(),
            sender: None,
            recipient: implementer.id.clone(),
            body: "JUST_REPLY".into(),
            attachments: vec![],
        },
    );
    until("the next turn", || turns().len() == 2);

    let [running, next] = <[String; 2]>::try_from(turns()).unwrap();
    assert_eq!(running, provider::skill(Role::Implementer));
    assert!(next.starts_with(&running));
    assert!(next.ends_with(&format!("## Your own rules for Implementing\nThese are the human's own rules for this skill. Where they disagree with the skill above, follow them.\n{rules}")));

    // The context probe sizes what the turns are sent, as the desktop builds it.
    let snapshot = snapshot(&service);
    let state = snapshot
        .skills
        .iter()
        .find(|s| s.skill == Skill::Implementing);
    let instructions = provider::role_instructions(Role::Implementer, state.unwrap());
    let runtime = runtime(&service);
    let profile = runtime.profile.unwrap();
    context::usage(&context::Probe {
        profile: &profile,
        provider_session: &runtime.provider_session_id.unwrap(),
        directory: Path::new(&runtime.directory.unwrap()),
        instructions: &instructions,
    })
    .unwrap();
    let probed = std::fs::read_to_string(prompts.join("context-instructions.txt")).unwrap();
    assert_eq!(probed, next);
}
