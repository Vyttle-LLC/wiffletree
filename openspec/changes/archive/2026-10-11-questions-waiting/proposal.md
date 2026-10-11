## Why

A worker may report `blocked` and keep working in the same turn. Since the small fixes of October 9 it shows as working while that turn runs, so its open question is no longer visible beside it. "Turn running" and "has an open question" are separate facts, and the human approved showing both ("Option A" of the question-waiting mockup).

## What Changes

- **A count of questions waiting.** The host counts, per session, the `blocked` reports it sent to its parent that the parent has not answered. A question is answered once a message the parent sent after the report is taken by one of the session's turns; a queued answer leaves it waiting. The count is read from stored messages, so it survives a restart.
- **In the snapshot and `workspace_context`.** The snapshot lists the count for each session that has one, and every `team` entry of `workspace_context` carries `questions_waiting`.
- **Beside the status.** An agent's status badge reads `Working · 1 question waiting` or `Blocked · 2 questions waiting`, and its sidebar row shows `1 question` or `2 questions`. The text is never yellow: a worker's question goes to its coordinator, not to the human.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: the count of questions waiting, in the snapshot and `workspace_context`.
- `native-workspace`: the badge suffix and the sidebar figure.

## Impact

- `crates/workspace-core/src/lib.rs`: `Snapshot.questions_waiting` and its lookup.
- `crates/workspace-host/src/lib.rs`, `live.rs`: the count, in the snapshot and in `workspace_context`'s team entries.
- `crates/workspace-desktop/src/tree.rs`, `ui.rs`, `sidebar.rs`: the badge label and the sidebar figure.
- Tests: `service.rs`, `tree.rs`.
- No schema change or data migration. Session status and ticket state are unchanged.
