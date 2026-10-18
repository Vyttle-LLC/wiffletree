# Phase 3b host measurements

Measured locally on October 17, 2026, against Phase 2 head
`181e06445b9e3fba319466af0c067ec4f855706d`.

| Measurement | Before | After |
| --- | ---: | ---: |
| Snapshot turnaround behind a GitHistory operation with a 300 ms fake Git delay | 330.24 ms | 1.34 ms |
| Admission passes for 30 streamed narration updates | 30 | 0 |

These are individual local samples, not throughput or percentile guarantees. Both
versions use the same small real repository, store fixture and Git adapter. The
adapter sleeps for 300 ms before invoking real Git. The timer starts before the
history request is handled and stops when the following snapshot reply arrives.
The after test also checks that the history reply is still pending: the read did
not merely run after Git happened to finish. The streaming measurement calls the
actor's event processing path, including notification and scheduling decisions.

Repeat the after measurement with:

```sh
cargo test --locked -p workspace-host --lib hot_path_measurements -- --nocapture
```

## Boundaries and coverage

`checkout` prepares store inputs, reserves conflicting ticket operations, runs Git
without a store writer, and revalidates results before applying them on the actor.
Creation reserves its worktree and branch durably. Keeper movements record their
intent before changing HEAD. A restart or panic between Git and application holds
an uncertain result as unreviewed, rather than allowing it to be published. A
rollback remains journaled and reserved through its following job. Shutdown waits
for local checkout completions before releasing the store lock. Network publish
steps keep their existing last-moment checks against the current store and Git.

Interface tests cover name collisions, creation restart recovery, stale ownership
and publication observations, dirty worktrees even with Git's untracked display
disabled, ordered move validation, interrupted movement, and a Git adapter that
panics after changing HEAD. Actor tests cover reads during slow Git, retained input
while preparing a worktree, a conflicting archive waiting for preparation, publish
approvals, keeper handoffs, and telemetry's separate signal.

The scheduling module owns admission invalidation, eligibility, round occupancy
and wake deadlines. Reads, streamed text, usage and stale timer events do not run
admission. Running verification cycles have an indexed store query. Progress joins
and revision merges use indexes while preserving their original order.

`Service::usage_changes()` has the same optional, coalesced `Receiver<()>` shape
as the existing state, steps and progress signals. Usage, quota and model-usage
records are persisted before this notification. Clients fetch Usage and
ProviderQuota on this signal. Clients that ignore it still query the same
persisted records; there is no protocol-version or stored-message change. This
branch changes no desktop files; the Phase 3a desktop owns the consumer.

Merge and sorting runs share no-tools attestation, structured-result recognition,
usage extraction and bounded stdout/stderr handling. Their prompts, schemas and
domain acceptance rules remain separate. Stdout uses the existing bounded event
reader; stderr retains only its last 16 KiB.

The Files tally and persisted process observation timestamps were not changed.

## Validation

All four requested checks passed on the final source:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked -p workspace-host && python3 scripts/test_communications.py
```

The workspace run passed 1,240 tests with three existing ignored tests. The
communications fixture completed its cross-provider handoff in 1.457 seconds and
passed steps, usage/quota, timer, permission, resume, cancellation and restart
checks. The final isolated slow-Git sample was 1.338583 ms, with zero admission
passes for the 30 streamed steps.

The monitoring and supervision fixtures now notify admission after changing their
clock or queue directly in SQLite. Snapshot polling only observes their results.

## Remaining lifecycle paths

The enumerated H2 paths use returned jobs. The coordinator approved these
remaining synchronous paths as follow-up scope:

- The archive/accept/close removal transaction keeps its data-loss guards and
  single-writer ordering in [check_removable](../../crates/workspace-host/src/worktrees.rs#L161)
  and [remove_worktree](../../crates/workspace-host/src/worktrees.rs#L201). It runs rarely
  and remains synchronous for now.
- Keeper polling's read-only ancestry checks were left for Phase 4. They now run
  through `checkout::KeeperPoll` off the actor; see the
  [Phase 4b measurements and contract](host-revisions.md).
