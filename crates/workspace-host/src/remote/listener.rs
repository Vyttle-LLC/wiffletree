//! The TCP listener: TLS 1.3 with a self-signed certificate carrying the host's key, then a
//! WebSocket on `/pair` or `/v1`.
use super::{Identity, Shared, ToActor, pairing, websocket};
use anyhow::{Context, Result, ensure};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use std::{
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tungstenite::{
    WebSocket,
    handshake::server::{Request, Response},
    protocol::WebSocketConfig,
};
use workspace_core::{MAX_LINE_BYTES, PAIR_PATH, V1_PATH};

/// Remote connections open at once, pairing included.
const MAX_CONNECTIONS: usize = 64;
/// How long the TLS and WebSocket handshakes, and each step before the host protocol, may take.
pub(super) const STEP_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the actor looks for uploads to expire while the host listens.
const TICK: Duration = Duration::from_secs(60);

pub(super) type ServerSocket = WebSocket<StreamOwned<ServerConnection, TcpStream>>;

/// Accepts connections until dropped; the port is free again once it is.
pub(crate) struct Listening {
    pub port: u16,
    stopping: Arc<AtomicBool>,
    accepting: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Listening {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        // Wakes the accepting thread, which then stops and closes the listener.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.join();
        }
    }
}

/// Listens on every interface at `port`, or a free one for `0`.
pub(super) fn listen(port: u16, identity: Arc<Identity>, shared: Arc<Shared>) -> Result<Listening> {
    let config = Arc::new(tls_config(&identity)?);
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .with_context(|| format!("Listen on port {port}"))?;
    let port = listener.local_addr()?.port();
    let stopping = Arc::new(AtomicBool::new(false));
    let (stop, requests) = (stopping.clone(), shared.requests.clone());
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) && requests.tell(ToActor::Tick).is_ok() {
            std::thread::sleep(TICK);
        }
    });
    let stop = stopping.clone();
    let accepting = std::thread::Builder::new()
        .name("remote-listener".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if shared.connections().len() >= MAX_CONNECTIONS {
                    continue;
                }
                let (config, identity, shared) = (config.clone(), identity.clone(), shared.clone());
                std::thread::spawn(move || {
                    if let Err(error) = serve(stream, config, &identity, &shared) {
                        eprintln!("Remote connection: {error:#}");
                    }
                });
            }
        })?;
    Ok(Listening {
        port,
        stopping,
        accepting: Some(accepting),
    })
}

/// A server configuration for TLS 1.3 only, with a self-signed certificate for the host's key.
fn tls_config(identity: &Identity) -> Result<ServerConfig> {
    let pkcs8 = identity.key.pkcs8_der()?;
    let key_pair = rcgen::KeyPair::try_from(pkcs8.as_slice())?;
    let certificate =
        rcgen::CertificateParams::new(vec!["wiffletree-host".into()])?.self_signed(&key_pair)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8)),
        )?)
}

/// The WebSocket limits: requests are bounded as on the socket, replies are not.
pub(super) fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_LINE_BYTES))
        .max_frame_size(Some(MAX_LINE_BYTES))
        .max_write_buffer_size(4 << 20)
}

fn serve(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    identity: &Identity,
    shared: &Arc<Shared>,
) -> Result<()> {
    let connection = shared.track(&stream)?;
    // Forgotten however serving ends, a panic included, so no slot is kept.
    let _tracked = Tracked(shared, connection);
    (|| {
        stream.set_read_timeout(Some(STEP_TIMEOUT))?;
        stream.set_write_timeout(Some(STEP_TIMEOUT))?;
        let mut tls = StreamOwned::new(ServerConnection::new(config)?, stream);
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock)?;
        }
        let mut path = String::new();
        // tungstenite's handshake callback fixes the error type.
        #[allow(clippy::result_large_err)]
        let route = |request: &Request, response: Response| {
            path = request.uri().path().to_owned();
            Ok(response)
        };
        let socket = tungstenite::accept_hdr_with_config(tls, route, Some(websocket_config()))
            .map_err(|error| anyhow::anyhow!("The WebSocket handshake failed: {error}"))?;
        ensure!(
            path == PAIR_PATH || path == V1_PATH,
            "No WebSocket at {path}"
        );
        match path.as_str() {
            PAIR_PATH => pairing::serve(socket, identity, shared),
            _ => websocket::serve(socket, connection, identity, shared),
        }
    })()
}

struct Tracked<'a>(&'a Shared, u64);
impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.0.forget(self.1);
    }
}
