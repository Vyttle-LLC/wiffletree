## 1. Host

- [x] 1.1 Count each session's unanswered `blocked` reports from stored messages, and add the counts to the snapshot and to `workspace_context`'s team entries. Test the count mid-turn, after the turn ends, with a queued answer, with two reports, after a restart and once the answer is delivered, with the snapshot and `workspace_context` agreeing throughout.

## 2. Desktop

- [x] 2.1 Add the count to the status badge's label and a figure to the agent's sidebar row, in the status color and the subtle color. Test the wording for none, one and two questions in `tree.rs`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` and `python3 scripts/test_communications.py`, all passing.
- [x] 3.2 Run `openspec archive questions-waiting`, then `openspec validate --all --strict`, which must pass.
