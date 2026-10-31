## 1. Core and the store

- [x] 1.1 Core: `Project.added_repositories`, `Space::lists`, `RepositoryUse`, `Project::repository_use`, `Snapshot::project_repositories`, `Snapshot.open_ticket_repositories`.
- [x] 1.2 Core: `Command::SetAddedRepositories`, human only, with its access row.
- [x] 1.3 Host: store schema 10, gated by the row. Verify `None`, `Some([])`, `Some` overlapping the Space, open tickets outside it, archived projects, a second run and that no Space changes.

## 2. Host

- [x] 2.1 `ensure_uses` wherever `uses` was read; the refusal at `create_ticket` and `create_session` with the Space's name and the way forward.
- [x] 2.2 `set_added_repositories` adds nothing new the Space lists and keeps what was already added, for `create_project`, Start project from issue, Attach and Add repositories too.
- [x] 2.3 Drop the "works in a repository you removed" refusal. Verify a verifier starts on an open ticket whose repository left the Space, and that the repository leaves once the last ticket closes.
- [x] 2.4 `move_project_to_space` keeps added repositories; `repository_uses` follows the new reasons.
- [x] 2.5 `create_project`'s description, `workspace_context` and the coordinator's core instructions, v13.

## 3. Desktop

- [x] 3.1 Overview › Repositories groups and Add menu, with the Space-lists-nothing note.
- [x] 3.2 Add to this project replaces Choose repositories….
- [x] 3.3 New project: plain rows for the Space, additions kept across a change of Space.
- [x] 3.4 Start project from issue says what the project will use.
- [x] 3.5 The Move dialog's gains, losses and what it keeps.
- [x] 3.6 Repositories page's projects use the snapshot.

## 4. Docs and checks

- [x] 4.1 `docs/PRD.md`, `docs/DEVELOPMENT.md`, `docs/host-protocol.md`.
- [x] 4.2 Run the app and check all screens, the Space-lists-nothing case and an open-ticket row. Compared in the app with the mockup on a seeded store: Overview with its Add menu, a Space listing nothing, a Space with nothing cloned here, an open-ticket row, Add to this project, New project, Move and the Repositories page. Screens 5 and 6 have no window in a seeded store: a test holds the refusal's text, and the upgrade's figures were computed read-only from the real store.
- [x] 4.3 No code path or text still offers "every workspace repository".
