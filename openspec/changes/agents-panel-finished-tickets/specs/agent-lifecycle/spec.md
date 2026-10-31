## MODIFIED Requirements

### Requirement: Verification retires its verifiers
When the host records a verifier's result as `passed`, it SHALL archive that verifier at once, whether or not its round has ended, because a passed result is never re-run. The host SHALL keep the failed verifiers for the next round. When a cycle ends, as `passed` or `blocked`, the host SHALL archive every remaining verifier session of that cycle. The host SHALL NOT archive an implementer through verification. A verdict from an archived verifier SHALL be stored quietly for the coordinator and SHALL NOT change the ticket's state or a cycle's outcome. It is recorded on a round only when that is the ticket's latest round and still lists the verifier as pending for the input the verdict answers; a verdict answering an earlier cycle changes no record.

#### Scenario: A verifier retires as it passes
- **WHEN** the Claude tester passes while the Codex tester and the style reviewer are still pending
- **THEN** the Claude tester is archived and the cycle goes on with the others, unaffected

#### Scenario: Passed verifiers retire after a failed round
- **WHEN** round 1 ends with the Claude tester and the style reviewer passed and the Codex tester failed, below the cap
- **THEN** the Claude tester and the style reviewer are archived
- **AND** the Codex tester and the implementer are not archived, and round 2 starts in the Codex tester's session

#### Scenario: A worktree changed after a pass
- **WHEN** the Claude tester passed and archived, the worktree then changes, and the Codex tester reports
- **THEN** the Codex tester's result is failed, the Claude tester's pass stands, and the next round does not include it

#### Scenario: All verifiers retire when the cycle ends
- **WHEN** a cycle ends as passed, or as blocked because the cap was reached or the implementer reported `blocked`
- **THEN** every verifier session of that cycle is archived and the implementer is not

#### Scenario: A late verdict from an archived verifier
- **WHEN** a verifier archived at the end of cycle 1 reports after cycle 2 started
- **THEN** its report reaches the coordinator quietly with its next turn
- **AND** the ticket's state and both cycles' records are unchanged
