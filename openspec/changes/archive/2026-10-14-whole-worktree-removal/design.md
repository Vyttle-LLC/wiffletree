## Decisions

- **Aside beside the worktree.** The folder moves to `.removing-<ticket id>` in the worktree's own parent folder rather than under the workspaces folder, so the rename never crosses filesystems, even for worktrees made before the workspaces folder moved. A rename is instant and cannot be cut off.
- **Git forgets with `git worktree prune`.** With the folder gone, prune drops its registration without touching files, so it stays inside the 15-second limit. The lock check that runs before removal is unchanged, because prune keeps locked worktrees.
- **Deletion on its own thread.** `fs::remove_dir_all` runs on a spawned thread so the host thread never waits for a large folder. A failure is logged; the folder is retried at the next start.
- **Startup cleanup by ticket.** Each ticket's aside path is fixed, so the start deletes exactly those that exist and nothing else.
