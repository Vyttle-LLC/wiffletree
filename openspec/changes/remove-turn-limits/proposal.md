## Why

The human decided Wiffletree should have no turn limits. Today three caps decide when work may run: a per-project concurrency limit, a host-wide limit, and a brake that pauses a project after 100 turns without the human. They make `verify_ticket` refuse larger verifier lists, make rounds wait for slots, and stop whole projects without anyone deciding to. Runaway work should instead be caught by the agents' parents, who already get 30-minute check-ins and can call `stop_agents`.

## What Changes

- **No per-project limit.** `Project.turn_limit` is removed. Stored projects that still carry it load unchanged; the field is ignored. **BREAKING** for readers of `workspace_context` and the snapshot: `project.turn_limit` no longer appears.
- **No host-wide limit.** The host-wide caps on active turns and on active worker turns are removed, so the scheduler starts every session that has due input and is not paused, held, disconnected or waiting for its ticket's worktree.
- **No 100-turn brake.** The host no longer sets a project not live or raises the "ran 100 turns" inbox item.
- **`verify_ticket` starts any number of verifiers.** Its two refusals about turn limits go. A round still starts all its verifiers together or none of them, but it no longer waits for capacity, and nothing else is held back while it waits.
- **Turn-count check-ins.** Notification only; nothing is paused or stopped.
  - A child that has run 25 turns since its parent last messaged it sends its parent a `[check-in]`, and again every 25 turns after. It reuses the 30-minute check-in's message path, so it wakes the parent even during a verification round.
  - A project coordinator that has run 100 turns since the human last stepped in raises a human inbox item, again every 100 turns. A human message, retry or answer clears it.
- **Supervision prose.** `main-coordinator.md` (v10) tells the coordinator to judge each check-in against the child's ticket and recent reports, and to stop a child that is looping or off course with `stop_agents`. `communication.md` (v5) mentions the turn-count check-in.
- **Desktop.** The Review card's "verifiers need a project turn limit of at least N" warning is removed.
- The 30-minute check-ins are unchanged.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `model-routing`: removes "Bounded turn concurrency".
- `workspace-host`: adds that the host imposes no turn limits, and the time and turn-count check-ins, which no main spec covers yet.
- `ticket-verification`: `verify_ticket` drops its turn-limit refusals; rounds no longer wait for capacity or hold other workers; the default verifier list no longer cites a turn limit; any check-in wakes the coordinator during a round.
- `native-workspace`: the Review card no longer notes project turn limits.

## Impact

- `crates/workspace-core/src/lib.rs`: `Project.turn_limit` and the unused `TurnLimits`.
- `crates/workspace-host/src/service.rs`: scheduler, round admission, the turn counter, check-ins. `verification.rs`: `verify_ticket`. `lib.rs`: `create_project`. `schedules.rs`: module comment.
- `crates/workspace-desktop/src/models.rs`, `models_view.rs`: the turn-limit note.
- Skills: `main-coordinator.md`, `communication.md`.
- Tests in `service.rs`, `tests/verification.rs`, `tests/contracts.rs` and `models.rs`.
- Docs: `docs/DEVELOPMENT.md`, `docs/PRD.md`.
- No schema change or data migration.
