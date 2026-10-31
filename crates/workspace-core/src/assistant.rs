//! The assistant's threads (OpenSpec change `assistant-spaces-jobs-boards`, stages 1 and 2). A
//! thread is a provider session of role `Assistant` owned by its Space, read-only but for its
//! scratch folder, kept only on this Mac. The router sends each message to its thread.
pub mod router;

use crate::{ModelProfile, Selection, SpaceId, Status};
use router::{Destination, Question};
use serde::{Deserialize, Serialize};

/// What the assistant is called until the user names it.
pub const DEFAULT_NAME: &str = "Assistant";
/// The longest name the user may give the assistant, in characters.
pub const MAX_NAME_CHARS: usize = 40;
/// The longest thread title, in characters.
pub const MAX_TITLE_CHARS: usize = 60;
/// A thread with no message for this long leaves the tab strip.
pub const IDLE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// A thread's record. Its id is its session's id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    /// A thread never changes its Space.
    pub space_id: SpaceId,
    pub title: String,
    pub created_at: i64,
    /// When the human last sent it a message.
    pub last_message_at: i64,
    /// When the user last opened its tab, from search or by starting it.
    pub opened_at: i64,
    /// The user closed its tab.
    #[serde(default)]
    pub closed: bool,
    /// Its place in the tab strip; lower comes first.
    #[serde(default)]
    pub position: i64,
    /// The project Make this a project created from it; the thread is read-only after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// What the thread's tools asked to show, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<ThreadCard>,
    /// The messages sent from this thread that went to another one, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routed: Vec<RoutedLine>,
    /// Its messages that Change moved to another thread, with their replies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<MovedMessage>,
    /// The job run this thread is; such a thread takes no message but its prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobRunOf>,
}
impl Thread {
    /// Whether its tab is in the strip at `now`: not a job's run, not closed, and a message or
    /// an opening within the last 24 hours.
    pub fn in_strip(&self, now: i64) -> bool {
        self.job.is_none()
            && !self.closed
            && now - self.last_message_at.max(self.opened_at) < IDLE_AFTER_MS
    }
}
/// The job and run a job run's thread belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRunOf {
    pub job: crate::job::JobKey,
    pub run_id: String,
}

/// A message the router holds unsent, while it decides and then until the user answers. It
/// is kept in the store, so it survives a restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldMessage {
    pub id: String,
    /// The thread open when the user sent it; `None` from an empty tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub body: String,
    pub created_at: i64,
    /// What the user is asked; `None` while the router decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
    /// The router's title for a new thread, when it gave one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}
/// The line a message leaves in the thread it was sent from when it went to another thread:
/// "New thread in Personal · Change".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedLine {
    /// The message's id in the thread it went to.
    pub message_id: String,
    pub body: String,
    pub created_at: i64,
    /// The thread it went to, with its Space and its title then, so the line still names it
    /// once that thread's tab is closed.
    pub to: String,
    #[serde(default)]
    pub to_space: SpaceId,
    #[serde(default)]
    pub to_title: String,
    /// It started that thread.
    pub new: bool,
    /// What Change offers before "Somewhere else…".
    pub choices: Vec<Destination>,
}
/// A message Change moved out of this thread; it and its reply show as moved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedMessage {
    pub message_id: String,
    /// The thread it went to.
    pub to: String,
}

/// A card a thread's tool put in its conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadCard {
    pub id: String,
    pub created_at: i64,
    #[serde(flatten)]
    pub kind: CardKind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardKind {
    /// `suggest_space`: opens Settings › Spaces filled in; nothing is created until the user
    /// saves there.
    SuggestSpace { name: String, description: String },
    /// `project_brief`: the brief Make this a project shows, for the user to edit and confirm.
    ProjectBrief { name: String, brief: String },
    /// `save_output`: a file copied to this Mac's private board store.
    SavedOutput {
        title: String,
        /// The file's name in the store.
        file: String,
        /// 1 for the first save under this title.
        version: u32,
    },
    /// `propose_job`: a recipe for the user to check and save; nothing is scheduled until
    /// they press Save.
    Recipe {
        recipe: Box<crate::job::Recipe>,
        preview: crate::job::RecipePreview,
    },
}

/// A thread as the assistant view shows it: its record and its session's state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadTab {
    pub thread: Thread,
    pub status: Status,
    /// The model its turns run on, once chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ModelProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    /// Changes whenever its messages do, as `Snapshot::message_revisions` does for sessions.
    #[serde(default)]
    pub message_revision: u64,
}

/// A thread search finds, open or idle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadHit {
    pub thread: Thread,
    /// The matching line of a message, when the title did not match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

/// Whether a thread's message is one the host wrote about a turn, such as why it stopped,
/// rather than the thread's reply: stored as the turn's output under `error:`, `start-error:`
/// or `stopped:` and its run.
pub fn is_notice(message_id: &str) -> bool {
    message_id
        .strip_prefix(crate::REPLY_PREFIX)
        .is_some_and(|run| {
            ["error:", "start-error:", "stopped:"]
                .iter()
                .any(|kind| run.starts_with(kind))
        })
}

/// The title a new thread takes from its first message: its first line, at most 60
/// characters, cut at a word where it can be.
pub fn title_from_message(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or("New thread");
    if line.chars().count() <= MAX_TITLE_CHARS {
        return line.into();
    }
    let cut: String = line.chars().take(MAX_TITLE_CHARS - 1).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > MAX_TITLE_CHARS / 2 => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end())
}

/// A thread title the user typed: one non-empty line of at most 60 characters.
pub fn validate_title(title: &str) -> anyhow::Result<&str> {
    let title = title.trim();
    anyhow::ensure!(!title.is_empty(), "Give the thread a title");
    anyhow::ensure!(
        title.chars().count() <= MAX_TITLE_CHARS && !title.chars().any(char::is_control),
        "A title is one line of at most {MAX_TITLE_CHARS} characters"
    );
    Ok(title)
}

/// The assistant's name the user typed: one non-empty line of at most 40 characters.
pub fn validate_name(name: &str) -> anyhow::Result<&str> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "Give the assistant a name");
    anyhow::ensure!(
        name.chars().count() <= MAX_NAME_CHARS && !name.chars().any(char::is_control),
        "A name is one line of at most {MAX_NAME_CHARS} characters"
    );
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(last_message_at: i64, opened_at: i64, closed: bool) -> Thread {
        Thread {
            id: "t".into(),
            space_id: SpaceId("sage".into()),
            title: "API date format".into(),
            created_at: 0,
            last_message_at,
            opened_at,
            closed,
            position: 0,
            project_id: None,
            cards: vec![],
            routed: vec![],
            moved: vec![],
            job: None,
        }
    }

    #[test]
    fn a_title_is_the_first_line_cut_at_a_word() {
        assert_eq!(
            title_from_message("\n What date format did we settle on for the API?\nThanks"),
            "What date format did we settle on for the API?"
        );
        let long = "Compare our three pricing tiers with the two competitors in the brain please";
        let title = title_from_message(long);
        assert!(title.chars().count() <= MAX_TITLE_CHARS, "{title}");
        assert_eq!(
            title,
            "Compare our three pricing tiers with the two competitors…"
        );
        assert_eq!(title_from_message("  "), "New thread");
    }

    #[test]
    fn a_thread_leaves_the_strip_a_day_after_its_last_message_or_opening() {
        let hour = 60 * 60 * 1000;
        let now = 100 * hour;
        assert!(thread(now - 23 * hour, 0, false).in_strip(now));
        assert!(!thread(now - 30 * hour, 0, false).in_strip(now));
        assert!(thread(now - 30 * hour, now - hour, false).in_strip(now));
        assert!(!thread(now, now, true).in_strip(now));
    }

    #[test]
    fn names_and_titles_are_one_short_line() {
        assert_eq!(validate_name(" Wren ").unwrap(), "Wren");
        assert!(validate_name("").is_err() && validate_name("a\nb").is_err());
        assert!(validate_name(&"x".repeat(41)).is_err());
        assert!(validate_title(&"x".repeat(61)).is_err());
        assert_eq!(validate_title("Pricing copy").unwrap(), "Pricing copy");
    }
}
