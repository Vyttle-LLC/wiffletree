# work-stream Specification

## Purpose

Lets the user watch an agent's in-progress work (narration, thinking, commands, file edits, tool calls and plans) as it happens, while the conversation keeps only durable messages and final replies.

## Requirements

### Requirement: Provider output becomes work steps
The host SHALL turn Claude and Codex streamed output into provider-neutral work steps. Each step SHALL have a stable identity within its run, a kind (narration, thinking, command, file edit, tool call, search or plan), a short title, an optional bounded detail and a state: running, succeeded, failed or interrupted. A plan step SHALL also carry its counts as data: the number of items done, the total number of items, and the text of the first unfinished item, bounded like a title. A checklist an agent records with the workspace tool `set_plan` SHALL become a plan step of its current run in the same form. A step that the provider reports in several events SHALL update one step rather than create duplicates. Provider events the host cannot classify SHALL NOT fail the turn.

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

#### Scenario: Plan from the workspace tool
- **WHEN** an agent calls `set_plan` with three items, the first done
- **THEN** its run shows a plan step titled "Plan · 1 of 3 done" with each item listed in its detail, as a provider plan would

#### Scenario: Plan step stored before this change
- **WHEN** a stored plan step has no counts
- **THEN** it still loads and is shown in the turn history, and it counts as no plan for progress

### Requirement: Conversation receives only the final reply
A turn's chat reply SHALL contain only the turn's final assistant text. Interim narration SHALL appear only as work steps. The reply SHALL be written when the turn ends. The session's own errors SHALL continue to appear in the conversation unchanged, and so SHALL the human's messages, including the human's Inbox answers. Peer messages, being anything another session sent this one (reports, verifier verdicts, turn results, check-ins, direct messages and a coordinator's instructions), the session's own timer fires and Wiffletree's notices (the PR watcher's, the PR keeper's, verification results, kept worktrees, migration notices, publish results and process notices), SHALL appear folded: each unbroken run of them SHALL be one row in the turn summary's place and style, naming how many agent messages and how many notices it holds, each sender once, and when, with failed, blocked and error counts in red. A run with no notice SHALL count only its agent messages, and a run of notices alone only its notices. Opening the row SHALL list each message's sender, kind, first line and time, and opening a line SHALL show its full text. A notice's kind SHALL be a muted pill naming what happened, told from the notice's id and, where needed, its words, such as merged, closed, checks failing, conflict, comments, not pushed, verification, publish, processes, worktree kept or migration; a notice whose kind cannot be told reliably SHALL read "Notice". A notice reporting failing checks, a conflict, a failed rebase, a failed verification round or a failed publish SHALL count as failed, and one reporting a blocked verification as blocked; its pill SHALL be red. Folding SHALL be display only: what is stored, delivered and given to agents SHALL NOT change. Questions to the human SHALL still appear on the card above the composer and in the Inbox. The parent notification for a finished worker turn SHALL use the same final reply.

#### Scenario: Narration between tool calls
- **WHEN** an agent writes "Let me check the tests", runs a command and then writes a summary
- **THEN** the conversation shows only the summary, and the Work stream shows the narration and the command

#### Scenario: Turn fails before a final reply
- **WHEN** a turn fails after emitting narration but no final text
- **THEN** the conversation shows the runtime error, and the narration remains available as work steps

#### Scenario: A run of reports
- **WHEN** the coordinator's conversation receives a ready-for-testing report, a tester's pass and a reviewer's failure in a row, followed by the coordinator's own reply
- **THEN** the three show as one row reading "3 agent messages" with the senders and "1 failed" in red, and the reply below it keeps its bubble and turn summary

#### Scenario: Opening a run
- **WHEN** the human clicks a run's row and then one of its lines
- **THEN** the run lists each message on its own line, and the clicked line shows its full text, and both stay open when the transcript refreshes

#### Scenario: A blocked report stays folded
- **WHEN** a worker reports blocked between two other reports
- **THEN** it stays in the run, which counts "1 blocked" in red

#### Scenario: A run of notices
- **WHEN** the coordinator's conversation receives five PR watcher notices that PRs merged, followed by the coordinator's own reply
- **THEN** the five show as one row reading "5 notices · Wiffletree PR watcher", each line opened shows the "Merged" pill, and the reply below keeps its bubble

#### Scenario: Notices and agent messages share a run
- **WHEN** an implementer's report, a verification result, another report and a PR keeper notice arrive in a row
- **THEN** they show as one row reading "2 agent messages, 2 notices · Implementer, Wiffletree, Wiffletree PR keeper"

#### Scenario: A notice reports a failure
- **WHEN** a run holds a PR watcher notice that checks failed
- **THEN** its line's pill reads "Checks failing" in red and the run counts "1 failed" in red

#### Scenario: The human's words still break a run
- **WHEN** an Inbox answer from the human arrives between two notices
- **THEN** the answer shows as the human's message, and each notice is in its own run

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

### Requirement: A compaction leaves one line in the conversation
A compaction that succeeds SHALL add one entry to the agent's conversation reading "Context compacted · ‹before› → ‹after›", with the provider's own before and after counts printed in the Context card's number format, and a second line "Kept: ‹instructions›" only when instructions were given. For a Codex agent the counts SHALL be Codex's totals for the whole context before and after, and the entry SHALL never have a "Kept:" line. When the provider reported no counts the entry SHALL read "Context compacted". A compaction that fails SHALL add one entry reading "Compaction failed · context unchanged", and one that the host's stopping cut off SHALL add one reading "Compaction interrupted". A compaction that found nothing to compact, and a request that was cancelled, SHALL add no entry. The entry SHALL be durable and SHALL be shown as a single line at the place a finished turn's summary takes, not as a message bubble; a failure SHALL use the failure color. It SHALL NOT be a reply, SHALL NOT be input to any turn, SHALL NOT wake or message any agent, and SHALL have no turn summary and no turn history of its own.

#### Scenario: Compacted with a note
- **WHEN** an agent's compaction succeeds with the provider reporting 812,000 tokens before and 5,000 after, with the instructions "The failing test names and the migration plan"
- **THEN** its conversation shows "Context compacted · 812k → 5k" and beneath it "Kept: The failing test names and the migration plan"

#### Scenario: The card counts more than the line
- **WHEN** that compaction's card is measured again at 41k, which also counts the system prompt and tools
- **THEN** the line still reads "812k → 5k"

#### Scenario: Compacted without a note
- **WHEN** a compaction succeeds with no instructions
- **THEN** the entry has no "Kept:" line

#### Scenario: Codex agent
- **WHEN** a Codex agent's compaction succeeds with Codex reporting a context of 212,000 tokens before and 14,000 after
- **THEN** its conversation shows "Context compacted · 212k → 14k" and no "Kept:" line

#### Scenario: Codex's figure right after a compaction
- **WHEN** that agent's card is measured again at 14k, and its next turn then reports 27k because Codex's figure after a compaction leaves out its fixed prompt and tools
- **THEN** the line still reads "212k → 14k"

#### Scenario: Failed
- **WHEN** an agent's compaction fails
- **THEN** its conversation shows "Compaction failed · context unchanged" in the failure color

#### Scenario: Nothing to compact
- **WHEN** a compaction ends because there was too little to summarize
- **THEN** the conversation gains no entry

#### Scenario: Order around a waiting message
- **WHEN** a compaction requested mid-turn runs while the human's message waits
- **THEN** the conversation shows the turn's reply, then the compaction line, then the turn that takes the message

#### Scenario: After a restart
- **WHEN** the host restarts after a compaction succeeded
- **THEN** the conversation still shows its line at the same place
