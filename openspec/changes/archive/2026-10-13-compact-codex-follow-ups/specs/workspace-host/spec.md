## MODIFIED Requirements

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

### Requirement: Compaction outcome comes from the provider's status record
The host SHALL decide a Claude agent's compaction's outcome from the status record Claude streams, not from the process's exit code or the result record's error flag. A `compact_result` of `success` SHALL be **compacted**, with the token counts before and after taken from the compaction boundary record when it is present. A `compact_result` of `failed` whose error starts with "Not enough messages to compact" SHALL be **nothing to compact**. Every other ending, including a missing status record, a nonzero exit and a process that could not start, SHALL be **failed**, with the provider's own reason when it gave one. When a process ended without a status record, the reason SHALL be what the run ended with held to one line of at most 200 characters, ended with "…" when it was cut, and the run SHALL keep the whole of it. A Codex agent's compaction SHALL be decided from its turn's completion notification, as its own requirement says. For either provider the host SHALL store the last outcome and its reason on the session's runtime, clear the request, and clear the stored outcome when a `DismissCompaction` command or a new request arrives. A compaction that is running when the host stops, by a shutdown or a crash, SHALL be settled as **interrupted**, at shutdown or at the next startup, through the handling every run gets. A shutdown SHALL cancel a running compaction and wait for its provider process group to be stopped as it does for a running turn, and SHALL record the run as finished only once the group is stopped; a compaction still running when the wait ends SHALL stay unfinished for the next start. At startup an unfinished compaction run SHALL be handled like any unfinished run: the host SHALL stop its process group only while the group's leader has the recorded process id and start time, and SHALL otherwise signal nothing and raise the leftover-process note, which holds the session's turns and compactions until the group exits or the note is dismissed. An interrupted compaction SHALL leave the session's stored status and its waiting messages as they were, SHALL hold no input and SHALL need no resume. It SHALL NOT be run again by the host and SHALL NOT be reported as compacted or as unchanged, and the host SHALL NOT tell the session's next turn that a turn was interrupted.

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
