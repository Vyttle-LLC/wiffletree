//! A coordinator stops and resumes its own children through the real Service, with the fake
//! provider standing in for the CLIs: stop_agents keeps a child paused while input queues,
//! resume_agent releases it, and a human stop wakes the parent, which may then retry the held
//! input. Kept in its own test binary because it sets the provider environment for the process.
mod common;
use common::{git, request, snapshot, until};
use serde_json::Value;
use workspace_core::*;
use workspace_host::{Host, service::Service};

fn messages(service: &Service, session: &str) -> Vec<Message> {
    serde_json::from_value(request(
        service,
        Command::Messages {
            session_id: session.into(),
            before: None,
            limit: 100,
        },
    ))
    .unwrap()
}

fn status(service: &Service, session: &str) -> Status {
    snapshot(service)
        .sessions
        .into_iter()
        .find(|s| s.id == session)
        .unwrap()
        .status
}

fn runtime(service: &Service, session: &str) -> SessionRuntime {
    snapshot(service)
        .runtimes
        .into_iter()
        .find(|r| r.session_id == session)
        .unwrap()
}

/// Sends the coordinator a command and returns the tool result its reply carries.
fn control(service: &Service, main: &str, command: &str) -> Value {
    let replies = |service: &Service| {
        messages(service, main)
            .into_iter()
            .filter(|m| m.body.starts_with("CONTROL "))
            .count()
    };
    let before = replies(service);
    request(
        service,
        Command::Send {
            id: format!("command-{before}"),
            sender: None,
            recipient: main.into(),
            body: command.into(),
            attachments: vec![],
        },
    );
    until(command, || replies(service) > before);
    let reply = messages(service, main)
        .into_iter()
        .rfind(|m| m.body.starts_with("CONTROL "))
        .unwrap();
    serde_json::from_str(&reply.body["CONTROL ".len()..]).unwrap()
}

#[test]
fn a_coordinator_stops_and_resumes_its_children_and_hears_of_a_human_stop() {
    let home = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Initial"]);
    let (main, child) = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.set_workspaces_dir(scratch.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Control").unwrap();
        let main = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let ticket = host
            .create_ticket(&main.id, &repository.id, "Slow", "Work slowly")
            .unwrap();
        // Its instruction is a turn that runs until something stops it.
        let child = host
            .assign_ticket(
                &ticket.id,
                Role::Implementer,
                Provider::Claude,
                "JUST_REPLY HANG_UNTIL_CANCELLED",
                None,
            )
            .unwrap();
        let profile = ModelProfile {
            provider: Provider::Claude,
            model: "opus".into(),
            effort: "medium".into(),
        };
        host.configure_session(&child.id, profile).unwrap();
        host.set_live(&project.id, true).unwrap();
        (main, child)
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
    let working = |service: &Service| status(service, &child.id) == Status::Working;
    until("the child's long turn", || working(&service));

    // Stopping every child stops the running turn and keeps the child paused.
    let stopped = control(&service, &main.id, "STOP_CHILDREN");
    assert_eq!(stopped["agents"][0]["session_id"], child.id.as_str());
    assert_eq!(stopped["agents"][0]["result"], "stopped");
    assert_eq!(status(&service, &child.id), Status::Paused);
    // The child is paused at once, but resume_agent refuses it until its provider has gone.
    until("the stopped turn to finish", || {
        runtime(&service, &child.id).last_finished_at.is_some()
    });
    let stopped = runtime(&service, &child.id);

    // A message to the stopped child queues with a hint, and starts no turn.
    let sent = control(&service, &main.id, "MESSAGE_CHILD");
    assert_eq!(sent["receipt"], "queued_while_held");
    assert!(
        sent["hint"].as_str().unwrap().contains("resume_agent"),
        "{sent}"
    );
    let queued = snapshot(&service)
        .undelivered
        .into_iter()
        .find(|u| u.session_id == child.id)
        .unwrap();
    assert_eq!((queued.held, queued.queued), (0, 1));
    assert_eq!(status(&service, &child.id), Status::Paused);
    assert_eq!(
        runtime(&service, &child.id).last_started_at,
        stopped.last_started_at
    );

    // Resuming with a message starts a turn with both inputs, so the first was still queued.
    let resumed = control(&service, &main.id, "RESUME_CHILD WITH_MESSAGE");
    assert_eq!(resumed["queued"], 2);
    until("the resumed turn", || {
        runtime(&service, &child.id).last_finished_at > stopped.last_finished_at
            && status(&service, &child.id) == Status::Ready
    });
    let log = std::fs::read_to_string(prompts.join(format!("{}.txt", child.id))).unwrap();
    // The stopped turn's provider logs its prompt only once it is running, and the stop may
    // come first, so read the latest prompt.
    let resumed_turn = log.rsplit("=====").nth(1).unwrap();
    for expected in [
        "Your parent stopped your previous turn",
        "queued while stopped",
        "carry on",
    ] {
        assert!(
            resumed_turn.contains(expected),
            "{expected}\n{resumed_turn}"
        );
    }

    // A human Pause holds the input and tells the parent, naming the child.
    let resumed_at = runtime(&service, &child.id).last_finished_at;
    request(
        &service,
        Command::Send {
            id: "slow-again".into(),
            sender: None,
            recipient: child.id.clone(),
            body: "JUST_REPLY HANG_UNTIL_CANCELLED".into(),
            attachments: vec![],
        },
    );
    until("the second long turn", || working(&service));
    request(
        &service,
        Command::SetStatus {
            session_id: child.id.clone(),
            status: Status::Paused,
        },
    );
    until("the parent to hear of the stop", || {
        messages(&service, &main.id).iter().any(|m| {
            m.body
                .starts_with(&format!("{} was stopped by the human", child.name))
        })
    });
    assert_eq!(status(&service, &child.id), Status::Paused);
    // The parent hears at once, before the paused turn's provider has gone.
    until("the paused turn to finish", || {
        runtime(&service, &child.id).last_finished_at > resumed_at
    });

    // Retrying redelivers the held input with a note that its turn was interrupted.
    let paused_at = runtime(&service, &child.id).last_finished_at;
    let retried = control(&service, &main.id, "RESUME_CHILD HELD_RETRY");
    assert_eq!(retried["queued"], 1);
    until("the retried turn", || {
        runtime(&service, &child.id).last_finished_at > paused_at
            && status(&service, &child.id) == Status::Ready
    });
    let log = std::fs::read_to_string(prompts.join(format!("{}.txt", child.id))).unwrap();
    let retried_turn = log.rsplit("=====").nth(1).unwrap();
    assert!(
        retried_turn.contains("Retry of interrupted turn"),
        "{retried_turn}"
    );
}
