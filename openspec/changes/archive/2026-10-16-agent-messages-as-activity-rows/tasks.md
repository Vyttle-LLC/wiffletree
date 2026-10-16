## 1. Desktop

- [x] 1.1 Classify peer messages, place each run under its first message, and draw the run as one row styled like the turn summary, with failed, blocked and error counts in red.
- [x] 1.2 Open a run to list its messages and a line to show its full text; keep both in memory, keyed by message id, and measure the run's row again when it changes.
- [x] 1.3 Unit tests for the predicate, placement and counts.

## 2. Docs

- [x] 2.1 Update the PRD's live work paragraph.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`, all passing.
- [x] 3.2 Run `openspec archive agent-messages-as-activity-rows`, then `openspec validate --all --strict`, which must pass.
