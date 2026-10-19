//! The socket protocol around `Request` and `Response`: the handshake that opens a connection,
//! the event stream and the replies of the host-file commands. `docs/host-protocol.md` is its
//! reference.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The protocol this build speaks on the host socket.
pub const PROTOCOL_VERSION: u32 = 2;
/// Scripts on the host's standard input speak version 1, with no handshake.
pub const STDIN_PROTOCOL: u32 = 1;
/// A host accepts its own protocol and the one before it.
pub const PROTOCOLS: [u32; 2] = [PROTOCOL_VERSION - 1, PROTOCOL_VERSION];
/// The longest line a host reads, a request or a `Hello`, in bytes.
pub const MAX_LINE_BYTES: usize = 256 * 1024;
/// The most `ReadFile` returns of one file, as for an attachment.
pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// One short string per optional feature a side supports.
pub mod capability {
    /// `Subscribe` and its events.
    pub const EVENTS: &str = "events";
    /// `ResolvePaths` and `ReadFile`.
    pub const FILES: &str = "files";
    /// `ListFolder`.
    pub const FOLDERS: &str = "folders";
    /// `RestartHost { when: Idle }`.
    pub const RESTART_WHEN_IDLE: &str = "restart-when-idle";
}

/// The newest protocol both ranges, each `[oldest, newest]`, include.
pub fn choose_protocol(host: [u32; 2], client: [u32; 2]) -> Option<u32> {
    let newest = host[1].min(client[1]);
    (newest >= host[0].max(client[0])).then_some(newest)
}

/// The first line of a connection, `{"hello": {...}}`. Its shape never changes between
/// protocol versions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloLine {
    pub hello: Hello,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The oldest and newest protocol versions the client speaks.
    pub protocols: [u32; 2],
    pub app_version: String,
    pub client: Client,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The app's client credential; without it the human-only commands are refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Client {
    pub name: String,
    /// What the client is, such as `desktop` or `script`; the host only reports it.
    pub kind: String,
}
/// The host's answer to `Hello`: `{"welcome": {...}}` or `{"refused": {...}}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelloAnswer {
    Welcome(Welcome),
    Refused(Refusal),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// The protocol every later request and reply on the connection uses.
    pub protocol: u32,
    /// Stays the same for a data folder across restarts.
    pub host_id: String,
    pub host_version: String,
    /// New each time the host starts; event cursors of another epoch cannot resume.
    pub epoch: String,
    pub capabilities: Vec<String>,
    /// The binary the host runs from.
    pub binary: PathBuf,
}
/// No protocol in common. After it the connection may send only `RestartHost`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub host_protocols: [u32; 2],
    pub client_protocols: [u32; 2],
    pub host_version: String,
    /// How many agents are in a turn.
    pub working: usize,
}

/// Where a subscriber is in the host's event stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub epoch: String,
    /// Grows by one per event within an epoch; `0` is before the first.
    pub seq: u64,
}
/// What to fetch again. Events carry no workspace state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// Reload the snapshot.
    State,
    /// Fetch these sessions' steps.
    Steps { sessions: Vec<String> },
    /// Fetch progress.
    Progress,
    /// Fetch usage and quotas.
    Usage,
    /// Events were lost: take a fresh snapshot, then continue from this event's cursor.
    Resync,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub cursor: Cursor,
    #[serde(flatten)]
    pub kind: EventKind,
}
/// An event as a connection carries it, `{"event": {...}}`, beside replies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLine {
    pub event: Event,
}
/// `Subscribe`'s reply: the cursor of the host's latest event when the subscription began.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscribed {
    pub cursor: Cursor,
}

/// A reference `ResolvePaths` found as a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPath {
    pub path: PathBuf,
    /// Opening it would run it: an executable, or a script Terminal runs.
    pub runs_when_opened: bool,
}
/// `ReadFile`'s reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContents {
    /// The path once symlinks and `..` are resolved.
    pub path: PathBuf,
    /// The file's bytes, base64-encoded.
    pub data: String,
}
/// `ListFolder`'s reply: the folders inside `path`, by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderListing {
    pub path: PathBuf,
    pub folders: Vec<FolderEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub name: String,
    /// The folder is a Git repository.
    pub repository: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartWhen {
    /// Once no turn and no compaction runs.
    Idle,
    /// At once, stopping running turns as a quit does.
    Now,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_common_protocol_is_chosen() {
        assert_eq!(
            choose_protocol(PROTOCOLS, PROTOCOLS),
            Some(PROTOCOL_VERSION)
        );
        let ahead = [PROTOCOL_VERSION, PROTOCOL_VERSION + 1];
        assert_eq!(choose_protocol(PROTOCOLS, ahead), Some(PROTOCOL_VERSION));
        let behind = [PROTOCOL_VERSION - 1, PROTOCOL_VERSION - 1];
        assert_eq!(
            choose_protocol(PROTOCOLS, behind),
            Some(PROTOCOL_VERSION - 1)
        );
        let far_ahead = [PROTOCOL_VERSION + 1, PROTOCOL_VERSION + 2];
        assert_eq!(choose_protocol(PROTOCOLS, far_ahead), None);
    }

    #[test]
    fn events_flatten_their_kind_beside_the_cursor() {
        let event = Event {
            cursor: Cursor {
                epoch: "e".into(),
                seq: 3,
            },
            kind: EventKind::Steps {
                sessions: vec!["s".into()],
            },
        };
        let line = serde_json::to_value(EventLine {
            event: event.clone(),
        })
        .unwrap();
        assert_eq!(
            line,
            serde_json::json!({"event":{"cursor":{"epoch":"e","seq":3},"kind":"steps","sessions":["s"]}})
        );
        assert_eq!(
            serde_json::from_value::<EventLine>(line).unwrap().event,
            event
        );
    }
}
