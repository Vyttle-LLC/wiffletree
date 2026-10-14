## 1. Behavior

- [x] 1.1 `run` closes the app-server's input when the outcome is read and stops its group after `EXIT_MARGIN` or at its exit. Test a server that writes its record only once its input has closed, and one that ignores the close.
- [x] 1.2 A failed turn whose message is empty or starts with a line break is named by its status. Test both forms.
- [x] 1.3 `compaction::outcome` bounds the reason a run ended with; `detail.result.reported` keeps it whole for either provider. Test both providers in `compaction.rs` and a Codex server that exits early with long error output.
- [x] 1.4 `takes_note` in `context_view.rs` decides the form's field. Test both providers.
- [x] 1.5 `CODEX_COMPACTION_LIMIT` is private to its module.

## 2. Tests

- [x] 2.1 The fake app-server stops and exits when its input closes. Checked that `false.then_some(writer)` in `run` fails the suite.
- [x] 2.2 The fake's `record: failed` writes a usage entry for a failed turn. Checked that dropping the "compacted only" condition fails the test.
- [x] 2.3 The line reads the same after the next turn.
- [x] 2.4 The limit test compares when the waiting message's turn began with when the compaction ended, in place of a look at the queue while it ran.

## 3. Docs and comments

- [x] 3.1 `docs/DEVELOPMENT.md`: `/compact` on stdin is Claude's; what `reported` holds; the margin; the fake's new options.
- [x] 3.2 Remove the comments that repeat the code in `context_view.rs`, `tests/compaction.rs` and `tests/fixtures/provider.py`.

## 4. Verify and archive

- [x] 4.1 One real Codex compaction through the headless host on a throwaway thread and a scratch store.
- [x] 4.2 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `python3 scripts/test_communications.py`, all passing.
- [x] 4.3 Run `openspec archive compact-codex-follow-ups`, then `openspec validate --all --strict`, which must pass.
