## ADDED Requirements

### Requirement: No turn limits
The host SHALL NOT limit how many turns run at once, in a project or across the host, and SHALL NOT pause a project or stop a turn because of how many turns it has run. A session with due input SHALL start a turn unless it is paused, disconnected, held, already in a turn, waiting for its ticket's worktree, or a verifier whose round has not been admitted. A stored project that still records a turn limit SHALL load, and the value SHALL be ignored and not reported.

#### Scenario: Many workers at once
- **WHEN** nine workers across two projects each have due input and nothing holds them
- **THEN** all nine turns start in the same scheduling pass

#### Scenario: Long-running project keeps going
- **WHEN** a project's sessions have run 150 turns since the human last messaged it
- **THEN** the project stays live and its next due turn starts

#### Scenario: Old store
- **WHEN** a stored project's data contains `"turn_limit": 4`
- **THEN** the project loads, and neither `workspace_context` nor the snapshot reports a turn limit

### Requirement: Check-ins keep parents informed
The host SHALL tell a session's parent about long or repetitive work, and SHALL never pause or stop work for it. Each check-in SHALL be a durable message from the session to its parent whose body starts with `[check-in]`, names the session, its ticket if any, and recent activity, and says how to stop it with `stop_agents`. A check-in SHALL wake the parent, also while the session's ticket is being verified, and SHALL NOT count as a report or a verdict.
- **Time:** when a turn has run 30 minutes, and every 30 minutes after while it runs, the parent SHALL receive a check-in naming the elapsed time, the last provider event and the latest step.
- **Turn count:** when a session with a parent starts its 25th turn since its parent last sent it a message, and every 25th after, the parent SHALL receive a check-in naming the turn count, when the parent last messaged it, and the start of the session's last reply.
- **Human:** a session without a parent, the project coordinator, SHALL raise a human inbox item instead of a message: for its own 30-minute turns, cleared when a newer one is raised or the turn ends; and when it starts its 100th turn since the human last messaged it, retried its input or answered one of its questions, and every 100th after, cleared when the human next does one of those things or a newer one is raised.
A host restart SHALL clear open time check-ins, and the human's turn count SHALL start again from zero.

#### Scenario: Thirty-minute check-in
- **WHEN** a tester's turn has run 30 minutes
- **THEN** its parent receives a `[check-in]` message naming the tester, its ticket, "30 minutes" and its latest step, and the turn keeps running

#### Scenario: Twenty-five turns without the parent
- **WHEN** an implementer starts its 25th turn since its parent last messaged it
- **THEN** its parent receives one `[check-in]` naming the implementer, its ticket and "25 turns", and the turn runs
- **AND** the 26th turn sends no check-in, and the 50th sends another

#### Scenario: Parent message resets the count
- **WHEN** an implementer has run 24 turns, its parent sends it a message, and it runs 24 more
- **THEN** its parent has received no turn-count check-in

#### Scenario: Coordinator runs 100 turns without the human
- **WHEN** the project coordinator starts its 100th turn since the human last messaged it
- **THEN** the human's inbox gains one item naming the coordinator and the turn count, the project stays live and the turn runs
- **AND** the item stays open after that turn ends, and the human's next message to the coordinator clears it

#### Scenario: Turn-count check-in during a round
- **WHEN** a verifier starts its 25th turn without a message from its parent while its ticket's cycle is running
- **THEN** the project coordinator wakes with the check-in, and the ticket's state and verification record are unchanged
