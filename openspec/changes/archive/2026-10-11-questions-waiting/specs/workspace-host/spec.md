## ADDED Requirements

### Requirement: Questions waiting are counted
The host SHALL count, for each session, the `blocked` reports it sent to its parent that have not been answered, and SHALL give that count in the snapshot and as `questions_waiting` on each `team` entry of `workspace_context`. A report SHALL count as answered once a message its parent sent after it has been delivered to the session by the start of a turn; an answer that is only queued SHALL leave it waiting. The count SHALL be read from stored messages, SHALL be the same after a host restart, and SHALL NOT change the session's status or its ticket's state.

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

#### Scenario: Answer sent between two reports
- **WHEN** an implementer reports `blocked`, its coordinator answers, and it reports `blocked` again before a turn takes the answer
- **THEN** the turn that takes the answer leaves one question waiting
