## ADDED Requirements

### Requirement: Host messages come from Wiffletree
Messages the host sends without a sender, such as a verification round's or cycle's result, a kept-worktree notice, a migration notice or a PR watcher message, SHALL be labelled as coming from Wiffletree: the recipient's turn input SHALL name the sender "Wiffletree", or "Wiffletree PR watcher" for the PR watcher, and the conversation view SHALL show them as Wiffletree notices. Messages the human sends, including answers to inbox items, SHALL keep the human label.

#### Scenario: Verification result
- **WHEN** a verification cycle ends and the coordinator's next turn includes its result
- **THEN** that message's sender reads "Wiffletree", not "human", and the chat shows it as a Wiffletree notice

#### Scenario: The human's message
- **WHEN** the human sends the coordinator a message
- **THEN** its sender still reads "human" and the chat shows it as the human's

### Requirement: A running turn shows as working
A session whose turn is running SHALL show the status `working` in the snapshot and in `workspace_context`, even after it reported `blocked`, `failed`, `ready_for_testing` or `completed` during that turn. The reported status SHALL still be stored and SHALL show once the turn ends. After a host restart, no turn SHALL show as running until one starts.

#### Scenario: Blocked report mid-turn
- **WHEN** an implementer reports `blocked` and keeps working in the same turn
- **THEN** the sidebar and `workspace_context` show it as working, and `workspace_context` lists its `blocked` report
- **AND** when the turn ends it shows as blocked

## MODIFIED Requirements

### Requirement: No turn limits
The host SHALL NOT limit how many turns run at once, in a project or across the host, and SHALL NOT pause a project or stop a turn because of how many turns it has run. A session with due input SHALL start a turn unless it is paused, disconnected, held, already in a turn, waiting for its ticket's worktree, or a verifier whose round has not been admitted. Sessions that cannot start SHALL NOT keep the scheduler from reaching those that can, however many messages they have queued. A stored project that still records a turn limit SHALL load, and the value SHALL be ignored and not reported.

#### Scenario: Many workers at once
- **WHEN** nine workers across two projects each have due input and nothing holds them
- **THEN** all nine turns start in the same scheduling pass

#### Scenario: Long-running project keeps going
- **WHEN** a project's sessions have run 150 turns since the human last stepped in
- **THEN** the project stays live and its next due turn starts

#### Scenario: Old store
- **WHEN** a stored project's data contains `"turn_limit": 4`
- **THEN** the project loads, and neither `workspace_context` nor the snapshot reports a turn limit

#### Scenario: Paused coordinator with a long queue
- **WHEN** the project coordinator is paused with 101 queued messages and a worker in the same project has due input
- **THEN** the scheduler reaches the worker in the same pass
