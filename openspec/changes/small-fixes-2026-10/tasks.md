## 1. PR watcher polish

- [ ] 1.1 In `tree.rs::pr_line`, check `Dirty` before the draft flag. Extend `the_pr_line_shows_only_the_parts_that_apply` with a draft that conflicts, for the line, its tone and `pr_sentence`.
- [ ] 1.2 In `repositories_view.rs::pr_check_status`, show nothing unless an open ticket's coordinator is not archived. Test a repository whose only open ticket belongs to an archived coordinator.
- [ ] 1.3 In `service.rs::refresh_pull_requests`, mark the ticket list stale on a change, read the store only when a pass is due or the list is stale and the last read is at least `ACTIVE_MS` old, and arm a wake-up for a stale list. Test that a change soon after a read does not reach the store, and that the wake-up time does.
- [ ] 1.4 Test in `pr_watch.rs` that a paused project's open ticket is still queried.

## 2. Host behaviour

- [ ] 2.1 Extend `workspace_core::host_notice` to `verification:`, `worktree-kept:` and `migration:` ids, labelled "Wiffletree". Test the labels in `workspace-core`, the turn input of a verification result in `service.rs`, and `from_human` in `conversation.rs`.
- [ ] 2.2 Keep `Host.in_turn`, filled at turn start and emptied at turn finish, and show those sessions as working in the snapshot and `workspace_context`. Test that a mid-turn `blocked` report shows working until the turn ends, while `close_ticket` still treats the agent as reported.
- [ ] 2.3 Group the scheduler query by recipient and drop its `LIMIT`. Test a paused coordinator with 101 queued messages and a worker with due input that the same pass reaches.
- [ ] 2.4 Make `tests/no_turn_limits.rs` wait on the change signal and check that the ten recorded turns overlapped.

## 3. Verify and archive

- [ ] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` twice and `python3 scripts/test_communications.py`, all passing.
- [ ] 3.2 Run `openspec archive small-fixes-2026-10`, then `openspec validate --all --strict`, which must pass.
