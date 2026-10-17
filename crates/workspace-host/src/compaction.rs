//! Compacting a session's context: the person's request and how a run of it ends. The
//! scheduler in `service.rs` decides when it runs; it is never a turn.
use crate::*;

/// The longest "What to keep" note, in characters.
const MAX_INSTRUCTIONS: usize = 1000;
/// How Claude's error starts when the session has too little to summarize.
const NOTHING_TO_COMPACT: &str = "Not enough messages to compact";
/// The longest reason the host builds from a provider's own text, in characters. The Context
/// card cuts every reason it shows at the same length.
pub const REASON_LIMIT: usize = 200;

/// One line of at most `REASON_LIMIT` characters, ended with "…" when it was cut.
pub(crate) fn bounded(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > REASON_LIMIT {
        format!("{}…", line.chars().take(REASON_LIMIT).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// What the provider said about a compaction, as `provider::run` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompactionRecord {
    /// How it ended: Claude's `compact_result` with its `compact_error`, or the status of
    /// Codex's compaction turn with the reason the host gives a failure.
    Status(std::result::Result<(), String>),
    /// The token counts before and after.
    Boundary { before: u64, after: u64 },
    /// The id Codex gave the compaction's turn; its usage entry in the session record has it.
    Turn(String),
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
/// run ended with otherwise, and is the reason only when the status record gave none: then it
/// can be an exit with the end of the process's error output, so it is held to one bounded
/// line for either provider. Only Claude finds nothing to compact; Codex compacts whenever it
/// is asked.
pub(crate) fn outcome(
    provider: Provider,
    status: Option<std::result::Result<(), String>>,
    error: Option<String>,
) -> (CompactionResult, Option<String>) {
    match status {
        Some(Ok(())) => (CompactionResult::Compacted, None),
        Some(Err(reason))
            if provider == Provider::Claude && reason.starts_with(NOTHING_TO_COMPACT) =>
        {
            (CompactionResult::NothingToCompact, Some(reason))
        }
        Some(Err(reason)) => (CompactionResult::Failed, Some(reason)),
        None => {
            let reason = error.map_or_else(
                || format!("{} reported no compaction result.", provider.label()),
                |error| bounded(&error),
            );
            (CompactionResult::Failed, Some(reason))
        }
    }
}

impl Host {
    /// Stores the person's request; it runs when the session's next turn could start.
    pub fn request_compaction(&mut self, id: &str, note: Option<&str>) -> Result<()> {
        let session = self.session(id)?;
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
        // Codex's compact request has no field for a note; dropping one silently would show a
        // compaction that kept nothing the person asked for.
        ensure!(
            session.provider == Provider::Claude || instructions.is_none(),
            "Codex chooses what the summary keeps; it takes no note."
        );
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
    /// Context card and its one line written, once however often this runs for a run. The
    /// session's status, its messages' receipts and its parent are left alone. `line_id` is the
    /// run's id, or any unique id for a compaction that could not start.
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
        let detail = match (&reason, compaction_line(result, tokens, None)) {
            (Some(reason), _) => reason.clone(),
            (None, line) => line.unwrap_or_default(),
        };
        runtime.last_compaction = Some(CompactionOutcome {
            result,
            reason,
            at: now(),
        });
        let line = compaction_line(result, tokens, kept);
        self.atomically(|host| {
            host.save_runtime(&runtime)?;
            if let Some(body) = line {
                host.db.execute(
                    "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?3,?4,'completed',?5) ON CONFLICT(id) DO NOTHING",
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
        let nothing = "Not enough messages to compact.".to_owned();
        for provider in [Provider::Claude, Provider::Codex] {
            let outcome = |status, error| outcome(provider, status, error);
            assert_eq!(outcome(Some(Ok(())), None), (Compacted, None));
            // A success the provider reported stands even if the process then ended badly.
            assert_eq!(outcome(Some(Ok(())), exit()), (Compacted, None));
            assert_eq!(
                outcome(Some(Err("Conversation too long".into())), exit()),
                (Failed, Some("Conversation too long".into()))
            );
            assert_eq!(outcome(None, exit()), (Failed, exit()));
        }
        // What a process ended with can be long and run over several lines.
        let output = format!("{}\nTraceback\n  File", "x".repeat(300));
        let early = format!("{} (exit status: 1). {output}", exit().unwrap());
        for provider in [Provider::Claude, Provider::Codex] {
            let (result, reason) = outcome(provider, None, Some(early.clone()));
            let reason = reason.unwrap();
            assert_eq!(result, Failed);
            assert_eq!(reason, format!("{}…", &early[..REASON_LIMIT]));
        }
        assert_eq!(
            outcome(Provider::Claude, Some(Err(nothing.clone())), None),
            (NothingToCompact, Some(nothing.clone()))
        );
        assert_eq!(
            outcome(Provider::Claude, None, None),
            (Failed, Some("Claude reported no compaction result.".into()))
        );
        // Codex compacts whenever it is asked, so the same words from it are a failure.
        assert_eq!(
            outcome(Provider::Codex, Some(Err(nothing.clone())), None),
            (Failed, Some(nothing))
        );
        assert_eq!(
            outcome(Provider::Codex, None, None),
            (Failed, Some("Codex reported no compaction result.".into()))
        );
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
