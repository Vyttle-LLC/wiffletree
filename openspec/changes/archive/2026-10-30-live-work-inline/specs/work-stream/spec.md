## ADDED Requirements

### Requirement: Live work in the conversation
While the selected session is working, its conversation SHALL show the current turn's work after the last message, in turn order. Narration SHALL be shown in full, with its file references linked. A file edit SHALL be shown with its diff open: a header with its path and lines added and removed, the first 10 diff lines, and a toggle naming how many more kept lines there are, which shows them all; lines past those kept SHALL be noted as today once every kept line is shown. A command SHALL be one line with its title, state and note, or its running time while it runs; a failed command SHALL be red, and its output SHALL open on click. Each unbroken run of read, search, thinking, plan and tool steps SHALL fold into one muted line counting them by kind, such as "Read 4 files · searched 2 times · 1 tool call", which SHALL open in place to the run's steps. A failed step of those kinds SHALL NOT fold: it SHALL break its run and show on its own, red. The live work SHALL be drawn through the transcript's list, so that a run of 2,000 steps does not block the interface, and a page of new steps SHALL measure again only the rows that changed.

A status line SHALL sit between the transcript and the composer while the session works, at a fixed height, showing that it is working, the elapsed time, the step count, the number of files changed and the current step. Activating it SHALL open the turn's history. Its chevron SHALL collapse the live work, leaving only the status line, which then shows the latest narration; the choice SHALL apply to later turns and sessions and SHALL persist across app launches. The conversation SHALL follow new work only while the reader is at the bottom; scrolled up, it SHALL keep the reader's place, and the status line SHALL offer "Jump to latest". The elapsed-time refresh SHALL run only while a visible session is working.

#### Scenario: Agent narrates, edits and runs a command
- **WHEN** the selected agent writes "Tightening the message", edits `protected.rs` with a 22-line diff and then runs `cargo test`, which fails with exit 101
- **THEN** the conversation shows the narration, the edit with its first 10 diff lines and "12 more lines", and a red `cargo test` line noting exit 101, in that order

#### Scenario: Quiet steps fold per run
- **WHEN** the agent reads four files, searches twice, narrates, and then reads one more file
- **THEN** one line reads "Read 4 files · searched 2 times" before the narration, and a second line follows it for the later read

#### Scenario: A failed quiet step stays visible
- **WHEN** a search fails between two reads
- **THEN** each read folds on its own and the failed search shows between them, red

#### Scenario: Reading while streaming
- **WHEN** steps arrive while the user reads earlier messages
- **THEN** the transcript's scroll position and the status line's height do not change, and the status line offers "Jump to latest"

#### Scenario: Collapse and restore
- **WHEN** the user collapses the live work, selects another working agent, and then restores it
- **THEN** the other agent shows only its status line, with its latest narration, and restoring shows its live work again

#### Scenario: Idle app
- **WHEN** no visible session is working
- **THEN** no activity timer runs

## REMOVED Requirements

### Requirement: Live activity card
**Reason**: The fixed-height card with the latest narration line and three latest actions is replaced by the turn's work drawn into the conversation and a status line.
**Migration**: The `minimize_activity` preference now collapses the live work to the status line; no stored data changes.

## MODIFIED Requirements

### Requirement: Turn history sheet
Activating the status line or a finished turn's summary SHALL open a sheet over the right side of the window, without resizing the conversation. The sheet SHALL show that turn's full history in order, with each narration paragraph followed by the steps that came after it. Each step SHALL show its kind, title and state, and SHALL expand to show its bounded detail. While the turn is running, the sheet SHALL update live. It SHALL keep the reader's scroll position when they have scrolled up, and follow new steps otherwise. Escape or clicking outside the sheet SHALL close it, and the composer draft SHALL be kept.

#### Scenario: Inspect a running turn
- **WHEN** the user clicks the status line while the agent is running tests
- **THEN** the sheet opens with the turn's history so far and shows new steps as they arrive

#### Scenario: Expand command output
- **WHEN** the user expands a finished command step
- **THEN** its output tail is shown, with a note if earlier output was omitted

#### Scenario: Reading while streaming
- **WHEN** the user has scrolled up in the sheet and new steps arrive
- **THEN** the scroll position is kept

### Requirement: Finished turn summary
When a turn ends, its live work and status line SHALL be replaced by a one-line summary placed directly above the turn's reply. The summary SHALL show the duration and step count, and failed or interrupted steps if there were any. Earlier turns SHALL show the same summary after a restart, using stored steps.

#### Scenario: Review a past turn
- **WHEN** the user clicks "Worked 9m 12s · 23 steps" on an earlier reply
- **THEN** the turn history sheet opens for that turn

#### Scenario: Turn with no steps
- **WHEN** a turn's only step is the narration that became its reply
- **THEN** no summary line is shown
