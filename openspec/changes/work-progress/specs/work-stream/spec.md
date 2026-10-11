## MODIFIED Requirements

### Requirement: Provider output becomes work steps
The host SHALL turn Claude and Codex streamed output into provider-neutral work steps. Each step SHALL have a stable identity within its run, a kind (narration, thinking, command, file edit, tool call, search or plan), a short title, an optional bounded detail and a state: running, succeeded, failed or interrupted. A plan step SHALL also carry its counts as data: the number of items done, the total number of items, and the text of the first unfinished item, bounded like a title. A step that the provider reports in several events SHALL update one step rather than create duplicates. Provider events the host cannot classify SHALL NOT fail the turn.

#### Scenario: Claude tool call
- **WHEN** a Claude turn emits a tool use followed by its tool result
- **THEN** one tool-call or command step appears as running, then becomes succeeded, or failed when the result is an error

#### Scenario: Codex command
- **WHEN** a Codex turn reports a command execution as started, updated and completed with a nonzero exit code
- **THEN** one command step shows the command, its output tail while running, and ends as failed

#### Scenario: Unknown event
- **WHEN** a provider emits an event type the host does not recognize
- **THEN** the turn continues and no step is created for that event

#### Scenario: Plan counts
- **WHEN** a Claude `TodoWrite` or a Codex `todo_list` lists three items, the first completed and the second "Add a median-of-15 harness"
- **THEN** the plan step is titled "Plan · 1 of 3 done" and carries 1 done, 3 total and the current item "Add a median-of-15 harness"

#### Scenario: Plan step stored before this change
- **WHEN** a stored plan step has no counts
- **THEN** it still loads and is shown in the turn history, and it counts as no plan for progress
