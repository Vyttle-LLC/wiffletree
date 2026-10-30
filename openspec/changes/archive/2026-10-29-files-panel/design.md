## Context

See proposal.md for why and `ui/mockups.html` for the approved look. At origin/main 6c71ab1, `stream.rs` puts a read or edit step's path in its title (Codex joins several with ", "), `service.rs::record_step` writes a step to the store when it starts or changes state, and steps are kept per run, at most `MAX_RUN_STEPS` of them, for 30 days.

## Decisions

### 1. `paths` on the step, stored when it arrives
`StepUpdate` and `Step` gain `paths: Vec<String>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`, so stored steps load unchanged and no migration is needed). Claude `Read`, `Edit`, `MultiEdit`, `Write` and `NotebookEdit` give one path; a Codex `file_change` gives one per change. Paths are relative to the session's working directory, which is the ticket worktree; a path outside it stays as the provider gave it.

A Claude tool call is first announced with empty input, and its path arrives with the complete message while the step is still running. `record_step` therefore also writes the step when its paths change. That is one more write per file tool call, and it keeps the store the only source: the file queries never read a running turn's memory.

### 2. File activity is a tally over stored steps
`Host::file_activity(session)` tallies the session's `read` and `file_edit` steps across all runs, per path: succeeded reads, succeeded edits, and whether a step on it is `Running`. Failed and interrupted steps count nothing, so a file whose only read failed is not listed. The result is sorted by path. It uses the existing `steps_session` index to find the session's steps and decodes only each step's kind, state and paths in Rust; SQLite's JSON functions were about four to eight times slower on the host thread (see [the measurement](../../../docs/performance/files-tally.md)). No index or table is added.

*Consequences of deriving from steps:*
- A run keeps its first `MAX_RUN_STEPS` (2,000) steps. Files touched only after that in one run are missing from the counts. Their edits still appear through the Git line counts.
- Steps older than 30 days are pruned, so counts on a very long ticket can shrink. No second store is added for either case; the panel is a working view, not an audit log.
- Codex reads files through shell commands, so Codex agents show edits but no reads.

### 3. One command, same shape for agent and coordinator
`Command::Files { session_id, whole_tree }` answers `Vec<TicketFiles>`. For a ticket agent it holds that agent's ticket with only that agent. For a project coordinator it holds each open ticket it owns with every non-archived agent on it. Any other session gets an empty list. Clash tags (several agents on a file, or the same repository path in two tickets) are derived in the client from this reply.

### 4. Git is read on a reply thread
`Host::ticket_files` gathers the reply and each ticket's worktree and base on the host thread, from the store only. The service then finishes the reply on its own thread, like repository scans:
- `git diff --numstat --no-renames -z --merge-base <base>` in the worktree gives `+N −M` for tracked files, uncommitted changes included.
- `git ls-files --others --exclude-standard -z` gives new files not yet tracked; each counts its lines as added. One over 1 MB or not UTF-8 is listed as changed with +0.
- With `whole_tree`, `git ls-files --cached --others --exclude-standard -z` lists the worktree.

A missing worktree or a failing Git query leaves that ticket without counts or tree; the reply never fails for it.

### 5. The panel rides the step-change signal
The desktop asks for `Command::Files` when the panel opens, when the selection or snapshot changes while it is open, and on each step-change signal while it is open. One request is in flight at a time, at most every 500 ms. A closed panel asks for nothing. The tree is built from the flat paths in the client: directories merge, a chain of single-child directories is one row, and directories sort before files, each by name, so rows do not move as counts change.

An agent's view shows that agent's files. Only the ticket's implementer writes in the worktree, so only its view, and the coordinator's By ticket rollup, also list files the ticket's diff changed that no step named, with their line counts and no badge; that is how shell-only writes surface. A tester or reviewer sees only the files its own steps named, with line counts on those, and the empty state when it has none even if the ticket has changes.

A failed request shows its error in the panel in place of the tree until the next reply succeeds.
