//! The kinds of message the host writes into conversations. The host writes each kind through
//! the functions here and the desktop classifies stored messages with `MessageKind::of`, so the
//! two sides read the same format. The stored text is what agents read, so it never changes
//! shape: a kind is told by its id prefix or by the tag on its first line.
use crate::{CompactionResult, compact_tokens};
use serde::{Deserialize, Serialize};

/// The id prefix of a session's reply at the end of a turn; the rest is the run's id.
pub const REPLY_PREFIX: &str = "output:";
/// The id prefix of the line a compaction leaves in its session's conversation; the rest is
/// the run's id. A session's other messages to itself are replies, under `output:` ids.
pub const COMPACTION_LINE_PREFIX: &str = "compaction:";
/// The id prefix of a timer's fire, which the session sends itself.
pub const TIMER_PREFIX: &str = "timer:";
/// The id prefix of the message telling a coordinator how a child's turn ended.
pub const TURN_RESULT_PREFIX: &str = "turn-result:";
const TIMER_TAG: &str = "[timer] ";
const CHECK_IN_TAG: &str = "[check-in] ";
const TURN_ERROR: &str = " stopped with an error: ";

/// What an agent reports to its parent with the `report` tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    Progress,
    Blocked,
    ReadyForTesting,
    Passed,
    Failed,
    Completed,
}
impl ReportKind {
    pub const ALL: [Self; 6] = [
        Self::Progress,
        Self::Blocked,
        Self::ReadyForTesting,
        Self::Passed,
        Self::Failed,
        Self::Completed,
    ];
    /// The name agents pass and messages carry, as serde writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Blocked => "blocked",
            Self::ReadyForTesting => "ready_for_testing",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// A report as its parent receives it: `[kind] sender\nbody`.
pub fn report_body(kind: ReportKind, sender: &str, body: &str) -> String {
    format!("[{}] {sender}\n{body}", kind.name())
}
/// A timer's fire: `[timer] label\ntext`.
pub fn timer_body(label: &str, text: &str) -> String {
    format!("{TIMER_TAG}{label}\n{text}")
}
/// The host checking in on a long turn: `[check-in] text`.
pub fn check_in_body(text: &str) -> String {
    format!("{CHECK_IN_TAG}{text}")
}
pub fn reply_id(run_id: &str) -> String {
    format!("{REPLY_PREFIX}{run_id}")
}
pub fn turn_result_id(run_id: &str) -> String {
    format!("{TURN_RESULT_PREFIX}{run_id}")
}
/// A child's turn that stopped with an error, told to its coordinator.
pub fn turn_error_body(name: &str, error: &str) -> String {
    format!(
        "{name}{TURN_ERROR}{error}. Inspect its worktree, then resume it with resume_agent, choosing held: retry or skip."
    )
}
/// A child's turn that ended without a report, told to its coordinator with its reply.
pub fn turn_finished_body(name: &str, output: &str) -> String {
    format!("{name} finished a turn. This is not ticket acceptance.\n{output}")
}

impl CompactionResult {
    /// How the conversation line and the Context card name the outcome.
    pub fn title(self) -> &'static str {
        match self {
            Self::Compacted => "Context compacted",
            Self::NothingToCompact => "Nothing to compact",
            Self::Failed => "Compaction failed",
            Self::Interrupted => "Compaction interrupted",
        }
    }
}

/// The conversation line for a compaction's outcome; finding nothing to compact leaves none.
pub fn compaction_line(
    result: CompactionResult,
    tokens: Option<(u64, u64)>,
    kept: Option<&str>,
) -> Option<String> {
    let title = result.title();
    Some(match result {
        CompactionResult::Compacted => {
            let figures = tokens.map_or_else(String::new, |(before, after)| {
                format!(" · {} → {}", compact_tokens(before), compact_tokens(after))
            });
            let kept = kept.map_or_else(String::new, |kept| format!("\nKept: {kept}"));
            format!("{title}{figures}{kept}")
        }
        CompactionResult::Failed => format!("{title} · context unchanged"),
        CompactionResult::Interrupted => title.into(),
        CompactionResult::NothingToCompact => return None,
    })
}

/// A compaction line split into its title, the rest of its first line and its "Kept:" line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompactionLine<'a> {
    pub title: &'a str,
    pub rest: &'a str,
    pub kept: Option<&'a str>,
    pub failed: bool,
}
impl<'a> CompactionLine<'a> {
    pub fn parse(body: &'a str) -> Self {
        let (first, kept) = match body.split_once('\n') {
            Some((first, kept)) => (first, Some(kept)),
            None => (body, None),
        };
        let (title, rest) = first.split_at(first.find(" · ").unwrap_or(first.len()));
        Self {
            title,
            rest,
            kept,
            failed: title != CompactionResult::Compacted.title(),
        }
    }
}

/// What a stored message is, by its id and the tag on its first line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// An agent's report to its parent.
    Report(ReportKind),
    /// One of the session's own timers firing.
    Timer,
    /// The host checking in on a long turn.
    CheckIn,
    /// How a child's turn ended, told to its coordinator.
    TurnResult { failed: bool },
    /// A session's reply at the end of a turn.
    Reply,
    /// The line a compaction left.
    Compaction,
    /// Anything else: the human's messages, direct messages, assignments and host notices.
    Plain,
}
impl MessageKind {
    pub fn of(id: &str, body: &str) -> Self {
        if id.starts_with(REPLY_PREFIX) {
            Self::Reply
        } else if id.starts_with(COMPACTION_LINE_PREFIX) {
            Self::Compaction
        } else if id.starts_with(TURN_RESULT_PREFIX) {
            Self::TurnResult {
                failed: body.contains(TURN_ERROR),
            }
        } else if id.starts_with(TIMER_PREFIX) && body.starts_with(TIMER_TAG) {
            Self::Timer
        } else if body.starts_with(CHECK_IN_TAG) {
            Self::CheckIn
        } else {
            tagged_report(body).map_or(Self::Plain, Self::Report)
        }
    }
    /// The kind's name as a tag, for the kinds a conversation labels.
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::Report(kind) => Some(kind.name()),
            Self::Timer => Some("timer"),
            Self::CheckIn => Some("check_in"),
            Self::TurnResult { .. } => Some("turn_result"),
            Self::Reply | Self::Compaction | Self::Plain => None,
        }
    }
    /// What the message says under its kind: a report or timer fire without its first line, a
    /// check-in without its tag, anything else whole.
    pub fn text(self, body: &str) -> &str {
        match self {
            Self::Report(_) | Self::Timer => body.split_once('\n').map_or("", |(_, text)| text),
            Self::CheckIn => body.strip_prefix(CHECK_IN_TAG).unwrap_or(body),
            _ => body,
        }
    }
    /// A timer fire's label, the rest of its first line.
    pub fn timer_label(self, body: &str) -> Option<&str> {
        let label = body.strip_prefix(TIMER_TAG)?.lines().next()?;
        (self == Self::Timer).then_some(label)
    }
}

/// The kind of a `[kind] sender` first line.
fn tagged_report(body: &str) -> Option<ReportKind> {
    let (kind, _) = body.strip_prefix('[')?.split_once("] ")?;
    ReportKind::named(kind)
}

/// The run a reply ended, from its id.
pub fn reply_run(id: &str) -> Option<&str> {
    id.strip_prefix(REPLY_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each kind as the host wrote it before this module, byte for byte, and how it reads.
    #[test]
    fn every_kind_writes_the_text_agents_have_always_read_and_reads_back() {
        let report = report_body(ReportKind::ReadyForTesting, "Implementer · Toolbar", "Done");
        assert_eq!(report, "[ready_for_testing] Implementer · Toolbar\nDone");
        let kind = MessageKind::of("report:s:1", &report);
        assert_eq!(kind, MessageKind::Report(ReportKind::ReadyForTesting));
        assert_eq!(
            (kind.name(), kind.text(&report)),
            (Some("ready_for_testing"), "Done")
        );

        let fire = timer_body(
            "Watch CI",
            "Your timer t1, scheduled for 2026-10-17T10:00:00Z. This is your own scheduled prompt, not a message from the human.\n\nCheck CI",
        );
        assert_eq!(
            fire,
            "[timer] Watch CI\nYour timer t1, scheduled for 2026-10-17T10:00:00Z. This is your own scheduled prompt, not a message from the human.\n\nCheck CI"
        );
        let kind = MessageKind::of("timer:t1:100", &fire);
        assert_eq!(kind, MessageKind::Timer);
        assert_eq!(kind.timer_label(&fire), Some("Watch CI"));
        assert!(kind.text(&fire).starts_with("Your timer t1"));

        let check_in = check_in_body("Tester has been in one turn for 30 minutes.");
        assert_eq!(
            check_in,
            "[check-in] Tester has been in one turn for 30 minutes."
        );
        let kind = MessageKind::of("check-in:r9:30", &check_in);
        assert_eq!(
            (kind, kind.text(&check_in)),
            (
                MessageKind::CheckIn,
                "Tester has been in one turn for 30 minutes."
            )
        );

        let error = turn_error_body("Implementer", "provider exited");
        assert_eq!(
            error,
            "Implementer stopped with an error: provider exited. Inspect its worktree, then resume it with resume_agent, choosing held: retry or skip."
        );
        assert_eq!(turn_result_id("r1"), "turn-result:r1");
        assert_eq!(
            MessageKind::of("turn-result:r1", &error),
            MessageKind::TurnResult { failed: true }
        );
        let finished = turn_finished_body("Implementer", "All green");
        assert_eq!(
            finished,
            "Implementer finished a turn. This is not ticket acceptance.\nAll green"
        );
        assert_eq!(
            MessageKind::of("turn-result:r2", &finished),
            MessageKind::TurnResult { failed: false }
        );

        assert_eq!(reply_id("r3"), "output:r3");
        assert_eq!(reply_run("output:r3"), Some("r3"));
        // A reply is a reply whatever it starts with.
        assert_eq!(
            MessageKind::of("output:r3", "[passed] x\ny"),
            MessageKind::Reply
        );
    }

    #[test]
    fn compaction_lines_read_as_before_and_split_back() {
        let compacted = compaction_line(
            CompactionResult::Compacted,
            Some((812_000, 5_000)),
            Some("The failing test names"),
        )
        .unwrap();
        assert_eq!(
            compacted,
            "Context compacted · 812k → 5k\nKept: The failing test names"
        );
        assert_eq!(
            CompactionLine::parse(&compacted),
            CompactionLine {
                title: "Context compacted",
                rest: " · 812k → 5k",
                kept: Some("Kept: The failing test names"),
                failed: false
            }
        );
        assert_eq!(
            compaction_line(CompactionResult::Compacted, Some((812_000, 5_000)), None).as_deref(),
            Some("Context compacted · 812k → 5k")
        );
        // A failure leaves the context as it was, so the note is not repeated.
        let failed = compaction_line(CompactionResult::Failed, None, Some("a note")).unwrap();
        assert_eq!(failed, "Compaction failed · context unchanged");
        assert!(CompactionLine::parse(&failed).failed);
        let interrupted = compaction_line(CompactionResult::Interrupted, None, None).unwrap();
        assert_eq!(interrupted, "Compaction interrupted");
        assert_eq!(
            CompactionLine::parse(&interrupted),
            CompactionLine {
                title: "Compaction interrupted",
                rest: "",
                kept: None,
                failed: true
            }
        );
        assert_eq!(
            compaction_line(CompactionResult::NothingToCompact, None, None),
            None
        );
        assert_eq!(
            compaction_line(CompactionResult::Compacted, None, None).as_deref(),
            Some("Context compacted")
        );
        assert_eq!(
            MessageKind::of("compaction:r", &compacted),
            MessageKind::Compaction
        );
        // A reply that merely says the same is still a reply, with its bubble and turn summary.
        assert_eq!(
            MessageKind::of("output:r", "Context compacted"),
            MessageKind::Reply
        );
    }

    #[test]
    fn ordinary_messages_are_plain() {
        for body in [
            "[x] A checklist item",
            "[See the docs] for details\nmore",
            "Plain text",
        ] {
            assert_eq!(MessageKind::of("m1", body), MessageKind::Plain);
        }
        // A timer's tag counts only on a timer's fire.
        assert_eq!(
            MessageKind::of("agent:a:1", "[timer] x\ny"),
            MessageKind::Plain
        );
        assert_eq!(
            MessageKind::of("report:s:2", "[completed] celadon-web"),
            MessageKind::Report(ReportKind::Completed)
        );
        assert_eq!(
            MessageKind::Report(ReportKind::Completed).text("[completed] celadon-web"),
            ""
        );
    }

    #[test]
    fn report_kinds_round_trip_through_their_names() {
        for kind in ReportKind::ALL {
            assert_eq!(ReportKind::named(kind.name()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.name());
        }
        assert_eq!(ReportKind::named("timer"), None);
    }
}
