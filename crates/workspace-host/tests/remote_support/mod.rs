//! A host in this process with remote access, and paired devices that drive it over the
//! network on this Mac, for the remote-access tests.
#![allow(dead_code)]
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use workspace_core::{
    pairing::KeyPair,
    remote_client::{self, PairedHost, RemoteError, RemoteSocket},
    *,
};
use workspace_host::{Host, remote, service::Service};

/// Points both providers at a missing binary, so a message the host delivers starts no real
/// agent, whatever this Mac has signed in.
pub fn no_providers() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: every test sets the same values here before its host starts, and the first holds
    // the others back until it has.
    ONCE.call_once(|| unsafe {
        std::env::set_var("WORKSPACE_CLAUDE_BIN", "/nonexistent/claude");
        std::env::set_var("WORKSPACE_CODEX_BIN", "/nonexistent/codex");
    });
}

/// What the host advertises: its name, port and TXT fields, while it advertises.
pub type Advertised = Arc<Mutex<Option<(String, u16, Vec<(String, String)>)>>>;

pub struct Recorder(Advertised);
impl remote::Advertiser for Recorder {
    fn advertise(&mut self, name: &str, port: u16, txt: &[(&str, &str)]) -> anyhow::Result<()> {
        let txt = txt.iter().map(|(k, v)| (k.to_string(), v.to_string()));
        *self.0.lock().unwrap() = Some((name.into(), port, txt.collect()));
        Ok(())
    }
    fn withdraw(&mut self) {
        *self.0.lock().unwrap() = None;
    }
}

/// A host named Studio Mac with a fixed key, a clock the test moves and a recorded
/// advertisement, and one project whose coordinator takes no turns and has asked the human one
/// question.
pub struct Studio {
    pub data: tempfile::TempDir,
    pub service: Service,
    pub key: KeyPair,
    pub clock: Arc<AtomicI64>,
    pub advertised: Advertised,
    pub coordinator: String,
}
impl Studio {
    pub fn start(port: u16) -> Self {
        no_providers();
        let data = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(data.path()).unwrap();
        let project = host.create_project("Tide alerts").unwrap();
        host.set_live(&project.id, false).unwrap();
        let coordinator = (host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project.id)
            .unwrap()
            .id;
        host.request_attention(&coordinator, "host", "Merge", "Merge the alerts PR?", &[])
            .unwrap();
        drop(host);
        Self::from(data, port, coordinator)
    }
    pub fn from(data: tempfile::TempDir, port: u16, coordinator: String) -> Self {
        let key = KeyPair::generate().unwrap();
        let clock = Arc::new(AtomicI64::new(workspace_host::now()));
        let advertised = Advertised::default();
        let options = remote::Options {
            port,
            key: Box::new({
                let key = key.clone();
                move || Ok(key.clone())
            }),
            advertiser: Box::new(Recorder(advertised.clone())),
            name: Some("Studio Mac".into()),
            clock: Arc::new({
                let clock = clock.clone();
                move || clock.load(Ordering::Relaxed)
            }),
            binary: env!("CARGO_BIN_EXE_workspace-host").into(),
        };
        let service = Service::start_with(
            data.path().into(),
            env!("CARGO_BIN_EXE_workspace-host").into(),
            options,
        )
        .unwrap();
        Self {
            data,
            service,
            key,
            clock,
            advertised,
            coordinator,
        }
    }
    /// A command from the host itself.
    pub fn host(&self, command: Command) -> Result<Value, String> {
        let reply = self.service.request(command)?;
        reply.recv_blocking().map_err(|e| e.to_string())?
    }
    pub fn state(&self) -> RemoteAccess {
        serde_json::from_value(self.host(Command::RemoteAccess).unwrap()).unwrap()
    }
    /// Turns remote access on and returns the port.
    pub fn on(&self) -> u16 {
        self.host(Command::SetRemoteAccess { enabled: true })
            .unwrap();
        let state = self.state();
        state
            .port
            .unwrap_or_else(|| panic!("not listening: {:?}", state.error))
    }
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.state().port.unwrap())
    }
    pub fn open_pairing(&self) -> String {
        let state: RemoteAccess =
            serde_json::from_value(self.host(Command::OpenPairing).unwrap()).unwrap();
        state.pairing.unwrap().code
    }
    /// Pairs `device` through pairing mode opened at the host.
    pub fn pair(&self, device: &KeyPair, name: &str) -> PairedHost {
        let code = self.open_pairing();
        remote_client::pair(&self.address(), &code, device, name).unwrap()
    }
    pub fn connect(&self, device: &KeyPair) -> Result<Connection, RemoteError> {
        let socket = remote_client::connect(&self.address(), &self.key.public_key(), device)?;
        let mut client = Connection {
            socket,
            asked: 0,
            received: vec![],
        };
        client.hello();
        Ok(client)
    }
    pub fn devices(&self) -> Vec<PairedDevice> {
        self.state().devices
    }
    pub fn level(&self, device: &KeyPair, level: AccessLevel) {
        self.host(Command::SetDeviceLevel {
            device_id: device.id(),
            level,
        })
        .unwrap();
    }
}

/// A paired device's connection after the challenge.
pub struct Connection {
    pub socket: RemoteSocket,
    asked: u64,
    /// Every message the host sent, as received.
    pub received: Vec<String>,
}
impl Connection {
    pub fn hello(&mut self) {
        let hello = HelloLine {
            hello: Hello {
                protocols: [PROTOCOL_VERSION, PROTOCOL_VERSION],
                app_version: "test".into(),
                client: workspace_core::Client {
                    name: "Wiffletree".into(),
                    kind: "desktop".into(),
                },
                capabilities: vec![],
                credential: None,
            },
        };
        self.socket.send(&hello).unwrap();
        let answer: HelloAnswer = self.socket.receive().unwrap();
        assert!(matches!(answer, HelloAnswer::Welcome(_)), "{answer:?}");
    }
    pub fn send(&mut self, command: Command) -> String {
        self.asked += 1;
        let id = format!("r{}", self.asked);
        let request = Request {
            version: PROTOCOL_VERSION,
            id: id.clone(),
            command,
        };
        self.socket.send(&request).unwrap();
        id
    }
    /// The reply to `command`, skipping events.
    pub fn call(&mut self, command: Command) -> Result<Value, String> {
        let id = self.send(command);
        loop {
            let message: Value = self.socket.receive().map_err(|e| e.to_string())?;
            self.received.push(message.to_string());
            if message["id"] == id.as_str() {
                let response: Response = serde_json::from_value(message).unwrap();
                assert!(response.host_ms.is_some(), "every reply carries host_ms");
                return match response.error {
                    Some(error) => Err(error),
                    None => Ok(response.result.unwrap_or(Value::Null)),
                };
            }
        }
    }
    /// Whether the host has closed the connection.
    pub fn closed(&mut self) -> bool {
        self.socket
            .socket
            .get_mut()
            .sock
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        loop {
            match self.socket.socket.read() {
                Ok(tungstenite::Message::Close(_)) => return true,
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return false;
                }
                Err(_) => return true,
            }
        }
    }
}

pub fn send(id: &str, recipient: &str) -> Command {
    Command::Send {
        id: id.into(),
        sender: None,
        recipient: recipient.into(),
        body: "Look at the alerts".into(),
        attachments: vec![],
        uploads: vec![],
    }
}

pub fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
