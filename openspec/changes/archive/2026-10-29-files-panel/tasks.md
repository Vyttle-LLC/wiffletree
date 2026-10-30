## 1. Core

- [x] 1.1 Add `Step.paths` (serde default) and `files.rs` with `FileActivity`, `AgentFiles`, `LineCount`, `TicketFiles`; add `Command::Files`.

## 2. Host

- [x] 2.1 `stream.rs`: set `StepUpdate.paths` for Claude read and edit tools and Codex file changes. Verify with the recorded and synthetic fixtures.
- [x] 2.2 `service.rs`: copy paths in `record_step` and store the step again when they change.
- [x] 2.3 `files.rs`: `Host::file_activity`, `Host::ticket_files`, and the Git reads (`line_counts`, `worktree_files`). Verify two runs of one session, a rollup across two tickets with a shared file, a running step that clears, a stored step without `paths`, uncommitted and untracked line counts, and a missing worktree.
- [x] 2.4 Answer `Command::Files` on a reply thread in `service.rs`.

## 3. Desktop

- [x] 3.1 `Panel::Files` with a folder icon between Git and Memory; fetch on open, selection, snapshot and step changes, one request at a time, nothing while closed.
- [x] 3.2 `files_view.rs`: tree building (verify merged directories and stable order), clash tags, agent view, coordinator view, live strip, empty states.

## 4. Docs

- [x] 4.1 Note the panel and its limits in `docs/DEVELOPMENT.md`.
