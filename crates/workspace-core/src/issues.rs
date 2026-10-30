//! Shared issues (OpenSpec change `shared-issues`): records of ideas, bugs and requests kept in a
//! Space's storage repository, its tracker. An issue is not a ticket: tickets stay local.
use crate::{Extra, Paused, SpaceId};
use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// An issue's ID: numbered once it reached its tracker, such as `CEL-152`, or temporary until
/// then, such as `CEL-new-qxvb`. Prefixes are unique on a Mac, so an ID names its tracker too.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IssueId {
    Numbered { prefix: String, number: u64 },
    Temporary { prefix: String, letters: String },
}
impl IssueId {
    pub const TEMPORARY_LETTERS: usize = 4;
    pub fn prefix(&self) -> &str {
        match self {
            Self::Numbered { prefix, .. } | Self::Temporary { prefix, .. } => prefix,
        }
    }
    pub fn is_temporary(&self) -> bool {
        matches!(self, Self::Temporary { .. })
    }
    pub fn parse(text: &str) -> Result<Self> {
        let Some((prefix, rest)) = text.split_once('-') else {
            bail!("{text} is not an issue ID");
        };
        if !(2..=5).contains(&prefix.len()) || !prefix.bytes().all(|b| b.is_ascii_uppercase()) {
            bail!("{text} is not an issue ID");
        }
        let prefix = prefix.to_owned();
        if let Some(letters) = rest.strip_prefix("new-") {
            if letters.len() == Self::TEMPORARY_LETTERS
                && letters.bytes().all(|b| b.is_ascii_lowercase())
            {
                let letters = letters.to_owned();
                return Ok(Self::Temporary { prefix, letters });
            }
            bail!("{text} is not an issue ID");
        }
        match rest.parse() {
            Ok(number) if !rest.starts_with(['0', '+']) => Ok(Self::Numbered { prefix, number }),
            _ => bail!("{text} is not an issue ID"),
        }
    }
}
impl fmt::Display for IssueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Numbered { prefix, number } => write!(f, "{prefix}-{number}"),
            Self::Temporary { prefix, letters } => write!(f, "{prefix}-new-{letters}"),
        }
    }
}
impl Serialize for IssueId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for IssueId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// The fixed states, in order. A tracker listing others is refused. A new issue starts in Triage.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    #[default]
    Triage,
    Backlog,
    Todo,
    InProgress,
    InReview,
    Done,
    Canceled,
}
impl IssueState {
    pub const ALL: [Self; 7] = [
        Self::Triage,
        Self::Backlog,
        Self::Todo,
        Self::InProgress,
        Self::InReview,
        Self::Done,
        Self::Canceled,
    ];
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Done | Self::Canceled)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Triage => "Triage",
            Self::Backlog => "Backlog",
            Self::Todo => "Todo",
            Self::InProgress => "In progress",
            Self::InReview => "In review",
            Self::Done => "Done",
            Self::Canceled => "Canceled",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
    #[default]
    None,
}

/// An issue as `issue.md` holds it: its front matter and, as `description`, the body. The
/// references to other issues are kept as written, so a hand-written one never hides the issue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: IssueId,
    pub title: String,
    pub state: IssueState,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// As `owner/name`.
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub related: Vec<String>,
    #[serde(default)]
    pub duplicate_of: Option<String>,
    /// A GitHub login.
    pub author: String,
    /// RFC 3339, in UTC.
    pub created: String,
    #[serde(default)]
    pub description: String,
    /// Front matter fields this release does not know, kept on write.
    #[serde(default, flatten)]
    pub extra: Extra,
}

/// A new issue as the human's form gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewIssue {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub state: IssueState,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub repositories: Vec<String>,
}

/// One comment file, `comments/<timestamp>-<login>.md`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    /// Its file name in the issue's `comments/`.
    pub file: String,
    pub author: String,
    pub created: String,
    #[serde(default)]
    pub edited: Option<String>,
    /// The login whose assistant wrote it.
    #[serde(default)]
    pub assistant_for: Option<String>,
    pub body: String,
}

/// One line of `events.jsonl`: when, by whom and what kind of change, with the kind's own
/// fields, such as `{"kind":"state","from":"todo","to":"in_progress"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueEvent {
    pub at: String,
    pub by: String,
    pub kind: String,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A file of an issue that sync holds on this Mac until the human settles it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PausedIssueFile {
    /// Relative to the tracker's root, such as `issues/CEL-151/issue.md`.
    pub path: String,
    pub why: Paused,
}

/// An issue as the Issues list shows it. An issue whose `issue.md` does not parse is listed by
/// its folder's name with `problem` and no fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueListing {
    pub id: IssueId,
    /// Its storage repository, as `StorageRepository::key`.
    pub tracker: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub state: Option<IssueState>,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub repositories: Vec<String>,
    /// A GitHub login.
    #[serde(default)]
    pub author: String,
    /// RFC 3339, in UTC.
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub problem: Option<String>,
    /// Changed since the human last opened it on this Mac.
    #[serde(default)]
    pub unread: bool,
}

/// One issue with its discussion, answering `Command::Issue`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueRecord {
    pub tracker: String,
    pub id: IssueId,
    /// `None` when `issue.md` does not parse; `raw` then holds it.
    pub issue: Option<Issue>,
    #[serde(default)]
    pub raw: Option<String>,
    /// Why `issue.md` could not be read.
    #[serde(default)]
    pub problem: Option<String>,
    /// In time order.
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub events: Vec<IssueEvent>,
    #[serde(default)]
    pub paused: Vec<PausedIssueFile>,
}

/// A Space's tracker as the snapshot summarizes it. No issue text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueTracker {
    pub space_id: SpaceId,
    /// Its storage repository, as `StorageRepository::key`.
    pub tracker: String,
    /// `None` for a GitHub storage repository with no `wiffletree-issues.yaml` yet.
    pub prefix: Option<String>,
    /// A local tracker is never pushed.
    pub local: bool,
    /// The human's GitHub login as `gh` reports it; without one, issues can be read only.
    pub login: Option<String>,
    pub issues: usize,
    pub open: usize,
    /// Issues changed since the human last opened them on this Mac.
    #[serde(default)]
    pub unread: usize,
    /// Files changed on this Mac that wait to be pushed.
    pub waiting: usize,
    pub paused: usize,
    /// When a pass last reached GitHub.
    pub synced_at: Option<i64>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_numbered_or_temporary_and_nothing_else() {
        for text in ["CEL-152", "CEL-new-qxvb", "SAGE-1"] {
            assert_eq!(IssueId::parse(text).unwrap().to_string(), text);
        }
        assert!(IssueId::parse("CEL-new-qxvb").unwrap().is_temporary());
        for text in [
            "cel-1",
            "CEL-",
            "CEL-01",
            "CEL-+1",
            "CEL-new-abc",
            "CEL-1a",
            "C-1",
        ] {
            assert!(IssueId::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn states_are_written_in_snake_case_in_their_order() {
        let states = serde_json::to_string(&IssueState::ALL).unwrap();
        assert_eq!(
            states,
            r#"["triage","backlog","todo","in_progress","in_review","done","canceled"]"#
        );
        assert!(IssueState::InReview.is_open() && !IssueState::Canceled.is_open());
    }
}
