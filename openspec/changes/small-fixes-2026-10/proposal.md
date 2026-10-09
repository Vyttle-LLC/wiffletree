## Why

The PR watcher shipped with four follow-ups from its review, and three host behaviours confused the human or could stall work: host messages read as the human's, a working agent shows its mid-turn report instead of "working", and a paused coordinator with a long queue can keep every worker from starting. One integration test also failed once under load. The human approved fixing them together as small changes.

## What Changes

- **Conflict outranks draft.** A draft PR that conflicts with its base shows the red `conflict` word on its row, not `draft`.
- **No stale check line.** A repository that leaves the watch, because every open ticket there belongs to an archived coordinator, shows no PR check line on the Repositories page, instead of a stale "PRs checked … ago".
- **Fewer store reads.** The watcher reads the store for its tickets only when a pass is due, or when something changed and it has not read in the last minute. Ordinary change events no longer cost a store read.
- **Paused projects.** A test pins that a paused project's open PR is still checked.
- **Host messages come from Wiffletree.** Messages the host sends without a sender, such as verification results, kept-worktree notices and migration notices, are labelled "Wiffletree" in the agent's turn input and in the chat, as the PR watcher's already are. The human's own messages, including inbox answers, keep the human label.
- **A running turn shows as working.** A session in a turn shows `working` in the sidebar and in `workspace_context`, even after it reported `blocked` or `ready_for_testing` mid-turn. The stored status, which `close_ticket` and `archive_agent` check, is unchanged.
- **No scheduler starvation.** The scheduler considers each session with due input once, so a paused coordinator with hundreds of queued messages no longer hides runnable workers.
- **Deterministic test.** `tests/no_turn_limits.rs` checks from the recorded turn starts and finishes that the ten turns overlapped, waking on the host's change signal instead of polling against a 30-second deadline.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `pr-watch`: the merge word order, the store reads between passes, a paused-project scenario, and no check line for a repository no longer watched.
- `workspace-host`: host messages labelled as Wiffletree, a running turn shown as working, and the scheduler reaching every runnable session.

## Impact

- `crates/workspace-core/src/lib.rs`: `host_notice` covers the host's other notice ids.
- `crates/workspace-desktop/src/tree.rs`: `pr_line`.
- `crates/workspace-host/src/service.rs`: `refresh_pull_requests`, the scheduler query, and turn start and finish keeping the host's set of running turns.
- `crates/workspace-desktop/src/repositories_view.rs`: `pr_check_status` skips repositories no longer watched.
- `crates/workspace-host/src/pr_watch.rs`: a paused-project test.
- `crates/workspace-host/src/lib.rs`, `live.rs`: the snapshot and `workspace_context` show a running turn as working.
- Tests: `tree.rs`, `repositories_view.rs`, `conversation.rs`, `service.rs`, `pr_watch.rs`, `workspace-core/tests/pull_requests.rs`, `tests/no_turn_limits.rs`.
- No schema change or data migration.
