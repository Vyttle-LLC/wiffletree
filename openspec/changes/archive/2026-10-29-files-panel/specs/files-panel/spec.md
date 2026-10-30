## Purpose

Shows which files agents read and change while they work: per agent over its ticket, and rolled up by ticket for the project coordinator.

## ADDED Requirements

### Requirement: Steps carry the files they touched
A read or file-edit step SHALL carry the paths of the files it touched as data, one entry per file, for both Claude and Codex. Paths SHALL be relative to the ticket worktree; a path outside it SHALL be kept as the provider gave it. A step stored before this field existed SHALL still load, with no paths. Files read or written only through shell commands are not recorded as paths.

#### Scenario: Claude edit
- **WHEN** Claude edits `/work/repo/src/new.rs` in the worktree `/work/repo`
- **THEN** the step's paths are `src/new.rs`

#### Scenario: Codex change to several files
- **WHEN** one Codex file change covers `src/lib.rs` and `tests/parse.rs`
- **THEN** the step's paths are those two files, one entry each

#### Scenario: Older stored step
- **WHEN** the store holds a step written without a paths field
- **THEN** it loads and counts toward no file

### Requirement: Ticket-long file activity
The host SHALL answer, for a ticket agent, every file its read and edit steps touched across all of its runs, each with its count of finished reads, its count of finished edits, and whether a read or a write of it is running now. The answer SHALL be derived from stored steps. Failed and interrupted steps SHALL count nothing.

#### Scenario: Two runs
- **WHEN** an agent read `a.rs` in one run and read and edited it in the next
- **THEN** `a.rs` is listed once with 2 reads and 1 edit

#### Scenario: Live then finished
- **WHEN** a write of `b.rs` is running
- **THEN** `b.rs` is listed as being written, and once the step succeeds it is listed with 1 edit and nothing running

### Requirement: Coordinator rollup
For a project coordinator the host SHALL answer the same activity for every non-archived agent on each open ticket the coordinator owns, grouped by ticket and naming the agent for each file.

#### Scenario: Shared file
- **WHEN** two open tickets each have an agent that touched `src/steps.rs`
- **THEN** both tickets list the file under the agent that touched it

### Requirement: Line counts from Git
Each ticket's answer SHALL carry added and removed line counts per changed file, from the ticket worktree's diff against the ticket base, including uncommitted changes and new untracked files. Git SHALL only be read, and SHALL NOT run on the thread that serves host commands. A missing worktree SHALL yield no counts and no error.

#### Scenario: Uncommitted change
- **WHEN** a tracked file in the worktree has two lines added and not committed
- **THEN** its count is +2 −0

#### Scenario: Worktree gone
- **WHEN** the ticket's worktree folder no longer exists
- **THEN** the file activity is still answered, without line counts

### Requirement: Files panel
The inspector rail SHALL offer a Files button to every role, between Git and Memory. While the panel is closed it SHALL request nothing. While open it SHALL follow step changes through the existing step-change signal.

For an agent, the panel SHALL show a collapsible tree of the files it touched, with `read` and `edited` badges and `×N` counts, line counts, a highlighted row with a pulsing dot and `reading…` or `writing…` while a step runs, a strip listing what is live now, header counts, and a Touched / Whole tree choice that defaults to Touched. Whole tree SHALL list the worktree's tracked and untracked-but-not-ignored files with touched ones marked.

For the project coordinator, the panel SHALL group files by ticket, name the agent for each file, offer By ticket / By agent, and mark with a yellow tag a file that more than one agent in a ticket touched or that appears in more than one ticket of the same repository.

The implementer's view, and the coordinator's view by ticket, SHALL also list files the ticket's Git diff changed that no step named, with their line counts. A tester's or reviewer's view SHALL list only files its own steps named. A failed request SHALL show its error in the panel until a later request succeeds.

#### Scenario: Nothing touched
- **WHEN** the selected implementer has touched no file and its ticket has no changes
- **THEN** the panel says so instead of showing an empty tree

#### Scenario: Reviewer on a changed ticket
- **WHEN** the selected reviewer has touched no file and the ticket's diff changes three files
- **THEN** the panel says the reviewer has touched nothing

#### Scenario: Request fails
- **WHEN** the host cannot answer the panel's request
- **THEN** the panel shows the error instead of a loading line

#### Scenario: No tickets
- **WHEN** the selected coordinator's project has no open tickets
- **THEN** the panel says there are no tickets to show

#### Scenario: Shared directories
- **WHEN** an agent touched `crates/core/src/a.rs` and `crates/core/src/b.rs`
- **THEN** both files sit under one `crates/core/src` row
