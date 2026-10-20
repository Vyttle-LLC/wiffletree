//! The host as its own process: `serve --child` on `host.sock`, started as the app starts it.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::Host;

/// The environment of a host whose login shell, written to `shell`, reports the test's own
/// environment, and in which neither provider is found, whatever this machine has signed in, so
/// detection at start is quick.
fn isolated(shell: &Path) -> Vec<(&'static str, OsString)> {
    let login = shell.join("shell");
    // It records which signals it starts with blocked, as every program the host starts would.
    let record = "/usr/bin/python3 -c 'import signal; print(sorted(int(s) for s in signal.pthread_sigmask(signal.SIG_BLOCK, [])))'";
    let script = format!("#!/bin/sh\n{record} > \"$(dirname \"$0\")/blocked\"\neval \"$4\"\n");
    std::fs::write(&login, script).unwrap();
    std::fs::set_permissions(&login, std::fs::Permissions::from_mode(0o755)).unwrap();
    vec![
        ("SHELL", login.into()),
        ("PATH", "/usr/bin:/bin".into()),
        ("WORKSPACE_CLAUDE_BIN", "/nonexistent/claude".into()),
        ("WORKSPACE_CODEX_BIN", "/nonexistent/codex".into()),
    ]
}

/// The files under `folder` holding `secret`, after checking there are some files at all.
fn files_holding(folder: &Path, secret: &str) -> Vec<PathBuf> {
    fn files(folder: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            match entry.file_type().unwrap() {
                kind if kind.is_dir() => files(&path, found),
                kind if kind.is_file() => found.push(path),
                _ => {}
            }
        }
    }
    let mut found = vec![];
    files(folder, &mut found);
    assert!(!found.is_empty());
    found.retain(|file| {
        let bytes = std::fs::read(file).unwrap_or_default();
        bytes
            .windows(secret.len())
            .any(|window| window == secret.as_bytes())
    });
    found
}

/// A child host on a scratch store whose login shell reports the test's own environment.
struct Fixture {
    data: tempfile::TempDir,
    shell: tempfile::TempDir,
    host: Option<ChildHost>,
}
impl Fixture {
    fn new(prepare: impl FnOnce(&mut Host)) -> Self {
        Self::from(
            Path::new(env!("CARGO_BIN_EXE_workspace-host")),
            None,
            tempfile::tempdir().unwrap(),
            prepare,
        )
    }
    /// A child host of the store in `data`, run from `binary`, which finds `provider` as both
    /// providers' CLI or else neither.
    fn from(
        binary: &Path,
        provider: Option<&Path>,
        data: tempfile::TempDir,
        prepare: impl FnOnce(&mut Host),
    ) -> Self {
        let mut host = Host::open_set_up(data.path()).unwrap();
        let workspaces = data.path().join("workspaces");
        host.set_workspaces_dir(workspaces.to_str().unwrap())
            .unwrap();
        prepare(&mut host);
        drop(host);
        let shell = tempfile::tempdir().unwrap();
        let mut command = process::Command::new(binary);
        command
            .args(["serve", "--data-dir"])
            .arg(data.path())
            .arg("--child")
            .envs(isolated(shell.path()))
            .stderr(std::process::Stdio::null());
        if let Some(provider) = provider {
            command
                .env("WORKSPACE_CLAUDE_BIN", provider)
                .env("WORKSPACE_CODEX_BIN", provider);
        }
        let host = ChildHost::start_command(command).unwrap();
        let socket = data.path().join("host.sock");
        let deadline = Instant::now() + Duration::from_secs(60);
        while UnixStream::connect(&socket).is_err() {
            assert!(Instant::now() < deadline, "the host never listened");
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            data,
            shell,
            host: Some(host),
        }
    }
    fn credential(&self) -> String {
        self.host.as_ref().unwrap().credential().to_owned()
    }
    fn connect(&self) -> Connection {
        let stream = UnixStream::connect(self.data.path().join("host.sock")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        Connection {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
        }
    }
    /// Waits for the host to exit on its own.
    fn exited(&mut self) -> process::ExitStatus {
        let host = self.host.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = host.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the host did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn welcomed(&self, credential: Option<String>) -> Connection {
        let mut client = self.connect();
        let answer = client.hello(PROTOCOLS, credential);
        assert!(answer.get("welcome").is_some(), "{answer}");
        client
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(host) = self.host.take() {
            assert!(host.stop().unwrap().success(), "the host exits cleanly");
        }
    }
}

struct Connection {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}
impl Connection {
    fn send(&mut self, line: Value) {
        writeln!(self.stream, "{line}").unwrap();
    }
    fn line(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    }
    fn hello(&mut self, protocols: [u32; 2], credential: Option<String>) -> Value {
        let hello = Hello {
            protocols,
            app_version: "test".into(),
            client: Client {
                name: "test".into(),
                kind: "script".into(),
            },
            capabilities: vec![],
            credential,
        };
        self.send(serde_json::to_value(HelloLine { hello }).unwrap());
        self.line()
    }
    /// Sends a request and returns its response, skipping events that arrive first.
    fn request(&mut self, version: u32, command: Command) -> Response {
        let id = new_id();
        let request = Request {
            version,
            id: id.clone(),
            command,
        };
        self.send(serde_json::to_value(request).unwrap());
        loop {
            let line = self.line();
            if line.get("event").is_none() {
                let response: Response = serde_json::from_value(line).unwrap();
                assert_eq!(response.id, id);
                return response;
            }
        }
    }
    fn result(&mut self, command: Command) -> Value {
        let response = self.request(PROTOCOL_VERSION, command);
        assert_eq!(response.error, None);
        response.result.unwrap()
    }
    fn event(&mut self) -> Event {
        serde_json::from_value::<EventLine>(self.line())
            .unwrap()
            .event
    }
}

fn snapshot(client: &mut Connection) -> Snapshot {
    serde_json::from_value(client.result(Command::Snapshot)).unwrap()
}

#[test]
fn a_request_before_hello_is_refused_and_runs_nothing() {
    let fixture = Fixture::new(|_| {});
    let mut client = fixture.connect();
    client
        .send(json!({"version":2,"id":"early","command":{"type":"create_project","name":"Early"}}));
    let refused: Response = serde_json::from_value(client.line()).unwrap();
    assert_eq!(refused.id, "early");
    assert_eq!(refused.error.as_deref(), Some("Send Hello first"));
    let answer = client.hello(PROTOCOLS, None);
    assert!(answer.get("welcome").is_some(), "{answer}");
    assert!(snapshot(&mut client).projects.is_empty());
}

#[test]
fn the_handshake_chooses_the_newest_common_protocol() {
    let fixture = Fixture::new(|_| {});
    let same = fixture.connect().hello(PROTOCOLS, None);
    let welcome: Welcome = serde_json::from_value(same["welcome"].clone()).unwrap();
    assert_eq!(welcome.protocol, PROTOCOL_VERSION);
    assert_eq!(
        welcome.capabilities,
        ["events", "files", "folders", "restart-when-idle"]
    );
    assert_eq!(welcome.host_version, env!("CARGO_PKG_VERSION"));
    assert!(welcome.binary.ends_with("workspace-host"));
    // The host id stays with the store; the epoch is this start's.
    let again = fixture.connect().hello(PROTOCOLS, None);
    assert_eq!(again["welcome"]["host_id"], json!(welcome.host_id));
    assert_eq!(again["welcome"]["epoch"], json!(welcome.epoch));

    // An app one protocol ahead gets the host's.
    let ahead = [PROTOCOL_VERSION, PROTOCOL_VERSION + 1];
    let mut client = fixture.connect();
    assert_eq!(
        client.hello(ahead, None)["welcome"]["protocol"],
        json!(PROTOCOL_VERSION)
    );
    // An app one behind gets its own, and must use it.
    let behind = [PROTOCOL_VERSION - 1, PROTOCOL_VERSION - 1];
    let mut client_behind = fixture.connect();
    let answer = client_behind.hello(behind, None);
    assert_eq!(answer["welcome"]["protocol"], json!(PROTOCOL_VERSION - 1));
    let wrong = client_behind.request(PROTOCOL_VERSION, Command::Snapshot);
    assert!(wrong.error.unwrap().contains("Use protocol 1"));
    let right = client_behind.request(PROTOCOL_VERSION - 1, Command::Snapshot);
    assert_eq!((right.error, right.version), (None, PROTOCOL_VERSION - 1));
    // The connection itself answers a ping.
    assert_eq!(
        client_behind
            .request(PROTOCOL_VERSION - 1, Command::Ping)
            .error,
        None
    );
    let snapshot = client.request(PROTOCOL_VERSION, Command::Snapshot);
    assert_eq!(snapshot.error, None);
}

#[test]
fn with_no_common_protocol_the_host_refuses_and_takes_only_restart_host() {
    let mut fixture = Fixture::new(|_| {});
    let mut client = fixture.connect();
    let far = [PROTOCOL_VERSION + 1, PROTOCOL_VERSION + 2];
    let answer = client.hello(far, None);
    let refusal: Refusal = serde_json::from_value(answer["refused"].clone()).unwrap();
    assert_eq!(
        refusal,
        Refusal {
            host_protocols: PROTOCOLS,
            client_protocols: far,
            host_version: env!("CARGO_PKG_VERSION").into(),
            working: 0,
        }
    );
    let snapshot = client.request(far[1], Command::Snapshot);
    assert!(snapshot.error.unwrap().contains("only RestartHost"));
    // With nothing working, an idle restart ends the host at once, and cleanly, so launchd
    // leaves it stopped for the app to start the new version.
    let restart = client.request(
        far[1],
        Command::RestartHost {
            when: RestartWhen::Idle,
        },
    );
    assert_eq!(restart.error, None);
    assert!(fixture.exited().success());
}

/// A provider whose turns run until they are stopped. Its `auth status` check passes.
const LINGERING_PROVIDER: &str = r#"#!/bin/sh
case "$1" in -p|exec) ;; *) exit 0;; esac
cat >/dev/null
echo '{"type":"system","subtype":"init","session_id":"s","mcp_servers":[{"name":"agent_workspace","status":"connected"}]}'
exec sleep 60
"#;

#[test]
fn with_no_common_protocol_the_refusal_counts_working_agents_and_restart_now_stops_them() {
    let scratch = tempfile::tempdir().unwrap();
    let provider = scratch.path().join("provider");
    std::fs::write(&provider, LINGERING_PROVIDER).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut main = None;
    let mut fixture = Fixture::from(
        Path::new(env!("CARGO_BIN_EXE_workspace-host")),
        Some(&provider),
        tempfile::tempdir().unwrap(),
        |host| {
            let project = host.create_project("Working").unwrap();
            host.set_live(&project.id, true).unwrap();
            let sessions = host.sessions().unwrap().into_iter();
            main = sessions.rev().find(|s| s.project_id == project.id);
        },
    );
    let main = main.unwrap();
    let mut app = fixture.welcomed(Some(fixture.credential()));
    app.result(Command::Send {
        id: "linger".into(),
        sender: None,
        recipient: main.id.clone(),
        body: "Keep working".into(),
        attachments: vec![],
        uploads: vec![],
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while snapshot(&mut app)
        .sessions
        .iter()
        .find(|s| s.id == main.id)
        .unwrap()
        .status
        != Status::Working
    {
        assert!(Instant::now() < deadline, "the turn never started");
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut updated = fixture.connect();
    let far = [PROTOCOL_VERSION + 1, PROTOCOL_VERSION + 2];
    let refusal: Refusal =
        serde_json::from_value(updated.hello(far, None)["refused"].clone()).unwrap();
    assert_eq!(refusal.working, 1);
    let restart = updated.request(
        far[1],
        Command::RestartHost {
            when: RestartWhen::Now,
        },
    );
    assert_eq!(restart.error, None);
    assert!(fixture.exited().success());
    let db = rusqlite::Connection::open(fixture.data.path().join("workspace.sqlite3")).unwrap();
    let error: String = db
        .query_row(
            "SELECT json_extract(detail,'$.result.error') FROM provider_runs WHERE session_id=?1",
            [&main.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        error.starts_with("Wiffletree quit during a turn"),
        "{error}"
    );
}

#[test]
fn stop_agents_and_quit_is_answered_and_the_host_exits_successfully() {
    let mut fixture = Fixture::new(|_| {});
    let mut client = fixture.welcomed(None);
    let quit = client.request(PROTOCOL_VERSION, Command::StopAgentsAndQuit);
    assert_eq!(quit.error, None);
    assert!(fixture.exited().success());
    assert!(!fixture.data.path().join("host.sock").exists());
}

#[test]
fn a_host_run_from_a_versioned_copy_keeps_its_path_and_removes_unused_copies() {
    let scratch = tempfile::tempdir().unwrap();
    let source = scratch.path().join("workspace-host");
    std::fs::copy(env!("CARGO_BIN_EXE_workspace-host"), &source).unwrap();
    let data = tempfile::tempdir().unwrap();
    let copy = |version: &str| {
        let copy = host_copy::path(data.path(), version);
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(&source, &copy).unwrap();
        copy
    };
    let unused = copy("0.9.0");
    let current = copy("1.0.0");
    // An update replaces the bundle's binary the copy was made from.
    std::fs::write(&source, "replaced").unwrap();

    let fixture = Fixture::from(&current, None, data, |_| {});

    let answer = fixture.connect().hello(PROTOCOLS, None);
    let welcome: Welcome = serde_json::from_value(answer["welcome"].clone()).unwrap();
    // The binary `Welcome` names is the one the host starts its agents' helpers from.
    assert_eq!(
        welcome.binary.canonicalize().unwrap(),
        current.canonicalize().unwrap()
    );
    assert!(
        !unused.parent().unwrap().exists(),
        "the unused copy is removed"
    );
    assert!(current.exists());
}

#[test]
fn a_second_host_from_another_copy_is_refused_and_leaves_the_running_hosts_copy() {
    let data = tempfile::tempdir().unwrap();
    let running = host_copy::path(data.path(), "1.0.0");
    std::fs::create_dir_all(running.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_workspace-host"), &running).unwrap();
    let data_path = data.path().to_owned();
    let fixture = Fixture::from(&running, None, data, |_| {});
    // An update arrives while the host runs: the app copies the new version beside it.
    let second = host_copy::path(&data_path, "2.0.0");
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_workspace-host"), &second).unwrap();

    let mut command = process::Command::new(&second);
    command
        .args(["serve", "--data-dir"])
        .arg(fixture.data.path())
        .arg("--child")
        .envs(isolated(fixture.shell.path()))
        .stderr(std::process::Stdio::piped());
    let mut refused = ChildHost::start_command(command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = refused.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the second host kept running");
        std::thread::sleep(Duration::from_millis(20));
    };

    assert!(!status.success(), "the store is the running host's");
    assert!(running.exists(), "the running host's copy is kept");
    assert!(snapshot(&mut fixture.welcomed(None)).projects.is_empty());
}

#[test]
fn inbox_answers_need_the_apps_credential_and_other_commands_do_not() {
    let fixture = Fixture::new(|host| {
        let project = host.create_project("Questions").unwrap();
        let session = host
            .sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.project_id == project.id)
            .unwrap();
        host.request_attention(&session.id, "local", "deploy", "Deploy now?", &[])
            .unwrap();
    });
    let mut agent = fixture.welcomed(None);
    let question = snapshot(&mut agent).attention[0].id.clone();
    let answer = |client: &mut Connection| {
        client.request(
            PROTOCOL_VERSION,
            Command::ResolveAttention {
                id: question.clone(),
                answer: "yes".into(),
            },
        )
    };
    let refused = answer(&mut agent);
    assert!(refused.error.unwrap().contains("client credential"));
    let dismissed = agent.request(
        PROTOCOL_VERSION,
        Command::DismissAttention {
            id: question.clone(),
        },
    );
    assert!(dismissed.error.unwrap().contains("client credential"));
    assert_eq!(
        snapshot(&mut agent).attention.len(),
        1,
        "the question stays open"
    );
    let mut guessing = fixture.welcomed(Some("0".repeat(64)));
    assert!(answer(&mut guessing).error.is_some());

    let mut app = fixture.welcomed(Some(fixture.credential()));
    assert_eq!(answer(&mut app).error, None);
    assert!(
        snapshot(&mut app).attention.is_empty(),
        "the answer is recorded"
    );
}

#[test]
fn the_child_hosts_credential_is_in_no_environment_and_no_file() {
    let fixture = Fixture::new(|_| {});
    let credential = fixture.credential();
    let pid = fixture.host.as_ref().unwrap().id().to_string();
    // `ps -E` shows a process's environment after its command line.
    let ps = process::Command::new("/bin/ps")
        .args(["-E", "-ww", "-o", "command=", "-p", &pid])
        .output()
        .unwrap();
    let shown = String::from_utf8_lossy(&ps.stdout);
    assert!(
        shown.contains("SHELL="),
        "ps shows the environment: {shown}"
    );
    assert!(!shown.contains(&credential));
    // Nothing in the data folder holds it.
    assert_eq!(
        files_holding(fixture.data.path(), &credential),
        [] as [PathBuf; 0]
    );
    let mode = std::fs::metadata(fixture.data.path().join("host.sock"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_subscription_streams_events_and_resumes_from_its_cursor() {
    // Renaming changes state without starting an agent, so no provider is needed.
    let project = std::cell::OnceCell::new();
    let fixture = Fixture::new(|host| {
        project
            .set(host.create_project("Events").unwrap().id)
            .unwrap();
    });
    let project = project.into_inner().unwrap();
    let rename = |name: &str| Command::RenameProject {
        project_id: project.clone(),
        name: name.into(),
    };
    let mut subscriber = fixture.welcomed(None);
    let subscribed: Subscribed = serde_json::from_value(subscriber.result(Command::Subscribe {
        after: None,
        kinds: None,
    }))
    .unwrap();
    let mut other = fixture.welcomed(None);
    other.result(rename("First"));
    let event = std::iter::repeat_with(|| subscriber.event())
        .find(|event| event.kind == EventKind::State)
        .unwrap();
    assert_eq!(event.cursor.epoch, subscribed.cursor.epoch);
    let read = event.cursor;
    drop(subscriber);
    // Changes while the client is away are replayed, in order, after its cursor.
    for name in ["Second", "Third"] {
        other.result(rename(name));
    }
    // A reply comes before its change's events; the next command runs after they are emitted.
    other.result(Command::Snapshot);
    let mut back = fixture.welcomed(None);
    let resumed: Subscribed = serde_json::from_value(back.result(Command::Subscribe {
        after: Some(read.clone()),
        kinds: None,
    }))
    .unwrap();
    let missed: Vec<_> = (read.seq + 1..=resumed.cursor.seq)
        .map(|_| back.event())
        .collect();
    let seqs: Vec<_> = missed.iter().map(|e| e.cursor.seq).collect();
    assert_eq!(
        seqs,
        (read.seq + 1..=resumed.cursor.seq).collect::<Vec<_>>()
    );
    let states = missed.iter().filter(|e| e.kind == EventKind::State).count();
    assert_eq!(states, 2, "{missed:?}");
    // Then live events.
    other.result(rename("Fourth"));
    assert!(back.event().cursor.seq > resumed.cursor.seq);
    // A cursor from before a restart gets resync first.
    let mut stale = fixture.welcomed(None);
    stale.result(Command::Subscribe {
        after: Some(Cursor {
            epoch: "an earlier start".into(),
            seq: read.seq,
        }),
        kinds: None,
    });
    assert_eq!(stale.event().kind, EventKind::Resync);
    // A second subscription on one connection is refused.
    let twice = stale.request(
        PROTOCOL_VERSION,
        Command::Subscribe {
            after: None,
            kinds: None,
        },
    );
    assert!(twice.error.unwrap().contains("already subscribed"));
}

#[test]
fn the_snapshot_names_the_running_hosts_core_instructions() {
    let fixture = Fixture::new(|_| {});
    let core = snapshot(&mut fixture.welcomed(None)).core_instructions;
    let shown: Vec<_> = core
        .iter()
        .map(|c| (c.audience.as_str(), c.version.as_str()))
        .collect();
    let bundled: Vec<_> = workspace_host::provider::CORE
        .iter()
        .map(|(audience, text)| (*audience, workspace_core::skills::version(text)))
        .collect();
    assert_eq!(shown, bundled);
    assert_eq!(core[0].name, "Wiffletree communication contract");
    assert!(
        core.iter()
            .all(|c| !c.version.is_empty() && !c.name.contains('—'))
    );
}

#[test]
fn programs_the_host_starts_block_no_signal_and_sigterm_stops_the_host_cleanly() {
    let mut fixture = Fixture::new(|_| {});
    let blocked = std::fs::read_to_string(fixture.shell.path().join("blocked")).unwrap();
    assert_eq!(blocked.trim(), "[]");
    let mut host = fixture.host.take().unwrap();
    // SAFETY: signals the child host this test started.
    assert_eq!(unsafe { libc::kill(host.id() as i32, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = host.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the host ignored SIGTERM");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}");
    assert!(!fixture.data.path().join("host.sock").exists());
}

#[test]
fn allowing_an_owner_needs_the_apps_credential() {
    let fixture = Fixture::new(|_| {});
    let allow = |client: &mut Connection| {
        client.request(
            PROTOCOL_VERSION,
            Command::SetOwnerAllowed {
                entry: "Vyttle-LLC".into(),
                allowed: true,
            },
        )
    };
    let mut agent = fixture.welcomed(None);
    let refused = allow(&mut agent);
    assert!(refused.error.unwrap().contains("client credential"));
    assert!(snapshot(&mut agent).allowed_owners.is_empty());
    let mut app = fixture.welcomed(Some(fixture.credential()));
    assert_eq!(allow(&mut app).error, None);
    assert_eq!(snapshot(&mut app).allowed_owners, ["Vyttle-LLC"]);
}

#[test]
fn a_message_from_the_human_needs_the_apps_credential() {
    let coordinator = std::cell::OnceCell::new();
    let fixture = Fixture::new(|host| {
        let project = host.create_project("Messages").unwrap();
        let session = (host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project.id)
            .unwrap();
        coordinator.set(session.id).unwrap();
    });
    let coordinator = coordinator.into_inner().unwrap();
    let send = |client: &mut Connection, id: &str| {
        client.request(
            PROTOCOL_VERSION,
            Command::Send {
                id: id.into(),
                sender: None,
                recipient: coordinator.clone(),
                body: "Ship it".into(),
                attachments: vec![],
                uploads: vec![],
            },
        )
    };
    let stored = |client: &mut Connection| -> Vec<String> {
        let messages = client.result(Command::Messages {
            session_id: coordinator.clone(),
            before: None,
            limit: 50,
        });
        let messages: Vec<Message> = serde_json::from_value(messages).unwrap();
        messages.into_iter().map(|m| m.id).collect()
    };
    let mut agent = fixture.welcomed(None);
    let refused = send(&mut agent, "from-an-agent");
    assert!(refused.error.unwrap().contains("client credential"));
    assert!(!stored(&mut agent).contains(&"from-an-agent".to_string()));
    let mut app = fixture.welcomed(Some(fixture.credential()));
    assert_eq!(send(&mut app, "from-the-human").error, None);
    assert!(stored(&mut app).contains(&"from-the-human".to_string()));
}

/// The app's client of a child host on `data`, started as the app starts one; it records the
/// events it reads.
fn app_client(data: &Path, shell: &Path) -> (HostClient, Arc<Mutex<Vec<EventKind>>>) {
    let mut launch = ChildLaunch::new(Path::new(env!("CARGO_BIN_EXE_workspace-host")), data);
    for (key, value) in isolated(shell) {
        launch = launch.env(key, value);
    }
    let events = Arc::new(Mutex::new(vec![]));
    let seen = events.clone();
    let client = HostClient::connect(
        Box::new(launch),
        Client {
            name: "Wiffletree".into(),
            kind: "desktop".into(),
        },
        Box::new(move |kind| seen.lock().unwrap().push(kind.clone())),
    )
    .unwrap();
    (client, events)
}

fn child_pid(client: &HostClient) -> i32 {
    client.with_launcher(|launch| launch.child().unwrap().id()) as i32
}

/// The reply to a request, waiting at most a minute.
fn reply(receiver: async_channel::Receiver<Result<Value, String>>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match receiver.try_recv() {
            Ok(reply) => return reply.unwrap(),
            Err(async_channel::TryRecvError::Empty) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(error) => panic!("no reply: {error}"),
        }
    }
}

fn answer<R: serde::de::DeserializeOwned>(client: &HostClient, ask: ask::Ask<R>) -> R {
    let value = reply(client.request(ask.command().clone()).unwrap());
    ask.decode(value).unwrap()
}

/// A scratch folder short enough for a socket path under `Library/Application Support`.
/// The labels of the Wiffletree host jobs launchd has loaded for this user, such as
/// `LaunchdJob::label()`; the running apps' own `application.…` entries are not jobs.
fn wiffletree_jobs() -> Vec<String> {
    let list = process::Command::new("/bin/launchctl")
        .arg("list")
        .output()
        .unwrap();
    assert!(list.status.success());
    let prefix = LaunchdJob::label()
        .trim_end_matches(".host")
        .trim_end_matches(".beta");
    String::from_utf8_lossy(&list.stdout)
        .lines()
        .filter_map(|line| line.split('\t').nth(2))
        .filter(|label| label.starts_with(prefix))
        .map(String::from)
        .collect()
}

fn short_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new().tempdir_in("/tmp").unwrap()
}

#[test]
fn an_unsigned_app_on_its_default_data_folder_starts_a_child_host_and_no_launchd_job() {
    let home = short_tempdir();
    let data = workspace_core::data_dir::default_directory(home.path());
    let app = std::env::current_exe().unwrap();
    assert_eq!(
        HostLaunch::choose(developer_id_signed(&app), true),
        HostLaunch::Child
    );
    let shell = tempfile::tempdir().unwrap();
    let jobs_before = wiffletree_jobs();
    let (client, _) = app_client(&data, shell.path());
    assert!(answer(&client, ask::snapshot()).projects.is_empty());
    // The host was started with the credential on its standard input, and holds the store.
    let credential = client.with_launcher(|launch| launch.child().unwrap().credential().to_owned());
    assert!(data.join("host.sock").exists());
    // No plist was written, launchd gained no Wiffletree job while the app ran, and none it
    // has serves this data folder.
    assert!(!data.join("host").exists());
    let jobs = wiffletree_jobs();
    assert!(jobs.iter().all(|job| jobs_before.contains(job)), "{jobs:?}");
    for label in jobs {
        let job = process::Command::new("/bin/launchctl")
            .args([
                "print",
                &format!("gui/{}/{label}", unsafe { libc::getuid() }),
            ])
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&job.stdout);
        assert!(
            !listed.contains(&*home.path().to_string_lossy()),
            "launchd's {label} serves this data folder: {listed}"
        );
    }
    // No file holds the credential.
    assert_eq!(files_holding(home.path(), &credential), [] as [PathBuf; 0]);
    client.stop();
}

#[test]
fn stopping_the_client_while_it_starts_a_host_again_stops_that_host_at_once() {
    let data = short_tempdir();
    let scratch = tempfile::tempdir().unwrap();
    // The real host the first time; after that a host that never listens, as one still
    // starting looks, and that exits when its standard input closes.
    let started = scratch.path().join("started");
    let wrapper = scratch.path().join("host");
    let script = format!(
        "#!/bin/sh\nif [ ! -e \"$0.ran\" ]; then touch \"$0.ran\"; exec '{}' \"$@\"; fi\necho $$ > '{}'\nexec cat >/dev/null\n",
        env!("CARGO_BIN_EXE_workspace-host"),
        started.display()
    );
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut launch = ChildLaunch::new(&wrapper, data.path());
    for (key, value) in isolated(scratch.path()) {
        launch = launch.env(key, value);
    }
    let client = HostClient::connect(
        Box::new(launch),
        Client {
            name: "Wiffletree".into(),
            kind: "desktop".into(),
        },
        Box::new(|_| {}),
    )
    .unwrap();
    // SAFETY: the child is this test's own host.
    unsafe { libc::kill(child_pid(&client), libc::SIGKILL) };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !started.exists() {
        assert!(
            Instant::now() < deadline,
            "the client never started a host again"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let restarted: i32 = std::fs::read_to_string(&started)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    let stopping = Instant::now();
    client.stop();

    // Not the minute a host may take to start, and the host it was starting is gone.
    assert!(
        stopping.elapsed() < Duration::from_secs(5),
        "{:?}",
        stopping.elapsed()
    );
    // SAFETY: signal 0 only asks whether the process exists.
    assert_eq!(
        unsafe { libc::kill(restarted, 0) },
        -1,
        "the new host still runs"
    );
}

#[test]
fn with_a_data_dir_no_plist_is_written_and_the_child_host_stops_with_the_app() {
    // Even a signed app starts a child host for a data folder it was given.
    assert_eq!(HostLaunch::choose(true, false), HostLaunch::Child);
    let data = tempfile::tempdir().unwrap();
    let shell = tempfile::tempdir().unwrap();
    let (client, _) = app_client(data.path(), shell.path());
    assert!(answer(&client, ask::snapshot()).projects.is_empty());
    let pid = child_pid(&client);
    client.stop();
    // SAFETY: signal 0 only asks whether the process exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "the host still runs");
    assert!(!data.path().join("host").exists());
    assert!(!data.path().join("host.sock").exists());
}

#[test]
fn a_message_sent_as_the_child_host_is_killed_is_stored_once() {
    let data = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(data.path()).unwrap();
    let project = host.create_project("Drops").unwrap();
    let coordinator = (host.sessions().unwrap().into_iter())
        .find(|s| s.project_id == project.id)
        .unwrap()
        .id;
    drop(host);
    let shell = tempfile::tempdir().unwrap();
    let (client, events) = app_client(data.path(), shell.path());
    let send = |id: &str| {
        let send = Command::Send {
            id: id.into(),
            sender: None,
            recipient: coordinator.clone(),
            body: "Across the drop".into(),
            attachments: vec![],
            uploads: vec![],
        };
        client.request(send).unwrap()
    };
    // A stopped host reads nothing, so the request is written and never answered before the
    // kill.
    let pid = child_pid(&client);
    // SAFETY: signals the child host this test started.
    unsafe { libc::kill(pid, libc::SIGSTOP) };
    let first = send("first-across-the-drop");
    std::thread::sleep(Duration::from_millis(200));
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let message: Message = serde_json::from_value(reply(first)).unwrap();
    assert_eq!(message.id, "first-across-the-drop");
    // Killed as the request is written, the host may have stored it or not.
    let pid = child_pid(&client);
    let second = send("second-across-the-drop");
    unsafe { libc::kill(pid, libc::SIGKILL) };
    reply(second);

    let messages = answer(&client, ask::messages(coordinator.clone(), None, 50));
    for id in ["first-across-the-drop", "second-across-the-drop"] {
        let copies = messages.iter().filter(|m| m.id == id).count();
        assert_eq!(copies, 1, "{id}");
    }
    // The restarted host is a new epoch, so the subscription resumed with resync.
    assert!(events.lock().unwrap().contains(&EventKind::Resync));
    client.stop();
}
