## ADDED Requirements

### Requirement: Verification retires its verifiers
When a verification round ends and its failures go back to the implementer, the host SHALL archive every verifier whose result in that round is `passed` and SHALL keep the failed verifiers for the next round. When a cycle ends, as `passed` or `blocked`, the host SHALL archive every remaining verifier session of that cycle. The host SHALL NOT archive an implementer through verification. A verdict from an archived verifier SHALL be stored quietly for the coordinator and SHALL NOT change the ticket's state or a cycle's outcome. It is recorded on a round only when that is the ticket's latest round and still lists the verifier as pending for the input the verdict answers; a verdict answering an earlier cycle changes no record.

#### Scenario: Passed verifiers retire after a failed round
- **WHEN** round 1 ends with the Claude tester and the style reviewer passed and the Codex tester failed, below the cap
- **THEN** the Claude tester and the style reviewer are archived
- **AND** the Codex tester and the implementer are not archived, and round 2 starts in the Codex tester's session

#### Scenario: All verifiers retire when the cycle ends
- **WHEN** a cycle ends as passed, or as blocked because the cap was reached or the implementer reported `blocked`
- **THEN** every verifier session of that cycle is archived and the implementer is not

#### Scenario: A late verdict from an archived verifier
- **WHEN** a verifier archived at the end of cycle 1 reports after cycle 2 started
- **THEN** its report reaches the coordinator quietly with its next turn
- **AND** the ticket's state and both cycles' records are unchanged

### Requirement: A new cycle uses fresh verifier sessions
A ticket's agent for a role and focus SHALL be looked up among its unarchived sessions only. A `verify_ticket` cycle SHALL send its round input only to unarchived sessions, creating fresh verifier sessions where the previous cycle's were archived.

#### Scenario: Second cycle
- **WHEN** the coordinator calls `verify_ticket` after an earlier cycle on the ticket ended
- **THEN** each verifier of the new cycle is an unarchived session that did not verify the earlier cycle
- **AND** no `verify:` message is addressed to an archived session

### Requirement: The coordinator archives agents it no longer needs
The `archive_agent` tool SHALL take a `session_id` and SHALL archive that agent, keeping its conversation and leaving it restorable, only when the caller is the project coordinator that owns the agent's ticket. It SHALL refuse, naming the reason and archiving nothing, when the session is not an agent of a ticket the caller owns, when the agent is mid-turn, when it is the implementer of an open ticket, or when it is a verifier whose result a running cycle still needs (pending, or failed and awaiting its re-check).

#### Scenario: Idle reviewer
- **WHEN** the coordinator calls `archive_agent` for an ad-hoc reviewer that reported and finished its turn
- **THEN** the reviewer is archived and its conversation is kept

#### Scenario: Guarded agents
- **WHEN** the coordinator calls `archive_agent` for an agent that is working, for its open ticket's implementer, or for a verifier still pending in a running cycle
- **THEN** the host refuses with an error naming that reason and the agent stays unarchived

#### Scenario: Another coordinator's agent
- **WHEN** a session calls `archive_agent` for an agent of a ticket it does not own
- **THEN** the host refuses and the agent stays unarchived

### Requirement: The implementer lives until the ticket is finished
A ticket's implementer SHALL be archived only by `accept_ticket` or `close_ticket` while its ticket is open; neither verification nor `archive_agent` SHALL archive it.

#### Scenario: Implementer across a full cycle
- **WHEN** a cycle runs through a failed round, a fix and a passed round
- **THEN** the implementer is still unarchived and can receive messages until the ticket is accepted or closed
