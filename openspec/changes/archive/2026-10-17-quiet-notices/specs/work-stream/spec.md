## MODIFIED Requirements

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
