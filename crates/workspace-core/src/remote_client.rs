//! A device's side of the network transport: TLS 1.3 to a host, pinned to the host's key once
//! paired, a WebSocket over it, the `code` pairing exchange on `/pair`, and the challenge on
//! `/v1` that opens a connection for the host protocol.
use crate::pairing::{self, CodeExchange, KeyPair, Side};
use crate::*;
use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fmt, io,
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex},
    time::Duration,
};
use tungstenite::{Message, WebSocket};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long each step of pairing or the challenge may wait for the host.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

pub type RemoteWebSocket = WebSocket<StreamOwned<ClientConnection, TcpStream>>;

/// Why a connection to a host failed.
#[derive(Debug)]
pub enum RemoteError {
    /// The host's certificate carries another key than the one stored at pairing. Nothing was
    /// sent.
    KeyChanged,
    /// The host does not know this device, or removed it.
    Unpaired,
    /// The host refused pairing, with its reason.
    Refused(String),
    /// A key confirmation failed: a wrong code, or a machine in the middle.
    Confirmation,
    Other(anyhow::Error),
}
impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyChanged => write!(f, "The host's key has changed"),
            Self::Unpaired => write!(f, "The host no longer accepts this device"),
            Self::Refused(reason) => write!(f, "{reason}"),
            Self::Confirmation => write!(f, "The code did not match"),
            Self::Other(error) => write!(f, "{error:#}"),
        }
    }
}
impl std::error::Error for RemoteError {}
impl From<anyhow::Error> for RemoteError {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}
impl From<io::Error> for RemoteError {
    fn from(error: io::Error) -> Self {
        Self::Other(error.into())
    }
}
impl From<tungstenite::Error> for RemoteError {
    fn from(error: tungstenite::Error) -> Self {
        Self::Other(error.into())
    }
}

/// A host this device paired with, as the client stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedHost {
    pub id: String,
    /// SEC1 uncompressed.
    pub public_key: Vec<u8>,
    pub name: String,
}

/// A connection the host accepted after the challenge, ready for `Hello`.
pub struct RemoteSocket {
    pub socket: RemoteWebSocket,
    pub level: AccessLevel,
}
impl RemoteSocket {
    pub fn send(&mut self, value: &impl Serialize) -> Result<(), RemoteError> {
        send(&mut self.socket, value)
    }
    pub fn receive<T: DeserializeOwned>(&mut self) -> Result<T, RemoteError> {
        receive(&mut self.socket)
    }
}

/// Pairs with the host at `address`, `name[:port]`, with the code it shows. The host's key is
/// taken from its certificate and confirmed through the exchange.
pub fn pair(
    address: &str,
    code: &str,
    device: &KeyPair,
    device_name: &str,
) -> Result<PairedHost, RemoteError> {
    let device_name = &pairing::device_name(device_name);
    let (mut socket, host_key) = open(address, None, PAIR_PATH)?;
    let (exchange, message) = CodeExchange::start(Side::Device, code)?;
    send(
        &mut socket,
        &PairStart {
            method: PAIR_METHOD_CODE.into(),
            device_name: device_name.into(),
            device_key: BASE64.encode(device.public_key()),
            spake: Some(BASE64.encode(message)),
        },
    )?;
    let (spake, host_name) = match receive(&mut socket)? {
        PairReply::Spake { spake, host_name } => (spake, host_name),
        PairReply::Refused { reason } => return Err(RemoteError::Refused(reason)),
        PairReply::Paired { .. } => return Err(anyhow!("The host answered out of turn").into()),
    };
    let key = exchange.finish(&decode(&spake)?)?;
    let transcript = pairing::transcript(&host_key, &device.public_key(), device_name, &host_name);
    let confirm = pairing::confirmation(&key, Side::Device, &transcript);
    send(
        &mut socket,
        &PairConfirm {
            confirm: BASE64.encode(confirm),
        },
    )?;
    match receive(&mut socket)? {
        PairReply::Paired { confirm, host_id }
            if pairing::confirms(&key, Side::Host, &transcript, &decode(&confirm)?)
                && host_id == pairing::key_id(&host_key) =>
        {
            let _ = socket.close(None);
            Ok(PairedHost {
                id: host_id,
                public_key: host_key,
                name: host_name,
            })
        }
        PairReply::Paired { .. } => Err(RemoteError::Confirmation),
        PairReply::Refused { reason } => Err(RemoteError::Refused(reason)),
        PairReply::Spake { .. } => Err(anyhow!("The host answered out of turn").into()),
    }
}

/// Connects to a paired host at `address`, refusing a certificate without `host_key`, and
/// answers its challenge with `device`'s key.
pub fn connect(
    address: &str,
    host_key: &[u8],
    device: &KeyPair,
) -> Result<RemoteSocket, RemoteError> {
    let (mut socket, _) = open(address, Some(host_key), V1_PATH)?;
    let challenge: Challenge = receive(&mut socket)?;
    if challenge.host_id != pairing::key_id(host_key) {
        return Err(RemoteError::KeyChanged);
    }
    let signed = pairing::challenge_message(
        &decode(&challenge.challenge)?,
        &challenge.host_id,
        &device.id(),
    )?;
    send(
        &mut socket,
        &ChallengeAnswer {
            device_id: device.id(),
            signature: BASE64.encode(device.sign(&signed)),
        },
    )?;
    match receive(&mut socket) {
        Ok(ChallengeVerdict::Accepted { level }) => {
            socket.get_mut().sock.set_read_timeout(None)?;
            Ok(RemoteSocket { socket, level })
        }
        Ok(ChallengeVerdict::Unpaired) => Err(RemoteError::Unpaired),
        Err(error) => Err(error),
    }
}

/// `name[:port]` or `[v6]:port`, with the default port when none is given.
pub fn split_address(address: &str) -> (String, u16) {
    let address = address.trim();
    if let Some(rest) = address.strip_prefix('[')
        && let Some((host, port)) = rest.split_once(']')
    {
        let port = port.strip_prefix(':').and_then(|p| p.parse().ok());
        return (host.into(), port.unwrap_or(REMOTE_PORT));
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host.into(), port),
            Err(_) => (address.into(), REMOTE_PORT),
        },
        _ => (address.into(), REMOTE_PORT),
    }
}

/// TCP, then TLS 1.3 checked by `pin`, then a WebSocket on `path`. Returns the key the host's
/// certificate carries.
fn open(
    address: &str,
    pinned: Option<&[u8]>,
    path: &str,
) -> Result<(RemoteWebSocket, Vec<u8>), RemoteError> {
    let (host, port) = split_address(address);
    let target = (host.as_str(), port)
        .to_socket_addrs()?
        .next()
        .with_context(|| format!("{host} has no address"))?;
    let tcp = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)?;
    tcp.set_read_timeout(Some(STEP_TIMEOUT))?;
    tcp.set_write_timeout(Some(STEP_TIMEOUT))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Pin {
        expected: pinned.map(<[u8]>::to_vec),
        seen: Mutex::default(),
        provider: provider.clone(),
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(anyhow::Error::from)?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    let name = ServerName::try_from("wiffletree-host").expect("a valid name");
    let connection = ClientConnection::new(Arc::new(config), name).map_err(anyhow::Error::from)?;
    let mut tls = StreamOwned::new(connection, tcp);
    // The handshake completes before anything is sent, so a changed key is refused first.
    while tls.conn.is_handshaking() {
        if let Err(error) = tls.conn.complete_io(&mut tls.sock) {
            let seen = verifier
                .seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            return Err(match (seen, pinned) {
                (Some(seen), Some(pinned)) if seen != pinned => RemoteError::KeyChanged,
                _ => error.into(),
            });
        }
    }
    let host_key = (verifier
        .seen
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone())
    .context("The host sent no certificate")?;
    let url = match host.contains(':') {
        true => format!("ws://[{host}]:{port}{path}"),
        false => format!("ws://{host}:{port}{path}"),
    };
    let (socket, _) = tungstenite::client::client(url.as_str(), tls)
        .map_err(|error| anyhow!("The WebSocket handshake failed: {error}"))?;
    Ok((socket, host_key))
}

fn send(socket: &mut RemoteWebSocket, value: &impl Serialize) -> Result<(), RemoteError> {
    socket.send(Message::text(
        serde_json::to_string(value).map_err(anyhow::Error::from)?,
    ))?;
    Ok(())
}

/// The next text message, as `T`. A close whose reason is `unpaired` is `Unpaired`.
fn receive<T: DeserializeOwned>(socket: &mut RemoteWebSocket) -> Result<T, RemoteError> {
    loop {
        match socket.read()? {
            Message::Text(text) => {
                return serde_json::from_str(&text).map_err(|error| {
                    anyhow!("The host sent an unexpected message: {error}").into()
                });
            }
            Message::Close(Some(frame)) if frame.reason.as_str() == UNPAIRED => {
                return Err(RemoteError::Unpaired);
            }
            Message::Close(_) => return Err(anyhow!("The host closed the connection").into()),
            _ => {}
        }
    }
}

fn decode(text: &str) -> anyhow::Result<Vec<u8>> {
    BASE64
        .decode(text)
        .context("The host sent malformed base64")
}

/// The SEC1 uncompressed P-256 key a certificate carries.
fn certificate_key(certificate: &[u8]) -> anyhow::Result<Vec<u8>> {
    use p256::pkcs8::DecodePublicKey;
    let der = CertificateDer::from(certificate);
    let parsed = webpki::EndEntityCert::try_from(&der).map_err(|e| anyhow!("{e:?}"))?;
    let spki = parsed.subject_public_key_info();
    let key = p256::PublicKey::from_public_key_der(spki.as_ref())
        .map_err(|e| anyhow!("The certificate's key is not P-256: {e}"))?;
    Ok(key.to_sec1_bytes().into_vec())
}

/// Accepts the host's certificate when it carries the pinned key, or any key while pairing,
/// noting the key either way. The handshake's signature is still checked against it.
#[derive(Debug)]
struct Pin {
    expected: Option<Vec<u8>>,
    seen: Mutex<Option<Vec<u8>>>,
    provider: Arc<CryptoProvider>,
}
impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let key = certificate_key(end_entity).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        let matches = self
            .expected
            .as_ref()
            .is_none_or(|expected| *expected == key);
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
        match matches {
            true => Ok(ServerCertVerified::assertion()),
            false => Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )),
        }
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(
            rustls::PeerIncompatible::Tls12NotOffered,
        ))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_take_the_default_port_unless_they_name_one() {
        let split = |a: &str| split_address(a);
        assert_eq!(split("studio.local"), ("studio.local".into(), REMOTE_PORT));
        assert_eq!(
            split("studio.tail4c2e.ts.net:9000"),
            ("studio.tail4c2e.ts.net".into(), 9000)
        );
        assert_eq!(split("[fe80::1]:9000"), ("fe80::1".into(), 9000));
        assert_eq!(split("[fe80::1]"), ("fe80::1".into(), REMOTE_PORT));
        assert_eq!(split("10.0.0.2"), ("10.0.0.2".into(), REMOTE_PORT));
    }
}
