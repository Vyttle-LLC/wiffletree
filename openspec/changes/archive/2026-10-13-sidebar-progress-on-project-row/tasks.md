## 1. Desktop

- [x] 1.1 Draw the progress figure and track on the project row only, and return ticket and agent rows to single-line rows.

## 2. Docs and study

- [x] 2.1 Update the PRD, `design/README.md` and the interactive study to match, and rebuild `index.html` with `python3 scripts/build_mockups.py`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked -p workspace-core -p workspace-host` and `python3 scripts/test_communications.py`, all passing.
- [x] 3.2 Run `openspec archive sidebar-progress-on-project-row`, then `openspec validate --all --strict`, which must pass.
