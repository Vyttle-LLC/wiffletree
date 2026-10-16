## 1. Desktop

- [x] 1.1 Keep a set of minimized project IDs on the main view. After each snapshot, drop the entry of any project with no open decision.
- [x] 1.2 Add the Minimize button to the card's header row and the minimized row to `decisions()`; the row and the Expand button restore the card.
- [x] 1.3 Test the rule: the entry is dropped when a project's decisions reach zero, kept when one more arrives, and kept for a project whose decisions are unchanged while another's reach zero.

## 2. Docs

- [x] 2.1 Add one sentence to `docs/DEVELOPMENT.md` (step 6) and `docs/PRD.md`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`, all passing; drive the app with `scripts/drive_desktop.py`.
- [x] 3.2 Run `openspec archive minimize-decision-card`, then `openspec validate --all --strict`, which must pass.
