//! Assistant threads: sessions a Space owns, run restricted in their scratch folders.
mod common;
use common::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use workspace_core::assistant::{CardKind, ThreadTab};
use workspace_core::*;
use workspace_host::Host;

/// Every test here sets the provider through the process environment.
static ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    _directory: tempfile::TempDir,
    home: PathBuf,
    turns: PathBuf,
    sage: SpaceId,
}
/// A store with a Sage Space that has instructions, and a folder for the double's log.
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let turns = directory.path().join("turns");
    std::fs::create_dir_all(&turns).unwrap();
    let mut host = Host::open_set_up(&home).unwrap();
    let sage = host
        .create_space("Sage", "Sage work", "Treat production as read-only.", None)
        .unwrap()
        .space
        .id;
    drop(host);
    Fixture {
        _directory: directory,
        home: std::fs::canonicalize(home).unwrap(),
        turns,
        sage,
    }
}
fn service(fixture: &Fixture) -> workspace_host::service::Service {
    let provider = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thread_provider.py");
    // SAFETY: the environment lock is held for the whole test.
    unsafe {
        ServiceFixture::new(&fixture.home)
            .provider(&provider)
            .env("WORKSPACE_TEST_THREAD_DIR", &fixture.turns)
            .start()
    }
}
fn turns(fixture: &Fixture) -> Vec<Value> {
    let log = std::fs::read_to_string(fixture.turns.join("turns.jsonl")).unwrap_or_default();
    log.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn argument<'a>(turn: &'a Value, flag: &str) -> &'a str {
    let args = turn["args"].as_array().unwrap();
    let at = args
        .iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("no {flag}"));
    args[at + 1].as_str().unwrap()
}
fn tab(snapshot: &Snapshot, id: &str) -> Option<ThreadTab> {
    snapshot.threads.iter().find(|t| t.thread.id == id).cloned()
}
fn start(service: &workspace_host::service::Service, id: &str, space: &SpaceId, body: &str) {
    request(
        service,
        Command::StartThread {
            id: id.into(),
            space_id: space.clone(),
            body: body.into(),
            attachments: vec![],
            profile: None,
        },
    );
}
fn replies(service: &workspace_host::service::Service, id: &str) -> Vec<Message> {
    let page = request(
        service,
        Command::Messages {
            session_id: id.into(),
            before: None,
            limit: 50,
        },
    );
    serde_json::from_value(page).unwrap()
}
/// Waits until the thread's latest turn has ended, `turns` in all.
fn after_turns(service: &workspace_host::service::Service, id: &str, turns: usize) -> Snapshot {
    until_changed(service, Instant::now() + Duration::from_secs(60), |s| {
        let ended = tab(s, id).is_some_and(|t| t.status != Status::Working);
        ended
            && replies(service, id)
                .iter()
                .filter(|m| m.sender.is_some())
                .count()
                >= turns
    })
}

#[test]
fn a_thread_runs_restricted_in_its_scratch_folder_and_saves_outputs_there_only() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = fixture();
    let service = service(&fixture);
    start(
        &service,
        "t1",
        &fixture.sage,
        "save costs.html as AWS costs",
    );
    let snapshot = after_turns(&service, "t1", 1);
    let scratch = fixture.home.join("assistant/threads/t1");

    // Started on its Space's sources only, never in YOLO mode, in its scratch folder.
    let turn = &turns(&fixture)[0];
    let args: Vec<&str> = (turn["args"].as_array().unwrap().iter())
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(argument(turn, "--permission-mode"), "dontAsk");
    assert_eq!(argument(turn, "--setting-sources"), "");
    assert!(!args.iter().any(|a| a.contains("dangerously")), "{args:?}");
    assert!(!argument(turn, "--tools").contains("Bash"));
    let allowed = argument(turn, "--allowedTools");
    assert!(
        allowed.contains(&format!("Write(/{}/**)", scratch.display())),
        "{allowed}"
    );
    assert!(!allowed.contains("Bash") && !allowed.contains("create_ticket"));
    assert_eq!(Path::new(turn["cwd"].as_str().unwrap()), scratch);
    // Offered only the thread tools; the prompt is the message alone.
    assert_eq!(
        turn["tools"],
        serde_json::json!(["save_output", "suggest_space", "project_brief"])
    );
    assert_eq!(turn["prompt"], "save costs.html as AWS costs");
    // The contract, the Assistant core, Assisting and then the Space's instructions.
    let instructions = argument(turn, "--append-system-prompt");
    let order = [
        "# Wiffletree thread contract",
        "# Assistant core",
        "# Assisting",
        "## Instructions from the Sage Space",
        "Treat production as read-only.",
    ];
    let at: Vec<usize> = order
        .iter()
        .map(|part| instructions.find(part).unwrap())
        .collect();
    assert!(at.is_sorted(), "{at:?}");
    assert!(!instructions.contains("report kind blocked"));

    // The output is in this Mac's board store, and the thread shows it was saved.
    let board = fixture
        .home
        .join("boards")
        .join(fixture.sage.as_str())
        .join("aws-costs");
    assert_eq!(
        std::fs::read_to_string(board.join("costs.html")).unwrap(),
        "# AWS costs\n"
    );
    let card = tab(&snapshot, "t1").unwrap().thread.cards.pop().unwrap();
    assert!(
        matches!(card.kind, CardKind::SavedOutput { version: 1, .. }),
        "{card:?}"
    );
    assert!(
        replies(&service, "t1")
            .last()
            .unwrap()
            .body
            .contains("Only on this Mac")
    );

    // The next message resumes the same provider session; saving again adds a version.
    request(
        &service,
        Command::Send {
            id: "second".into(),
            sender: None,
            recipient: "t1".into(),
            body: "save costs.html as AWS costs".into(),
            attachments: vec![],
            uploads: vec![],
        },
    );
    let snapshot = after_turns(&service, "t1", 2);
    assert!(
        turns(&fixture)[1]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "--resume")
    );
    let card = tab(&snapshot, "t1").unwrap().thread.cards.pop().unwrap();
    assert!(
        matches!(card.kind, CardKind::SavedOutput { version: 2, .. }),
        "{card:?}"
    );
    // No project, session or agent lists the thread.
    assert!(snapshot.sessions.iter().all(|s| s.id != "t1"));
    assert!(snapshot.runtimes.iter().all(|r| r.session_id != "t1"));
}

#[test]
fn a_codex_call_to_a_server_or_tool_it_was_not_given_stops_the_run_and_says_so() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = fixture();
    let service = service(&fixture);
    let codex = ModelProfile {
        provider: Provider::Codex,
        model: "gpt-6.1-sol".into(),
        effort: "medium".into(),
    };
    let calls = [
        ("c1", "call agent_workspace save_output", None),
        (
            "c2",
            "call github list_issues",
            Some("an MCP server its Space's sources do not list (github)"),
        ),
        (
            "c3",
            "call agent_workspace create_ticket",
            Some("a tool it was not given (agent_workspace create_ticket)"),
        ),
    ];
    for (id, body, _) in calls {
        let start = Command::StartThread {
            id: id.into(),
            space_id: fixture.sage.clone(),
            body: body.into(),
            attachments: vec![],
            profile: Some(codex.clone()),
        };
        request(&service, start);
    }
    until_changed(&service, Instant::now() + Duration::from_secs(60), |s| {
        calls.iter().all(|(id, _, _)| {
            tab(s, id).is_some_and(|t| !matches!(t.status, Status::Working | Status::Ready))
                || replies(&service, id).iter().any(|m| m.sender.is_some())
        })
    });
    for (id, _, stopped) in calls {
        let shown = replies(&service, id);
        match stopped {
            Some(why) => assert!(
                shown.iter().any(|m| m.body.contains(why)),
                "{id}: {shown:?}"
            ),
            None => assert!(
                shown.iter().any(|m| m.body == "Called it"),
                "{id}: {shown:?}"
            ),
        }
    }
}

#[test]
fn a_run_offered_an_unlisted_mcp_server_is_stopped_and_says_so() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = fixture();
    let service = service(&fixture);
    start(&service, "t2", &fixture.sage, "offer github");
    let snapshot = until_changed(&service, Instant::now() + Duration::from_secs(60), |s| {
        tab(s, "t2").is_some_and(|t| t.status == Status::Disconnected)
    });
    assert!(tab(&snapshot, "t2").unwrap().thread.cards.is_empty());
    let shown = replies(&service, "t2");
    assert!(
        shown.iter().any(|m| m
            .body
            .contains("an MCP server its Space's sources do not list (github)")),
        "{shown:?}"
    );
}

fn route(service: &workspace_host::service::Service, id: &str, from: Option<&str>, body: &str) {
    request(
        service,
        Command::RouteMessage {
            id: id.into(),
            thread_id: from.map(str::to_owned),
            body: body.into(),
        },
    );
}
fn router_runs(fixture: &Fixture) -> Vec<Value> {
    let log = std::fs::read_to_string(fixture.turns.join("router.jsonl")).unwrap_or_default();
    log.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn the_router_sends_each_message_where_it_answers_and_holds_it_when_unsure() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = fixture();
    let service = service(&fixture);
    start(&service, "t1", &fixture.sage, "release notes");
    after_turns(&service, "t1", 1);
    let deadline = || Instant::now() + Duration::from_secs(60);

    // A clear continuation goes to the current thread.
    route(
        &service,
        "m1",
        Some("t1"),
        "the same thread, and the watch app?",
    );
    after_turns(&service, "t1", 2);
    let run = &router_runs(&fixture)[0];
    // Started with every tool off, of no session, outside the thread's folder, without the
    // workspace's socket, and told only the router's input.
    assert_eq!(argument(run, "--tools"), "");
    assert!(
        run["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "--no-session-persistence")
    );
    assert_ne!(
        Path::new(run["cwd"].as_str().unwrap()),
        fixture.home.join("assistant/threads/t1")
    );
    assert_eq!(run["socket"], false);
    let current = &run["input"]["current_thread"];
    assert_eq!(current["id"], "t1");
    assert_eq!(current["last_messages"][0], "User: release notes");

    // A new topic opens a new thread in its Space and leaves a line in the current one.
    route(&service, "m2", Some("t1"), "for Personal: book the dentist");
    let snapshot = until_changed(&service, deadline(), |s| {
        tab(s, "t1").is_some_and(|t| !t.thread.routed.is_empty())
    });
    let line = tab(&snapshot, "t1").unwrap().thread.routed[0].clone();
    assert!(line.new);
    let routed = tab(&snapshot, &line.to).unwrap();
    assert_eq!(routed.thread.title, "Routed");
    after_turns(&service, &line.to, 1);

    // Unsure, the message waits unsent, and no turn starts until the user answers.
    let ran = turns(&fixture).len();
    route(&service, "m3", Some("t1"), "hmm");
    let snapshot = until_changed(&service, deadline(), |s| {
        s.held.first().is_some_and(|h| h.question.is_some())
    });
    let question = snapshot.held[0].question.clone().unwrap();
    assert!(
        question.why.starts_with("Not sure where this belongs"),
        "{question:?}"
    );
    assert_eq!(turns(&fixture).len(), ran);
    let continue_here = workspace_core::assistant::router::Destination::Thread {
        thread_id: "t1".into(),
    };
    assert!(question.choices.contains(&continue_here));
    request(
        &service,
        Command::AnswerHeld {
            id: "m3".into(),
            destination: continue_here,
        },
    );
    let snapshot = after_turns(&service, "t1", 3);
    assert!(snapshot.held.is_empty());
    assert_eq!(turns(&fixture).len(), ran + 1);
}
