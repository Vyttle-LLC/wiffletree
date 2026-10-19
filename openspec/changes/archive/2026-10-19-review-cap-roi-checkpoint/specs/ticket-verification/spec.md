## MODIFIED Requirements

### Requirement: Cycle outcomes reach the coordinator once
This requirement covers cycles started with `verify_ticket`; how a review the host starts for the PR keeper ends is defined under "A keeper review that passes stays" and "A keeper review that does not pass is withdrawn". Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when the latest result of every verifier of the cycle's level that ran passed, a skipped verifier not counting, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number, the cycle's level and whether it was a re-check, for a full review which of the full reviews the cycle cap allows it was, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided, and SHALL include the report of any verifier that reported `blocked` and each skipped verifier with its reason. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries and accepting; after a block, sending fixes and verifying again with a full review only while the cycle cap allows another one, and never with a re-check, accepting with a waiver only when no verifier reported `blocked`, closing the ticket, or asking the human. Once no full review remains, it SHALL offer asking the human only for a product, security or data-loss decision. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every verifier of the level it names.

#### Scenario: All pass
- **WHEN** every verifier passes in round 1 and one of them reported a non-blocking finding
- **THEN** the ticket's state is `passed` and the project coordinator wakes once with a summary naming each verifier, the verified commit and the untriaged entry
- **AND** the summary tells it to triage, then accept, and does not mention starting a fresh cycle

#### Scenario: Check-in during a round
- **WHEN** a verifier's turn in a running round reaches 30 minutes
- **THEN** the project coordinator wakes with its check-in, and the ticket's state and verification record are unchanged

#### Scenario: Cap reached
- **WHEN** with a round cap of 2 the Regressions reviewer reports an evidenced blocking finding in rounds 1 and 2
- **THEN** the ticket's state is `blocked`, the implementer receives no third request, and the project coordinator wakes once with the open entry, the rounds used and both caps
- **AND** the human's inbox gains no item from the host

#### Scenario: Last cycle blocked
- **WHEN** the second full review under a cycle cap of 2 ends blocked at the round cap
- **THEN** the coordinator's message offers accepting with a waiver, closing the ticket, or asking the human only for a product, security or data-loss decision, and does not offer verifying again

#### Scenario: Configurable cap
- **WHEN** the round cap is set to 4 and a verifier fails in rounds 1, 2 and 3
- **THEN** round 4 starts with that verifier

#### Scenario: Verifier cannot verify
- **WHEN** a verifier reports `blocked`
- **THEN** after its round ends the ticket is `blocked` and the project coordinator wakes with that report, without another round, and the message does not offer a waiver

#### Scenario: Every verifier of the level passes
- **WHEN** four verifiers are configured, the cycle's level "Light" runs only Correctness, and Correctness passes in round 1
- **THEN** the cycle's outcome is `passed`, the ticket's state is `passed`, and the three other verifiers received no message

#### Scenario: A re-check passes
- **WHEN** cycle 2, a "Light" re-check, passes in round 1
- **THEN** the project coordinator wakes once with a message naming cycle 2 as a Light re-check, its verifier and the verified commit

#### Scenario: A re-check ends blocked
- **WHEN** cycle 2, a "Light" re-check, ends blocked at the round cap after one full review under a cycle cap of 2
- **THEN** the coordinator's message offers sending fixes and running a full review, accepting with a waiver, closing the ticket or asking the human, and does not offer another re-check

### Requirement: Cycle cap
`verify_ticket` SHALL refuse a level whose kind is full review, starting nothing, when the ticket has already started as many full reviews as the cycle cap allows. The refusal SHALL treat the cap as the coordinator's point to judge whether more review still pays for itself: it SHALL tell the coordinator to read any unreviewed changes itself, then accept with a waiver naming them, file what remains as follow-ups, or close the ticket, and to ask the human only when the waiver would settle a product, security or data-loss decision. It SHALL NOT tell the coordinator that the human can raise the cap or offer raising it as an option. Every started full review SHALL count, including one an implementer's report ended early and a cycle recorded without a level. A re-check SHALL NOT count against the cycle cap and SHALL NOT be refused by it. Cycle numbers SHALL stay one sequence across levels.

#### Scenario: Third cycle refused
- **WHEN** a ticket has run two full reviews under a cycle cap of 2 and the coordinator calls `verify_ticket` naming a full review
- **THEN** the call is refused with that guidance and no verifier is started

#### Scenario: The refusal never asks for a higher cap
- **WHEN** `verify_ticket` naming a full review is refused by the cycle cap
- **THEN** the refusal tells the coordinator to decide itself and does not say the human can raise the cap, nor tell the coordinator to ask the human to raise it

#### Scenario: Human raises the cap
- **WHEN** the human raises the cycle cap to 3 on the Review card
- **THEN** the next `verify_ticket` naming a full review on that ticket starts its third full review

#### Scenario: Re-checks are not counted
- **WHEN** under a cycle cap of 2 a ticket's cycle 1 was a full review that passed and cycles 2 and 3 were re-checks that passed
- **THEN** `verify_ticket` naming the re-check level starts cycle 4
- **AND** `verify_ticket` naming a full review instead would start cycle 4 as the ticket's second full review

#### Scenario: Cap reached, then a re-check
- **WHEN** a ticket has used both full reviews of a cycle cap of 2, the second one passed, and the coordinator names the re-check level after a rebase
- **THEN** the re-check starts

#### Scenario: Cap reached after a blocked re-check
- **WHEN** a ticket has used both full reviews of a cycle cap of 2 and its latest cycle, a re-check, ended blocked
- **THEN** `verify_ticket` naming a full review is refused by the cap, and naming the re-check level is refused because the latest review did not pass
