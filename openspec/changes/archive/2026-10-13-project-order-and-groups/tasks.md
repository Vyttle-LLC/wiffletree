## 1. Host

- [x] 1.1 Add schema 7: a position for each project, with existing stores keeping their newest-first order.
- [x] 1.2 Return projects in the stored order and place a new project at the top.
- [x] 1.3 Add `Command::MoveProjects` for one project or a block, refusing invalid moves.
- [x] 1.4 Test persistence across reopen, the migration, new projects, single and block moves, and refused moves.

## 2. Desktop

- [x] 2.1 Group shown projects by first word in `tree.rs`, with stripped labels and the collapsed rollup, and unit tests.
- [x] 2.2 Add the Group button and save its state in `desktop-preferences.json`, on by default.
- [x] 2.3 Reorder by drag and drop with an insertion line and a dimmed dragged row.

## 3. Docs and study

- [x] 3.1 Add `design/mockups/current/project-groups.html`, and update `docs/PRD.md` and `design/README.md`.

## 4. Verify and archive

- [x] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` and `python3 scripts/test_communications.py`, all passing.
- [x] 4.2 Run `openspec archive project-order-and-groups`, then `openspec validate --all --strict`, which must pass.
