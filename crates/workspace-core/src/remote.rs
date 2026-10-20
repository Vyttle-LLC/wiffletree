//! Remote access: paired devices drive a host over TLS and WebSocket, as
//! `openspec/changes/remote-hosts/design.md` describes. These are the types both sides share:
//! access levels, the host's remote-access state, uploads, and the JSON messages of the pairing
//! exchange on `/pair` and the challenge on `/v1`.
use serde::{Deserialize, Serialize};

/// The TCP port a host listens on while remote access is on.
pub const REMOTE_PORT: u16 = 47821;
/// The Bonjour service type a host advertises.
pub const SERVICE_TYPE: &str = "_wiffletree._tcp";
/// The WebSocket path for pairing, served only while pairing mode is open.
pub const PAIR_PATH: &str = "/pair";
/// The WebSocket path for paired devices.
pub const V1_PATH: &str = "/v1";
/// How long pairing mode stays open.
pub const PAIRING_MS: i64 = 2 * 60 * 1000;
/// Wrong tries that close pairing mode.
pub const PAIRING_TRIES: u32 = 3;
/// The largest piece of an upload, which keeps each request under the line bound once base64.
pub const UPLOAD_PIECE_BYTES: usize = 128 * 1024;
/// How long the host keeps an upload no message has named.
pub const UPLOAD_KEPT_MS: i64 = 60 * 60 * 1000;

/// What a paired device may do on a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    /// Every workspace command, as the human at the host could give it.
    Control,
    /// Reads, the handshake and the event subscription only.
    ViewOnly,
}
impl AccessLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Control => "Control",
            Self::ViewOnly => "View only",
        }
    }
}

/// A device paired with this host, as the host stores it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    /// The SHA-256 of its public key, in hex.
    pub id: String,
    pub name: String,
    pub level: AccessLevel,
    /// Milliseconds since the epoch.
    pub paired_at: i64,
    pub last_connected_at: Option<i64>,
    /// It has a connection open now.
    pub connected: bool,
}

/// Pairing mode while it is open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pairing {
    /// The 4-digit code the human reads out.
    pub code: String,
    /// When pairing mode closes, in milliseconds since the epoch.
    pub expires_at: i64,
}

/// `RemoteAccess`'s reply, and that of every command that changes remote access.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAccess {
    /// "Let other devices use this Mac's Wiffletree".
    pub enabled: bool,
    /// The name devices see.
    pub host_name: String,
    /// The SHA-256 of the host's public key, in hex; empty until the key is first needed.
    pub host_id: String,
    /// Names and addresses devices can reach this Mac at.
    pub addresses: Vec<String>,
    /// The port listened on, while listening.
    pub port: Option<u16>,
    /// Why the host is not listening although remote access is on.
    pub error: Option<String>,
    pub pairing: Option<Pairing>,
    pub devices: Vec<PairedDevice>,
}

/// `UploadAttachment`'s reply: how many bytes of the upload the host holds, where the next
/// piece starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uploaded {
    pub upload_id: String,
    pub stored: u64,
}

/// The first message on `/pair`, naming its method. `code` sends its SPAKE2 message; the host
/// refuses a method it does not offer without counting a wrong try.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairStart {
    pub method: String,
    pub device_name: String,
    /// The device's P-256 public key, SEC1 uncompressed, base64.
    pub device_key: String,
    /// The device's SPAKE2 message, base64, for `code`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spake: Option<String>,
}
/// The pairing method this host offers.
pub const PAIR_METHOD_CODE: &str = "code";
/// The iPhone's method, specified but not yet offered.
pub const PAIR_METHOD_QR: &str = "qr";

/// The host's messages on `/pair`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairReply {
    /// The host's SPAKE2 message and name; the device answers with `PairConfirm`.
    Spake {
        spake: String,
        host_name: String,
    },
    /// The device's confirmation held: the host stored it with Control. Carries the host's own
    /// confirmation, which the device checks before storing the host.
    Paired {
        confirm: String,
        host_id: String,
    },
    Refused {
        reason: String,
    },
}
/// The device's key confirmation, an HMAC over the transcript, base64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairConfirm {
    pub confirm: String,
}

/// The host's first message on `/v1`: a fresh random challenge, base64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    pub challenge: String,
    pub host_id: String,
}
/// The device's answer: its id and its ECDSA P-256 signature over the challenge, the host's id
/// and its own, as `pairing::challenge_message` lays out, in raw `r ‖ s` form, base64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeAnswer {
    pub device_id: String,
    pub signature: String,
}
/// The host's verdict on the answer. After `Accepted` the connection carries `Hello` and then
/// the host protocol, one JSON message per text frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeVerdict {
    Accepted {
        level: AccessLevel,
    },
    /// The device is unknown or was removed; it must pair again.
    Unpaired,
}

/// The reason a host closes a device's connection when the device is unknown or removed.
pub const UNPAIRED: &str = "unpaired";
