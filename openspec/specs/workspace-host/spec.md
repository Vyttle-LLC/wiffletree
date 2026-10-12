# workspace-host Specification

## Purpose
Provide durable, provider-independent project, ticket and agent ownership and message routing in a headless Rust host.

## Requirements

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository.

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

### Requirement: Durable ordered messages
The host SHALL persist bounded messages before acknowledging queue acceptance, deduplicate stable IDs, reject changed payloads under an existing ID, preserve recipient ordering, paginate transcripts and distinguish queued, delivered, acknowledged and completed receipts.

#### Scenario: Restart and duplicate send
- **WHEN** the host restarts after accepting a message and that message is submitted again
- **THEN** exactly one local record remains and its receipt remains observable

### Requirement: Recovery and attention
The host SHALL retain pending attention bound to the originating operation and allow one terminal answer. Worker blocked state SHALL remain independent from attention. Working sessions SHALL recover as disconnected, with ambiguous accepted work held for reconciliation.

#### Scenario: Replayed approval response
- **WHEN** a resolved request receives another answer after restart
- **THEN** the stale answer is rejected and the original resolution is preserved

### Requirement: Portable local command boundary
The host SHALL expose versioned bounded commands over JSON-lines and build without GPUI. Database ownership SHALL be exclusive per store.

#### Scenario: Competing host
- **WHEN** another process attempts to open an already owned store
- **THEN** it fails visibly instead of becoming a competing writer

### Requirement: Assignment tools take an exact model and reason
The `assign_ticket` MCP tool SHALL require a `profile` (provider, model, effort) and a one-line `reason` in place of `provider`, `size` and `complexity`. Its other arguments SHALL be unchanged. Calls that still send `size`, `complexity` or `provider` SHALL fail with a message naming the replacement. A call for an agent that already exists SHALL return it unchanged, without checking or switching its model.

#### Scenario: Old size argument
- **WHEN** the coordinator calls `assign_ticket` with `size: "big"`
- **THEN** the call fails, saying to choose an exact profile within the role's providers, and no agent is created

#### Scenario: Existing assignment is not switched
- **WHEN** the coordinator repeats `assign_ticket` for an existing role and focus with a different profile
- **THEN** the existing agent is returned with its original profile

### Requirement: The coordinator receives the model configuration
`workspace_context` SHALL give the project coordinator the enabled providers with their allowed models, the providers per role, the configured verifiers and the guide, read fresh on each call. Workers SHALL NOT receive them.

#### Scenario: Configuration edit reaches a running coordinator
- **WHEN** the human adds Codex to the Implementer role while the project coordinator is idle
- **THEN** its next `workspace_context` call shows Implementer using Claude and Codex, with the new revision

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

### Requirement: Check-ins keep parents informed
The host SHALL tell a session's parent about long or repetitive work, and SHALL never pause or stop work for it. Each check-in SHALL be a durable message from the session to its parent whose body starts with `[check-in]`, names the session, its ticket if any, and recent activity, and says how to stop it with `stop_agents`. A check-in SHALL wake the parent, also while the session's ticket is being verified, and SHALL NOT count as a report or a verdict.
- **Time:** when a turn has run 30 minutes, and every 30 minutes after while it runs, the parent SHALL receive a check-in naming the elapsed time, the last provider event and the latest step.
- **Turn count:** when a session with a parent starts its Nth turn since its parent last sent it a message, where N is the `checkin_child_turns` setting, and at every further multiple of N, the parent SHALL receive a check-in naming the turn count, when the parent last messaged it, and the start of the session's last reply.
- **Human:** a session without a parent, the project coordinator, SHALL raise a human inbox item instead of a message: for its own 30-minute turns, cleared when a newer one is raised or the turn ends; and when it starts its Mth turn since the human last stepped in anywhere in the project, where M is the `checkin_human_turns` setting, and at every further multiple of M, cleared when the human next steps in or a newer one is raised. The human steps in by sending any session of the project a message, answering or resolving any of its inbox items, including permission requests, or retrying any of its sessions' input.
A host restart SHALL clear open time check-ins, and the human's turn count SHALL start again from zero.

#### Scenario: Thirty-minute check-in
- **WHEN** a tester's turn has run 30 minutes
- **THEN** its parent receives a `[check-in]` message naming the tester, its ticket, "30 minutes" and its latest step, and the turn keeps running

#### Scenario: Twenty-five turns without the parent
- **WHEN** `checkin_child_turns` is 25 and an implementer starts its 25th turn since its parent last messaged it
- **THEN** its parent receives one `[check-in]` naming the implementer, its ticket and "25 turns", and the turn runs
- **AND** the 26th turn sends no check-in, and the 50th sends another

#### Scenario: Parent message resets the count
- **WHEN** `checkin_child_turns` is 25, an implementer has run 24 turns, its parent sends it a message, and it runs 24 more
- **THEN** its parent has received no turn-count check-in

#### Scenario: Coordinator runs 100 turns without the human
- **WHEN** `checkin_human_turns` is 100 and the project coordinator starts its 100th turn since the human last stepped in
- **THEN** the human's inbox gains one item naming the coordinator and the turn count, the project stays live and the turn runs
- **AND** the item stays open after that turn ends, and the human's next message to the coordinator clears it

#### Scenario: Turn-count check-in during a round
- **WHEN** `checkin_child_turns` is 25 and a verifier starts its 25th turn without a message from its parent while its ticket's cycle is running
- **THEN** the project coordinator wakes with the check-in, and the ticket's state and verification record are unchanged

### Requirement: Check-in thresholds are host settings
The host's `settings.json` SHALL hold `checkin_child_turns` and `checkin_human_turns` beside `verification`. Settings saved without them SHALL use 25 and 100. A `set_check_ins` command SHALL save both; it SHALL refuse a child threshold outside 5–500 or a human threshold outside 10–1000, keeping the saved values, and SHALL keep other settings and unknown fields. A saved change SHALL apply from the next turn that starts, without a restart.

#### Scenario: Settings saved before this change
- **WHEN** `settings.json` has a workspace folder and verification but no check-in thresholds
- **THEN** the host reads 25 and 100, and the verification setting is unchanged

#### Scenario: Lower child threshold
- **WHEN** the human saves `checkin_child_turns` 10 and an implementer then starts its 10th turn since its parent last messaged it
- **THEN** its parent receives a turn-count check-in naming "10 turns"

#### Scenario: Out of range
- **WHEN** `set_check_ins` is called with a child threshold of 4 or a human threshold of 1001
- **THEN** it is refused and the saved thresholds stay in effect

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

### Requirement: A report during a running turn does not move the ticket yet
A ticket whose state a report set outside a verification cycle (`blocked`, `failed`, `completed`, `passed` or `ready_for_testing`) SHALL keep showing the state it had before that report, in the snapshot, in `workspace_context` and on its progress bar, while the reporter's turn is running. When that turn ends, the ticket SHALL show the state of the turn's last such report. The reported state SHALL be stored at once and is what ticket operations such as `verify_ticket` check; if an operation or another report moves the ticket before the turn ends, the ticket SHALL show that new state. Reports routed by a running verification cycle are unaffected. After a host restart no turn counts as running, so the stored state shows.

#### Scenario: Blocked report mid-turn on a passed ticket
- **WHEN** an implementer reports `blocked` on a ticket that passed verification and keeps working in the same turn
- **THEN** the snapshot and `workspace_context` show the ticket as `passed` and its bar is not blocked
- **AND** when the turn ends they show it as `blocked`

#### Scenario: A later report in the same turn
- **WHEN** the implementer then reports `completed` before the turn ends
- **THEN** the ticket shows `completed` once the turn ends, never `blocked`

#### Scenario: Restart
- **WHEN** the host restarts after a mid-turn `blocked` report
- **THEN** the ticket shows `blocked`

### Requirement: A message to a session in a turn is queued behind it
The `send_message` tool result SHALL read `queued_behind_turn` and carry `recipient_turn_started_at` whenever the recipient's turn is running, whatever status the recipient reported during it. A paused or disconnected recipient SHALL still read `queued_while_held`.

#### Scenario: Recipient reported blocked mid-turn
- **WHEN** a worker reports `blocked`, keeps working, and its coordinator sends it a message
- **THEN** the result is `queued_behind_turn` with the time the worker's turn started

### Requirement: Questions waiting are counted
The host SHALL count, for each session, the `blocked` reports it sent to its parent that have not been answered, and SHALL give that count in the snapshot and as `questions_waiting` on each `team` entry of `workspace_context`. A report SHALL count as answered once a message its parent sent after it has been delivered to the session by the start of a turn; an answer that is only queued SHALL leave it waiting. The host's own report that a session's turn is held SHALL NOT count. The count SHALL be read from stored messages, SHALL be the same after a host restart, and SHALL NOT change the session's status or its ticket's state.

#### Scenario: Blocked report mid-turn
- **WHEN** an implementer reports `blocked` and keeps working in the same turn
- **THEN** the snapshot and `workspace_context` show it as working with one question waiting
- **AND** when the turn ends unanswered they show it as blocked with one question waiting

#### Scenario: Answer queued, then delivered
- **WHEN** the coordinator answers a blocked implementer and the implementer's next turn has not started
- **THEN** the question still counts as waiting
- **AND** once that turn starts with the answer, it no longer counts

#### Scenario: Two reports
- **WHEN** an implementer has sent two `blocked` reports and its coordinator has answered neither
- **THEN** the snapshot and `workspace_context` both count two, also after a host restart

#### Scenario: Held turn
- **WHEN** a reviewer's turn is held because its provider is disabled, and the host reports that to the coordinator
- **THEN** the reviewer has no question waiting

#### Scenario: Answer sent between two reports
- **WHEN** an implementer reports `blocked`, its coordinator answers, and it reports `blocked` again before a turn takes the answer
- **THEN** the turn that takes the answer leaves one question waiting

### Requirement: A quit stops running turns
The host SHALL offer a shutdown that cancels every running turn and returns only once each of those turns' provider process groups has been stopped, as at any turn's end, or a bounded wait has passed. The desktop app SHALL call it before it exits from a quit, and the headless host SHALL call it when its input closes. A run SHALL be recorded as finished only after its process group has been stopped; a turn still running when the wait ends SHALL stay unfinished, so the next start handles its process.

#### Scenario: Quit during a turn
- **WHEN** the host is shut down while a provider is mid-turn
- **THEN** the provider's process group no longer runs when the shutdown returns
- **AND** the run is finished with the quit as its error and its input is held for a retry

#### Scenario: Quit with no turn running
- **WHEN** the host is shut down while no turn runs
- **THEN** the shutdown returns without waiting

### Requirement: Startup stops provider processes the last host left running
Each run SHALL record its provider's process group and that group leader's start time. At startup, for every run the last host left unfinished whose process group still runs, the host SHALL stop the group the way a turn's end does, SIGTERM, a grace period and then SIGKILL, and SHALL then mark the run interrupted and record when the process was stopped. The host SHALL send each signal only while the group's leader has the recorded process id and the recorded start time. When it cannot confirm that, because no start time was recorded, the start time differs or the leader has exited and other members remain, it SHALL signal nothing, SHALL still mark the run interrupted, and SHALL raise one inbox note on the session that names the session and the process id.

#### Scenario: Leftover provider
- **WHEN** the host starts and an unfinished run's provider process is still running with its recorded start time
- **THEN** the process group is stopped before any turn starts
- **AND** the run is interrupted and records when its process was stopped

#### Scenario: Recycled process id
- **WHEN** the host starts and an unfinished run's process id now belongs to a process with a different start time
- **THEN** that process receives no signal
- **AND** one inbox note names the session and the process id

#### Scenario: Run recorded without a start time
- **WHEN** the host starts and an unfinished run recorded before start times were kept has a process group that still runs
- **THEN** the group receives no signal and one inbox note names the session and the process id

#### Scenario: Process already gone
- **WHEN** the host starts and an unfinished run's process group no longer runs
- **THEN** the run is interrupted, nothing is signalled and no note is raised

### Requirement: No turn starts beside a leftover process
While a session's leftover-process note is open and the process group it names still runs, the host SHALL start no turn for that session. It SHALL hold the turn as other held turns are, with the reason naming the process id, and SHALL check again without waiting for another event. When the group no longer runs, the host SHALL settle the note itself and start the turn. When the human dismisses the note, the hold SHALL end.

#### Scenario: Retry while the process runs
- **WHEN** an interrupted session is retried while the process group its note names still runs
- **THEN** no turn starts and the session shows as blocked with a reason naming the process id

#### Scenario: The process exits
- **WHEN** that process group exits
- **THEN** the host settles the note and starts the session's turn

#### Scenario: The human dismisses the note
- **WHEN** the human dismisses the note because the process is not the agent's
- **THEN** the session's turn starts

### Requirement: The retry note names a process stopped at startup
When startup stopped an interrupted run's leftover process, the note an agent gets for that run, on a retry of its input or on its first turn after the restart, SHALL say that the process kept running after the host stopped and when it was stopped, so the agent checks the worktree for work done after the turn's last recorded step.

#### Scenario: Retry after a leftover was stopped
- **WHEN** an agent retries the input of a run whose leftover process startup stopped
- **THEN** its prompt names the run and says its process was stopped at startup, with the time

#### Scenario: Retry with no leftover
- **WHEN** an agent retries the input of an interrupted run whose process had already exited
- **THEN** its prompt names the run and says nothing about a stopped process
