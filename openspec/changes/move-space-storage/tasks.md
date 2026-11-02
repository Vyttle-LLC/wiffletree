## 1. Host

- [x] 1.1 `storage_moves` in `sync.sql`, `repos::finish_moves`, `Record::restart` on a given connection.
- [x] 1.2 `storage_move.rs`: the check and every refusal, copy, scan, push, the switch and its one transaction.
- [x] 1.3 The gate on `storage_pass` and `sync_pass`, and the service's steps.
- [x] 1.4 `Command::CheckStorageMove`, `Command::MoveStorage`, `StorageMove`, `is_human_only`, the remote access row.
- [x] 1.5 Tests with a local bare repository as GitHub: each direction, each refusal, a held scan, a failed push, a stop at each step, adopting a tracker, two Spaces together, the human-only guard.

## 2. Desktop

- [x] 2.1 Move storage… in the Storage and issues box.
- [x] 2.2 The dialog of screens 2 to 5, with its check and busy states.
- [x] 2.3 Tests of its wording and the side it starts on.

## 3. Docs

- [x] 3.1 `ui/mockups.html`, `docs/DEVELOPMENT.md`, `docs/PRD.md`, `docs/FEATURES.md`, `docs/host-protocol.md`.
- [x] 3.2 The Spaces design's non-goal.
- [x] 3.3 Compare the app on a seeded store with the mockup, in dark and light.
