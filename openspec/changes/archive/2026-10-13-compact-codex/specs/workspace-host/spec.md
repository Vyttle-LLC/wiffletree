## ADDED Requirements

### Requirement: A Codex compaction runs through the app-server
The host SHALL run a Codex agent's compaction as one `codex app-server` process that it starts for that compaction and stops when the compaction's outcome is read. The process SHALL lead its own process group and SHALL be recorded, cancelled at a shutdown and handled at startup like every provider run. It SHALL be started in the agent's directory with the configuration overrides a turn of that agent gets: its reasoning effort, its role instructions and the workspace's tool server, whose token SHALL NOT be one any turn holds. The host SHALL send, in order, `initialize`, the `initialized` notification, `thread/resume` and `thread/compact/start`. `thread/resume` SHALL name the agent's thread, its directory, the model of the agent's profile, the sandbox and approval policy a turn runs with, and SHALL ask for no turn history. `thread/compact/start` SHALL carry the thread id and nothing else. The host SHALL write nothing to the process after those four, and SHALL NOT send a second `thread/compact/start` to a process.

#### Scenario: The requests
- **WHEN** a Codex agent whose thread is `T` and whose profile names the model `gpt-6.1-sol` is compacted
- **THEN** the server receives `initialize`, `initialized`, a `thread/resume` for `T` with that model, the agent's directory, sandbox `danger-full-access`, approval policy `never` and `excludeTurns` true, and a `thread/compact/start` holding only `T`

#### Scenario: Resumed as a turn would be
- **WHEN** a Codex agent is compacted
- **THEN** the process's arguments carry the same reasoning effort, role instructions and workspace tool server as that agent's turn, and no prompt

#### Scenario: Stopped when the outcome is read
- **WHEN** the server reports that the compaction's turn completed and keeps running
- **THEN** the host stops its process group, and the run is finished with that group gone

#### Scenario: Quit while a Codex agent is compacting
- **WHEN** the host shuts down while a Codex agent's compaction is running
- **THEN** the server's process group no longer runs when the shutdown returns, and the outcome is interrupted

### Requirement: A Codex compaction's outcome comes from its turn's completion
The host SHALL decide a Codex agent's compaction's outcome from what the app-server sends, never from the process's exit code. A `turn/completed` notification whose `turn.status` is `completed` SHALL be **compacted**. A `turn/completed` with the status `failed` SHALL be **failed** with `turn.error.message` as the reason, bounded as every reason is, and one with any other status, with none the host can read, or failed without a message, SHALL be **failed** with a reason naming that status. An error reply to `initialize`, `thread/resume` or `thread/compact/start` SHALL be **failed** with a reason naming the refused request and Codex's message; when several are refused the first SHALL be the reason. A request the server sends to the host SHALL NOT be answered and SHALL be **failed** with a reason naming what Codex asked for. A reason built from Codex's text SHALL be one line of at most 200 characters, the name of a refused request included, and SHALL end with "…" when it was cut. A refusal SHALL be given as a short reason that names the request and says this Codex has no such method only when Codex's message says the unknown variant is that request's own method; a refusal that opens the same way about anything else, such as a value Codex does not have for one of the request's parameters, SHALL keep Codex's words in the bounded form. Codex's full text SHALL be kept with the run. A process that ends, or cannot start, before an outcome is read SHALL be **failed** with what it ended with. A compaction whose outcome has not been read within a fixed limit SHALL be stopped and SHALL be **failed** with a reason saying Codex did not finish in that time. A Codex compaction SHALL never end as nothing to compact. Notifications the host does not use SHALL be ignored. The token counts of a compacted outcome SHALL be the context total in the thread's session record as the compaction starts and the `tokenUsage.last.totalTokens` Codex reports during the compaction's turn, as Codex gave them; when either is missing the outcome SHALL carry no counts.

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

## MODIFIED Requirements

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
The host SHALL decide a Claude agent's compaction's outcome from the status record Claude streams, not from the process's exit code or the result record's error flag. A `compact_result` of `success` SHALL be **compacted**, with the token counts before and after taken from the compaction boundary record when it is present. A `compact_result` of `failed` whose error starts with "Not enough messages to compact" SHALL be **nothing to compact**. Every other ending, including a missing status record, a nonzero exit and a process that could not start, SHALL be **failed**, with the provider's own reason when it gave one. A Codex agent's compaction SHALL be decided from its turn's completion notification, as its own requirement says. For either provider the host SHALL store the last outcome and its reason on the session's runtime, clear the request, and clear the stored outcome when a `DismissCompaction` command or a new request arrives. A compaction that is running when the host stops, by a shutdown or a crash, SHALL be settled as **interrupted**, at shutdown or at the next startup, through the handling every run gets. A shutdown SHALL cancel a running compaction and wait for its provider process group to be stopped as it does for a running turn, and SHALL record the run as finished only once the group is stopped; a compaction still running when the wait ends SHALL stay unfinished for the next start. At startup an unfinished compaction run SHALL be handled like any unfinished run: the host SHALL stop its process group only while the group's leader has the recorded process id and start time, and SHALL otherwise signal nothing and raise the leftover-process note, which holds the session's turns and compactions until the group exits or the note is dismissed. An interrupted compaction SHALL leave the session's stored status and its waiting messages as they were, SHALL hold no input and SHALL need no resume. It SHALL NOT be run again by the host and SHALL NOT be reported as compacted or as unchanged, and the host SHALL NOT tell the session's next turn that a turn was interrupted.

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

## RENAMED Requirements

- FROM: `### Requirement: A person can ask to compact a Claude agent's context`
- TO: `### Requirement: A person can ask to compact an agent's context`
