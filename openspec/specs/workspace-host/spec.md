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
`workspace_context` SHALL give the project coordinator the enabled providers with their allowed models, the providers per role, the configured verifiers, the review levels, each with its name, its kind, the focuses of its verifiers and its note on when to use it, and the guide, read fresh on each call. Workers SHALL NOT receive them.

#### Scenario: Configuration edit reaches a running coordinator
- **WHEN** the human adds Codex to the Implementer role while the project coordinator is idle
- **THEN** its next `workspace_context` call shows Implementer using Claude and Codex, with the new revision

#### Scenario: Review levels reach the coordinator
- **WHEN** the levels are "Full", a full review that runs Tests, Correctness and Regressions, and "Light", a re-check that runs Correctness, and the project coordinator calls `workspace_context`
- **THEN** `model_selection.review_levels` lists both in that order, each with its name, kind, verifiers and note

#### Scenario: A level edit reaches a running coordinator
- **WHEN** the human adds the level "Docs" while the project coordinator is idle
- **THEN** its next `workspace_context` call lists "Docs"

#### Scenario: Workers do not receive the levels
- **WHEN** an implementer calls `workspace_context`
- **THEN** the result has no `model_selection`

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
The host SHALL count, for each session, the `blocked` reports it sent to its parent that have not been answered, and SHALL give that count in the snapshot and as `questions_waiting` on each live (not archived) agent's `team` entry of `workspace_context`. A report SHALL count as answered once a message its parent sent after it has been delivered to the session by the start of a turn; an answer that is only queued SHALL leave it waiting. The host's own report that a session's turn is held SHALL NOT count. The count SHALL be read from stored messages, SHALL be the same after a host restart, and SHALL NOT change the session's status or its ticket's state.

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
The host SHALL offer a shutdown that cancels every running turn, looks afresh at what each running turn has started, stops every process group recorded for any agent under the confirmation rule, and returns only once each of those turns' provider process groups has been stopped, as at any turn's end, or a bounded wait has passed. The desktop app SHALL call it before it exits from a quit, and the headless host SHALL call it when its input closes. A run SHALL be recorded as finished only after its process group has been stopped; a turn still running when the wait ends SHALL stay unfinished, and a group still running then SHALL stay recorded as running, so the next start handles both. Each group the shutdown stops SHALL be recorded with the reason `quit`.

#### Scenario: Quit during a turn
- **WHEN** the host is shut down while a provider is mid-turn
- **THEN** the provider's process group no longer runs when the shutdown returns
- **AND** the run is finished with the quit as its error and its input is held for a retry

#### Scenario: Quit with no turn running
- **WHEN** the host is shut down while no turn runs and no agent has a process recorded
- **THEN** the shutdown returns without waiting

#### Scenario: Quit while an agent's process runs
- **WHEN** the host is shut down while an agent between turns has a process it started still running
- **THEN** that process no longer runs when the shutdown returns
- **AND** its group is recorded as stopped with the reason `quit`

### Requirement: No turn starts beside a leftover process
While the provider process group of one of a session's finished runs is still recorded as running and still runs, the host SHALL start no turn for that session. It SHALL hold the turn as other held turns are, with the reason naming the process id, and SHALL check again without waiting for another event. When the group no longer runs, the host SHALL settle its note itself and start the turn. The host SHALL settle a leftover-process note once it sees its group gone whether or not the session has a turn due, and SHALL look again without waiting for another event while a note is open, so no note keeps saying a process runs after it exited. When the human dismisses the note, the hold SHALL end and the group SHALL no longer be recorded as running. A run an earlier version finished without its process having been seen to end, marked interrupted or finished by a quit, SHALL be carried into the record when the store is opened, so it holds its session the same way; once its group no longer runs it SHALL be dropped and never looked at again, since process ids are reused. Only a provider's own process group SHALL hold a session: a process group the agent started with its tools SHALL never hold the agent's turns, whether or not the host could confirm it.

#### Scenario: Retry while the process runs
- **WHEN** an interrupted session is retried while the provider process group recorded for its interrupted run still runs
- **THEN** no turn starts and the session shows as blocked with a reason naming the process id

#### Scenario: The process exits
- **WHEN** that process group exits
- **THEN** the host settles the note and starts the session's turn

#### Scenario: Run an earlier version marked interrupted
- **WHEN** a session has a finished, interrupted run with no note whose process group still runs, and input is queued for it
- **THEN** no turn starts, one inbox note names the session and the process id, and the turn starts once the group exits

#### Scenario: Process id reused later
- **WHEN** a run's process id is given to another process after its group was dropped from the record
- **THEN** the session is not held and no note is raised

#### Scenario: The human dismisses the note
- **WHEN** the human dismisses the note because the process is not the agent's
- **THEN** the session's turn starts

#### Scenario: The process exits while no turn is due
- **WHEN** the process group a note names exits while its session is disconnected with its input held
- **THEN** the host settles the note without the session being retried
#### Scenario: The agent's own process keeps running
- **WHEN** an agent's turn left a process running and a message arrives for the agent
- **THEN** its next turn starts while that process runs

### Requirement: The retry note names a process stopped at startup
When startup stopped an interrupted run's leftover process, the note an agent gets for that run, on a retry of its input or on its first turn after the restart, SHALL say that the process kept running after the host stopped and when it was stopped, so the agent checks the worktree for work done after the turn's last recorded step.

#### Scenario: Retry after a leftover was stopped
- **WHEN** an agent retries the input of a run whose leftover process startup stopped
- **THEN** its prompt names the run and says its process was stopped at startup, with the time

#### Scenario: Retry with no leftover
- **WHEN** an agent retries the input of an interrupted run whose process had already exited
- **THEN** its prompt names the run and says nothing about a stopped process

### Requirement: A queued compaction runs before waiting input
A session's queued compaction SHALL start as soon as a turn for that session could start: the session is not in a turn, not paused, not disconnected and not archived, it has no held input, its project is live, its provider is enabled, and no leftover provider process holds it. It SHALL NOT need due input, and the ticket's worktree and verification round SHALL NOT delay it. While a session has a request, queued or running, the host SHALL NOT start a turn for it by any path, and its waiting messages SHALL stay queued; the first turn after the compaction settles SHALL take them. A verification round with such a verifier SHALL wait whole, as it does for a paused verifier, and SHALL be admitted once the compaction has settled; a verifier whose turn is held anyway SHALL be held in its round as it is without a request. A request that cannot start SHALL stay queued and SHALL remain cancellable. A request SHALL NOT change how a turn that cannot start is held: a session whose turn is held, such as one whose provider is disabled, SHALL be blocked and reported to its parent as it is without a request. A queued request SHALL survive a host restart unchanged.

#### Scenario: Idle agent
- **WHEN** a person asks to compact an idle Claude agent with a provider session
- **THEN** the compaction starts without waiting for a message

#### Scenario: Requested mid-turn with a message waiting
- **WHEN** a person asks to compact an agent while its turn is running, and its coordinator then sends it a message
- **THEN** when the turn ends the compaction runs first
- **AND** the coordinator's message is delivered by the turn that starts after the compaction settles

#### Scenario: Verifier with a round waiting
- **WHEN** a paused Claude verifier has a queued request and its round-2 message waiting, and it is resumed
- **THEN** the compaction runs first and no verifier of that round starts a turn while it runs
- **AND** the round is admitted after the compaction settles, and the verifier's turn takes the round message

#### Scenario: The turn fails
- **WHEN** the turn a request waits on ends with an error and the session becomes interrupted
- **THEN** the request stays queued and no compaction runs
- **AND** when the session is resumed with its held input retried, the compaction runs before that turn

#### Scenario: Paused agent
- **WHEN** a person asks to compact an agent that is paused
- **THEN** the request is stored as queued and runs when the agent is resumed

#### Scenario: Disabled provider with a request and a message waiting
- **WHEN** an agent whose provider is disabled has a queued request and a message waiting
- **THEN** no compaction and no turn starts, the agent is blocked with the reason, and its parent is told once
- **AND** when the provider is enabled again the compaction runs first and the turn takes the message

#### Scenario: Restart while queued
- **WHEN** the host restarts while an agent has a queued request
- **THEN** the request is still queued with its instructions, and nothing ran during startup

### Requirement: A compaction is not a turn
The host SHALL run a Claude agent's compaction by resuming its Claude session with a prompt that is exactly `/compact`, or `/compact` followed by a space and the instructions, without the wrapper every turn's messages get and without attachments. It SHALL run a Codex agent's compaction through Codex's app-server, with no prompt at all. For either provider the run SHALL deliver no message and change no message's receipt, SHALL add no reply to the conversation, SHALL send the session's parent no message, SHALL NOT count toward a turn-count check-in for a parent or the human, and SHALL raise no time check-in. It SHALL NOT count as the session's latest turn: what the next turn is told about a turn its parent stopped or a restart interrupted, and the steps and plan shown for the session's latest turn, SHALL be the same as if no compaction had run. The session's stored status SHALL be the same after the compaction as before it. A compaction that has started SHALL NOT be interrupted by a pause or a stop of the session or its project; the pause or stop SHALL still take effect for turns.

#### Scenario: Bare command
- **WHEN** a compaction runs with the instructions "the failing test names"
- **THEN** Claude's input is exactly `/compact the failing test names`

#### Scenario: Parent stays asleep
- **WHEN** a worker's compaction succeeds
- **THEN** its parent receives no message and is not woken

#### Scenario: Check-in count
- **WHEN** `checkin_child_turns` is 25 and an implementer has run 24 turns and one compaction since its parent last messaged it
- **THEN** its parent has received no turn-count check-in, and the next turn is counted as the 25th

#### Scenario: Stopped by its parent, then compacted
- **WHEN** a request is queued mid-turn, the agent's parent stops that turn and later resumes the agent, and the compaction runs before the next turn
- **THEN** that turn's prompt still says the parent stopped the previous turn, with its reason

#### Scenario: Blocked agent
- **WHEN** an agent whose status is blocked is compacted
- **THEN** its status is blocked afterwards and its questions waiting are unchanged

#### Scenario: Paused while compacting
- **WHEN** a person pauses an agent whose compaction is running and which has a message waiting
- **THEN** the compaction finishes, the agent is paused, and the message stays queued

#### Scenario: Codex agent's parent stays asleep
- **WHEN** a Codex worker's compaction succeeds
- **THEN** its parent receives no message, no reply is added to its conversation, and the run counts toward no check-in

### Requirement: Compaction outcome comes from the provider's status record
The host SHALL decide a Claude agent's compaction's outcome from the status record Claude streams, not from the process's exit code or the result record's error flag. A `compact_result` of `success` SHALL be **compacted**, with the token counts before and after taken from the compaction boundary record when it is present. A `compact_result` of `failed` whose error starts with "Not enough messages to compact" SHALL be **nothing to compact**. Every other ending, including a missing status record, a nonzero exit and a process that could not start, SHALL be **failed**, with the provider's own reason when it gave one. When a process ended without a status record, the reason SHALL be what the run ended with held to one line of at most 200 characters, ended with "…" when it was cut, and the run SHALL keep the whole of it. A Codex agent's compaction SHALL be decided from its turn's completion notification, as its own requirement says. For either provider the host SHALL store the last outcome and its reason on the session's runtime, clear the request, and clear the stored outcome when a `DismissCompaction` command or a new request arrives. A compaction that is running when the host stops, by a shutdown or a crash, SHALL be settled as **interrupted**, at shutdown or at the next startup, through the handling every run gets. A shutdown SHALL cancel a running compaction and wait for its provider process group to be stopped as it does for a running turn, and SHALL record the run as finished only once the group is stopped; a compaction still running when the wait ends SHALL stay unfinished for the next start. At startup an unfinished compaction run SHALL be handled like any unfinished run, under the rule that confirms every recorded group before it is signalled: the host SHALL stop its process group only while a member it recorded for that group, matched by process id and start time, is still alive in it, whether or not that member is the group's leader, and SHALL otherwise signal nothing and raise the leftover-process note, which holds the session's turns and compactions until the group exits or the note is dismissed. An interrupted compaction SHALL leave the session's stored status and its waiting messages as they were, SHALL hold no input and SHALL need no resume. It SHALL NOT be run again by the host and SHALL NOT be reported as compacted or as unchanged, and the host SHALL NOT tell the session's next turn that a turn was interrupted.

#### Scenario: Success
- **WHEN** the stream has `"compact_result":"success"` and a boundary record with `pre_tokens` 812000 and `post_tokens` 5000
- **THEN** the outcome is compacted with those two figures and the request is cleared

#### Scenario: Nothing to compact
- **WHEN** the stream has `"compact_result":"failed"` with `"compact_error":"Not enough messages to compact."`, exit code 0 and `is_error` false
- **THEN** the outcome is nothing to compact, with that text as its reason

#### Scenario: Failure with exit code 0
- **WHEN** the stream has `"compact_result":"failed"` with another error and the process exits 0
- **THEN** the outcome is failed, with that error as its reason

#### Scenario: No status record
- **WHEN** the process exits without streaming a `compact_result`
- **THEN** the outcome is failed

#### Scenario: Quit while compacting
- **WHEN** the host shuts down while an agent's compaction is running and the agent has a message waiting
- **THEN** the compaction's process group no longer runs when the shutdown returns, the outcome is interrupted, the request is cleared, and the agent's status is what it was
- **AND** when the host starts again no compaction runs, and the agent's next turn takes the waiting message with a prompt that names no interrupted turn

#### Scenario: Crash while compacting
- **WHEN** the host starts with an unfinished compaction run whose provider process still runs with its recorded start time, and the agent has a message waiting
- **THEN** the process group is stopped, the compaction is settled as interrupted and is not run again, and the agent's status is what it was
- **AND** the agent's next turn takes the waiting message

#### Scenario: Crash while compacting, process not confirmed
- **WHEN** the host starts with an unfinished compaction run whose process id now belongs to a process with a different start time, and the agent has a message waiting
- **THEN** that process receives no signal, the compaction is settled as interrupted, and one inbox note names the agent and the process id
- **AND** no turn and no compaction starts for the agent while that process group runs, and its turn starts once the note is dismissed

### Requirement: Compaction usage is recorded in the ledger
The host SHALL record a Claude agent's compaction's tokens in the usage ledger from the result record's `modelUsage`, because that record's `usage` is zero for a compaction. This requirement SHALL apply to Claude agents only; a Codex agent's compaction is recorded from its session record, as its own requirement says. `modelUsage` counts the session's whole life, by model, so the host SHALL keep the latest reading from every Claude result record, a turn's included, with the run that took it, and SHALL record for a compaction only what each model's counts rose by since the reading before it: one request per model whose counts rose, attributed to the agent and its coordinator like any other request. A reading SHALL be that baseline only when the session's run immediately before the compaction took it. A run that was stopped, interrupted or ended without a result record reports no totals, while the provider still counts its requests and the ledger already holds them, so a compaction that follows such a run SHALL record no request. `modelUsage` follows Claude's convention, in which `inputTokens` excludes tokens read from or written to the cache, so the recorded input SHALL be their sum, as for a turn's request. When the session has no such baseline, or a count went down, the host SHALL record no request rather than count the session's earlier turns again; the compaction's own reading SHALL still become the latest. A reading and the requests recorded from it SHALL be stored together or not at all. The session totals the record reports, its cost included, SHALL be kept with the run as reported. A compaction with no `modelUsage` SHALL record no request. Usage reports SHALL count these requests in their existing totals and SHALL NOT report them separately. Turns SHALL keep recording usage as before, and a turn's reading SHALL add no request.

#### Scenario: Tokens from modelUsage
- **WHEN** a compaction's result record has zero `usage` and a `modelUsage` entry that exceeds the session's last reading by `inputTokens` 1,502, `cacheReadInputTokens` 811,900, `cacheCreationInputTokens` 0 and `outputTokens` 2,960
- **THEN** the ledger gains one request for that agent and model with input 813,402, cache read 811,900, cache write 0 and output 2,960
- **AND** the agent's and its coordinator's totals in the usage report rise by them

#### Scenario: Earlier turns are not counted again
- **WHEN** an agent has run three turns, whose requests are in the ledger, and is then compacted
- **THEN** the compaction's request holds only what `modelUsage` rose by since the third turn's result record

#### Scenario: No earlier reading
- **WHEN** a compaction's result record is the first reading the host has for the session
- **THEN** the ledger gains no request, and the reading is kept for the next one

#### Scenario: The run before it reported no totals
- **WHEN** an agent's turn is cut off after its requests were recorded, and a compaction runs before its next turn
- **THEN** the ledger gains no request for that compaction, so the cut-off turn is not counted twice
- **AND** a compaction that follows the agent's next finished turn records only its own tokens

#### Scenario: The request cannot be stored
- **WHEN** storing a compaction's request fails
- **THEN** the session's reading has not moved, and recording the same result record again stores both

#### Scenario: Nothing spent
- **WHEN** a compaction ends as nothing to compact with `modelUsage` empty or unchanged
- **THEN** the ledger gains no request

### Requirement: Projects have one manual order
The host SHALL store one flat order of projects that contains every project exactly once, and SHALL return projects in that order, topmost first, in every snapshot. A new project SHALL be placed at the top. The order SHALL be kept in the store, so it is the same after a restart and for every client. A store from before this order existed SHALL open with its projects newest first, as they were listed until then, and with nothing else changed.

#### Scenario: A new project lands at the top
- **WHEN** the order is "Calls", "Billing" and a project "Alerts" is created
- **THEN** a snapshot lists "Alerts", "Calls", "Billing"

#### Scenario: The order survives a restart
- **WHEN** projects are moved into the order "Billing", "Alerts", "Calls" and the host restarts
- **THEN** a snapshot lists "Billing", "Alerts", "Calls"

#### Scenario: An existing store opens newest first
- **WHEN** a store from before the order existed holds "Calls", then "Billing", then "Alerts", created in that sequence
- **THEN** it opens listing "Alerts", "Billing", "Calls", and every project, session, ticket and message is as it was

### Requirement: Moving projects in the order
The host SHALL offer one command that moves one or more projects to just before a named project, or to the end of the order when none is named. The moved projects SHALL end up next to each other in the relative order they had before the move, and every other project SHALL keep its relative order. The host SHALL refuse a move that lists no project, lists a project twice, names a project that does not exist, or names one of the moved projects as the place to move before; a refused move SHALL leave the order unchanged.

#### Scenario: One project moves up
- **WHEN** the order is A, B, C, D and D is moved before B
- **THEN** the order is A, D, B, C

#### Scenario: A block moves and keeps its own order
- **WHEN** the order is A, B, C, D, E and the projects D and B are moved before A
- **THEN** the order is B, D, A, C, E

#### Scenario: A move to the end
- **WHEN** the order is A, B, C and A is moved with no project to move before
- **THEN** the order is B, C, A

#### Scenario: An unknown project is refused
- **WHEN** a move lists a project that does not exist, or names one as the place to move before
- **THEN** the host refuses it and the order is unchanged

### Requirement: A person can ask to compact an agent's context
The host SHALL accept a `CompactContext` command naming a session and optional instructions on what to keep, and SHALL store it as that session's one compaction request, in state `queued`, in the session's persisted runtime. It SHALL refuse the command, with the reason and without storing anything, when the session has no provider session yet, when it is archived, when it already has a request, or when it names a Codex agent and carries instructions, because Codex's compact request has no field for them. A Codex agent's request SHALL be stored without instructions. A Claude agent's instructions SHALL be trimmed, SHALL have line breaks replaced by spaces, SHALL be absent when empty and SHALL be refused beyond 1,000 characters. A `CancelCompaction` command SHALL remove a `queued` request and SHALL be refused once the request is `running`. The workspace tools offered to agents SHALL NOT include any way to request, cancel or run a compaction.

#### Scenario: Codex agent
- **WHEN** `CompactContext` names a Codex agent that has a provider session, with no instructions
- **THEN** the agent has one queued request with no instructions

#### Scenario: Codex agent with instructions
- **WHEN** `CompactContext` names a Codex agent with the instructions "the failing test names"
- **THEN** it is refused, saying Codex chooses what the summary keeps, and the agent has no request

#### Scenario: No provider session yet
- **WHEN** `CompactContext` names a Claude or a Codex agent that has not run a turn
- **THEN** it is refused, saying it is available after the agent's first turn

#### Scenario: Second request
- **WHEN** `CompactContext` names an agent that already has a queued request
- **THEN** it is refused and the first request keeps its instructions

#### Scenario: Cancel before it starts
- **WHEN** `CancelCompaction` names an agent whose request is queued behind its running turn
- **THEN** the request is removed, and when the turn ends no compaction runs

#### Scenario: Cancel after it started
- **WHEN** `CancelCompaction` names an agent whose compaction is running
- **THEN** it is refused and the compaction continues

#### Scenario: No agent tool
- **WHEN** any managed session lists its workspace tools
- **THEN** none of them requests, cancels or runs a compaction

### Requirement: A Codex compaction runs through the app-server
The host SHALL run a Codex agent's compaction as one `codex app-server` process that it starts for that compaction and that is gone once the compaction's outcome is read. When the outcome is read the host SHALL close the process's input and SHALL give it a bounded time of one second to exit on its own, so that what it still writes to the thread's session record is written, and SHALL then stop its process group as it ends any run. The fixed limit on a compaction, a cancellation and a shutdown SHALL stop the group at once, with no such wait. The process SHALL lead its own process group and SHALL be recorded, cancelled at a shutdown and handled at startup like every provider run. It SHALL be started in the agent's directory with the configuration overrides a turn of that agent gets: its reasoning effort, its role instructions and the workspace's tool server, whose token SHALL NOT be one any turn holds. The host SHALL send, in order, `initialize`, the `initialized` notification, `thread/resume` and `thread/compact/start`. `thread/resume` SHALL name the agent's thread, its directory, the model of the agent's profile, the sandbox and approval policy a turn runs with, and SHALL ask for no turn history. `thread/compact/start` SHALL carry the thread id and nothing else. The host SHALL write nothing to the process after those four, and SHALL NOT send a second `thread/compact/start` to a process.

#### Scenario: The requests
- **WHEN** a Codex agent whose thread is `T` and whose profile names the model `gpt-6.1-sol` is compacted
- **THEN** the server receives `initialize`, `initialized`, a `thread/resume` for `T` with that model, the agent's directory, sandbox `danger-full-access`, approval policy `never` and `excludeTurns` true, and a `thread/compact/start` holding only `T`

#### Scenario: Resumed as a turn would be
- **WHEN** a Codex agent is compacted
- **THEN** the process's arguments carry the same reasoning effort, role instructions and workspace tool server as that agent's turn, and no prompt

#### Scenario: Stopped when the outcome is read
- **WHEN** the server reports that the compaction's turn completed and keeps running after its input closes
- **THEN** the host stops its process group once the bounded time has passed, and the run is finished with that group gone

#### Scenario: The record is finished after the input closes
- **WHEN** the server reports that the compaction's turn completed, writes the compaction's entries to the thread's session record only once its input has closed, and then exits
- **THEN** the host does not stop it first: the compaction's request is recorded from that record and the context total read from it is the one after the compaction

#### Scenario: Quit while a Codex agent is compacting
- **WHEN** the host shuts down while a Codex agent's compaction is running
- **THEN** the server's process group no longer runs when the shutdown returns, and the outcome is interrupted

### Requirement: A Codex compaction's outcome comes from its turn's completion
The host SHALL decide a Codex agent's compaction's outcome from what the app-server sends, never from the process's exit code. A `turn/completed` notification whose `turn.status` is `completed` SHALL be **compacted**. A `turn/completed` with the status `failed` SHALL be **failed** with `turn.error.message` as the reason, bounded as every reason is, and one with any other status, with none the host can read, or failed without a message, SHALL be **failed** with a reason naming that status. An error reply to `initialize`, `thread/resume` or `thread/compact/start` SHALL be **failed** with a reason naming the refused request and Codex's message; when several are refused the first SHALL be the reason. A request the server sends to the host SHALL NOT be answered and SHALL be **failed** with a reason naming what Codex asked for. A reason built from Codex's text SHALL be one line of at most 200 characters, the name of a refused request included, and SHALL end with "…" when it was cut. A refusal SHALL be given as a short reason that names the request and says this Codex has no such method only when Codex's message says the unknown variant is that request's own method; a refusal that opens the same way about anything else, such as a value Codex does not have for one of the request's parameters, SHALL keep Codex's words in the bounded form. Codex's full text SHALL be kept with the run. A process that ends, or cannot start, before an outcome is read SHALL be **failed** with what it ended with, held to one line of at most 200 characters as every reason is, the whole of it kept with the run. A compaction whose outcome has not been read within a fixed limit SHALL be stopped and SHALL be **failed** with a reason saying Codex did not finish in that time. A Codex compaction SHALL never end as nothing to compact. Notifications the host does not use SHALL be ignored. The token counts of a compacted outcome SHALL be the context total in the thread's session record as the compaction starts and the `tokenUsage.last.totalTokens` Codex reports during the compaction's turn, as Codex gave them; when either is missing the outcome SHALL carry no counts.

#### Scenario: Success
- **WHEN** the server sends `turn/started`, a `thread/tokenUsage/updated` whose `last.totalTokens` is 6,550, and `turn/completed` with `turn.status` `completed`, on a thread whose record's last total was 29,532
- **THEN** the outcome is compacted with the figures 29,532 and 6,550, and the request is cleared

#### Scenario: The turn failed
- **WHEN** the server sends `turn/completed` with `turn.status` `failed` and a `turn.error.message` of 204 characters on one line, starting "Error running remote compact task: "
- **THEN** the outcome is failed, and its reason is the first 200 characters of that message followed by "…"
- **AND** the run keeps the whole message

#### Scenario: The turn was interrupted
- **WHEN** the server sends `turn/completed` with `turn.status` `interrupted` and no error
- **THEN** the outcome is failed, and its reason says Codex ended the compaction as interrupted

#### Scenario: Codex has no record of the thread
- **WHEN** `thread/resume` is answered with the error "no rollout found for thread id …" and `thread/compact/start` with "thread not found: …"
- **THEN** the outcome is failed, and its reason names `thread/resume` and the first message

#### Scenario: A Codex without the compact method
- **WHEN** `thread/compact/start` is answered with the error "Invalid request: unknown variant `thread/compact/start` …"
- **THEN** the outcome is failed with the reason "Codex refused thread/compact/start: this Codex has no such method", and the process is stopped
- **AND** the run keeps Codex's whole message

#### Scenario: A long refusal is cut
- **WHEN** a request is refused with a message of 4,000 characters over three lines
- **THEN** the reason is one line of 200 characters, the request's name and then the start of the message's first line, and ends with "…"

#### Scenario: A parameter value Codex does not have
- **WHEN** `thread/resume` is answered with the error "Invalid request: unknown variant `no-such-sandbox`, expected one of `read-only`, `workspace-write`, `danger-full-access`"
- **THEN** the outcome is failed, and its reason names `thread/resume` and gives that message, not that this Codex has no such method

#### Scenario: A reply out of order
- **WHEN** `thread/compact/start` is answered with "thread not found: …" before the reply to `thread/resume` arrives
- **THEN** the outcome is failed, and its reason names `thread/compact/start` and that message

#### Scenario: Codex asks the client something
- **WHEN** the server sends a request of its own, such as `item/commandExecution/requestApproval`, before the compaction's turn completes
- **THEN** the host writes nothing in answer, the outcome is failed saying Codex asked for that request, which a compaction cannot answer, and the process is stopped

#### Scenario: A status the host does not know
- **WHEN** the server sends `turn/completed` whose `turn.status` is a word this host has not seen
- **THEN** the outcome is failed with that word in its reason, not compacted

#### Scenario: The process ends early
- **WHEN** the server exits before sending `turn/completed`
- **THEN** the outcome is failed, with the exit and the end of its error output as the reason

#### Scenario: The process ends early with long error output
- **WHEN** the server exits before sending `turn/completed`, having written 300 characters and then more lines to its error output
- **THEN** the reason is one line of 200 characters, the exit and then the start of that output, and ends with "…"
- **AND** the run keeps the whole text

#### Scenario: The turn failed with no message to show
- **WHEN** the server sends `turn/completed` with `turn.status` `failed` and a `turn.error.message` that is empty or starts with a line break
- **THEN** the outcome is failed, and its reason says Codex ended the compaction as failed

#### Scenario: No completion arrives
- **WHEN** the server answers every request and then sends nothing more
- **THEN** when the limit passes the host stops the process group and the outcome is failed, saying Codex did not finish compacting in that time
- **AND** the agent's waiting message is delivered by its next turn

#### Scenario: No figure reported
- **WHEN** the compaction's turn completes without a `thread/tokenUsage/updated`
- **THEN** the outcome is compacted with no counts

### Requirement: A Codex compaction's usage is recorded from the session record
The host SHALL record a Codex agent's compaction that ended as compacted as one request in the usage ledger, attributed to the agent and its coordinator like any other request. Codex's notifications report no tokens for a compaction and its cumulative usage does not include it, so the host SHALL read them from the thread's session record: the usage entry Codex writes for a model request, identified by carrying the id of the compaction's turn, which the completion notification names. The host SHALL read the record only after the compaction's process has been stopped. It SHALL record a request only when exactly one entry carries that id and its own usage can be read, taking the input count as including cached tokens, as for a Codex turn, with its cached, cache-write, output and reasoning counts. When no entry, more than one entry, an unreadable entry, no turn id or no session record is found, the host SHALL record no request, SHALL NOT take any other entry, any running sum in an entry or any difference of totals in its place, and SHALL still settle the compaction as compacted. A compaction that failed or was interrupted SHALL record no request. The request's id SHALL be distinct from every turn's. The request and the compaction's settlement SHALL be stored together or not at all, and settling a compaction again SHALL NOT add a second request. The request SHALL NOT be a reading of the thread's cumulative usage and SHALL NOT change the reading the next turn is measured from; a compaction run SHALL NOT count as the run before a turn when the host decides whether that turn's request is complete. The first turn after a Codex compaction SHALL be recorded as it would have been without the compaction. Usage reports SHALL count the request in their existing totals and SHALL NOT report it separately.

#### Scenario: One request from the session record
- **WHEN** a Codex agent's compaction completes as turn `T`, and the session record holds one usage entry for `T` with input 29,552, cached input 29,312, cache write 0, output 440 and reasoning 0
- **THEN** the ledger gains one request for that agent and its model with input 29,552, cache read 29,312, cache write 0, output 440 and reasoning 0
- **AND** the agent's and its coordinator's totals in the usage report rise by them

#### Scenario: No usage entry in the record
- **WHEN** a Codex agent's compaction completes and the session record holds no usage entry for its turn
- **THEN** the ledger gains no request, the outcome is compacted, and the conversation line is written

#### Scenario: Two usage entries for the turn
- **WHEN** the session record holds two usage entries carrying the compaction turn's id
- **THEN** the ledger gains no request and the outcome is compacted

#### Scenario: An entry that cannot be read
- **WHEN** the one usage entry for the compaction's turn has no readable input count, or more cached tokens than input
- **THEN** the ledger gains no request and the outcome is compacted

#### Scenario: Another turn's entry is not taken
- **WHEN** the session record's only usage entries belong to the agent's earlier turns
- **THEN** the ledger gains no request for the compaction

#### Scenario: A failed compaction records nothing
- **WHEN** a Codex agent's compaction ends as failed or interrupted
- **THEN** the ledger gains no request for that run

#### Scenario: The next turn is counted whole
- **WHEN** a Codex agent runs a turn, is compacted with its request recorded, and runs another turn whose cumulative usage is 19,882 tokens higher than the first turn's
- **THEN** the second turn's request holds 19,882 tokens and is recorded as complete

#### Scenario: Settled twice
- **WHEN** a compaction whose request was recorded is settled again
- **THEN** the ledger still has one request for that run

### Requirement: The Codex context estimate counts what a compaction kept
When a Codex thread's session record holds a compaction entry, the host's context measurement SHALL estimate the conversation categories from what that entry kept, its retained messages and its summary item, and from the items recorded after it, and SHALL NOT count items recorded before it. The total SHALL stay the one Codex reported.

#### Scenario: Measured after a compaction
- **WHEN** a Codex thread's record ends with a compaction entry that kept two user messages and a summary item, followed by Codex's total of 6,550
- **THEN** the measurement's total is 6,550 and its categories include Messages, sized from those kept items

### Requirement: A started process holds only its own descriptors
A process the host starts, whether a provider turn, a compaction, a provider probe or a Git or `gh` call, SHALL hold no file descriptor of the host's other than its own standard input, output and error. This SHALL hold when other threads are starting processes at the same moment, so that neither the process nor anything it starts can keep another process's pipe open. A provider SHALL still receive its three pipes and the `WORKSPACE_SOCKET` and `WORKSPACE_TOKEN` environment.

#### Scenario: Providers started together
- **WHEN** the host starts several providers at the same moment while it also runs a Git query
- **THEN** each provider holds its standard input, output and error and no other descriptor of the host's

#### Scenario: A provider leaves a process running
- **WHEN** two providers start together and one leaves a long-lived process behind
- **THEN** the other provider still sees the end of its prompt, because that process holds neither end of its input pipe

### Requirement: Startup stops processes the last host left running
At startup, before any turn starts, the host SHALL stop every process group still recorded as running for any agent, whether its run was left unfinished or its agent was between turns, under the confirmation rule, and SHALL record each group it stops with the reason `startup`. It SHALL then mark each run the last host left unfinished as interrupted and record on the run when its provider's process group was stopped, also when part of that group still runs after the host signalled it: the provider itself was stopped, the session stays held, and the note says the group was signalled. A run whose provider's group was neither stopped nor signalled SHALL record no such time, also when a process its agent had started was stopped at startup. A run an earlier version recorded with only a process group and its leader's start time SHALL be carried into the record when the store is opened, with that leader as its one member, or with no member when no start time was kept.

#### Scenario: Leftover provider
- **WHEN** the host starts and an unfinished run's provider process is still running with its recorded start time
- **THEN** the process group is stopped before any turn starts
- **AND** the run is interrupted and records when its process was stopped

#### Scenario: Killed mid-turn with a tool process running
- **WHEN** the last host was killed mid-turn while a process the agent started was running
- **THEN** startup stops that process

#### Scenario: The provider had gone and a tool had not
- **WHEN** the last host was killed mid-turn, the run's provider has since exited and a process the agent started still runs
- **THEN** startup stops that process
- **AND** the run records no stop time, and the agent's retry note does not say its provider kept running

#### Scenario: Killed between turns
- **WHEN** the last host was killed while an agent between turns had a process it started still running
- **THEN** startup stops that process

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

#### Scenario: The provider exits and a group member ignores the signal
- **WHEN** the host starts, signals a confirmed leftover, its leader exits on SIGTERM and another member of the group ignores it
- **THEN** that member is killed, the run is interrupted and records when its process was stopped, and no note is raised

#### Scenario: Part of a stopped provider's group survives
- **WHEN** the host starts, signals a confirmed leftover, every process it confirmed exits and a process that joined the group after that keeps running
- **THEN** the run is interrupted and records when its process was stopped
- **AND** one inbox note says the group was signalled and that part of it still runs
- **AND** the session starts no turn while that group runs

### Requirement: The host records every process group an agent starts
While a provider run is live, the host SHALL watch the provider's process and its descendants on a clock that does not depend on the provider's output, sleeping no more than 5 ms between two looks, and listing the process table once per look however many runs are live. It SHALL record against the run's agent every process group seen among them, with each member's process id and start time, and SHALL add as members the processes it later finds in a recorded group while a recorded member is still in it. The record SHALL belong to the agent across its turns, SHALL hold any number of groups and SHALL survive a host restart. A group whose every member has exited SHALL be dropped from the record. A process seen in a group SHALL be added to the agent's open record of that group id only while a member that record holds is still alive in the group, by the rule that confirms a group before it is signalled; otherwise that group emptied unseen and its id was given out again, so the old record SHALL be dropped and a new one opened for the run that saw the process, with no notice yet. What the watcher saw SHALL keep reaching the record for as long as the provider runs, also after the provider has closed its output. When the watcher could not look for longer than a second, a process id may have been given out again in the meantime: a recorded group whose last known member has gone but whose id still names a running group SHALL then stay recorded as running, with none of that group's processes added as members, so the host later stops it or asks the human about it. A normal turn end SHALL stop the provider's own process group and SHALL leave every other recorded group running.

#### Scenario: A tool starts a process in its own session
- **WHEN** a turn's provider starts a child in a new session and process group, and the turn finishes normally
- **THEN** the child is still running
- **AND** its group is recorded against the agent with the child's process id and start time

#### Scenario: The group leader exits first
- **WHEN** the shell that leads a recorded group starts a child and exits before the turn ends
- **THEN** the group stays recorded with the child as a member

#### Scenario: The command finishes
- **WHEN** every process of a recorded group has exited
- **THEN** the group is no longer recorded

#### Scenario: A shell that lives between two looks
- **WHEN** a shell starts a child and exits without the watcher having listed the shell once
- **THEN** the child is not recorded

#### Scenario: The provider closes its output and keeps running
- **WHEN** a provider closes its output, then starts a process in a group of its own and keeps running
- **THEN** that group is recorded while the provider still runs

#### Scenario: The watcher could not look for over a second
- **WHEN** the watcher did not look for over a second, the last member it knew of a recorded group has exited meanwhile, and the group's id still names a running group
- **THEN** the group stays recorded as running and none of its processes is added as a member

#### Scenario: A group id given out again
- **WHEN** an agent's open record of a group holds only members that have exited, and the agent's current run is seen with a process in a new group that has the same id
- **THEN** the process is recorded in a new record of the current run, which no notice has named yet, and the old record is gone

#### Scenario: Several turns at once
- **WHEN** three providers run at the same time
- **THEN** the process table is listed once per look, not once per provider

### Requirement: A process group is signalled only while a recorded member is alive in it
The host SHALL signal a recorded process group only while at least one of its recorded members, matched by process id and start time, is alive and still in that group. It SHALL NOT require the group's leader to be alive and SHALL NOT signal on a group id alone. It SHALL send SIGTERM, wait the grace period, and send SIGKILL only if the group still runs and a process it confirmed is still alive in it. The same check SHALL be used when an agent is archived, when it is stopped, at a quit and at startup. A recorded group that no longer runs SHALL be dropped from the record with no signal. A group that still runs and cannot be confirmed SHALL receive no signal, SHALL stay recorded as running, and SHALL raise one inbox note on the agent that names the agent and the process group and says the host did not stop it. A group that still runs after the host signalled it, because processes joined it after the host confirmed it or outlast SIGKILL, SHALL NOT be signalled again while no process the host confirmed is alive in it, SHALL stay recorded as running, and SHALL raise the note saying that the host signalled the group and that part of it still runs, not that it did not stop it. No note SHALL be raised when nothing still runs. Each group the host stops SHALL be recorded with the time and one reason: `archived`, `archive_failed`, `stopped`, `quit` or `startup`.

#### Scenario: Orphans of a leader that exited
- **WHEN** the host stops a recorded group whose leader has exited and whose recorded members still run
- **THEN** every process in the group is stopped
- **AND** the record says when and why

#### Scenario: A process that ignores SIGTERM
- **WHEN** a confirmed group still runs after the grace period
- **THEN** it is killed

#### Scenario: A member that ignores SIGTERM beside a leader that exits on it
- **WHEN** only a group's leader is recorded, the leader exits on SIGTERM and another process that was in the group when the host confirmed it ignores the signal
- **THEN** that process is killed and the group is recorded as stopped

#### Scenario: A process that joined after the group was confirmed
- **WHEN** every process the host confirmed exits on SIGTERM and a process that joined the group after that keeps running
- **THEN** the group receives no second signal and stays recorded as running
- **AND** one inbox note says the host signalled the group and that part of it still runs

#### Scenario: Recycled group id
- **WHEN** every recorded member of a group has gone and the group's id now belongs to an unrelated process
- **THEN** that process receives no signal
- **AND** one inbox note names the agent and the process group

#### Scenario: Nothing left
- **WHEN** every recorded member of a group has gone and no process has that group id
- **THEN** nothing is signalled, no note is raised and the group is no longer recorded

### Requirement: Archiving an agent stops what it started
When a session is archived, by any path, the host SHALL stop every process group recorded for that session across all its turns, under the confirmation rule, and SHALL do so before any ticket worktree is removed. The paths are the human archiving an agent or a project, `accept_ticket`, `close_ticket`, `archive_agent`, verification retiring a verifier and `verify_ticket` replacing one. A session archived while its turn still runs SHALL have its groups stopped when that turn ends. An archive that is refused because a worktree cannot be removed SHALL stop nothing. When an archive, a close or an accept fails after that check, what it stopped stays stopped: each such group SHALL be recorded with the reason `archive_failed` instead of `archived`, and each agent that is still unarchived SHALL receive the notice of what was stopped, saying that the archive then failed, with its next turn.

#### Scenario: Accepting a ticket
- **WHEN** a ticket is accepted and its implementer left a process running
- **THEN** the process is stopped before the ticket's worktree is removed
- **AND** its group is recorded as stopped with the reason `archived`

#### Scenario: Closing a ticket
- **WHEN** a ticket is closed and one of its agents left a process running
- **THEN** the process is stopped

#### Scenario: The coordinator archives an agent
- **WHEN** `archive_agent` archives a reviewer that left a process running
- **THEN** the process is stopped

#### Scenario: A verifier is retired
- **WHEN** verification archives a verifier that left a process running
- **THEN** the process is stopped, at once if its turn has ended and otherwise when that turn ends

#### Scenario: The human archives a project
- **WHEN** the human archives a project whose agents left processes running
- **THEN** every one of them is stopped

#### Scenario: Refused archive
- **WHEN** an archive is refused because a worktree holds uncommitted work
- **THEN** no process is signalled

#### Scenario: An archive that fails after the check
- **WHEN** a ticket's close passes the worktree check, stops a process its agent left running, and then fails
- **THEN** the agent is still unarchived and the process stays stopped
- **AND** its group is recorded as stopped with the reason `archive_failed`
- **AND** the agent's next turn takes the notice, which says the process was stopped when the agent was to be archived and that this then failed

### Requirement: Stopping an agent stops what it started
When the human pauses a session, when the human stops a project, and when a coordinator calls `stop_agents`, the host SHALL stop every process group recorded for each agent it stops, under the confirmation rule, before the command or the tool call returns. It SHALL first look afresh at what each stopped agent's running turn has started, without waiting for that turn to pass it on, and SHALL record every process then followed for that turn, not only what is new since the turn last asked, so a process started a moment before the stop is stopped too, also when its sighting was on its way to the record. The watcher SHALL keep answering for a turn until the host has stored everything that turn sent, its last sightings included, so the look names what the turn started also after the provider has exited and while the turn's end is on its way. A turn that ends on its own as its agent is stopped SHALL be treated as a stopped turn: what it started SHALL be stopped and SHALL NOT be announced as left running. A stop lets a compaction that has started finish, so it SHALL leave alone what that compaction's provider started, such as a Codex app-server's tool servers; when the compaction ends and its agent is still paused or its project still stopped, what the compaction left running SHALL be stopped then. Stopping a project SHALL do so for every agent of the project, idle ones included: an emergency stop leaves nothing running unsupervised. When a cancelled turn ends, the host SHALL stop the groups recorded for its agent since. Each group SHALL be recorded as stopped with the reason `stopped`.

#### Scenario: Pausing an idle agent
- **WHEN** the human pauses an agent that left a process running in an earlier turn
- **THEN** the process no longer runs when the command returns

#### Scenario: A coordinator stops a working child
- **WHEN** a coordinator calls `stop_agents` on a child whose turn has a tool process running
- **THEN** the tool process no longer runs when the call returns
- **AND** anything the turn started after that is stopped when the turn ends

#### Scenario: A tool started a moment before the stop
- **WHEN** the human pauses an agent whose turn started a tool process a moment ago, before the host had recorded it
- **THEN** that process no longer runs when the command returns

#### Scenario: A sighting on its way
- **WHEN** a coordinator stops a child whose turn had taken a tool process's sighting from the watcher but not yet passed it to the record
- **THEN** that process no longer runs when the call returns

#### Scenario: The turn's last sightings on their way
- **WHEN** a provider has finished on its own leaving a process running, its last sightings and its end have not reached the host yet, and the agent is stopped
- **THEN** that process no longer runs when the stop returns

#### Scenario: The turn was finishing as its agent was stopped
- **WHEN** an agent is stopped and its turn's own, normal end reaches the host afterwards, with a process the stop's look came too early for
- **THEN** that process is stopped when the end is handled, with the reason `stopped`
- **AND** no notice says the turn left it running

#### Scenario: Stopping an agent whose compaction runs
- **WHEN** an agent is stopped while its compaction runs, with one process an earlier turn left running and one the compaction's provider started
- **THEN** the earlier turn's process no longer runs when the stop returns, and the compaction's process still runs
- **AND** when the compaction ends with the agent still stopped, that process is stopped with the reason `stopped`

#### Scenario: Stopping a project
- **WHEN** the human stops a project while one of its agents has a tool process running in its turn
- **THEN** that process no longer runs when the command returns

#### Scenario: Stopping a project with an idle agent
- **WHEN** the human stops a project one of whose agents is between turns and left a process running in an earlier turn
- **THEN** that process no longer runs when the command returns
- **AND** its group is recorded as stopped with the reason `stopped`

### Requirement: A turn that leaves processes running says so
When a turn ends normally and a group recorded for its agent still has live members that no notice has named, the host SHALL add one notice from Wiffletree to the agent's conversation that first states the number of processes, then names their programs, then each process group and when it started, and says they keep running until the agent is archived. Every notice SHALL name a program as a program, as in "All 8 are the program `yes`." or "2 are the program `node` and 1 is `esbuild`.", and SHALL NOT put a program's name beside a bare count. Before deciding, it SHALL wait up to 300 ms for processes that are exiting with the provider. The notice SHALL be delivered to the agent as input from Wiffletree with its next turn and SHALL NOT start a turn. A group SHALL be named in at most one such notice. When the host stops an agent's process groups, it SHALL add one notice from Wiffletree to that agent's conversation that first states the number of processes stopped, then names their programs the same way, then the groups and why. Neither notice SHALL raise an inbox item.

#### Scenario: Once per set of processes
- **WHEN** a turn leaves a process running and the agent's next two turns leave nothing new
- **THEN** the conversation has one notice naming that process, and the inbox has no item for it

#### Scenario: A later set of processes
- **WHEN** a later turn of the same agent leaves another process running
- **THEN** the conversation gains a second notice, which names only the new process group

#### Scenario: Eight of one program
- **WHEN** a Tester's turn leaves eight processes of the program `yes` running
- **THEN** the notice reads "Tester left 8 processes running. All 8 are the program `yes`."

#### Scenario: Several programs
- **WHEN** a turn leaves two `node` processes and one `esbuild` process running
- **THEN** the notice says "2 are the program `node` and 1 is `esbuild`."

#### Scenario: The agent is told
- **WHEN** the agent's next turn starts
- **THEN** its input includes the notice, from Wiffletree

#### Scenario: Stopped on archive
- **WHEN** the agent is archived and the host stops that process
- **THEN** the agent's conversation ends with a notice naming the process, its group and that it was stopped when the agent was archived

### Requirement: Agents are told how to stop what they start
The communication contract given to every agent SHALL say that a process the agent starts keeps running after its command ends until the agent is archived, that the agent stops what it does not mean to keep, that it saves each process id from `$!` and kills those ids, that it does not use `jobs -p`, which is empty in a zsh subshell, and that it afterwards confirms by name that nothing unintended is still running.

#### Scenario: Every role
- **WHEN** any agent's turn starts
- **THEN** its instructions include that rule
