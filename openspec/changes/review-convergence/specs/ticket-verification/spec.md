## MODIFIED Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers, a round cap and a cycle cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, an optional instruction, and optionally a provider that limits the coordinator's model choice for that verifier. A saved `size` SHALL be ignored. The round cap SHALL default to 3, the cycle cap SHALL default to 2, and each SHALL accept values from 1 to 5. With no saved setting, the list SHALL be a tester (focus "Tests"), a Claude reviewer (focus "Correctness", instruction "Correctness against the ticket's acceptance criteria.") and a Codex reviewer (focus "Regressions", instruction "Regressions and test coverage."), which fits the default project turn limit of 4. A saved setting SHALL keep its verifiers and caps; a saved setting without a cycle cap SHALL use the default. The setting SHALL be edited only on the Models page's Review card.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` starts a tester "Tests", a Claude reviewer "Correctness" and a Codex reviewer "Regressions", each reviewer's instruction naming its lens, with a round cap of 3 and a cycle cap of 2

#### Scenario: Saved setting is not migrated
- **WHEN** the human saved a list with reviewers "Claude" and "Codex" and a round cap of 2 before this change
- **THEN** `verify_ticket` keeps starting those reviewers with a round cap of 2, and the cycle cap is 2

#### Scenario: Invalid setting
- **WHEN** the human saves two verifiers with the same focus, a round cap of 0 or a cycle cap of 6
- **THEN** the setting is rejected and the previous setting stays in effect

### Requirement: Ticket state during a cycle
While a verification cycle is running, the ticket's state SHALL change only when a round starts, to `verifying` (for round 1 and every later round), and when a round ends, to `passed`, `failed` or `blocked` as defined by the cycle outcome rules. A verifier's verdict or the implementer's `ready_for_testing` SHALL NOT set the ticket's state during a cycle. Any other implementer report except `progress`, such as `blocked`, SHALL end the cycle immediately with outcome `blocked`, set the ticket to `blocked`, and wake the project coordinator with that report. `accept_ticket` SHALL refuse while a cycle is running.

#### Scenario: First verdict does not pass the ticket
- **WHEN** in a round of three verifiers the first verifier reports `passed` while the other two are still running
- **THEN** the ticket's state stays `verifying`

#### Scenario: Accept refused mid-round
- **WHEN** the project coordinator calls `accept_ticket` after one of three verifiers has reported `passed` and the others are still running
- **THEN** the host refuses because a verification cycle is running, and the ticket, its agents and its worktree are unchanged

#### Scenario: Implementer blocked between rounds
- **WHEN** round 1 fails and the implementer then reports `blocked` instead of `ready_for_testing`
- **THEN** the cycle ends with outcome `blocked`, the ticket is `blocked`, no further round starts, and the project coordinator wakes at once with the implementer's report

#### Scenario: Verdict after the cycle ended
- **WHEN** a verifier reports `failed` after the implementer's `blocked` report ended the cycle
- **THEN** the verdict is recorded on its round and reaches the project coordinator in its next batch
- **AND** the ticket stays `blocked` and the cycle's outcome stays `blocked`

#### Scenario: Verdict answering an earlier cycle
- **WHEN** a verifier's turn took cycle 1's input, cycle 2 starts while that turn runs, and the turn then reports `passed` before the verifier has taken cycle 2's input
- **THEN** the verdict is stored quietly and cycle 2's round keeps that verifier pending, with the ticket `verifying`

#### Scenario: Verdict after the cycle passed
- **WHEN** a verifier reports again after its cycle passed
- **THEN** the report reaches the project coordinator with its next turn without waking it, and the ticket and cycle are unchanged

#### Scenario: Migrated passed ticket
- **WHEN** a ticket migrated in state `passed` has no verification record
- **THEN** `accept_ticket` refuses it and `verify_ticket` starts its first cycle

#### Scenario: Next round starts verifying again
- **WHEN** the implementer reports `ready_for_testing` after a failed round 1
- **THEN** the ticket's state becomes `verifying` when round 2 starts, never `ready_for_testing`

### Requirement: Failures go back to the implementer
A round SHALL end when every verifier in it has reported. If any verifier's result is failed and the round is below the cap, the host SHALL send the ticket's implementer session one message, from Wiffletree and not in the project coordinator's name, listing every `open` ledger entry the round reported, with each entry's id, location, summary, trigger and evidence, and naming each verifier whose result the host recorded as failed because the worktree changed, with that reason. The message SHALL NOT include the verifiers' full reports or any other finding. The host SHALL set the ticket's state to `failed` and leave the project coordinator asleep. When that implementer next reports `ready_for_testing`, the host SHALL refuse the report while the worktree holds uncommitted or untracked files; otherwise it SHALL start the next round with only the verifiers whose result failed, pinned to the new HEAD. Verifiers that passed SHALL keep their earlier result.

#### Scenario: One verifier fails
- **WHEN** in round 1 the Regressions reviewer reports one evidenced blocking finding and one non-blocking finding, and the other two verifiers pass
- **THEN** the same implementer session that reported ready receives one message from Wiffletree listing only the blocking finding with its id
- **AND** the project coordinator has no turn started by those three reports

#### Scenario: Only failed verifiers re-run
- **WHEN** the implementer commits a fix and reports `ready_for_testing` after a round 1 failure by the Regressions reviewer
- **THEN** round 2 starts with only the Regressions reviewer, pinned to the implementer's new commit
- **AND** the tester and the Correctness reviewer receive no new message

#### Scenario: Worktree changed by a verifier
- **WHEN** in round 1 the Correctness reviewer edits the worktree and reports `passed`, and no verifier reports a blocking finding
- **THEN** the implementer's message names the Correctness reviewer with the reason "worktree changed during verification", so the failed round is never sent with an empty list

#### Scenario: Ready with unsaved work
- **WHEN** the implementer reports `ready_for_testing` during a cycle while its worktree has uncommitted changes
- **THEN** the report fails with an instruction to commit or discard them, and no round starts

### Requirement: Cycle outcomes reach the coordinator once
Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's 30-minute check-in SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when every configured verifier's latest result passed, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number and the cycle cap, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided, and SHALL include the report of any verifier that reported `blocked`. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries and accepting; after a block, sending fixes and verifying again only while the cycle cap allows another cycle, accepting with a waiver only when no verifier reported `blocked`, closing the ticket, or asking the human. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call within the cycle cap SHALL start a new cycle with every configured verifier.

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
- **WHEN** cycle 2 of a cycle cap of 2 ends blocked at the round cap
- **THEN** the coordinator's message offers accepting with a waiver, closing the ticket or asking the human, and does not offer verifying again

#### Scenario: Configurable cap
- **WHEN** the round cap is set to 4 and a verifier fails in rounds 1, 2 and 3
- **THEN** round 4 starts with that verifier

#### Scenario: Verifier cannot verify
- **WHEN** a verifier reports `blocked`
- **THEN** after its round ends the ticket is `blocked` and the project coordinator wakes with that report, without another round, and the message does not offer a waiver

## ADDED Requirements

### Requirement: Verifiers report structured findings
A verifier's `passed` or `failed` report SHALL accept a list of findings. Each finding SHALL have a severity of `blocking`, `non_blocking` or `pre_existing`; a location in `path:line` form; a one-line summary; a trigger; and evidence. It MAY name the id of a ledger entry of the verifier's own focus whose status is `open`, `wont_fix` or `follow_up`, to report that entry again. The host SHALL refuse the whole report, recording nothing, when a finding is malformed or exceeds the text bounds, or names any other id. Findings SHALL be recorded only from a report that is a verdict for the verifier's current round; in any other report they SHALL be ignored.

#### Scenario: Malformed finding
- **WHEN** a verifier reports `failed` with a finding whose severity is "critical"
- **THEN** the report is refused, and the verifier's result and the ledger are unchanged

#### Scenario: Findings in a late verdict
- **WHEN** a verifier whose cycle already ended reports `failed` with findings
- **THEN** the report is stored quietly as today and the ledger gains no entry

### Requirement: The host decides each verifier's result
For a verifier's `passed` or `failed` report at an unchanged worktree, the host SHALL record the result as failed if and only if the report contains an evidenced blocking finding, whatever kind was reported. An evidenced blocking finding has severity `blocking`, a location ending in `:` and a line number, and a non-empty trigger and evidence, and is not a re-report of a `wont_fix` or `follow_up` entry. When the recorded result differs from the reported kind, the host SHALL record the reason on the verifier's run. A `blocked` report and the worktree-changed rule SHALL behave as before.

#### Scenario: Failure without evidence
- **WHEN** a verifier reports `failed` with one blocking finding that has no evidence
- **THEN** its result is recorded as passed with the reason "reported failed without an evidenced blocking finding"
- **AND** the finding enters the ledger as `untriaged`

#### Scenario: Pass with an evidenced blocking finding
- **WHEN** a verifier reports `passed` with an evidenced blocking finding
- **THEN** its result is recorded as failed with the reason "reported passed with an evidenced blocking finding"

#### Scenario: Only non-blocking findings
- **WHEN** a verifier reports `failed` with two non-blocking findings and one pre-existing finding
- **THEN** its result is recorded as passed and the three findings enter the ledger as `untriaged`

### Requirement: Decision ledger
Each ticket SHALL keep a ledger of every recorded finding, across all its cycles. Each entry SHALL have an id unique in the ticket (`F1`, `F2`, …), the finding's severity, location, summary, trigger and evidence, its source (verifier focus, cycle and round), a status, and a reason once the coordinator decides it. The status SHALL be one of `open`, `fixed`, `untriaged`, `fix_now`, `follow_up` or `wont_fix`. Only a completed check SHALL add or change entries: a `passed` or `failed` report whose worktree check passed. A `blocked` report, or a result the host recorded as failed because the worktree changed, SHALL leave the ledger unchanged, so an `open` entry stays `open` until a completed check clears it. An evidenced blocking finding SHALL enter as `open`, and every other finding as `untriaged`. After a completed check, each `open` entry of the verifier's focus that the report did not name again SHALL become `fixed`; an entry it names again SHALL stay `open`, with its round and evidence updated. A `wont_fix` or `follow_up` entry named again SHALL stay unchanged. When a cycle ends, an `open` entry whose focus had no verifier in that cycle SHALL become `untriaged`. Entries SHALL never be deleted.

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

### Requirement: The coordinator triages findings
The project coordinator SHALL have a `triage_findings` tool for tickets it owns, taking a ticket id and a list of decisions, each with an entry id, a decision of `fix_now`, `follow_up` or `wont_fix`, and a one-line reason. Each named entry SHALL be `untriaged`, or `open` with a decision of `follow_up` or `wont_fix`. An `open` entry is already routed for a fix, so `fix_now` on it SHALL be refused. The host SHALL apply every decision or none, refusing the call when an entry is unknown, already decided, named twice, given a decision its status does not allow, or given a reason that is not one line. Marking an `open` entry `follow_up` or `wont_fix` SHALL overrule it: later rounds SHALL list it as already decided, and reporting its id again SHALL fail nothing. A decision SHALL never change once recorded. The tool SHALL record a `findings_triaged` event and SHALL NOT wake or message any agent.

#### Scenario: Triage once
- **WHEN** the coordinator marks F2 `wont_fix` with the reason "Matches the existing naming" and later tries to mark F2 `follow_up`
- **THEN** the first call records the decision and the second is refused, leaving F2 `wont_fix`

#### Scenario: Overruling a routed finding mid-cycle
- **WHEN** the implementer objects to the open entry F3, the coordinator marks it `wont_fix`, and the Regressions reviewer reports F3 again in the next round with no other blocking finding
- **THEN** the reviewer's result is recorded as passed and F3 stays `wont_fix`

#### Scenario: Fix now does not overrule
- **WHEN** the coordinator marks the open entry F3 `fix_now`
- **THEN** the call is refused, F3 stays `open`, and a later re-report of F3 with evidence still fails its round

#### Scenario: All or nothing
- **WHEN** a call decides F2 and an unknown id F99
- **THEN** the call is refused and F2 is unchanged

### Requirement: Cycle cap
`verify_ticket` SHALL refuse, starting nothing, when the ticket has already started as many cycles as the cycle cap allows, saying to accept with a waiver, close the ticket or ask the human. Every started cycle SHALL count, including one an implementer's report ended early.

#### Scenario: Third cycle refused
- **WHEN** a ticket has run cycles 1 and 2 under a cycle cap of 2 and the coordinator calls `verify_ticket`
- **THEN** the call is refused with that guidance and no verifier is started

#### Scenario: Human raises the cap
- **WHEN** the human raises the cycle cap to 3 on the Review card
- **THEN** the next `verify_ticket` on that ticket starts cycle 3

### Requirement: Verifiers receive the ticket's review memory
Every round's message to a verifier SHALL name the ticket's base commit (the merge-base of the repository's base and the pinned commit), the diff range from that base to the pinned commit with its changed-file and line counts, the ledger's `wont_fix` and `follow_up` entries marked as already decided and not to be raised again unless the cited code changed, and the `open` entries of the verifier's own focus with an instruction to report one again by its id only if it is still present. From round 2 on, the message SHALL also name the range from the previous round's commit with its counts, and SHALL tell the verifier to check only whether its open blocking findings are fixed and whether the new diff introduces a blocking defect, raising no new findings on code it already reviewed. A verifier's configured instruction SHALL be included in every round. When the base cannot be computed, the message SHALL say so with the reason, and the round SHALL still start.

#### Scenario: Round 1 memory
- **WHEN** cycle 2 starts on a ticket whose ledger holds F2 `wont_fix` and F4 `follow_up`
- **THEN** each verifier's message names the base commit, the diff range with its counts, and F2 and F4 as already decided

#### Scenario: Round 2 scope
- **WHEN** round 2 starts for the Regressions reviewer after its finding F3 was routed
- **THEN** its message lists F3 as its open finding, names the range from round 1's commit, limits the check to F3 and the new diff, and repeats its configured instruction

#### Scenario: Base not available
- **WHEN** the repository's base ref does not exist in the ticket's worktree
- **THEN** the round still starts and each message says the base is unavailable and why

### Requirement: Cycle history is kept
Starting a new cycle SHALL keep every earlier cycle of the ticket, with its rounds, results and outcome, instead of replacing it. The latest cycle SHALL remain the one that verdicts, readiness and acceptance refer to.

#### Scenario: Second cycle
- **WHEN** cycle 1 passed and the coordinator starts cycle 2
- **THEN** the ticket's record holds cycle 2 as its latest cycle and cycle 1 with its rounds and outcome unchanged

### Requirement: The ledger is visible to the coordinator and the human
`workspace_context` SHALL include each ticket's ledger, earlier cycles and waiver reason. The Tickets panel SHALL show, on each ticket's card, every cycle's verification lines, any waiver reason, and a "Decision ledger" section listing each entry's id, summary, location, source, status and reason.

#### Scenario: Coordinator reads the ledger
- **WHEN** the coordinator calls `workspace_context` after a cycle with three findings
- **THEN** that ticket's entry lists the three ledger entries with their ids, statuses and sources

#### Scenario: Human reads the ledger
- **WHEN** the human opens the Tickets panel for a ticket with two cycles and five ledger entries
- **THEN** the ticket's card shows both cycles' lines and all five entries with their status and reason
