//! The host as its own process: `serve --child` on `host.sock`, started as the app starts it.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::Host;

/// A child host on a scratch store whose login shell reports the test's own environment.
struct Fixture {
    data: tempfile::TempDir,
    shell: tempfile::TempDir,
    host: Option<ChildHost>,
}
impl Fixture {
    fn new(prepare: impl FnOnce(&mut Host)) -> Self {
        let data = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(data.path()).unwrap();
        let workspaces = data.path().join("workspaces");
        host.set_workspaces_dir(workspaces.to_str().unwrap())
            .unwrap();
        prepare(&mut host);
        drop(host);
        let shell = tempfile::tempdir().unwrap();
        let login = shell.path().join("shell");
        // It records which signals it starts with blocked, as every program the host starts would.
        let record = "/usr/bin/python3 -c 'import signal; print(sorted(int(s) for s in signal.pthread_sigmask(signal.SIG_BLOCK, [])))'";
        let script = format!("#!/bin/sh\n{record} > \"$(dirname \"$0\")/blocked\"\neval \"$4\"\n");
        std::fs::write(&login, script).unwrap();
        std::fs::set_permissions(&login, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = process::Command::new(env!("CARGO_BIN_EXE_workspace-host"));
        command
            .args(["serve", "--data-dir"])
            .arg(data.path())
            .arg("--child")
            .env("SHELL", &login)
            // No provider CLI is found, so detection at start is quick.
            .env("PATH", "/usr/bin:/bin")
            .stderr(std::process::Stdio::null());
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
    /// Sends a request and returns its response, keeping events that arrive first.
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
    assert_eq!(welcome.capabilities, ["events", "files", "folders"]);
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
    let snapshot = client.request(PROTOCOL_VERSION, Command::Snapshot);
    assert_eq!(snapshot.error, None);
}

#[test]
fn with_no_common_protocol_the_host_refuses_and_takes_only_restart_host() {
    let fixture = Fixture::new(|_| {});
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
    // `RestartHost` reaches the host, which cannot restart itself until ticket C.
    let restart = client.request(
        far[1],
        Command::RestartHost {
            when: RestartWhen::Idle,
        },
    );
    assert!(!restart.error.unwrap().contains("only RestartHost"));
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
    files(fixture.data.path(), &mut found);
    assert!(!found.is_empty());
    for file in found {
        let bytes = std::fs::read(&file).unwrap();
        let holds = bytes
            .windows(credential.len())
            .any(|window| window == credential.as_bytes());
        assert!(!holds, "{} holds the credential", file.display());
    }
    let mode = std::fs::metadata(fixture.data.path().join("host.sock"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_subscription_streams_events_and_resumes_from_its_cursor() {
    let fixture = Fixture::new(|_| {});
    let mut subscriber = fixture.welcomed(None);
    let subscribed: Subscribed =
        serde_json::from_value(subscriber.result(Command::Subscribe { after: None })).unwrap();
    let mut other = fixture.welcomed(None);
    other.result(Command::CreateProject {
        name: "First".into(),
    });
    let event = std::iter::repeat_with(|| subscriber.event())
        .find(|event| event.kind == EventKind::State)
        .unwrap();
    assert_eq!(event.cursor.epoch, subscribed.cursor.epoch);
    let read = event.cursor;
    drop(subscriber);
    // Changes while the client is away are replayed, in order, after its cursor.
    for name in ["Second", "Third"] {
        other.result(Command::CreateProject { name: name.into() });
    }
    // A reply comes before its change's events; the next command runs after they are emitted.
    other.result(Command::Snapshot);
    let mut back = fixture.welcomed(None);
    let resumed: Subscribed = serde_json::from_value(back.result(Command::Subscribe {
        after: Some(read.clone()),
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
    other.result(Command::CreateProject {
        name: "Fourth".into(),
    });
    assert!(back.event().cursor.seq > resumed.cursor.seq);
    // A cursor from before a restart gets resync first.
    let mut stale = fixture.welcomed(None);
    stale.result(Command::Subscribe {
        after: Some(Cursor {
            epoch: "an earlier start".into(),
            seq: read.seq,
        }),
    });
    assert_eq!(stale.event().kind, EventKind::Resync);
    // A second subscription on one connection is refused.
    let twice = stale.request(PROTOCOL_VERSION, Command::Subscribe { after: None });
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
