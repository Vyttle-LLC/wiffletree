## 1. Desktop

- [x] 1.1 Scroll the project tree while a row is dragged near its top or bottom edge, with the insertion line following.
- [x] 1.2 Cancel a drag released on its own row after leaving it; a press that never left its row stays a click.
- [x] 1.3 Make a moving click on a project's chevron toggle it and a moving double-click open rename.
- [x] 1.4 Clear the rename press on the release of any button, anywhere.
- [x] 1.5 Unit test the scroll step and the release on a drag's own row.

## 2. Host test

- [x] 2.1 Find why the stop and resume test fails under load and make it deterministic.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` and `python3 scripts/test_communications.py`, all passing.
- [x] 3.2 Drive the four behaviours in a debug build on a scratch store and record it in `docs/DEVELOPMENT.md`.
- [x] 3.3 Run `openspec archive project-drag-follow-ups`, then `openspec validate --all --strict`, which must pass.
