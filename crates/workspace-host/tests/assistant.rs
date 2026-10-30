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
