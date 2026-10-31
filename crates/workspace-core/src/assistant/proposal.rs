//! Issue threads and proposals (OpenSpec change `assistant-spaces-jobs-boards`, stage 6). An
//! issue thread is an assistant thread with an issue, or its tracker's list, attached. Every
//! change a thread suggests to its Space's tracker is a proposal stored with the thread, and
//! nothing reaches the tracker until the human applies it with their own command.
use crate::issues::{Issue, IssueId, IssueState, NewIssue, Priority};
use serde::{Deserialize, Serialize};

/// The key of an issue's `extra` front matter naming whose assistant filed it, as a comment's
/// `assistant_for` does.
pub const FILED_FOR: &str = "assistant_for";

/// Whose assistant filed `issue`, when one did.
pub fn filed_for(issue: &Issue) -> Option<&str> {
    issue.extra.get(FILED_FOR).and_then(|login| login.as_str())
}

/// What an issue thread works on, in its own Space's tracker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Attached {
    /// One issue: the thread the issue page's Ask the assistant opens.
    Issue { id: IssueId },
    /// The tracker's list: the thread the Issues page's Ask the assistant opens.
    Issues,
}

/// A change a thread suggested, as its card shows it until the human applies or discards it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub created_at: i64,
    pub change: ProposalChange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    /// The new issues a split's Apply filed before it stopped, in order; trying again files
    /// only the rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filed: Vec<IssueId>,
}

/// The five kinds of proposal. Those that change an issue keep the issue as it was read when
/// the proposal was made, so the card can say when it changed since.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposalChange {
    /// `propose_edit`: some of an issue's fields and its description.
    Edit {
        id: IssueId,
        base: Box<Issue>,
        changes: IssueChanges,
    },
    /// `propose_comment`: a comment, posted as "Assistant for ‹name›".
    Comment { id: IssueId, body: String },
    /// `propose_split`: new issues, each related to the original, and an edit to the original.
    Split {
        id: IssueId,
        base: Box<Issue>,
        changes: IssueChanges,
        new: Vec<NewIssue>,
    },
    /// `propose_duplicate`: `duplicate_of` on `id`, naming `of`. No state changes.
    Duplicate {
        id: IssueId,
        of: IssueId,
        base: Box<Issue>,
    },
    /// `propose_issue`: a new issue in the thread's Space's tracker.
    NewIssue { new: NewIssue },
}
impl ProposalChange {
    /// The existing issue it changes, if any.
    pub fn issue(&self) -> Option<&IssueId> {
        match self {
            Self::Edit { id, .. }
            | Self::Comment { id, .. }
            | Self::Split { id, .. }
            | Self::Duplicate { id, .. } => Some(id),
            Self::NewIssue { .. } => None,
        }
    }
    /// The issue as it was when the proposal was made, for those that change one's fields.
    pub fn base(&self) -> Option<&Issue> {
        match self {
            Self::Edit { base, .. } | Self::Split { base, .. } | Self::Duplicate { base, .. } => {
                Some(base)
            }
            Self::Comment { .. } | Self::NewIssue { .. } => None,
        }
    }
    /// The card's heading, as "Edit", with the issue it names.
    pub fn heading(&self) -> String {
        match self {
            Self::Edit { id, .. } => format!("Edit · {id}"),
            Self::Comment { id, .. } => format!("Comment · {id}"),
            Self::Split { id, new, .. } => format!("Split · {id} into {}", new.len()),
            Self::Duplicate { id, of, .. } => format!("Duplicate · {id} of {of}"),
            Self::NewIssue { .. } => "New issue".into(),
        }
    }
    /// What the issue becomes when the proposal applies to `current`: its own changes made on
    /// top of the current version, never over a newer one.
    pub fn applied_to(&self, current: &Issue) -> Option<Issue> {
        match self {
            Self::Edit { changes, .. } | Self::Split { changes, .. } => {
                Some(changes.apply(current))
            }
            Self::Duplicate { of, .. } => Some(Issue {
                duplicate_of: Some(of.to_string()),
                ..current.clone()
            }),
            Self::Comment { .. } | Self::NewIssue { .. } => None,
        }
    }
    /// Names `number` wherever it named `temporary`, once the tracker numbered that issue.
    pub fn renumber(&mut self, temporary: &IssueId, number: &IssueId) {
        let swap = |id: &mut IssueId| {
            if id == temporary {
                *id = number.clone();
            }
        };
        match self {
            Self::Edit { id, base, .. } | Self::Split { id, base, .. } => {
                swap(id);
                swap(&mut base.id);
            }
            Self::Duplicate { id, of, base } => {
                swap(id);
                swap(of);
                swap(&mut base.id);
            }
            Self::Comment { id, .. } => swap(id),
            Self::NewIssue { .. } => {}
        }
    }
    /// Whether `edited` is the same kind of proposal for the same issue, as the human's Edit
    /// may change only its text.
    pub fn same_kind(&self, edited: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(edited)
            && self.issue() == edited.issue()
    }
}

/// The fields a proposal changes; a field left `None` keeps its current value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueChanges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<IssueState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// `Some(None)` unassigns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<String>>,
}
impl IssueChanges {
    pub fn apply(&self, issue: &Issue) -> Issue {
        let mut issue = issue.clone();
        if let Some(title) = &self.title {
            issue.title = title.clone();
        }
        if let Some(description) = &self.description {
            issue.description = description.clone();
        }
        if let Some(state) = self.state {
            issue.state = state;
        }
        if let Some(priority) = self.priority {
            issue.priority = priority;
        }
        if let Some(assignee) = &self.assignee {
            issue.assignee = assignee.clone();
        }
        if let Some(labels) = &self.labels {
            issue.labels = labels.clone();
        }
        if let Some(repositories) = &self.repositories {
            issue.repositories = repositories.clone();
        }
        issue
    }
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the human did with a proposal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// Applied as the human: the issues it filed or changed.
    Applied {
        at: i64,
        issues: Vec<IssueId>,
    },
    Discarded {
        at: i64,
    },
}

/// A pending proposal as its card shows it now: the issue it changes as it stands, and who
/// changed it since the proposal was made. Answered by `Command::Proposals`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalView {
    pub proposal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Issue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_since: Option<ChangedSince>,
}
/// The last change to an issue made after a proposal read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSince {
    /// Who made it, as its commit names them, when the tracker's history says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// When, as RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// One line of a proposal's diff, as its card shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// A field that stays, shown for context.
    Same(String),
    Removed(String),
    Added(String),
}

/// The lines between `before` and `after`: each changed field as removed and added, then the
/// description's lines, those both share shown as context.
pub fn diff(before: &Issue, after: &Issue) -> Vec<DiffLine> {
    let mut lines = vec![];
    let list = |items: &[String]| items.join(", ");
    let fields = [
        ("title", before.title.clone(), after.title.clone()),
        (
            "state",
            before.state.label().into(),
            after.state.label().into(),
        ),
        (
            "priority",
            priority_label(before.priority).into(),
            priority_label(after.priority).into(),
        ),
        (
            "assignee",
            before.assignee.clone().unwrap_or_else(|| "nobody".into()),
            after.assignee.clone().unwrap_or_else(|| "nobody".into()),
        ),
        ("labels", list(&before.labels), list(&after.labels)),
        (
            "repositories",
            list(&before.repositories),
            list(&after.repositories),
        ),
        (
            "duplicate of",
            before.duplicate_of.clone().unwrap_or_default(),
            after.duplicate_of.clone().unwrap_or_default(),
        ),
    ];
    for (name, was, now) in fields {
        if was != now {
            if !was.is_empty() {
                lines.push(DiffLine::Removed(format!("{name}: {was}")));
            }
            lines.push(DiffLine::Added(format!("{name}: {now}")));
        }
    }
    if before.description != after.description {
        let was: Vec<&str> = before.description.lines().collect();
        let now: Vec<&str> = after.description.lines().collect();
        lines.extend(line_diff(&was, &now));
    }
    lines
}

/// The most line pairs `line_diff` compares; past it, every line shows removed and added.
const MAX_DIFF_CELLS: usize = 1_000_000;

/// `was` to `now` in order, by their longest common run of lines: each line kept, removed or
/// added where it is, so a deleted repeat or a move shows.
fn line_diff(was: &[&str], now: &[&str]) -> Vec<DiffLine> {
    let line = |text: &&str| text.to_string();
    if (was.len() + 1) * (now.len() + 1) > MAX_DIFF_CELLS {
        let removed = was.iter().map(line).map(DiffLine::Removed);
        return removed
            .chain(now.iter().map(line).map(DiffLine::Added))
            .collect();
    }
    // `common[i][j]`: the longest common run of `was[i..]` and `now[j..]`.
    let mut common = vec![vec![0usize; now.len() + 1]; was.len() + 1];
    for i in (0..was.len()).rev() {
        for j in (0..now.len()).rev() {
            common[i][j] = match was[i] == now[j] {
                true => common[i + 1][j + 1] + 1,
                false => common[i + 1][j].max(common[i][j + 1]),
            };
        }
    }
    let (mut i, mut j, mut lines) = (0, 0, vec![]);
    while i < was.len() || j < now.len() {
        if i < was.len() && j < now.len() && was[i] == now[j] {
            lines.push(DiffLine::Same(line(&was[i])));
            (i, j) = (i + 1, j + 1);
        } else if j < now.len() && (i == was.len() || common[i][j + 1] >= common[i + 1][j]) {
            lines.push(DiffLine::Added(line(&now[j])));
            j += 1;
        } else {
            lines.push(DiffLine::Removed(line(&was[i])));
            i += 1;
        }
    }
    lines
}

fn priority_label(priority: Priority) -> &'static str {
    match priority {
        Priority::Urgent => "Urgent",
        Priority::High => "High",
        Priority::Medium => "Medium",
        Priority::Low => "Low",
        Priority::None => "None",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Extra;

    fn issue(description: &str) -> Issue {
        Issue {
            id: IssueId::parse("SAGE-151").unwrap(),
            title: "Delivery webhook retries forever".into(),
            state: IssueState::Todo,
            priority: Priority::High,
            assignee: None,
            labels: vec!["bug".into()],
            repositories: vec![],
            blocks: vec![],
            blocked_by: vec![],
            related: vec![],
            duplicate_of: None,
            author: "treyes".into(),
            created: "2026-10-31T12:00:00Z".into(),
            description: description.into(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn an_edit_applies_its_own_fields_on_top_of_a_newer_version() {
        let base = issue("Since Monday it retries.");
        let changes = IssueChanges {
            description: Some("Since Monday it retries.\n## Acceptance criteria".into()),
            labels: Some(vec!["bug".into(), "regression".into()]),
            ..IssueChanges::default()
        };
        let edit = ProposalChange::Edit {
            id: base.id.clone(),
            base: Box::new(base.clone()),
            changes,
        };
        let theirs = Issue {
            priority: Priority::Urgent,
            ..base.clone()
        };
        let applied = edit.applied_to(&theirs).unwrap();
        assert_eq!(
            applied.priority,
            Priority::Urgent,
            "a teammate's field stays"
        );
        assert_eq!(applied.labels, ["bug", "regression"]);
        let lines = diff(&theirs, &applied);
        assert_eq!(
            lines,
            [
                DiffLine::Removed("labels: bug".into()),
                DiffLine::Added("labels: bug, regression".into()),
                DiffLine::Same("Since Monday it retries.".into()),
                DiffLine::Added("## Acceptance criteria".into()),
            ]
        );
    }

    #[test]
    fn the_description_diff_shows_a_deleted_repeat_and_a_move() {
        let changed = |was: &str, now: &str| diff(&issue(was), &issue(now));
        let same = |text: &str| DiffLine::Same(text.into());
        let (removed, added) = (
            |text: &str| DiffLine::Removed(text.into()),
            |text: &str| DiffLine::Added(text.into()),
        );
        assert_eq!(
            changed("Required\nRequired\nDone", "Required\nDone"),
            [same("Required"), removed("Required"), same("Done")]
        );
        assert_eq!(
            changed("First\nThen", "Then\nFirst"),
            [added("Then"), same("First"), removed("Then")]
        );
    }

    #[test]
    fn a_duplicate_only_links_and_an_edited_proposal_keeps_its_kind_and_issue() {
        let base = issue("");
        let of = IssueId::parse("SAGE-150").unwrap();
        let duplicate = ProposalChange::Duplicate {
            id: base.id.clone(),
            of: of.clone(),
            base: Box::new(base.clone()),
        };
        let applied = duplicate.applied_to(&base).unwrap();
        assert_eq!(applied.duplicate_of.as_deref(), Some("SAGE-150"));
        assert_eq!(applied.state, base.state);
        let comment = |id: &str| ProposalChange::Comment {
            id: IssueId::parse(id).unwrap(),
            body: "text".into(),
        };
        assert!(comment("SAGE-151").same_kind(&comment("SAGE-151")));
        assert!(!comment("SAGE-151").same_kind(&comment("SAGE-152")));
        assert!(!duplicate.same_kind(&comment("SAGE-151")));
    }
}
