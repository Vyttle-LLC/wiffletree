## Why

Accepting a ticket ran `git worktree remove` on a worktree holding a 4 GB `target/` of 20,346 files. Every Git call has a 15-second limit, so Git was killed partway: tracked files were gone, `target/` remained, the ticket stayed unaccepted and the call failed with "Git query timed out". Removal is meant to be all or nothing. The human approved this fix on October 14, 2026.

## What Changes

- Removing a ticket's worktree (accept, close, archive, a pending removal or an approved leftover) renames the folder aside beside it, has Git forget it with `git worktree prune` under the usual limit, then deletes the renamed folder without a time limit off the host thread.
- If Git fails, the folder is renamed back, so the worktree is whole and today's failure handling applies.
- At startup the host deletes any folder a stop left renamed aside.
- The keeper's own checkouts are unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ticket-workspaces`: removing a ticket's worktree is whole whatever its size.
- `leftover-worktree-cleanup`: approved removals use that removal instead of `git worktree remove`.

## Impact

`crates/workspace-host/src/worktrees.rs` (`remove_worktree`, `delete_removed_worktrees`), the startup in `service.rs`, a new `tests/worktree_removal.rs`, and the removal paragraph in `docs/PRD.md`.
