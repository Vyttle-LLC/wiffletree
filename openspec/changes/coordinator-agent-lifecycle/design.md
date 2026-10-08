## Context

- `verification.rs` runs a cycle: `verify_ticket` reuses the unarchived (role, focus) session at the same pinned profile (`live.rs::existing_assignment`, which skips archived sessions), archives one pinned to another profile (`verifier_replaced`) or creates one through `ticket_agent`, `end_round` decides the round, `next_round` re-runs failed verifiers, and the implementer's non-ready report ends the cycle (`Routing::Ends`).
- A verdict counts only for the verifier's latest consumed `verify:` input in the current round and while its result is pending; any other verdict from one of the ticket's verifiers is *late*: stored quietly, changing nothing (flatten-coordination-hierarchy, decision 5).
- `send` refuses archived recipients.
- `set_archived` archives a session and its tree. Ticket agents own no sessions or tickets, so archiving one archives only it and removes no worktree. It keeps the conversation and is restorable from the sidebar.
- `close_ticket` and `accept_ticket` archive every ticket agent through `finish_ticket`.

## Goals / Non-Goals

**Goals:** retire verifiers automatically; let the coordinator retire ad-hoc agents; keep the implementer until accept or close; never route input to an archived session.

**Non-Goals:** deleting sessions, any new storage, changing accept or close, or archiving agents of other coordinators' tickets.

## Decisions

### 1. Retire at round end and cycle end
`end_round` archives the round's passed verifiers when it hands failures back to the implementer. Whenever a cycle ends, as passed or blocked in `end_round` or through `Routing::Ends`, every verifier session named in the cycle's rounds is archived. One helper, `retire`, calls `set_archived` for each named session that is not archived and is not an implementer (defensive: a verifier is a tester or reviewer). It runs inside the report's `atomically`, so a failure leaves nothing half-done.

A verifier still in its turn when the cycle ends (the implementer reported `blocked` mid-round) is archived too. Archiving does not stop the turn: `change_archived` only flips the stored status from working to ready, so the verifier finishes its turn and its verdict arrives after the cycle ended. Nothing cancels that turn or its queued input.

### 2. Fresh sessions for a new cycle
The (role, focus) lookup ignores archived sessions. Every verifier of the last cycle is archived when that cycle ends, so the next `verify_ticket` creates fresh sessions and sends nothing to an archived one. Model selection's same-profile reuse therefore applies only to a verifier the human restored; a verifier at the same profile is no longer carried into the next cycle. *Alternative:* restore the archived verifier. Rejected: restoring re-runs the archive machinery in reverse, and a fresh session also gives an independent second look, which is what a new cycle is for. The same rule means `assign_ticket` after `archive_agent` creates a new reviewer instead of silently returning the archived one.

Late verdicts need no new code. A verdict from an archived verifier answering an earlier cycle's input is classified as late. A verdict answering the pending input of a cycle that has since ended, such as a verifier archived mid-turn when the implementer reported `blocked`, is still recorded on that ended round, with its result and `report_id`, for the record (flatten-coordination-hierarchy, decision 5). Either way it is stored quietly for the coordinator and never changes the ticket's state, the cycle's outcome or its rounds, and never wakes anyone. Reports from archived sessions are already accepted, because a turn in flight keeps its credential.

### 3. `archive_agent`
`{session_id}`. The caller must be the project coordinator owning the ticket in the agent's runtime (`owned_ticket`); an agent with no ticket is refused. Refusals, in order:

1. Mid-turn (`Status::Working`): stop it with `stop_turn` or wait for its report.
2. The implementer of an open ticket: it stays until `accept_ticket` or `close_ticket`.
3. A verifier the running cycle still needs: its latest result is not `passed`. That covers a pending verdict and a failed verifier waiting to re-check the implementer's fix; archiving the latter would make `next_round` send to an archived session.

An already archived agent is returned unchanged. Otherwise `set_archived(id, true)`.

### 4. Role contracts
`main-coordinator.md` v5 names `archive_agent`: archive an ad-hoc reviewer or tester once its findings are handed to the implementer; verifiers retire themselves; when a ticket ships as a PR, accept only after the PR merges so the implementer can address PR feedback. `tester.md` v4 says a verifier's session may be archived once its result is recorded, and a new cycle starts fresh sessions. `communication.md` needs no change.

## Risks / Trade-offs

- **A restored verifier can have two live sessions for one (role, focus).** If the human restores an archived verifier after a fresh one exists, the lookup returns the older one. Both work; nothing routes to an archived session. Accepted.
- **A queued `verify:` message stays queued on an archived verifier that never started its turn.** The scheduler never starts turns for archived sessions, and if the human restores one, its verdict is late and inert. Accepted rather than adding message cancellation.
