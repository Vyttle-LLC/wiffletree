## Why

Reports, verdicts, check-ins and other agent-to-agent messages filled the coordinator's conversation with full bubbles, so the human had to scroll past coordination traffic to find the replies meant for them. The human approved a mockup that treats this traffic like tool calls: visible that it happened, with the detail on demand.

## What Changes

- **Peer messages fold into one row.** A peer message is anything another session sent this one (reports and verifier verdicts, turn results, check-ins, direct messages, a coordinator's assignments and instructions) and the session's own timer fires. Each unbroken run of them draws as one line where a turn summary sits, in its style: "5 agent messages · Implementer, Tester, Claude reviewer · 1 failed · 2h ago – 5m ago".
- **Trouble shows in red, still folded.** Failed and blocked reports and turn results that stopped with an error are counted in red on the row. They stay folded; the coordinator resolves blocks or brings them to the human as a decision card.
- **Detail on demand.** Clicking the row lists one line per message: sender, kind, first line and time. Clicking a line shows its full text. What is open is kept in memory per run and survives transcript refreshes.
- **Unchanged.** The human's messages, the session's own replies with their turn summaries, its own errors, Wiffletree's notices and compaction lines render as before. Nothing is stored, delivered or given to agents differently.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `work-stream`: "Conversation receives only the final reply" no longer shows peer messages unchanged; they fold into activity rows.

## Impact

- `crates/workspace-desktop/src/conversation.rs`: the peer-message predicate, run placement, the folded row and its lines.
- `crates/workspace-desktop/src/main.rs`: placements recomputed when messages change, and the open runs and lines.
- `docs/PRD.md`: the live work paragraph.
- The interactive study shows no peer messages, so it is unchanged.
- No host, core, storage or schema change.
