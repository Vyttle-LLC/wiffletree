## 1. Desktop

- [x] 1.1 Split `attention_card` into header, body and button row; given a height, cap the card and scroll the body, keyed by the item. Without one, draw as before.
- [x] 1.2 Pass half the window's height from `decisions()`.

## 2. Docs

- [x] 2.1 Add a sentence to `docs/DEVELOPMENT.md` (step 6) and `docs/PRD.md`; copy the approved study into `design/mockups/current/`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`; drive the app with `scripts/drive_desktop.py` against a long question.
- [x] 3.2 Run `openspec archive scroll-long-decision-card`, then `openspec validate --all --strict`.
