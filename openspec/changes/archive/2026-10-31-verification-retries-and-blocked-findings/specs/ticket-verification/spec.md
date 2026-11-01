## MODIFIED Requirements

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

#### Scenario: A retried verdict counts once its input is taken
- **WHEN** a verifier's report was stored quietly because it did not answer its current round's input, such as one sent while an earlier host held the cycle queued, and after taking that input the verifier sends the same report again with the same id
- **THEN** the host returns the stored report without storing it again, and records it as the verifier's verdict for the round
- **AND** a further retry with that id changes nothing

#### Scenario: Verdict after the cycle passed
- **WHEN** a verifier reports again after its cycle passed
- **THEN** the report reaches the project coordinator with its next turn without waking it, and the ticket and cycle are unchanged

#### Scenario: Migrated passed ticket
- **WHEN** a ticket migrated in state `passed` has no verification record
- **THEN** `accept_ticket` refuses it and `verify_ticket` starts its first cycle

#### Scenario: Next round starts verifying again
- **WHEN** the implementer reports `ready_for_testing` after a failed round 1
- **THEN** the ticket's state becomes `verifying` when round 2 starts, never `ready_for_testing`

### Requirement: Cycle outcomes reach the coordinator once
This requirement covers cycles started with `verify_ticket`; how a review the host starts for the PR keeper ends is defined under "A keeper review that passes stays" and "A keeper review that does not pass is withdrawn". Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when the latest result of every verifier of the cycle's level that ran passed, a skipped verifier not counting, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier's result is blocked. The message SHALL be sent from Wiffletree and SHALL name the cycle number, the cycle's level and whether it was a re-check, for a full review which of the full reviews the cycle cap allows it was, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided or noted in earlier cycles, and SHALL include the report of any verifier whose result is blocked and each skipped verifier with its reason. It SHALL list the `noted` entries this cycle recorded in one paragraph headed with their count, one line each with the entry's id, location and summary, at most 10 lines, followed, when there are more, by how many more and that `workspace_context` holds them. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries when there are any, and accepting, and when the cycle recorded `noted` entries, marking any of them `fix_now` with `triage_findings` and sending it to the implementer first; after a block, sending fixes and verifying again with a full review only while the cycle cap allows another one, and never with a re-check, accepting or publishing with a waiver naming the open findings only when no verifier's result is blocked, closing the ticket, or asking the human. It SHALL NOT ask the coordinator to triage `noted` entries before accepting. Once no full review remains, the waiver it offers SHALL also name, by short SHA, any commit no review checked, and it SHALL offer asking the human only for a product, security or data-loss decision. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every verifier of the level it names.

#### Scenario: All pass
- **WHEN** every verifier passes in round 1 and one of them reported a non-blocking finding
- **THEN** the ticket's state is `passed` and the project coordinator wakes once with a summary naming each verifier, the verified commit and the `noted` entry under "Noted findings (1)"
- **AND** the summary tells it to accept, says it may first mark the noted finding `fix_now` and send it to the implementer, does not ask it to triage, and does not mention starting a fresh cycle

#### Scenario: Pass with an untriaged finding
- **WHEN** every verifier passes in round 1 and one of them reported a blocking finding without evidence
- **THEN** the summary lists that entry as untriaged and tells the coordinator to triage it, then accept

#### Scenario: Many noted findings
- **WHEN** a cycle passes with 14 new `noted` entries
- **THEN** the summary's noted paragraph is headed with 14, lists the first 10 one line each, and says 4 more are in `workspace_context`

#### Scenario: Check-in during a round
- **WHEN** a verifier's turn in a running round reaches 30 minutes
- **THEN** the project coordinator wakes with its check-in, and the ticket's state and verification record are unchanged

#### Scenario: Cap reached
- **WHEN** with a round cap of 2 the Regressions reviewer reports an evidenced blocking finding in rounds 1 and 2
- **THEN** the ticket's state is `blocked`, the implementer receives no third request, and the project coordinator wakes once with the open entry, the rounds used and both caps
- **AND** the human's inbox gains no item from the host

#### Scenario: Last cycle blocked
- **WHEN** the second full review under a cycle cap of 2 ends blocked at the round cap
- **THEN** the coordinator's message offers accepting or publishing with a waiver, closing the ticket, or asking the human only for a product, security or data-loss decision, and does not offer verifying again

#### Scenario: Configurable cap
- **WHEN** the round cap is set to 4 and a verifier fails in rounds 1, 2 and 3
- **THEN** round 4 starts with that verifier

#### Scenario: Verifier cannot verify
- **WHEN** a verifier reports `blocked` and its result is recorded as blocked
- **THEN** after its round ends the ticket is `blocked` and the project coordinator wakes with that report, without another round, and the message does not offer a waiver

#### Scenario: Every verifier of the level passes
- **WHEN** four verifiers are configured, the cycle's level "Light" runs only Correctness, and Correctness passes in round 1
- **THEN** the cycle's outcome is `passed`, the ticket's state is `passed`, and the three other verifiers received no message

#### Scenario: A re-check passes
- **WHEN** cycle 2, a "Light" re-check, passes in round 1
- **THEN** the project coordinator wakes once with a message naming cycle 2 as a Light re-check, its verifier and the verified commit

#### Scenario: A re-check ends blocked
- **WHEN** cycle 2, a "Light" re-check, ends blocked at the round cap after one full review under a cycle cap of 2
- **THEN** the coordinator's message offers sending fixes and running a full review, accepting or publishing with a waiver, closing the ticket or asking the human, and does not offer another re-check

### Requirement: Verifiers report structured findings
A verifier's `passed`, `failed` or `blocked` report SHALL accept a list of findings. Each finding SHALL have a severity of `blocking`, `non_blocking` or `pre_existing`; a location in `path:line` form; a one-line summary; a trigger; and evidence. It MAY name the id of a ledger entry of the verifier's own focus whose status is `open`, `noted`, `wont_fix` or `follow_up`, to report that entry again. The host SHALL refuse the whole report, recording nothing, when a finding is malformed or exceeds the text bounds, or names any other id. Findings SHALL be recorded only from a report that is a verdict for the verifier's current round; in any other report they SHALL be ignored.

#### Scenario: Malformed finding
- **WHEN** a verifier reports `failed` with a finding whose severity is "critical"
- **THEN** the report is refused, and the verifier's result and the ledger are unchanged

#### Scenario: Findings in a late verdict
- **WHEN** a verifier whose cycle already ended reports `failed` with findings
- **THEN** the report is stored quietly as today and the ledger gains no entry

#### Scenario: Reporting a noted entry again
- **WHEN** the Codex reviewer's entry F2 is `noted` and Codex reports F2 again by its id
- **THEN** the report is accepted, while the same id from the Style reviewer is refused

### Requirement: The host decides each verifier's result
For a verifier's `passed` or `failed` report at an unchanged worktree, the host SHALL record the result as failed if and only if the report contains an evidenced blocking finding, whatever kind was reported. An evidenced blocking finding has severity `blocking`, a location ending in `:` and a line number, and a non-empty trigger and evidence, and is not a re-report of a `wont_fix` or `follow_up` entry. A re-report of a `noted` entry that is an evidenced blocking finding SHALL count as one. When the recorded result differs from the reported kind, the host SHALL record the reason on the verifier's run. A `blocked` report at an unchanged worktree that contains an evidenced blocking finding SHALL be recorded as failed with the reason "reported blocked with an evidenced blocking finding", so its findings go to the implementer like any failed result; any other `blocked` report SHALL be recorded as blocked. The worktree-changed rule SHALL behave as before.

#### Scenario: Failure without evidence
- **WHEN** a verifier reports `failed` with one blocking finding that has no evidence
- **THEN** its result is recorded as passed with the reason "reported failed without an evidenced blocking finding"
- **AND** the finding enters the ledger as `untriaged`

#### Scenario: Pass with an evidenced blocking finding
- **WHEN** a verifier reports `passed` with an evidenced blocking finding
- **THEN** its result is recorded as failed with the reason "reported passed with an evidenced blocking finding"

#### Scenario: Only non-blocking findings
- **WHEN** a verifier reports `failed` with two non-blocking findings and one pre-existing finding
- **THEN** its result is recorded as passed and the three findings enter the ledger as `noted`

#### Scenario: A noted finding becomes blocking
- **WHEN** F2 is `noted` and its verifier reports F2 again by its id as an evidenced blocking finding
- **THEN** its result is recorded as failed and F2 is routed to the implementer

#### Scenario: Blocked with an evidenced blocking finding
- **WHEN** a verifier reports `blocked` with an evidenced blocking finding and a non-blocking finding, and no other verifier of the round is blocked
- **THEN** its result is recorded as failed with the reason "reported blocked with an evidenced blocking finding", the findings enter the ledger as `open` and `noted`, and the open one is routed to the implementer

### Requirement: Decision ledger
Each ticket SHALL keep a ledger of every recorded finding, across all its cycles. Each entry SHALL have an id unique in the ticket (`F1`, `F2`, …), the finding's severity, location, summary, trigger and evidence, its source (verifier focus, cycle and round), a status, and a reason once the coordinator decides it. The status SHALL be one of `open`, `fixed`, `untriaged`, `noted`, `fix_now`, `follow_up` or `wont_fix`. Only a verdict whose worktree check passed SHALL add or change entries; a result the host recorded as failed because the worktree changed SHALL leave the ledger unchanged. A completed check is such a `passed` or `failed` report. A `blocked` report's findings SHALL be recorded like a completed check's, except that it SHALL NOT clear any entry, so an `open` entry stays `open` until a completed check clears it. An evidenced blocking finding SHALL enter as `open`, a finding of severity `non_blocking` or `pre_existing` as `noted`, and a `blocking` finding that is not evidenced as `untriaged`. After a completed check, each `open` entry of the verifier's focus that the report did not name again SHALL become `fixed`; an entry it names again SHALL stay `open`, with its round and evidence updated. A `noted` entry named again SHALL take the new finding and round, and SHALL become `open` when the new finding is an evidenced blocking finding and otherwise stay `noted`. In a keeper review a `noted` entry named again SHALL stay unchanged instead, and an evidenced blocking re-report of it SHALL enter as a new `open` entry of that review, so withdrawing the review leaves the `noted` entry as it was. A `noted` entry not named again SHALL stay `noted`. A `wont_fix` or `follow_up` entry named again SHALL stay unchanged. When a cycle ends, an `open` entry whose focus had no verifier in that cycle SHALL become `untriaged`. Entries SHALL never be deleted, except that withdrawing a keeper review SHALL remove the entries that review recorded, and no others.

#### Scenario: Blocking finding fixed
- **WHEN** the Regressions reviewer's finding F3 is open after round 1, and in round 2 it reports `passed` without naming F3
- **THEN** F3 becomes `fixed`

#### Scenario: Blocking finding persists
- **WHEN** in round 2 the Regressions reviewer reports F3 again by its id with new evidence
- **THEN** F3 stays `open` with round 2 and the new evidence, and no new entry is added

#### Scenario: Re-check cannot run
- **WHEN** F3 is open after round 1 and in round 2 the Regressions reviewer reports `blocked` because it cannot run its check, with one non-blocking finding and without naming F3
- **THEN** F3 stays `open`, the finding enters as `noted`, and the next cycle's message to the Regressions reviewer still lists F3

#### Scenario: Re-check from a changed worktree
- **WHEN** F3 is open and in round 2 the Regressions reviewer edits the worktree and reports `passed` without naming F3
- **THEN** its result is failed with the reason "worktree changed during verification" and F3 stays `open`

#### Scenario: Verifier removed from settings
- **WHEN** an entry from a "Claude" reviewer is open and the next cycle has no verifier with that focus
- **THEN** the entry becomes `untriaged` when that cycle ends

#### Scenario: A noted entry is not cleared by silence
- **WHEN** F2 is `noted` after round 1 and its verifier reports `passed` in cycle 2 without naming F2
- **THEN** F2 stays `noted`

#### Scenario: A withdrawn keeper review leaves no entry
- **WHEN** the ledger holds F1 to F4, a keeper review records F5 and F6, and that review is withdrawn
- **THEN** the ledger holds F1 to F4, each with whatever decision it has at that moment, and F5 and F6 are kept only on the keeper's record of that review
