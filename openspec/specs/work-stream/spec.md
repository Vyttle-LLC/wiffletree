# work-stream Specification

## Purpose

Lets the user watch an agent's in-progress work (narration, thinking, commands, file edits, tool calls and plans) as it happens, while the conversation keeps only durable messages and final replies.

## Requirements

### Requirement: Provider output becomes work steps
The host SHALL turn Claude and Codex streamed output into provider-neutral work steps. Each step SHALL have a stable identity within its run, a kind (narration, thinking, command, file edit, tool call, search or plan), a short title, an optional bounded detail and a state: running, succeeded, failed or interrupted. A step that the provider reports in several events SHALL update one step rather than create duplicates. Provider events the host cannot classify SHALL NOT fail the turn.

#### Scenario: Claude tool call
- **WHEN** a Claude turn emits a tool use followed by its tool result
- **THEN** one tool-call or command step appears as running, then becomes succeeded, or failed when the result is an error

#### Scenario: Codex command
- **WHEN** a Codex turn reports a command execution as started, updated and completed with a nonzero exit code
- **THEN** one command step shows the command, its output tail while running, and ends as failed

#### Scenario: Unknown event
- **WHEN** a provider emits an event type the host does not recognize
- **THEN** the turn continues and no step is created for that event

### Requirement: Conversation receives only the final reply
A turn's chat reply SHALL contain only the turn's final assistant text. Interim narration SHALL appear only as work steps. The reply SHALL be written when the turn ends. Reports, direct messages and errors SHALL continue to appear in the conversation unchanged. The parent notification for a finished worker turn SHALL use the same final reply.

#### Scenario: Narration between tool calls
- **WHEN** an agent writes "Let me check the tests", runs a command and then writes a summary
- **THEN** the conversation shows only the summary, and the Work stream shows the narration and the command

#### Scenario: Turn fails before a final reply
- **WHEN** a turn fails after emitting narration but no final text
- **THEN** the conversation shows the runtime error, and the narration remains available as work steps

### Requirement: Live delivery without waking agents
Step changes SHALL reach the desktop app incrementally: the client requests only steps changed since its last cursor, never a full workspace snapshot. Display updates SHALL be coalesced to at most about 30 per second per session. Step changes SHALL NOT wake, notify or message any agent, and SHALL NOT delay message receipts, permission requests or failures.

#### Scenario: Fast narration
- **WHEN** a provider streams hundreds of text deltas per second
- **THEN** the visible narration updates at no more than about 30 frames per second and no snapshot reload occurs per delta

#### Scenario: Coordinator stays asleep
- **WHEN** a worker emits steps during its turn
- **THEN** its coordinator is not woken and receives no message until the worker reports or finishes

### Requirement: Steps are durable and bounded
The host SHALL persist each step when it starts and when it changes state, so a restart preserves the run's step history. Intermediate text deltas need not be persisted. Steps still running when the host stops SHALL be shown as interrupted after restart. Titles, details and the number of stored steps per run SHALL be bounded. When a bound is reached, the UI SHALL state how much was omitted instead of truncating silently. Steps older than the retention period SHALL be pruned.

#### Scenario: Host restarts mid-command
- **WHEN** the host stops while a command step is running and is then restarted
- **THEN** that run's steps are visible and the command step is marked interrupted

#### Scenario: Long command output
- **WHEN** a command produces more output than the detail bound
- **THEN** the step keeps the most recent output and indicates that earlier output was omitted

### Requirement: Live activity card
While the selected session is working, the conversation SHALL show one activity card after the last message. The card's headline SHALL be the latest narration line, which replaces the previous one as new narration arrives. Beneath it, the card SHALL show the three most recent non-narration steps with their state, newest last. It SHALL also show the elapsed time and the step count. The card SHALL keep a fixed height in each state, so that updates do not move the transcript. The user SHALL be able to minimize the card to a single line that shows the elapsed time, the step count and the current step, and to restore it. The choice SHALL apply to later turns and sessions, and SHALL persist across app launches. The elapsed-time refresh SHALL run only while a visible session is working.

#### Scenario: Agent narrates and runs a command
- **WHEN** the selected agent writes "Now checking which tests fail" and then starts `pytest`
- **THEN** the card's headline shows that narration, and `pytest` appears as the newest running step

#### Scenario: Steady layout
- **WHEN** steps arrive while the user reads earlier messages
- **THEN** the transcript's scroll position and the card's height do not change

#### Scenario: Minimize and restore
- **WHEN** the user minimizes the card, selects another working agent, and then restores the card
- **THEN** the other agent's card also appears minimized and still updates its current step, and restoring shows the narration headline and recent steps again

#### Scenario: Open history from the minimized line
- **WHEN** the user activates the minimized line
- **THEN** the turn history sheet opens

#### Scenario: Idle app
- **WHEN** no visible session is working
- **THEN** no activity timer runs

### Requirement: Turn history sheet
Activating the activity card or a finished turn's summary SHALL open a sheet over the right side of the window, without resizing the conversation. The sheet SHALL show that turn's full history in order, with each narration paragraph followed by the steps that came after it. Each step SHALL show its kind, title and state, and SHALL expand to show its bounded detail. While the turn is running, the sheet SHALL update live. It SHALL keep the reader's scroll position when they have scrolled up, and follow new steps otherwise. Escape or clicking outside the sheet SHALL close it, and the composer draft SHALL be kept.

#### Scenario: Inspect a running turn
- **WHEN** the user clicks the activity card while the agent is running tests
- **THEN** the sheet opens with the turn's history so far and shows new steps as they arrive

#### Scenario: Expand command output
- **WHEN** the user expands a finished command step
- **THEN** its output tail is shown, with a note if earlier output was omitted

#### Scenario: Reading while streaming
- **WHEN** the user has scrolled up in the sheet and new steps arrive
- **THEN** the scroll position is kept

### Requirement: Finished turn summary
When a turn ends, its activity card SHALL be replaced by a one-line summary placed directly above the turn's reply. The summary SHALL show the duration and step count, and failed or interrupted steps if there were any. Earlier turns SHALL show the same summary after a restart, using stored steps.

#### Scenario: Review a past turn
- **WHEN** the user clicks "Worked 9m 12s · 23 steps" on an earlier reply
- **THEN** the turn history sheet opens for that turn

#### Scenario: Turn with no steps
- **WHEN** a turn's only step is the narration that became its reply
- **THEN** no summary line is shown
