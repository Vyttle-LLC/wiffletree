//! Issue threads: sessions a Space owns, run restricted in their scratch folders.
mod common;
use common::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use workspace_core::assistant::proposal::Attached;
use workspace_core::assistant::{CardKind, Thread, ThreadTab};
use workspace_core::*;
use workspace_host::Host;

/// Every test here sets the provider through the process environment.
static ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    _directory: tempfile::TempDir,
    home: PathBuf,
    turns: PathBuf,
    /// Sage, which has instructions, and two more Spaces, each with a tracker whose list
    /// thread is a thread of its own to open.
    spaces: Vec<SpaceId>,
}
/// A store with those Spaces and a folder for the double's log. With `row`, each Space's
/// Assistant row starts its threads there.
fn fixture_on(row: Option<ModelProfile>) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let turns = directory.path().join("turns");
    std::fs::create_dir_all(&turns).unwrap();
    let mut host = Host::open_set_up(&home).unwrap();
    let spaces = [
        ("Sage", "Treat production as read-only.", "SAGE"),
        ("Watch", "", "WATCH"),
        ("Web", "", "WEB"),
    ]
    .map(|(name, additions, prefix)| {
        let mut space = (host.create_space(name, "", additions, Some(prefix)))
            .unwrap()
            .space;
        if let Some(row) = &row {
            space.models.threads = Some(row.clone().into());
            space = host.update_space(space).unwrap().space;
        }
        space.id
    })
    .to_vec();
    drop(host);
    Fixture {
        _directory: directory,
        home: std::fs::canonicalize(home).unwrap(),
        turns,
        spaces,
    }
}
fn fixture() -> Fixture {
    fixture_on(None)
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
/// Opens the list thread of the fixture's `space`th Space's tracker and sends it `body`; the
/// thread's id.
fn start(
    service: &workspace_host::service::Service,
    fixture: &Fixture,
    space: usize,
    body: &str,
) -> String {
    let open = Command::OpenIssueThread {
        attached: Attached::Issues,
        space_id: Some(fixture.spaces[space].clone()),
    };
    let thread: Thread = serde_json::from_value(request(service, open)).unwrap();
    send(service, &thread.id, body);
    thread.id
}
fn send(service: &workspace_host::service::Service, thread: &str, body: &str) {
    request(
        service,
        Command::Send {
            id: format!("{thread}-{}", replies(service, thread).len()),
            sender: None,
            recipient: thread.into(),
            body: body.into(),
            attachments: vec![],
            uploads: vec![],
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
fn an_issue_thread_runs_restricted_in_its_scratch_folder_and_saves_outputs_there_only() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = fixture();
    let service = service(&fixture);
    let t1 = start(&service, &fixture, 0, "save costs.html as AWS costs");
    let t1 = t1.as_str();
    let snapshot = after_turns(&service, t1, 1);
    let scratch = fixture.home.join("assistant/threads").join(t1);

    // Started with no source and no web, never in YOLO mode, in its scratch folder.
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
    assert!(!allowed.contains("WebSearch") && !allowed.contains("WebFetch"));
    assert_eq!(Path::new(turn["cwd"].as_str().unwrap()), scratch);
    // Offered only the issue thread's tools, with no project brief or job proposal; the prompt
    // is the message alone.
    assert_eq!(
        turn["tools"],
        serde_json::json!([
            "save_output",
            "suggest_space",
            "propose_issue",
            "query_brain",
            "list_issues",
            "read_issue",
            "search_repositories",
            "read_repository_file",
            "propose_edit",
            "propose_comment",
            "propose_split",
            "propose_duplicate"
        ])
    );
    assert_eq!(turn["prompt"], "save costs.html as AWS costs");
    // The contract, the Assistant core, Assisting, the issue and then the Space's additions.
    let instructions = argument(turn, "--append-system-prompt");
    let order = [
        "# Wiffletree thread contract",
        "# Assistant core",
        "# Assisting",
        "## This issue thread",
        "## Additions from the Sage Space",
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
        .join(fixture.spaces[0].as_str())
        .join("aws-costs");
    assert_eq!(
        std::fs::read_to_string(board.join("costs.html")).unwrap(),
        "# AWS costs\n"
    );
    let card = tab(&snapshot, t1).unwrap().thread.cards.pop().unwrap();
    assert!(
        matches!(card.kind, CardKind::SavedOutput { version: 1, .. }),
        "{card:?}"
    );
    assert!(
        replies(&service, t1)
            .last()
            .unwrap()
            .body
            .contains("Only on this Mac")
    );

    // The next message resumes the same provider session; saving again adds a version.
    send(&service, t1, "save costs.html as AWS costs");
    let snapshot = after_turns(&service, t1, 2);
    assert!(
        turns(&fixture)[1]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "--resume")
    );
    let card = tab(&snapshot, t1).unwrap().thread.cards.pop().unwrap();
    assert!(
        matches!(card.kind, CardKind::SavedOutput { version: 2, .. }),
        "{card:?}"
    );
    // No project, session or agent lists the thread.
    assert!(snapshot.sessions.iter().all(|s| s.id != t1));
    assert!(snapshot.runtimes.iter().all(|r| r.session_id != t1));
}

#[test]
fn a_codex_call_to_a_server_or_tool_it_was_not_given_stops_the_run_and_says_so() {
    let _environment = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let codex = ModelProfile {
        provider: Provider::Codex,
        model: "gpt-6.1-sol".into(),
        effort: "medium".into(),
    };
    let fixture = fixture_on(Some(codex));
    let service = service(&fixture);
    let calls = [
        "call agent_workspace save_output",
        "call github list_issues",
        "call agent_workspace create_ticket",
    ];
    let stops = [
        None,
        Some("an MCP server its Space's sources do not list (github)"),
        Some("a tool it was not given (agent_workspace create_ticket)"),
    ];
    let ids: Vec<String> = (calls.iter().enumerate())
        .map(|(issue, body)| start(&service, &fixture, issue, body))
        .collect();
    until_changed(&service, Instant::now() + Duration::from_secs(60), |s| {
        ids.iter().all(|id| {
            tab(s, id).is_some_and(|t| !matches!(t.status, Status::Working | Status::Ready))
                || replies(&service, id).iter().any(|m| m.sender.is_some())
        })
    });
    for (id, stopped) in ids.iter().zip(stops) {
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
    let t2 = start(&service, &fixture, 1, "offer github");
    let snapshot = until_changed(&service, Instant::now() + Duration::from_secs(60), |s| {
        tab(s, &t2).is_some_and(|t| t.status == Status::Disconnected)
    });
    assert!(tab(&snapshot, &t2).unwrap().thread.cards.is_empty());
    let shown = replies(&service, &t2);
    assert!(
        shown.iter().any(|m| m
            .body
            .contains("an MCP server its Space's sources do not list (github)")),
        "{shown:?}"
    );
}

/// The hook before a job run's shell command exits 2, the only refusal Claude honours, for
/// anything but an explicit allow.
#[test]
fn the_job_shell_hook_refuses_with_exit_2_unless_it_allows() {
    use std::io::Write;
    let hook = |input: &[u8]| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wiffletree-host"))
            .arg("job-bash-check")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        (output.status.code(), !output.stderr.is_empty())
    };
    let command = |text: &str| serde_json::json!({"tool_input":{"command":text}}).to_string();
    assert_eq!(hook(command("gh pr list").as_bytes()), (Some(0), false));
    for refused in [command("gh pr list > x"), String::new(), "not json".into()] {
        assert_eq!(hook(refused.as_bytes()), (Some(2), true), "{refused}");
    }
}
