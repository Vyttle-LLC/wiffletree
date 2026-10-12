## Why

An agent's conversation only shrinks when the Claude CLI compacts it on its own, near the top of the window. Wiffletree shows where that threshold is ("compacts at 967k") and nothing else. A person who sees an agent at 81% before a long piece of work, or who wants a fresh start that keeps a few named facts, has no way to compact it now. This change adds that action, for a person only, and makes its cost and its outcome visible.

## What Changes

- **A Compact action on the Context card.** The card the header's context chip opens gains one row with a **Compact…** button. It opens an inline form with one optional field, "What to keep", a line saying the summary replaces the conversation, can't be undone and uses one model call, and **Compact now** or **Cancel**.
- **Queued behind a running turn.** While the agent is working the action reads **Compact after this turn**. The request is stored, the card shows "Compacts when this turn ends" with **Cancel**, and the chip adds "compact queued". It runs when the turn ends, before any waiting message is delivered. Cancel works until it starts; a compaction that has started cannot be cancelled.
- **Not a turn.** The host sends Claude the bare `/compact` command, not a wrapped message. It has no reply, wakes no parent, counts toward no check-in and delivers no input. While it runs the agent's badge reads "Compacting" and the chip adds "compacting".
- **The outcome comes from Claude's status record,** because the CLI exits 0 either way. Success writes one line in the agent's conversation, "Context compacted · 812k → 41k", with a "Kept:" line when a note was given. A failure writes "Compaction failed · context unchanged" and a red notice on the card. "Nothing to compact" writes only a quiet notice on the card.
- **Its tokens are in the usage ledger.** A compaction's `result` record reports zero `usage`; the real counts are in `modelUsage`. The host records them against the agent like any other request. The Usage page gains no new UI: the request counts in its existing totals.
- **The card is measured again** after every outcome.
- **Claude only.** On a Codex agent, and on an agent with no provider session yet, the row is shown with the button disabled and the reason beside it.
- **Not included.** A coordinator or agent tool for compaction, Codex compaction, changing Claude's automatic threshold, recording Claude's own automatic compactions, and summarizing conversations for the transcript.

## Capabilities

### New Capabilities
<!-- None. Compaction is a host operation on an existing session and a control on an existing card. -->

### Modified Capabilities
- `workspace-host`: the compact, cancel and dismiss commands; a stored request that runs before waiting input and survives a restart; a compaction run that is not a turn; the outcome read from the status record; ledger rows from `modelUsage`.
- `work-stream`: the one-line compaction entry in the agent's conversation.
- `native-workspace`: the Compact row, form and notices on the Context card; the chip's and the badge's compaction states.

## Impact

- `workspace-core` (`lib.rs`): `Compaction`, `CompactionState`, `CompactionOutcome`; new fields `SessionRuntime.compaction` and `SessionRuntime.last_compaction`; commands `CompactContext`, `CancelCompaction` and `DismissCompaction`; the token formatter moved here from `context_view.rs`. Runtimes are stored as JSON, so serde defaults handle old records without a schema migration.
- `workspace-host`: `service.rs` (requests, scheduling, the compaction run, restart recovery, the check-in count), `provider.rs` (the bare command, the status, boundary and `modelUsage` records), `usage.rs` (a ledger row from `modelUsage`).
- `workspace-desktop`: `context_view.rs` (row, form, notices, chip note, re-measure), `ui.rs` (the "Compacting" badge), `conversation.rs` (the compaction line, Pause disabled while compacting), `bridge.rs`.
- Tests: a new `crates/workspace-host/tests/compaction.rs` with recorded streams under `tests/fixtures/streams`, and unit tests beside the changed code.
- Docs: `docs/PRD.md` and `docs/DEVELOPMENT.md`.
- Mockup: `design/mockups/current/compact-context.html`, approved October 12, 2026.
