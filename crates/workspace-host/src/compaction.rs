//! Compacting a Claude session's context: the person's request and how a run of it ends. The
//! scheduler in `service.rs` decides when it runs; it is never a turn.
use crate::*;

/// The longest "What to keep" note, in characters.
const MAX_INSTRUCTIONS: usize = 1000;
/// How Claude's error starts when the session has too little to summarize.
const NOTHING_TO_COMPACT: &str = "Not enough messages to compact";

/// What Claude streamed about a compaction, as `provider::run` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompactionRecord {
    /// The status record's `compact_result`: success, or its `compact_error`.
    Status(std::result::Result<(), String>),
    /// The boundary record's token counts before and after.
    Boundary { before: u64, after: u64 },
}

/// A note as the compact command takes it: one line, or none when it is empty.
fn instructions(note: Option<&str>) -> Result<Option<String>> {
    let note = note.unwrap_or_default().replace("\r\n", " ");
    let note = note.replace(['\r', '\n'], " ");
    let note = note.trim();
    ensure!(
        note.chars().count() <= MAX_INSTRUCTIONS,
        "What to keep can be at most 1,000 characters"
    );
    Ok((!note.is_empty()).then(|| note.to_owned()))
}

/// The outcome of a compaction whose process ended on its own, from the status record alone:
/// the exit code and the result record's error flag never decide success. `error` is what the
/// run ended with otherwise, and is the reason only when the status record gave none.
pub(crate) fn outcome(
    status: Option<std::result::Result<(), String>>,
    error: Option<String>,
) -> (CompactionResult, Option<String>) {
    match status {
        Some(Ok(())) => (CompactionResult::Compacted, None),
        Some(Err(reason)) if reason.starts_with(NOTHING_TO_COMPACT) => {
            (CompactionResult::NothingToCompact, Some(reason))
        }
        Some(Err(reason)) => (CompactionResult::Failed, Some(reason)),
        None => (
            CompactionResult::Failed,
            Some(error.unwrap_or_else(|| "Claude reported no compaction result.".into())),
        ),
    }
}

/// The conversation line for an outcome; finding nothing to compact leaves none.
fn line(
    result: CompactionResult,
    tokens: Option<(u64, u64)>,
    kept: Option<&str>,
) -> Option<String> {
    Some(match result {
        CompactionResult::Compacted => {
            let figures = tokens.map_or_else(String::new, |(before, after)| {
                format!(" · {} → {}", compact_tokens(before), compact_tokens(after))
            });
            let kept = kept.map_or_else(String::new, |kept| format!("\nKept: {kept}"));
            format!("Context compacted{figures}{kept}")
        }
        CompactionResult::Failed => "Compaction failed · context unchanged".into(),
        CompactionResult::Interrupted => "Compaction interrupted".into(),
        CompactionResult::NothingToCompact => return None,
    })
}

impl Host {
    /// Stores the person's request; it runs when the session's next turn could start.
    pub fn request_compaction(&mut self, id: &str, note: Option<&str>) -> Result<()> {
        let session = self.session(id)?;
        ensure!(
            session.provider == Provider::Claude,
            "Not available for Codex agents: the Codex CLI has no compact command."
        );
        ensure!(
            !session.archived,
            "{} is archived; restore it before compacting",
            session.name
        );
        let mut runtime = self.session_runtime(id)?;
        ensure!(
            runtime.provider_session_id.is_some(),
            "Available after this agent’s first turn."
        );
        ensure!(
            runtime.compaction.is_none(),
            "{} already has a compaction waiting or running",
            session.name
        );
        let instructions = instructions(note)?;
        runtime.compaction = Some(Compaction {
            instructions: instructions.clone(),
            requested_at: now(),
            state: CompactionState::Queued,
        });
        runtime.last_compaction = None;
        self.save_runtime(&runtime)?;
        Self::event(
            &self.db,
            &session.project_id,
            Some(id),
            "compaction_requested",
            instructions.as_deref().unwrap_or_default(),
        )
    }
    /// Withdraws a request that has not started; one that has cannot be taken back.
    pub fn cancel_compaction(&mut self, id: &str) -> Result<()> {
        let session = self.session(id)?;
        let mut runtime = self.session_runtime(id)?;
        let request = runtime.compaction.take();
        let request = request.with_context(|| format!("{} has no compaction", session.name))?;
        ensure!(
            request.state == CompactionState::Queued,
            "The compaction has started and cannot be cancelled"
        );
        self.save_runtime(&runtime)?;
        Self::event(
            &self.db,
            &session.project_id,
            Some(id),
            "compaction_cancelled",
            "",
        )
    }
    pub fn dismiss_compaction(&mut self, id: &str) -> Result<()> {
        let mut runtime = self.session_runtime(id)?;
        runtime.last_compaction = None;
        self.save_runtime(&runtime)
    }
    /// Sessions with a compaction waiting to start.
    pub(crate) fn queued_compactions(&self) -> Result<Vec<Session>> {
        self.list_data(
            "SELECT s.data FROM runtimes r JOIN sessions s ON s.id=r.session_id WHERE json_extract(r.data,'$.compaction.state')='queued' ORDER BY r.rowid",
            [],
        )
    }
    /// Settles a session's compaction: the request is cleared, the outcome stored for the
    /// Context card and its one line written. The session's status, its messages' receipts
    /// and its parent are left alone. `line_id` is the run's id, or any unique id for a
    /// compaction that could not start.
    pub(crate) fn compaction_ended(
        &mut self,
        session: &Session,
        line_id: &str,
        result: CompactionResult,
        reason: Option<String>,
        tokens: Option<(u64, u64)>,
    ) -> Result<()> {
        let mut runtime = self.session_runtime(&session.id)?;
        let request = runtime.compaction.take();
        let kept = request.as_ref().and_then(|r| r.instructions.as_deref());
        let event = if result == CompactionResult::Compacted {
            "context_compacted"
        } else {
            "compaction_failed"
        };
        let detail = match (&reason, line(result, tokens, None)) {
            (Some(reason), _) => reason.clone(),
            (None, line) => line.unwrap_or_default(),
        };
        runtime.last_compaction = Some(CompactionOutcome {
            result,
            reason,
            at: now(),
        });
        let line = line(result, tokens, kept);
        self.atomically(|host| {
            host.save_runtime(&runtime)?;
            if let Some(body) = line {
                host.db.execute(
                    "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?3,?4,'completed',?5)",
                    params![format!("{COMPACTION_LINE_PREFIX}{line_id}"), session.project_id, session.id, body, now()],
                )?;
            }
            Self::event(&host.db, &session.project_id, Some(&session.id), event, &detail)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_record_alone_decides_the_outcome() {
        use CompactionResult::*;
        let exit = || Some("Provider exited without a successful terminal result".to_owned());
        assert_eq!(outcome(Some(Ok(())), None), (Compacted, None));
        // A success the provider reported stands even if the process then ended badly.
        assert_eq!(outcome(Some(Ok(())), exit()), (Compacted, None));
        let nothing = "Not enough messages to compact.".to_owned();
        assert_eq!(
            outcome(Some(Err(nothing.clone())), None),
            (NothingToCompact, Some(nothing))
        );
        assert_eq!(
            outcome(Some(Err("Conversation too long".into())), exit()),
            (Failed, Some("Conversation too long".into()))
        );
        assert_eq!(outcome(None, exit()), (Failed, exit()));
        assert_eq!(
            outcome(None, None),
            (Failed, Some("Claude reported no compaction result.".into()))
        );
    }

    #[test]
    fn the_line_prints_the_providers_figures_and_the_note_only_when_given() {
        use CompactionResult::*;
        let figures = Some((812_000, 5_000));
        assert_eq!(
            line(
                Compacted,
                figures,
                Some("The failing test names and the migration plan")
            )
            .unwrap(),
            "Context compacted · 812k → 5k\nKept: The failing test names and the migration plan"
        );
        assert_eq!(
            line(Compacted, figures, None).unwrap(),
            "Context compacted · 812k → 5k"
        );
        assert_eq!(line(Compacted, None, None).unwrap(), "Context compacted");
        assert_eq!(
            line(Failed, None, Some("a note")).unwrap(),
            "Compaction failed · context unchanged"
        );
        assert_eq!(
            line(Interrupted, None, None).unwrap(),
            "Compaction interrupted"
        );
        assert_eq!(line(NothingToCompact, None, Some("a note")), None);
    }

    #[test]
    fn a_note_becomes_one_trimmed_line_or_nothing() {
        assert_eq!(instructions(None).unwrap(), None);
        assert_eq!(instructions(Some(" \n ")).unwrap(), None);
        assert_eq!(
            instructions(Some(" the failing tests\r\nand the plan ")).unwrap(),
            Some("the failing tests and the plan".into())
        );
        assert!(instructions(Some(&"é".repeat(1000))).is_ok());
        assert!(instructions(Some(&"é".repeat(1001))).is_err());
    }
}
