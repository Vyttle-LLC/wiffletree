//! The brains of this Mac's Spaces (OpenSpec change `one-space-repository`): a Space's brain is
//! `second-brain/` in its repository's folder, and every change lands through the repository's
//! sync engine. Nothing here holds a brain file's content or a check's output text.
use crate::SpaceId;
use serde::{Deserialize, Serialize};

/// The brain of the Spaces in one repository's folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brain {
    /// The repository, as `StorageRepository::key` names it.
    pub repository: String,
    /// The Spaces in the folder.
    pub spaces: Vec<SpaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<i64>,
    /// Why the last pass could not reach the repository; `None` once one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// When a folder's wiki compiles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainCompile {
    /// `HH:MM`.
    pub at: String,
    pub timezone: String,
    #[serde(flatten)]
    pub extra: crate::Extra,
}

/// A log entry a scan held on this Mac. It names the rule and the line, never the matched text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldEntry {
    /// The repository, as `Brain::repository` names it.
    pub repository: String,
    /// The entry's file in the repository.
    pub path: String,
    /// The project that wrote it; with none, its card is on the Brains page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// `sensitive-info` or `secret-scan`.
    pub check: String,
    /// The rule it failed, or `None` when the check could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Why the check could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
impl HeldEntry {
    /// Not sensitive is for a `sensitive-info` finding: one click should not push what looks
    /// like a live secret, and a check that could not run found nothing to mark.
    pub fn offers_not_sensitive(&self) -> bool {
        self.check == SENSITIVE_INFO && self.rule.is_some() && self.line.is_some()
    }
}

/// Something on a repository's default branch that failed a scan of what this Mac pulled. Each
/// Mac raises and settles its own. It names the rule and the line, never the matched text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanAlert {
    /// The repository, as `Brain::repository` names it.
    pub repository: String,
    /// The file in the repository.
    pub path: String,
    /// `sensitive-info` or `secret-scan`.
    pub check: String,
    pub rule: String,
    /// The line of the latest finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// The commit of the latest finding.
    pub commit: String,
    /// Who that commit names as its author.
    pub pushed_by: String,
    /// GitHub's web editor made that commit.
    #[serde(default)]
    pub web_editor: bool,
    /// The projects of the Spaces using the folder; with none, it is on the Brains page alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}
impl ScanAlert {
    /// Who pushed the commit, as the card says it.
    pub fn pushed(&self) -> String {
        match self.web_editor {
            true => "pushed from GitHub's web editor".to_owned(),
            false => format!("pushed by {}", self.pushed_by),
        }
    }
    /// What the card says was found: the check, rule, file and line.
    pub fn found(&self) -> String {
        let line = (self.line).map_or(String::new(), |line| format!(" line {line}"));
        format!(
            "{} · rule {} · {}{line} · {}",
            self.check,
            self.rule,
            self.path,
            self.pushed()
        )
    }
    /// The file on GitHub, at the commit of the latest finding.
    pub fn url(&self) -> String {
        let path = (self.path.replace('%', "%25").replace(' ', "%20"))
            .replace('#', "%23")
            .replace('?', "%3F");
        format!(
            "https://github.com/{}/blob/{}/{path}",
            self.repository, self.commit
        )
    }
}

/// A check that cannot run on what this Mac pulls, which catches up when it can.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanStopped {
    pub check: String,
    /// The repositories it is not scanning, by name.
    pub repositories: Vec<String>,
    pub text: String,
}

/// The check whose findings an inline `gate:allow` marker may suppress.
pub const SENSITIVE_INFO: &str = "sensitive-info";

/// A batch pull request an earlier release left open on an old brain. It stays on GitHub for the
/// human to merge or close, and nothing watches it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftPullRequest {
    /// The old brain, `owner/name`.
    pub repository: String,
    pub number: u64,
}

/// What the Brains command answers: every wiki of this Mac's Spaces.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrainsPage {
    pub brains: Vec<Brain>,
    /// The compile of each wiki.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compiles: Vec<crate::BrainCompileState>,
    /// Open conflicts no project's Inbox shows, as no project includes their claim's repository.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<crate::ConflictCard>,
    /// The entries held on this Mac that no project wrote.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<HeldEntry>,
    /// What this Mac's pull scans raised.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<ScanAlert>,
    /// The pull requests an earlier release left open, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_pull_requests: Vec<LeftPullRequest>,
}
