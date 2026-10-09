## Why

Verifier sessions are reused across rounds and cycles, and ad-hoc reviewers and testers stay in the sidebar until their ticket is accepted or closed. A ticket that ships as a PR stays open until the PR merges, so its finished agents pile up with nothing left to do. Coordinators cannot retire one agent without finishing the whole ticket, which would also archive the implementer they still need for PR feedback.

## What Changes

- Verification retires its own verifiers. At the end of a round, every verifier that passed in that round is archived; failed verifiers stay for the next round. When a cycle ends (passed or blocked), every remaining verifier of that cycle is archived. A later `verify_ticket` cycle starts fresh verifier sessions, so no input is ever sent to an archived session. The implementer is never archived by this path.
- A ticket's (role, focus) agent lookup already skips archived sessions (`existing_assignment`, model-selection change), so once a cycle's verifiers are retired the next cycle and any later assignment get fresh sessions. Model selection's rule that an existing verifier is reused only at the same profile now applies only to a verifier the human restored.
- New `archive_agent` MCP tool for the coordinator: `{session_id}`, limited to agents of tickets it owns. It archives (restorable, conversation kept) and refuses, naming the reason, when the agent is mid-turn, is the implementer of an open ticket, or still owes a result to a running verification cycle.
- The implementer's lifetime is unchanged: only `accept_ticket` and `close_ticket` archive it. The coordinator contract says to archive ad-hoc reviewers once their findings reach the implementer, and to accept a ticket that ships as a PR only after the PR merges.
- Docs: `docs/DEVELOPMENT.md` describes both archiving paths.

## Capabilities

### New Capabilities
- `agent-lifecycle`: when ticket agents are archived — automatic verifier retirement, the coordinator's `archive_agent` tool and the implementer's lifetime.

### Modified Capabilities
<!-- None in openspec/specs. The verifier-reuse clause of the unarchived flatten-coordination-hierarchy change's ticket-verification spec ("the same session in every round") still holds within a cycle; design.md records that across cycles verifiers are now fresh. -->

## Impact

- `workspace-host`: `verification.rs` (retirement at round and cycle end), `live.rs` (`archive_agent` and its tool), `mcp.rs` (tool list).
- Role contracts: `skills/main-coordinator.md` (v5), `skills/tester.md` (v4).
- `docs/DEVELOPMENT.md`.
- No schema or stored-data change.
