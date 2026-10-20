//! Remote access: the listener, pairing, the challenge and pinning, advertising and the access
//! table, against a host in this process and devices over the network on this Mac.
mod remote_support;

use remote_support::*;
use serde_json::Value;
use std::{
    net::{Ipv4Addr, TcpListener},
    path::Path,
    sync::{Arc, atomic::Ordering},
};
use workspace_core::{
    pairing::{KeyPair, short_id},
    remote_client::{self, RemoteError},
    *,
};

#[test]
fn turning_remote_access_on_binds_47821() {
    // Wiffletree's own remote access, or another program, may hold the port on this Mac.
    if TcpListener::bind((Ipv4Addr::UNSPECIFIED, REMOTE_PORT)).is_err() {
        eprintln!("Skipped: port {REMOTE_PORT} is taken by another process on this machine");
        return;
    }
    let studio = Studio::start(REMOTE_PORT);
    assert_eq!(studio.on(), REMOTE_PORT);
    assert!(
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, REMOTE_PORT)).is_err(),
        "the host holds 47821"
    );
}

#[test]
fn off_by_default_binds_nothing_and_off_closes_a_live_connection() {
    let studio = Studio::start(0);
    let state = studio.state();
    assert!(!state.enabled && state.port.is_none(), "{state:?}");
    assert!(studio.advertised.lock().unwrap().is_none());
    let port = studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "MacBook");
    let mut client = studio.connect(&device).unwrap();
    client.call(Command::Snapshot).unwrap();

    studio
        .host(Command::SetRemoteAccess { enabled: false })
        .unwrap();

    assert!(client.closed(), "the live connection closes at once");
    assert!(
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok(),
        "the port is released"
    );
    let address = format!("127.0.0.1:{port}");
    assert!(remote_client::connect(&address, &studio.key.public_key(), &device).is_err());
    assert_eq!(studio.devices().len(), 1, "the device stays paired");
    assert!(studio.advertised.lock().unwrap().is_none());
    assert!(
        !studio.host(Command::Settings).unwrap()["remote_access"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn the_setting_is_kept_and_listens_again_when_the_host_restarts() {
    let studio = Studio::start(0);
    studio.on();
    studio.service.shutdown();
    let Studio {
        data, coordinator, ..
    } = studio;
    let restarted = Studio::from(data, 0, coordinator);
    assert!(restarted.state().enabled);
    assert!(restarted.state().port.is_some());
}

#[test]
fn pairing_with_the_code_stores_the_device_with_control_and_closes_pairing() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    let code = studio.open_pairing();

    let host =
        remote_client::pair(&studio.address(), &code, &device, "Michael's MacBook Air").unwrap();

    assert_eq!(host.name, "Studio Mac");
    assert_eq!(host.public_key, studio.key.public_key());
    assert_eq!(host.id, studio.key.id());
    let state = studio.state();
    assert!(state.pairing.is_none(), "pairing mode closes");
    let [paired] = &state.devices[..] else {
        panic!("{:?}", state.devices)
    };
    assert_eq!(
        (paired.name.as_str(), paired.level, paired.id.as_str()),
        (
            "Michael's MacBook Air",
            AccessLevel::Control,
            device.id().as_str()
        )
    );
    // The code is used once.
    let second = KeyPair::generate().unwrap();
    let reused = remote_client::pair(&studio.address(), &code, &second, "Office iMac");
    assert!(matches!(reused, Err(RemoteError::Refused(_))), "{reused:?}");
    assert_eq!(studio.devices().len(), 1);
}

#[test]
fn the_third_wrong_code_closes_pairing_and_qr_counts_no_try() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    let code = studio.open_pairing();
    let wrong = format!("{:04}", (code.parse::<u32>().unwrap() + 1) % 10_000);
    // A method the host does not offer is refused without counting a wrong try.
    let qr = qr_start(&studio.address(), &device);
    assert!(qr.contains("does not offer pairing by qr"), "{qr}");
    for _ in 0..2 {
        let failed = remote_client::pair(&studio.address(), &wrong, &device, "MacBook");
        assert!(matches!(failed, Err(RemoteError::Refused(_))), "{failed:?}");
        assert!(
            studio.state().pairing.is_some(),
            "still open after a wrong try"
        );
    }
    let third = remote_client::pair(&studio.address(), &wrong, &device, "MacBook");
    assert!(matches!(third, Err(RemoteError::Refused(_))), "{third:?}");
    assert!(
        studio.state().pairing.is_none(),
        "the third wrong try closes pairing"
    );
    let late = remote_client::pair(&studio.address(), &code, &device, "MacBook");
    assert!(late.is_err(), "the right code no longer works");
    assert!(studio.devices().is_empty());
}

/// Starts pairing by `qr` and returns the host's refusal.
fn qr_start(address: &str, device: &KeyPair) -> String {
    let mut socket = raw_pair_socket(address);
    let start = PairStart {
        method: PAIR_METHOD_QR.into(),
        device_name: "iPhone".into(),
        device_key: base64(&device.public_key()),
        spake: None,
    };
    socket
        .send(tungstenite::Message::text(
            serde_json::to_string(&start).unwrap(),
        ))
        .unwrap();
    let reply = socket.read().unwrap().into_text().unwrap();
    match serde_json::from_str::<PairReply>(&reply).unwrap() {
        PairReply::Refused { reason } => reason,
        other => panic!("{other:?}"),
    }
}

/// A WebSocket on the host's `/pair`, trusting any certificate.
fn raw_pair_socket(
    address: &str,
) -> tungstenite::WebSocket<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>> {
    let tcp = std::net::TcpStream::connect(address).unwrap();
    let tls = rustls::StreamOwned::new(trusting_client(), tcp);
    tungstenite::client::client(format!("ws://{address}{PAIR_PATH}").as_str(), tls)
        .unwrap()
        .0
}

fn trusting_client() -> rustls::ClientConnection {
    #[derive(Debug)]
    struct Anything;
    impl rustls::client::danger::ServerCertVerifier for Anything {
        fn verify_server_cert(
            &self,
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &[rustls::pki_types::CertificateDer<'_>],
            _: &rustls::pki_types::ServerName<'_>,
            _: &[u8],
            _: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            vec![rustls::SignatureScheme::ECDSA_NISTP256_SHA256]
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Anything))
        .with_no_client_auth();
    rustls::ClientConnection::new(Arc::new(config), "wiffletree-host".try_into().unwrap()).unwrap()
}

#[test]
fn a_code_typed_after_two_minutes_finds_pairing_closed() {
    let studio = Studio::start(0);
    studio.on();
    let code = studio.open_pairing();
    studio
        .clock
        .fetch_add(PAIRING_MS + 1_000, Ordering::Relaxed);
    let device = KeyPair::generate().unwrap();
    let expired = remote_client::pair(&studio.address(), &code, &device, "MacBook");
    match expired {
        Err(RemoteError::Refused(reason)) => assert!(reason.contains("not open"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(studio.state().pairing.is_none());
    assert!(studio.devices().is_empty());
}

#[test]
fn a_relaying_machine_with_its_own_certificate_fails_confirmation_on_both_sides() {
    let studio = Studio::start(0);
    studio.on();
    let code = studio.open_pairing();
    let relay = Relay::start(studio.address());
    let device = KeyPair::generate().unwrap();

    let relayed = remote_client::pair(&relay.address, &code, &device, "MacBook");

    // The host's confirmation check fails, so it refuses and the device stores nothing.
    match relayed {
        Err(RemoteError::Refused(reason)) => assert!(reason.contains("did not match"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(studio.devices().is_empty(), "the host stores nothing");
}

/// A machine in the middle: it answers TLS with its own key and relays the WebSocket messages
/// of one pairing exchange to the host.
struct Relay {
    address: String,
}
impl Relay {
    fn start(host: String) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let relay_key = KeyPair::generate().unwrap();
        let pkcs8 = relay_key.pkcs8_der().unwrap();
        let key_pair = rcgen::KeyPair::try_from(pkcs8.as_slice()).unwrap();
        let certificate = rcgen::CertificateParams::new(vec!["wiffletree-host".into()])
            .unwrap()
            .self_signed(&key_pair)
            .unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![certificate.der().clone()],
                    rustls::pki_types::PrivateKeyDer::Pkcs8(pkcs8.into()),
                )
                .unwrap(),
        );
        std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), tcp);
            let mut device = tungstenite::accept(tls).unwrap();
            let mut host = raw_pair_socket(&host);
            // Device, host, device, host: the four messages of the exchange.
            for turn in 0..4 {
                let (from, to) = match turn % 2 {
                    0 => (
                        &mut device as &mut dyn Relayed,
                        &mut host as &mut dyn Relayed,
                    ),
                    _ => (
                        &mut host as &mut dyn Relayed,
                        &mut device as &mut dyn Relayed,
                    ),
                };
                let Some(message) = from.next() else { return };
                to.put(message);
            }
        });
        Self { address }
    }
}
trait Relayed {
    fn next(&mut self) -> Option<tungstenite::Message>;
    fn put(&mut self, message: tungstenite::Message);
}
impl<S: std::io::Read + std::io::Write> Relayed for tungstenite::WebSocket<S> {
    fn next(&mut self) -> Option<tungstenite::Message> {
        loop {
            match self.read().ok()? {
                message @ tungstenite::Message::Text(_) => return Some(message),
                tungstenite::Message::Close(_) => return None,
                _ => {}
            }
        }
    }
    fn put(&mut self, message: tungstenite::Message) {
        let _ = self.send(message);
    }
}

#[test]
fn an_unknown_device_is_unpaired_and_a_changed_host_key_is_refused_before_any_request() {
    let studio = Studio::start(0);
    studio.on();
    let stranger = KeyPair::generate().unwrap();
    assert!(matches!(
        studio.connect(&stranger),
        Err(RemoteError::Unpaired)
    ));
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "MacBook");
    let other_host = KeyPair::generate().unwrap();
    let refused = remote_client::connect(&studio.address(), &other_host.public_key(), &device);
    assert!(
        matches!(refused, Err(RemoteError::KeyChanged)),
        "{:?}",
        refused.err()
    );
    let device_state = &studio.devices()[0];
    assert_eq!(
        device_state.last_connected_at, None,
        "the host never got as far as the challenge"
    );
    let connected = studio.connect(&device).unwrap();
    assert_eq!(
        connected.socket.level,
        AccessLevel::Control,
        "the verdict names the level"
    );
    assert!(studio.devices()[0].last_connected_at.is_some());
    studio.level(&device, AccessLevel::ViewOnly);
    let lowered = studio.connect(&device).unwrap();
    assert_eq!(lowered.socket.level, AccessLevel::ViewOnly);
}

#[test]
fn the_advertisement_has_its_fields_and_says_when_pairing_is_open() {
    let studio = Studio::start(0);
    let port = studio.on();
    let txt = || {
        let advertised = studio.advertised.lock().unwrap().clone().unwrap();
        assert_eq!((advertised.0.as_str(), advertised.1), ("Studio Mac", port));
        advertised
            .2
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let fields = txt();
    assert_eq!(fields["id"], short_id(&studio.key.id()));
    assert_eq!(fields["id"].len(), 16, "the first 8 bytes in hex");
    assert_eq!(fields["name"], "Studio Mac");
    assert_eq!(fields["version"], build_version());
    assert_eq!(fields["pairing"], "0");
    studio.open_pairing();
    assert_eq!(txt()["pairing"], "1");
    studio.host(Command::CancelPairing).unwrap();
    assert_eq!(txt()["pairing"], "0");
    studio
        .host(Command::SetRemoteAccess { enabled: false })
        .unwrap();
    assert!(
        studio.advertised.lock().unwrap().is_none(),
        "nothing while off"
    );
}

#[test]
fn view_only_reads_but_refuses_a_send_and_stores_nothing() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "Office iMac");
    studio.level(&device, AccessLevel::ViewOnly);
    let mut client = studio.connect(&device).unwrap();
    let coordinator = studio.coordinator.clone();

    let refused = client.call(send("view-only", &coordinator)).unwrap_err();

    assert_eq!(
        refused,
        "View only: Studio Mac refuses changes from this device"
    );
    for read in [
        Command::Snapshot,
        Command::Messages {
            session_id: coordinator.clone(),
            before: None,
            limit: 50,
        },
        Command::Settings,
    ] {
        client.call(read).unwrap();
    }
    let messages = studio
        .host(Command::Messages {
            session_id: coordinator,
            before: None,
            limit: 50,
        })
        .unwrap();
    assert!(
        !messages.to_string().contains("view-only"),
        "nothing is stored: {messages}"
    );
}

#[test]
fn no_remote_device_can_grant_access_or_stop_the_host() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "MacBook");
    let mut client = studio.connect(&device).unwrap();
    for command in [
        Command::OpenPairing,
        Command::SetDeviceLevel {
            device_id: device.id(),
            level: AccessLevel::Control,
        },
        Command::RemoveDevice {
            device_id: device.id(),
        },
        Command::SetRemoteAccess { enabled: false },
        Command::RemoteAccess,
    ] {
        assert_eq!(
            client.call(command).unwrap_err(),
            "Only Studio Mac itself can change remote access"
        );
    }
    for command in [
        Command::StopAgentsAndQuit,
        Command::RestartHost {
            when: RestartWhen::Now,
        },
    ] {
        assert!(client.call(command).is_err());
    }
    assert!(studio.state().pairing.is_none());
    client.call(Command::Snapshot).unwrap();
}

#[test]
fn path_rules_bound_what_a_device_reads_and_lists() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "MacBook");
    let mut client = studio.connect(&device).unwrap();
    let home = std::env::var("HOME").unwrap();
    // Control may list the home folder, to choose a repository.
    client
        .call(Command::ListFolder {
            path: home.clone().into(),
        })
        .unwrap();
    // No device reads host paths as attachments.
    let inspect = client.call(Command::InspectAttachments {
        paths: vec![Path::new(&home).join(".ssh/id_ed25519")],
    });
    assert!(inspect.is_err());
    // A symlink out of a project folder is refused.
    let workspace = studio.host(Command::Settings).unwrap()["workspaces_dir"]
        .as_str()
        .unwrap()
        .to_owned();
    let worktree = tempfile::tempdir_in(&workspace).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", worktree.path().join("creds")).unwrap();
    let read = client.call(Command::ReadFile {
        path: worktree.path().join("creds"),
        max_bytes: 1024,
    });
    assert!(read.is_err(), "{read:?}");
    let resolved = client
        .call(Command::ResolvePaths {
            references: vec!["creds".into(), "/etc/hosts".into()],
            bases: vec![worktree.path().into()],
        })
        .unwrap();
    assert_eq!(
        resolved,
        serde_json::json!([null, null]),
        "outside the roots"
    );

    studio.level(&device, AccessLevel::ViewOnly);
    let mut client = studio.connect(&device).unwrap();
    let listed = client.call(Command::ListFolder { path: home.into() });
    assert!(listed.is_err(), "View only cannot list the home folder");
    let scan = client.call(Command::DiscoverRepositories {
        folders: vec![workspace],
    });
    assert_eq!(
        scan.unwrap_err(),
        "View only: Studio Mac refuses changes from this device"
    );
}

#[test]
fn changing_a_level_or_removing_a_device_closes_its_connection_at_once() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "Michael's MacBook Air");
    let mut client = studio.connect(&device).unwrap();
    until("the device to show as connected", || {
        studio.devices()[0].connected
    });

    studio.level(&device, AccessLevel::ViewOnly);

    assert!(client.closed());
    let mut client = studio.connect(&device).unwrap();
    assert!(
        client
            .call(send("lowered", &studio.coordinator))
            .unwrap_err()
            .starts_with("View only")
    );

    studio
        .host(Command::RemoveDevice {
            device_id: device.id(),
        })
        .unwrap();

    assert!(client.closed());
    assert!(matches!(
        studio.connect(&device),
        Err(RemoteError::Unpaired)
    ));
    assert!(studio.devices().is_empty());
}

#[test]
fn a_device_subscribed_to_attention_hears_only_the_count() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "MacBook");
    let mut client = studio.connect(&device).unwrap();
    let attention_only = Command::Subscribe {
        after: None,
        kinds: Some(vec![EventKindName::Attention]),
    };
    client.send(attention_only);
    let mut events = vec![];
    let mut next = |client: &mut Connection| loop {
        let message: Value = client.socket.receive().unwrap();
        if let Some(event) = message.get("event") {
            let event: Event = serde_json::from_value(event.clone()).unwrap();
            events.push(event.kind.clone());
            return event.kind;
        }
    };
    assert_eq!(
        next(&mut client),
        EventKind::Attention { count: 1 },
        "the count at once"
    );
    // A rename changes state, which this subscriber does not hear about; answering the
    // question changes the count, which it does.
    let snapshot = client.call(Command::Snapshot).unwrap();
    let project = snapshot["projects"][0]["id"].as_str().unwrap().to_owned();
    studio
        .host(Command::RenameProject {
            project_id: project,
            name: "Tide alerts, renamed".into(),
        })
        .unwrap();
    let question = snapshot["attention"][0]["id"].as_str().unwrap().to_owned();
    client.send(Command::DismissAttention { id: question });
    assert_eq!(next(&mut client), EventKind::Attention { count: 0 });
    assert!(
        events
            .iter()
            .all(|kind| matches!(kind, EventKind::Attention { .. })),
        "{events:?}"
    );
}

#[test]
fn an_upload_resumes_after_a_drop_and_is_stored_once_with_its_message() {
    let studio = Studio::start(0);
    studio.on();
    let device = KeyPair::generate().unwrap();
    studio.pair(&device, "MacBook");
    let file: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let upload = |client: &mut Connection, offset: usize| -> Uploaded {
        let end = (offset + UPLOAD_PIECE_BYTES).min(file.len());
        let piece = client.call(Command::UploadAttachment {
            upload_id: "u1".into(),
            name: "shot.png".into(),
            offset: offset as u64,
            bytes: base64(&file[offset..end]),
        });
        serde_json::from_value(piece.unwrap()).unwrap()
    };
    let mut client = studio.connect(&device).unwrap();
    assert_eq!(upload(&mut client, 0).stored, UPLOAD_PIECE_BYTES as u64);
    // The connection drops, and the device sends the first piece again.
    drop(client);
    let mut client = studio.connect(&device).unwrap();
    let mut stored = upload(&mut client, 0).stored as usize;
    assert_eq!(stored, UPLOAD_PIECE_BYTES, "the piece is stored once");
    while stored < file.len() {
        stored = upload(&mut client, stored).stored as usize;
    }
    let message = Command::Send {
        id: "with-shot".into(),
        sender: None,
        recipient: studio.coordinator.clone(),
        body: "Here is the screenshot".into(),
        attachments: vec![],
        uploads: vec!["u1".into()],
    };
    let sent: Message = serde_json::from_value(client.call(message.clone()).unwrap()).unwrap();
    let [attachment] = &sent.attachments[..] else {
        panic!("{:?}", sent.attachments)
    };
    assert_eq!(
        (attachment.name().as_str(), attachment.size),
        ("shot.png", 300_000)
    );
    assert_eq!(std::fs::read(&attachment.path).unwrap(), file);
    assert!(
        !studio.data.path().join("uploads/u1").exists(),
        "the upload is used up"
    );
    // Its reply lost, the message is sent again with its id and stored once.
    client.call(message).unwrap();
    let messages = studio
        .host(Command::Messages {
            session_id: studio.coordinator.clone(),
            before: None,
            limit: 50,
        })
        .unwrap();
    let stored = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["id"] == "with-shot");
    assert_eq!(stored.count(), 1, "{messages}");
}
