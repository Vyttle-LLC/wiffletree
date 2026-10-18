## 1. Core

- [x] 1.1 Add `Notice` and `MessageKind::Notice`: the pill's words from a host notice's id and body, whether it reports a failure or a block, and a generic notice when the words cannot be told.

## 2. Desktop

- [x] 2.1 Make every host notice a peer message, so it folds into runs; keep the human's messages and Inbox answers breaking a run.
- [x] 2.2 Count agent messages and notices apart in a run's heading, and count notice failures among the run's troubles.
- [x] 2.3 Draw a notice's line with its sender and a muted pill, red when it reports trouble; remove `Shape::Notice` and `notice_message`.
- [x] 2.4 Update the transcript tests and add tests for the mixed heading, notice pills and notice trouble counts.

## 3. Design and docs

- [x] 3.1 Copy the approved study to `design/mockups/current/quiet-notices.html` and link it from `design/README.md`.
- [x] 3.2 Record the by-eye pass in `docs/DEVELOPMENT.md`.

## 4. Verify and archive

- [x] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`; drive the app with `scripts/drive_desktop.py` on a throwaway store in Tidal Light and Dark.
- [x] 4.2 Run `openspec archive quiet-notices`, then `openspec validate --all --strict`.
