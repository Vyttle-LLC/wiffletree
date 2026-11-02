## Why

A Space's storage is chosen once, when the Space is created, and nothing changes it. A local Space that a team later wants to share has no way to GitHub, a shared Space has no way out, and a repository chosen in a hurry stays chosen. The Spaces design listed moving storage as a non-goal so that it would not hold up the first release.

The human approved the study in `ui/mockups.html` on November 1, 2026, as drawn.

## What Changes

- **Move storage…** joins the Storage and issues box of every Space's page and opens a dialog: This Mac or A GitHub repository, starting on the side the Space is not on.
- **Check says what happens first.** It reuses New Space's check, says who can read the target and lists what moves, naming every Space stored in the same repository, as Spaces in one repository share its tracker and move together. Moving checks again, as the answer can go stale.
- **Refusals, each with its own sentence, write nothing:** a public repository; a repository that is a brain here; a prefix another storage repository here uses; a target with a tracker when the storage has issues; a target already holding a Space in the same folder; storage that is also this Mac's settings repository; a job run in progress in a Space that would move; an import from Linear running; issue changes still waiting to be pushed.
- **All or nothing.** The files are copied to a staging clone, scanned and pushed, and only then does the Space switch. A held scan or a failed push leaves the Space where it was and the dialog says why, with no match text.
- **One switch.** Every reference to the storage repository changes in one transaction, which also records the folders still to rename, as `repo_moves` does, so a stop at any step ends fully moved or fully not moved.
- **Nothing is deleted.** Old local storage goes to `backups/` with its Git history; an old GitHub clone stays and stops syncing; GitHub's old repository is never written.
- **Human-only.** Moving is in `is_human_only` and the remote access list, and no agent or workspace tool can start it.
- **Not included:** moving storage that is also this Mac's settings repository; moving one Space out of a repository that holds several; carrying Git history into the new repository; merging a tracker with issues into another.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `spaces`: a Space's storage can move, by the human, and the Spaces page offers it.

## Impact

- `workspace-core`: `Command::CheckStorageMove`, `Command::MoveStorage`, `StorageMove`, and their place in `is_human_only` and the read and write lists.
- `workspace-host`: `storage_move.rs`, `storage_moves` in `sync.sql`, `repos::finish_moves`, `Record::restart` on a given connection, a gate that holds off passes while a move runs, the service's steps, and the remote access row.
- `workspace-desktop`: the Move storage… button and dialog in `spaces_view.rs`.
- Docs: `docs/DEVELOPMENT.md`, `docs/PRD.md`, `docs/FEATURES.md`, `docs/host-protocol.md`, and the Spaces design's non-goal.
