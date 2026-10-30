## Why

While agents work, the human cannot see which files they are in. The work stream shows one step at a time, and a file's path lives only in a step's title. There is no view of what an agent has read and changed over a ticket, and none of where several agents or tickets meet in the same file.

## What Changes

- **Steps carry their file paths as data.** `Step.paths` lists each file a read or edit step touched, for Claude and Codex, one entry per file.
- **A file-activity query in the host.** `Command::Files` answers, for an agent, its ticket with every file it touched across all its runs: read count, edit count and whether a read or write is running now. For the project coordinator it answers the same for every non-archived agent on each open ticket. It is derived from stored steps; nothing new is stored.
- **Line counts from Git.** Each ticket's reply carries `+N −M` per changed file from the worktree's diff against the ticket base, uncommitted changes included. Git runs off the host thread and only reads.
- **A Files panel on the inspector rail**, between Git and Memory, for every role. The agent view is a collapsible tree with `read` / `edited` badges, live `reading…` / `writing…` rows, a strip of what is live now and a Touched / Whole tree toggle. The coordinator view groups by ticket or by agent and tags files that several agents or tickets touch.
- **Not included:** files read or written only through shell commands (the Git diff still shows such writes), opening or previewing files, and any change to verification or tickets.

## Capabilities

### New Capabilities
- `files-panel`: structured step paths, the file-activity and rollup query, Git line counts, and the Files panel's agent and coordinator views.

### Modified Capabilities
<!-- None. work-stream's step requirements are unchanged; the added field is specified in files-panel. -->

## Impact

- `workspace-core`: `Step.paths`; a new `files.rs` with `FileActivity`, `AgentFiles`, `LineCount`, `TicketFiles`; `Command::Files`.
- `workspace-host`: `stream.rs` (paths on `StepUpdate`), `service.rs` (`record_step` stores a step again when its paths arrive; `Command::Files` answered off the host thread), a new `files.rs` (queries and Git reads).
- `workspace-desktop`: `Panel::Files`, a new `files_view.rs`, a pinned head and foot in `inspector.rs::panel_view`, `activity.rs` (the step-change signal also refreshes the open panel).
- Mockup: `ui/mockups.html`, approved as drawn.
