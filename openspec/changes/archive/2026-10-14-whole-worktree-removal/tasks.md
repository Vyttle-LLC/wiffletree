## 1. Removal

- [x] 1.1 In `worktrees.rs`, make `remove_worktree` rename the folder aside, prune, rename back on a Git failure and delete the aside folder off the host thread. Verify "A large worktree is removed whole" and "Git fails to forget the worktree".
- [x] 1.2 Add `delete_removed_worktrees` and call it at service start. Verify "A stop during deletion".
- [x] 1.3 Update the removal paragraph in `docs/PRD.md`.

## 2. Checks

- [x] 2.1 Run fmt, clippy, the workspace tests twice, `scripts/test_communications.py` and `openspec validate --all --strict`.
