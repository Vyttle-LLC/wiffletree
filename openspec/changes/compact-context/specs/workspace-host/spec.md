## ADDED Requirements

### Requirement: A person can ask to compact a Claude agent's context
The host SHALL accept a `CompactContext` command naming a session and optional instructions on what to keep, and SHALL store it as that session's one compaction request, in state `queued`, in the session's persisted runtime. It SHALL refuse the command, with the reason and without storing anything, when the session's provider is not Claude, when the session has no provider session yet, when it is archived, or when it already has a request. Instructions SHALL be trimmed, SHALL have line breaks replaced by spaces, SHALL be absent when empty and SHALL be refused beyond 1,000 characters. A `CancelCompaction` command SHALL remove a `queued` request and SHALL be refused once the request is `running`. The workspace tools offered to agents SHALL NOT include any way to request, cancel or run a compaction.

#### Scenario: Codex agent
- **WHEN** `CompactContext` names a Codex agent
- **THEN** it is refused, saying the Codex CLI has no compact command, and the agent has no request

#### Scenario: No provider session yet
- **WHEN** `CompactContext` names a Claude agent that has not run a turn
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

### Requirement: A queued compaction runs before waiting input
A session's queued compaction SHALL start as soon as a turn for that session could start: the session is not in a turn, not paused, not disconnected and not archived, it has no held input, its project is live, and its provider is enabled. It SHALL NOT need due input, and the ticket's worktree and verification round SHALL NOT delay it. While a session has a request, queued or running, the host SHALL NOT start a turn for it by any path, and its waiting messages SHALL stay queued; the first turn after the compaction settles SHALL take them. A verification round with such a verifier SHALL wait whole, as it does for a paused verifier, and SHALL be admitted once the compaction has settled. A request that cannot start SHALL stay queued and SHALL remain cancellable. A queued request SHALL survive a host restart unchanged.

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

#### Scenario: Restart while queued
- **WHEN** the host restarts while an agent has a queued request
- **THEN** the request is still queued with its instructions, and nothing ran during startup

### Requirement: A compaction is not a turn
The host SHALL run a compaction by resuming the agent's Claude session with a prompt that is exactly `/compact`, or `/compact` followed by a space and the instructions, without the wrapper every turn's messages get and without attachments. The run SHALL deliver no message and change no message's receipt, SHALL add no reply to the conversation, SHALL send the session's parent no message, SHALL NOT count toward a turn-count check-in for a parent or the human, and SHALL raise no time check-in. It SHALL NOT count as the session's latest turn: what the next turn is told about a turn its parent stopped or a restart interrupted, and the steps and plan shown for the session's latest turn, SHALL be the same as if no compaction had run. The session's stored status SHALL be the same after the compaction as before it. A compaction that has started SHALL NOT be interrupted by a pause or a stop of the session or its project; the pause or stop SHALL still take effect for turns.

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

### Requirement: Compaction outcome comes from the provider's status record
The host SHALL decide a compaction's outcome from the status record Claude streams, not from the process's exit code or the result record's error flag. A `compact_result` of `success` SHALL be **compacted**, with the token counts before and after taken from the compaction boundary record when it is present. A `compact_result` of `failed` whose error starts with "Not enough messages to compact" SHALL be **nothing to compact**. Every other ending, including a missing status record, a nonzero exit and a process that could not start, SHALL be **failed**, with the provider's own reason when it gave one. The host SHALL store the last outcome and its reason on the session's runtime, clear the request, and clear the stored outcome when a `DismissCompaction` command or a new request arrives. When the host shuts down it SHALL stop every running compaction's provider process and settle that compaction as **interrupted**. After that clean shutdown the session's status SHALL be what it was. A compaction left unfinished by a host that did not shut down cleanly SHALL be settled as **interrupted** at startup, and its session SHALL then be disconnected like any session whose run a crash cut off, with a stored reason saying that the host stopped unexpectedly while compacting and that Claude may still be finishing. No turn and no compaction SHALL start for that session, and its waiting messages SHALL stay queued, until the human or its parent resumes it through the existing resume, which SHALL need no choice between retrying and skipping because nothing is held. The host SHALL NOT signal or terminate a process or process group it knows only from a stored identifier. An interrupted compaction SHALL NOT be run again by the host and SHALL NOT be reported as compacted or as unchanged, and the session's next turn SHALL NOT be told that a turn was interrupted.

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

#### Scenario: Shutdown while compacting
- **WHEN** the host shuts down while an agent's compaction is running
- **THEN** the compaction's provider process has exited before the host does, and the outcome is interrupted

#### Scenario: Clean restart while compacting
- **WHEN** the host shuts down cleanly while an agent's compaction is running and starts again
- **THEN** the outcome is interrupted, the request is cleared, no compaction starts by itself, and the agent has the status it had before

#### Scenario: Crash while compacting leaves the agent interrupted
- **WHEN** the host starts after a crash that cut off an agent's compaction, and the agent has a message waiting
- **THEN** the compaction is settled as interrupted, the agent is disconnected with the reason that the host stopped unexpectedly while compacting, and no turn or compaction starts for it
- **AND** the host signals no process

#### Scenario: Resumed after that crash
- **WHEN** the agent's coordinator calls `resume_agent` for it without `held`, or the human resumes it
- **THEN** the agent is ready, its next turn takes the waiting message, and that turn's prompt names no interrupted turn

### Requirement: Compaction usage is recorded in the ledger
The host SHALL record a compaction's tokens in the usage ledger from the result record's `modelUsage`, because that record's `usage` is zero for a compaction: one request per model named there, attributed to the agent and its coordinator like any other request. `modelUsage` follows Claude's convention, in which `inputTokens` excludes tokens read from or written to the cache, so the recorded input SHALL be their sum, as for a turn's request. The reported total cost SHALL be kept with the run. A compaction with no `modelUsage` SHALL record no request. Usage reports SHALL count these requests in their existing totals and SHALL NOT report them separately. Turns SHALL keep recording usage as before.

#### Scenario: Tokens from modelUsage
- **WHEN** a compaction's result record has zero `usage` and a `modelUsage` entry with `inputTokens` 1,502, `cacheReadInputTokens` 811,900, `cacheCreationInputTokens` 0 and `outputTokens` 2,960
- **THEN** the ledger gains one request for that agent and model with input 813,402, cache read 811,900, cache write 0 and output 2,960
- **AND** the agent's and its coordinator's totals in the usage report rise by them

#### Scenario: Nothing spent
- **WHEN** a compaction ends as nothing to compact with an empty `modelUsage`
- **THEN** the ledger gains no request
