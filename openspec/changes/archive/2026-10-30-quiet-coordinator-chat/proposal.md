## Why

The coordinator chat floods with near-identical status replies. Every check-in, child report, timer fire or Wiffletree notice starts a coordinator turn, its final reply shows in full, and the coordinating skill asks for a per-ticket table, so a two-hour wait becomes about ten repeated status messages and the human's own questions get buried. Agent-to-agent messages already fold into one line; the coordinator's replies to them do not.

The human approved the v2 mockup (`ui/mockups.html`) with one refinement: the pinned table shows only tickets in progress. The rejected v1 folded every background reply, which risked hiding news. v2 folds only what Wiffletree's own records prove unchanged.

## What Changes

- **A pinned live ticket table** at the top of the coordinator's conversation, filled from Wiffletree's records: this project's tickets in progress, with their status (and verification round) and PR. Accepted and closed tickets collapse to one "N done" line that opens. Backlog items, such as shared issues, never appear. The table collapses, and that choice is a client preference. It redraws from the snapshot the client already reloads; nothing polls.
- **The host decides whether a coordinator reply repeats itself.** When a coordinator turn the human did not start ends, the host compares a snapshot of recorded project facts (ticket states, ticket branch heads, verification cycles and findings, PR state, open questions, stopped and held agents) with the snapshot taken at the coordinator's last reply shown in full, and stores the result on the reply message.
- **Unchanged replies fold; changed ones say what changed.** An unchanged reply folds into the surrounding run of agent messages with a "No change" pill and opens to its full text. A changed one stays full and opens with a muted "Since last update:" line.
- **Always full:** replies to turns the human started, replies to turns whose input held a failed, blocked or error message, and replies the coordinator marks as news with the new optional `news` field on `set_plan`.
- **Coordinator instructions:** routine updates say only what is new in one or two sentences; the per-ticket table and review accounting are kept for finished, accepted, published or ready tickets and for when the human asks for status.

## Capabilities

### New Capabilities
- `coordinator-updates`: the pinned ticket table, the host's reply comparison, the folding and "Since last update" line, the news mark, and the coordinator's instructions for routine updates.

### Modified Capabilities
<!-- None: the folding of peer messages in work-stream is unchanged; this adds a kind of row beside it. -->

## Impact

- `workspace-core`: `Message::update` (`ReplyUpdate`), `Message::from_human`, `MessageKind::trouble`.
- `workspace-host`: new `updates.rs` (facts, comparison, baseline table, `reply_update` column), the turn end in `service.rs`, `set_plan`'s `news` field in `service.rs` and `mcp.rs`, the message-revision trigger.
- `workspace-desktop`: `transcript.rs` placements and rows, a new `ticket_table.rs` pinned in `conversation.rs`, the `collapse_ticket_table` preference.
- Skills: Coordinating v4 → v5 and coordinator core v7 → v8, with the fixtures that track the stock versions. The communication contract is unchanged: the news mark is display only and coordinator-specific, so the coordinator contract documents it.
- `docs/PRD.md`: Live work folding paragraph, the Tickets inspector line and the coordinator section.
- Older stores load unchanged: existing messages have no update and show as before.
