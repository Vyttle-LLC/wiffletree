## ADDED Requirements

### Requirement: The host may start a review for the PR keeper
The host SHALL be able to start one verification cycle on a ticket for the PR keeper, without a `verify_ticket` call and without the ticket being `ready_for_testing`, only for a commit the keeper has moved onto the ticket's branch. The cycle SHALL run the level the keeper's settings name, whose kind SHALL be re-check, and every rule for starting a level SHALL apply unchanged: the ticket's latest cycle passed, and the level leaves out no configured verifier that has an open finding. It SHALL be one round. Each verifier SHALL run on the exact model the coordinator last chose for that verifier on this ticket, which SHALL still be allowed for it; the host SHALL NOT choose or substitute a model. When any of this cannot be met, no cycle SHALL start and nothing about the ticket SHALL change. The cycle SHALL be recorded as the keeper's, SHALL take the next cycle number, and SHALL NOT count against the cycle cap. The Tickets panel and `workspace_context` SHALL mark it as the PR keeper's. Verifiers SHALL get the same message and SHALL be bound by the same read-only and pinned-commit rules as in any cycle.

#### Scenario: A keeper review starts
- **WHEN** the keeper has moved a fix onto the branch of a ticket whose cycle 1, a "Full" review, passed with Correctness on Claude · sonnet · high, and the keeper's level is "Light", a re-check that runs Correctness
- **THEN** cycle 2 starts with the Correctness reviewer on Claude · sonnet · high, pinned to the keeper's commit, and the card shows "Verification cycle 2 · Light re-check by the PR keeper"

#### Scenario: No model was ever chosen
- **WHEN** the keeper's level runs Style, and Style has never run on this ticket
- **THEN** no cycle starts, and the ticket's cycles, ledger and state are unchanged

#### Scenario: The chosen model is no longer allowed
- **WHEN** the model the coordinator last chose for Correctness on this ticket has since been removed from the allowed models
- **THEN** no cycle starts and no other model is used

#### Scenario: The latest review did not pass
- **WHEN** the ticket's latest cycle ended blocked
- **THEN** no keeper review starts

#### Scenario: A verifier with an open finding is left out
- **WHEN** the ledger holds an open entry from Regressions and the keeper's level runs only Correctness
- **THEN** no keeper review starts

#### Scenario: Not counted against the cap
- **WHEN** a ticket has used both full reviews of a cycle cap of 2 and the second one passed
- **THEN** a keeper review may still start

### Requirement: A keeper review that passes stays
When every verifier of a keeper review passes in its one round, the cycle SHALL end `passed` and SHALL remain the ticket's latest cycle, treated by `accept_ticket`, `publish_ticket` and later re-checks as any passed re-check is. The ticket's state SHALL be `passed`. The host SHALL wake the coordinator with the cycle's message only when the review left an entry to triage; otherwise it SHALL send no cycle message.

#### Scenario: Passed with nothing to triage
- **WHEN** a keeper review passes with no finding
- **THEN** it is the ticket's latest cycle, the ticket is `passed`, and the coordinator is not woken

#### Scenario: Passed with a non-blocking finding
- **WHEN** a keeper review passes and a verifier reported one non-blocking finding
- **THEN** the finding is `untriaged` in the ledger and the coordinator gets the cycle's message telling it to triage

### Requirement: A keeper review that does not pass is withdrawn
A keeper review SHALL end after its one round. When any verifier's result is failed or blocked, the host SHALL withdraw the review instead of ending it blocked: it SHALL NOT send findings to the implementer, SHALL NOT start another round, and SHALL NOT send the coordinator a cycle message.

The host SHALL withdraw only the cycle that the keeper's running try recorded as its own, and only while that cycle is the ticket's latest; it SHALL never withdraw a cycle started with `verify_ticket`, an earlier cycle, or a keeper review that passed.

Withdrawing SHALL, in one store change: make the cycle before it the ticket's latest cycle again, with its level, rounds, results and outcome unchanged; put the ledger back as it was when the review started; and put the ticket's state back to what it was then. The withdrawn cycle, its verdicts and its findings SHALL be kept on the keeper's record of the try and SHALL be shown there, not among the ticket's cycles. The cycle's activity events SHALL remain, and a `keeper_review_withdrawn` event SHALL be logged. Verifier sessions SHALL be kept.

The host SHALL then put the ticket's branch back to the commit it had before the keeper moved it, under the rule for moving a ticket's branch (see pr-keeper): only while the worktree is on the ticket's branch at the keeper's commit, with no uncommitted or untracked files, and no agent of the ticket is in a turn. While an agent is in a turn it SHALL wait. When the worktree is not clean or its HEAD is no longer the keeper's commit, the host SHALL NOT change the worktree or the branch; the records SHALL stay withdrawn, so that `accept_ticket` and `publish_ticket` refuse because HEAD is not the verified commit. The coordinator SHALL be told by the keeper only after the branch is back or has been found not restorable, and in the second case the message SHALL name the commit the worktree is at and the verified commit.

#### Scenario: Withdrawn
- **WHEN** cycle 1, a "Full" review, passed at `3daf610` with ledger entries F1 to F4, and the keeper's review, cycle 2 at `77b0c13`, records a blocking finding F5
- **THEN** the ticket's latest cycle is cycle 1, passed at `3daf610`; the ledger holds F1 to F4 as before; the ticket is `passed`; and cycle 2 with F5 is on the keeper's record of the try

#### Scenario: The branch is put back
- **WHEN** that review is withdrawn and the worktree is clean at `77b0c13` with no agent in a turn
- **THEN** the ticket's branch and worktree are at `3daf610`, and `accept_ticket` and a re-check are possible exactly as before the try

#### Scenario: Nothing reaches the implementer
- **WHEN** a keeper review's verifier fails
- **THEN** the implementer receives no findings, no second round starts, and the coordinator receives no cycle message

#### Scenario: A re-check is still allowed afterwards
- **WHEN** a keeper review was withdrawn and an implementer then resolves the conflict and reports `ready_for_testing`
- **THEN** `verify_ticket` naming the re-check level starts, because the ticket's latest cycle is the passed one

#### Scenario: Only the keeper's own cycle
- **WHEN** a cycle the coordinator started with `verify_ticket` ends blocked
- **THEN** it stays in the ticket's cycles as blocked, and nothing is withdrawn

#### Scenario: The verifier's turn is still ending
- **WHEN** the failing verifier's turn has not finished when its verdict is recorded
- **THEN** the records are withdrawn at once, the worktree is not touched until that turn ends, and the coordinator is told after the branch is back

#### Scenario: The worktree is not clean
- **WHEN** a verifier left an untracked file in the worktree before the review was withdrawn
- **THEN** the worktree and the branch are not changed, the records stay withdrawn, and the coordinator's message names `77b0c13`, where the worktree is, and `3daf610`, the verified commit

#### Scenario: HEAD has moved
- **WHEN** the implementer committed on top of the keeper's commit before the branch could be put back
- **THEN** nothing is reset, the implementer's commit is kept, `publish_ticket` refuses because HEAD is not the verified commit, and the coordinator's message names both commits

#### Scenario: A verifier cannot check
- **WHEN** the keeper review's only verifier reports `blocked`
- **THEN** the review is withdrawn in the same way

## MODIFIED Requirements

### Requirement: Cycle outcomes reach the coordinator once
This requirement covers cycles started with `verify_ticket`; how a review the host starts for the PR keeper ends is defined under "A keeper review that passes stays" and "A keeper review that does not pass is withdrawn". Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when the latest result of every verifier of the cycle's level passed, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number, the cycle's level and whether it was a re-check, for a full review which of the full reviews the cycle cap allows it was, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided, and SHALL include the report of any verifier that reported `blocked`. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries and accepting; after a block, sending fixes and verifying again with a full review only while the cycle cap allows another one, and never with a re-check, accepting with a waiver only when no verifier reported `blocked`, closing the ticket, or asking the human. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every verifier of the level it names.

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
- **THEN** the coordinator's message offers accepting with a waiver, closing the ticket or asking the human, and does not offer verifying again

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

### Requirement: Decision ledger
Each ticket SHALL keep a ledger of every recorded finding, across all its cycles. Each entry SHALL have an id unique in the ticket (`F1`, `F2`, …), the finding's severity, location, summary, trigger and evidence, its source (verifier focus, cycle and round), a status, and a reason once the coordinator decides it. The status SHALL be one of `open`, `fixed`, `untriaged`, `fix_now`, `follow_up` or `wont_fix`. Only a completed check SHALL add or change entries: a `passed` or `failed` report whose worktree check passed. A `blocked` report, or a result the host recorded as failed because the worktree changed, SHALL leave the ledger unchanged, so an `open` entry stays `open` until a completed check clears it. An evidenced blocking finding SHALL enter as `open`, and every other finding as `untriaged`. After a completed check, each `open` entry of the verifier's focus that the report did not name again SHALL become `fixed`; an entry it names again SHALL stay `open`, with its round and evidence updated. A `wont_fix` or `follow_up` entry named again SHALL stay unchanged. When a cycle ends, an `open` entry whose focus had no verifier in that cycle SHALL become `untriaged`. Entries SHALL never be deleted, except that withdrawing a keeper review SHALL put the ledger back as it was when that review started.

#### Scenario: Blocking finding fixed
- **WHEN** the Regressions reviewer's finding F3 is open after round 1, and in round 2 it reports `passed` without naming F3
- **THEN** F3 becomes `fixed`

#### Scenario: Blocking finding persists
- **WHEN** in round 2 the Regressions reviewer reports F3 again by its id with new evidence
- **THEN** F3 stays `open` with round 2 and the new evidence, and no new entry is added

#### Scenario: Re-check cannot run
- **WHEN** F3 is open after round 1 and in round 2 the Regressions reviewer reports `blocked` because it cannot run its check
- **THEN** F3 stays `open`, and the next cycle's message to the Regressions reviewer still lists F3

#### Scenario: Re-check from a changed worktree
- **WHEN** F3 is open and in round 2 the Regressions reviewer edits the worktree and reports `passed` without naming F3
- **THEN** its result is failed with the reason "worktree changed during verification" and F3 stays `open`

#### Scenario: Verifier removed from settings
- **WHEN** an entry from a "Claude" reviewer is open and the next cycle has no verifier with that focus
- **THEN** the entry becomes `untriaged` when that cycle ends

#### Scenario: A withdrawn keeper review leaves no entry
- **WHEN** the ledger holds F1 to F4, a keeper review records F5 and F6, and that review is withdrawn
- **THEN** the ledger holds F1 to F4 exactly as before, and F5 and F6 are kept only on the keeper's record of that try

### Requirement: Cycle history is kept
Starting a new cycle SHALL keep every earlier cycle of the ticket, with its rounds, results and outcome, instead of replacing it. A keeper review that is withdrawn SHALL NOT remain among the ticket's cycles, and its cycle number SHALL NOT be given to a later cycle. The latest cycle SHALL remain the one that verdicts, readiness and acceptance refer to.

#### Scenario: Second cycle
- **WHEN** cycle 1 passed and the coordinator starts cycle 2
- **THEN** the ticket's record holds cycle 2 as its latest cycle and cycle 1 with its rounds and outcome unchanged

#### Scenario: The number of a withdrawn review is not reused
- **WHEN** cycle 1 passed, the keeper's review ran as cycle 2 and was withdrawn, and the coordinator then starts a review
- **THEN** that review is cycle 3, the ticket's cycles are 1 and 3, and each of its verifiers receives its message
