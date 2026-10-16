## MODIFIED Requirements

### Requirement: Conversation receives only the final reply
A turn's chat reply SHALL contain only the turn's final assistant text. Interim narration SHALL appear only as work steps. The reply SHALL be written when the turn ends. The session's own errors SHALL continue to appear in the conversation unchanged, and so SHALL the human's messages and Wiffletree's notices. Peer messages, being anything another session sent this one (reports, verifier verdicts, turn results, check-ins, direct messages and a coordinator's instructions) and the session's own timer fires, SHALL appear folded: each unbroken run of them SHALL be one row in the turn summary's place and style, naming how many messages it holds, who sent them and when, with failed, blocked and error counts in red. Opening the row SHALL list each message's sender, kind, first line and time, and opening a line SHALL show its full text. Folding SHALL be display only: what is stored, delivered and given to agents SHALL NOT change. The parent notification for a finished worker turn SHALL use the same final reply.

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
