## Context

See proposal.md for why. Turn limits live in four places on origin/main be5f484:

- `workspace-core/src/lib.rs`: `Project.turn_limit` (no serde default, so every stored project has it) and `TurnLimits`, which nothing uses.
- `workspace-host/src/service.rs`:
  - `HOST_TURNS` (8, every active turn), checked twice in `Actor::schedule`.
  - `HOST_WORKER_TURNS` (6) and `worker_turns(project)` (`turn_limit - 1`), checked in `schedule` and `admit_round`.
  - `Actor.turns: HashMap<project, usize>`, incremented in `start_turn` for every turn and reset to 0 by a human `Send`, a `ReconcileSession` retry, `ResolveAttention` or `SetLive { enabled: true }`. At 100, `schedule` sets the project not live and raises a `turn-budget:` inbox item; `admit_round` reports `Stuck`. The step-in branch in `handle` resolves open `turn-budget` items.
  - `admit_rounds` returns `hold_workers`, true when a round is `Waiting`. `schedule` then starts no worker turn in any project, so freed capacity collects for the round.
- `workspace-host/src/verification.rs` `verify_ticket`: two `ensure!`s on the verifier count.
- `workspace-desktop`: `models::turn_limit_note`, used by `ModelsView::review_card`.

Check-ins today: `check_in_long_turns` and `check_in` send `check-in:{run}:{minutes}` from the child to its parent, or for the root coordinator raise an inbox item with that operation id. `settle_check_ins` clears those items when the turn ends or a newer one arrives, and `recover_unfinished_turns` clears them on restart. A check-in message is not quiet, so it wakes the coordinator during a verification round. `flatten.rs` cancels `check-in:` messages during migration instead of re-sending them.

## Goals / Non-Goals

**Goals:** remove every concurrency cap and the turn brake; add turn-count check-ins on the existing message and inbox paths; keep the diff small.

**Non-Goals:** human-configurable check-in thresholds, durable turn counts for the human, rate or spend controls, and any change to the 30-minute check-ins.

## Decisions

### Remove the caps

- Delete `Project.turn_limit`, its value in `Host::create_project`, `TurnLimits`, `HOST_TURNS`, `HOST_WORKER_TURNS`, `worker_turns` and `TURN_BUDGET`. serde ignores unknown fields and no struct sets `deny_unknown_fields`, so stored projects load without a migration. Their JSON keeps the old key until the project is next written, which is harmless. `tests/repositories.rs` already writes `"turn_limit":4` into a fixture, which now doubles as the old-store test.
- `Actor::schedule` drops both `HOST_TURNS` early exits, the per-project and worker checks, and the 100-turn block.
- `admit_round` drops the 100-turn `Stuck` check and the capacity `Waiting` check. It still returns `Waiting` when a member is already active, the worktree is busy or input is not yet due, and `Stuck` for a paused, disconnected, held or archived member, or a project that is not live.
- **`hold_workers` goes too.** It exists only so that freed capacity collects for a waiting round. With no capacity, a round waits only for the ticket's own implementer to leave the worktree. Holding every worker in every project during that wait would be a new stall. `admit_rounds` returns just the round members, which `schedule` still skips so verifiers start only as a round.
- `verify_ticket` drops both count refusals; the `worker_turns` and `HOST_WORKER_TURNS` import goes.
- Desktop: delete `turn_limit_note`, its test, and the `note` in `review_card`.

### Turn-count check-ins

- **Child to parent: counted from the store.** In `start_turn`, after `record_turn_start`, a session with a parent counts its `provider_runs` rows whose `started_at` is after the latest message it received from its parent (`sender = parent_id`); with no such message, all its runs count. When the count is a positive multiple of `CHILD_CHECK_IN_TURNS` (25), the host sends `check-in:{run}:turns` from the child to the parent. The run id makes the id unique, and the `check-in:` prefix keeps today's behavior: not quiet, wakes the coordinator during a round, and cancelled rather than re-sent by `flatten.rs`. The body follows `check_in`'s: `[check-in] {name}{ticket} has run 25 turns since you last messaged it at {time}. Its last reply: {first 300 characters of its latest output:… message}.` and the same `stop_agents` line. *Alternative:* an in-memory counter reset on parent messages would need a hook in every send path; one indexed query per turn start is simpler and survives restarts.
- **Coordinator to human: the existing counter, narrowed.** The human is not a session, and a human message cannot be told apart in the store from host messages such as cycle outcomes, which also have no sender. So the counter stays in `Actor.turns`, reset by the same human commands, but counts only turns of sessions without a parent. That means the project coordinator's own turns, as the brief asks, not all of the project's turns. At each multiple of `HUMAN_CHECK_IN_TURNS` (100), the host raises an inbox item for the coordinator with operation `turn-count:{run}`. It first resolves any open `turn-count:` item for that session as superseded. The step-in branch in `handle`, which today clears `turn-budget` items, clears `turn-count` items instead and no longer touches live state. Because the prefix is not `check-in:`, the item survives the turn's end and restarts, and `settle_check_ins` and recovery stay unchanged. `ensure_unreserved` in `lib.rs` reserves `turn-count:` as it already reserves `check-in:`, so agents and clients cannot create such an item. `SetLive` no longer resets the count, since it no longer gates anything.
- **Thresholds are constants, not settings**, as YAGNI suggests. The brief's 25 and 100 are used.

### Prose

- `main-coordinator.md` v9 → v10 replaces the check-in sentences: "You receive a [check-in] when a child's turn has run 30 minutes, and when it has run 25 turns without a message from you. Treat each one as a decision: compare its recent activity with its ticket and its last reports. Let it continue when it is making progress. Stop it with stop_agents and a reason when it repeats itself, works outside its ticket or makes no progress, then message it with a correction or resume it. Your own long or many turns raise a check-in in the human's inbox."
- `communication.md` v4 → v5 adds to the check-in sentence: "…and after every 25 turns you run without a message from your parent."
- `tests/contracts.rs` pins the new versions.

## Risks / Trade-offs

- [Unbounded concurrency can exceed provider rate limits or machine resources] → The human chose this. Check-ins and `stop_agents` are the control. A provider error already holds the session for reconciliation rather than retrying.
- [The human's turn count resets on host restart, as today] → The notice may come later than 100 turns after a restart. It is advisory, and a restart often follows a human action anyway.
- [Child turns started by host messages, such as routed verifier findings, do not reset the count] → This is intended: the count measures turns without parent attention.
- [Old stores may hold an open `turn-budget:` inbox item and a project the brake set not live] → No migration. The item is still dismissable, and the human's next message sets the project live, as today.

## Migration Plan

None. Stored `turn_limit` values are ignored. Rollback restores the field. Projects written by the new code then lack it, so a rollback would need `#[serde(default = 4)]`. That is noted here, not built.

## Open Questions

None that change the plan. The decisions above that go beyond the brief, which are removing `HOST_TURNS` and `hold_workers`, counting coordinator turns rather than all project turns, and keeping the human count in memory, are listed for the human in the approval report.
