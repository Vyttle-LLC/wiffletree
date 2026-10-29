## ADDED Requirements

### Requirement: Noted findings need no decision
A `noted` ledger entry SHALL NOT need a coordinator decision. It SHALL NOT refuse `accept_ticket` or `publish_ticket`, with or without a waiver, and SHALL NOT count among the open findings that a blocked review's publish approval lists or that a waiver names. It SHALL stay `noted` until the coordinator decides it with `triage_findings` or a verifier's evidenced blocking re-report makes it `open`; accepting or closing the ticket SHALL leave it `noted`. Entries stored as `untriaged` before `noted` existed SHALL keep that status.

#### Scenario: Accept with only noted findings
- **WHEN** the latest cycle passed, the ledger holds F1 and F2 `noted` and nothing `untriaged`, and the coordinator calls `accept_ticket`
- **THEN** the ticket is accepted and F1 and F2 stay `noted`

#### Scenario: Publish with only noted findings
- **WHEN** the latest cycle passed with F1 `noted` in an allowed owner's repository and the implementer calls `publish_ticket`
- **THEN** the publish runs without asking, as after any passed review

#### Scenario: A blocked review's approval leaves out noted findings
- **WHEN** a cycle ended blocked with F3 `open` and F4 `noted`, and `publish_ticket` is called without `waived`
- **THEN** the approval lists F3 among the open findings and does not list F4

#### Scenario: An older untriaged nit
- **WHEN** a ticket stored before this change holds a `non_blocking` entry F2 `untriaged`
- **THEN** it still loads with F2 `untriaged`, and `accept_ticket` refuses until F2 is triaged

## MODIFIED Requirements

### Requirement: Cycle outcomes reach the coordinator once
This requirement covers cycles started with `verify_ticket`; how a review the host starts for the PR keeper ends is defined under "A keeper review that passes stays" and "A keeper review that does not pass is withdrawn". Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when the latest result of every verifier of the cycle's level that ran passed, a skipped verifier not counting, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number, the cycle's level and whether it was a re-check, for a full review which of the full reviews the cycle cap allows it was, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided or noted in earlier cycles, and SHALL include the report of any verifier that reported `blocked` and each skipped verifier with its reason. It SHALL list the `noted` entries this cycle recorded in one paragraph headed with their count, one line each with the entry's id, location and summary, at most 10 lines, followed, when there are more, by how many more and that `workspace_context` holds them. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries when there are any, and accepting, and when the cycle recorded `noted` entries, marking any of them `fix_now` with `triage_findings` and sending it to the implementer first; after a block, sending fixes and verifying again with a full review only while the cycle cap allows another one, and never with a re-check, accepting or publishing with a waiver naming the open findings only when no verifier reported `blocked`, closing the ticket, or asking the human. It SHALL NOT ask the coordinator to triage `noted` entries before accepting. Once no full review remains, the waiver it offers SHALL also name, by short SHA, any commit no review checked, and it SHALL offer asking the human only for a product, security or data-loss decision. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every verifier of the level it names.

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
- **THEN** the coordinator's message offers sending fixes and running a full review, accepting or publishing with a waiver, closing the ticket or asking the human, and does not offer another re-check

### Requirement: Verifiers report structured findings
A verifier's `passed` or `failed` report SHALL accept a list of findings. Each finding SHALL have a severity of `blocking`, `non_blocking` or `pre_existing`; a location in `path:line` form; a one-line summary; a trigger; and evidence. It MAY name the id of a ledger entry of the verifier's own focus whose status is `open`, `noted`, `wont_fix` or `follow_up`, to report that entry again. The host SHALL refuse the whole report, recording nothing, when a finding is malformed or exceeds the text bounds, or names any other id. Findings SHALL be recorded only from a report that is a verdict for the verifier's current round; in any other report they SHALL be ignored.

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
For a verifier's `passed` or `failed` report at an unchanged worktree, the host SHALL record the result as failed if and only if the report contains an evidenced blocking finding, whatever kind was reported. An evidenced blocking finding has severity `blocking`, a location ending in `:` and a line number, and a non-empty trigger and evidence, and is not a re-report of a `wont_fix` or `follow_up` entry. A re-report of a `noted` entry that is an evidenced blocking finding SHALL count as one. When the recorded result differs from the reported kind, the host SHALL record the reason on the verifier's run. A `blocked` report and the worktree-changed rule SHALL behave as before.

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

### Requirement: Decision ledger
Each ticket SHALL keep a ledger of every recorded finding, across all its cycles. Each entry SHALL have an id unique in the ticket (`F1`, `F2`, …), the finding's severity, location, summary, trigger and evidence, its source (verifier focus, cycle and round), a status, and a reason once the coordinator decides it. The status SHALL be one of `open`, `fixed`, `untriaged`, `noted`, `fix_now`, `follow_up` or `wont_fix`. Only a completed check SHALL add or change entries: a `passed` or `failed` report whose worktree check passed. A `blocked` report, or a result the host recorded as failed because the worktree changed, SHALL leave the ledger unchanged, so an `open` entry stays `open` until a completed check clears it. An evidenced blocking finding SHALL enter as `open`, a finding of severity `non_blocking` or `pre_existing` as `noted`, and a `blocking` finding that is not evidenced as `untriaged`. After a completed check, each `open` entry of the verifier's focus that the report did not name again SHALL become `fixed`; an entry it names again SHALL stay `open`, with its round and evidence updated. A `noted` entry named again SHALL take the new finding and round, and SHALL become `open` when the new finding is an evidenced blocking finding and otherwise stay `noted`. A `noted` entry not named again SHALL stay `noted`. A `wont_fix` or `follow_up` entry named again SHALL stay unchanged. When a cycle ends, an `open` entry whose focus had no verifier in that cycle SHALL become `untriaged`. Entries SHALL never be deleted, except that withdrawing a keeper review SHALL remove the entries that review recorded, and no others.

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

#### Scenario: A noted entry is not cleared by silence
- **WHEN** F2 is `noted` after round 1 and its verifier reports `passed` in cycle 2 without naming F2
- **THEN** F2 stays `noted`

#### Scenario: A withdrawn keeper review leaves no entry
- **WHEN** the ledger holds F1 to F4, a keeper review records F5 and F6, and that review is withdrawn
- **THEN** the ledger holds F1 to F4, each with whatever decision it has at that moment, and F5 and F6 are kept only on the keeper's record of that review

### Requirement: The coordinator triages findings
The project coordinator SHALL have a `triage_findings` tool for tickets it owns, taking a ticket id and a list of decisions, each with an entry id, a decision of `fix_now`, `follow_up` or `wont_fix`, and a one-line reason. Each named entry SHALL be `untriaged` or `noted`, or `open` with a decision of `follow_up` or `wont_fix`. An `open` entry is already routed for a fix, so `fix_now` on it SHALL be refused. The host SHALL apply every decision or none, refusing the call when an entry is unknown, already decided, named twice, given a decision its status does not allow, or given a reason that is not one line. Marking an `open` entry `follow_up` or `wont_fix` SHALL overrule it: later rounds SHALL list it as already decided, and reporting its id again SHALL fail nothing. A decision SHALL never change once recorded. The tool SHALL record a `findings_triaged` event and SHALL NOT wake or message any agent.

#### Scenario: Triage once
- **WHEN** the coordinator marks F2 `wont_fix` with the reason "Matches the existing naming" and later tries to mark F2 `follow_up`
- **THEN** the first call records the decision and the second is refused, leaving F2 `wont_fix`

#### Scenario: Pulling noted findings into a fix
- **WHEN** F2, F4 and F5 are `noted` and one call marks F2 and F5 `fix_now`
- **THEN** F2 and F5 are `fix_now` with their reasons, F4 stays `noted`, and no agent is messaged

#### Scenario: Overruling a routed finding mid-cycle
- **WHEN** the implementer objects to the open entry F3, the coordinator marks it `wont_fix`, and the Regressions reviewer reports F3 again in the next round with no other blocking finding
- **THEN** the reviewer's result is recorded as passed and F3 stays `wont_fix`

#### Scenario: Fix now does not overrule
- **WHEN** the coordinator marks the open entry F3 `fix_now`
- **THEN** the call is refused, F3 stays `open`, and a later re-report of F3 with evidence still fails its round

#### Scenario: All or nothing
- **WHEN** a call decides F2 and an unknown id F99
- **THEN** the call is refused and F2 is unchanged

### Requirement: Verifiers receive the ticket's review memory
Every round's message to a verifier SHALL name the ticket's base commit (the merge-base of the repository's base and the pinned commit), the diff range from that base to the pinned commit with its changed-file and line counts, the ledger's `wont_fix` and `follow_up` entries marked as already decided and not to be raised again unless the cited code changed, the ledger's `noted` entries marked as already noted and not to be raised again, with an instruction to report one again by its id only with an evidenced blocking finding about it, and the `open` entries of the verifier's own focus with an instruction to report one again by its id only if it is still present. From round 2 on, the message SHALL also name the range from the previous round's commit with its counts, and SHALL tell the verifier to check only whether its open blocking findings are fixed and whether the new diff introduces a blocking defect, raising no new findings on code it already reviewed. A verifier's configured instruction SHALL be included in every round. When the base cannot be computed, the message SHALL say so with the reason, and the round SHALL still start.

#### Scenario: Round 1 memory
- **WHEN** cycle 2 starts on a ticket whose ledger holds F2 `wont_fix`, F4 `follow_up` and F5 `noted`
- **THEN** each verifier's message names the base commit, the diff range with its counts, F2 and F4 as already decided, and F5 as already noted

#### Scenario: Round 2 scope
- **WHEN** round 2 starts for the Regressions reviewer after its finding F3 was routed
- **THEN** its message lists F3 as its open finding, names the range from round 1's commit, limits the check to F3 and the new diff, and repeats its configured instruction

#### Scenario: Base not available
- **WHEN** the repository's base ref does not exist in the ticket's worktree
- **THEN** the round still starts and each message says the base is unavailable and why

### Requirement: A keeper review that passes stays
When every verifier of a keeper review passes in its one round, the cycle SHALL end `passed` and SHALL remain the ticket's latest cycle, treated by `accept_ticket`, `publish_ticket` and later re-checks as any passed re-check is. The ticket's state SHALL be `passed`. The host SHALL wake the coordinator with the cycle's message only when the review left an `untriaged` entry or skipped a verifier; a `noted` entry alone SHALL NOT wake it. Otherwise it SHALL send no cycle message.

#### Scenario: Passed with nothing to triage
- **WHEN** a keeper review passes with no finding
- **THEN** it is the ticket's latest cycle, the ticket is `passed`, and the coordinator is not woken

#### Scenario: Passed with a non-blocking finding
- **WHEN** a keeper review passes and a verifier reported one non-blocking finding
- **THEN** the finding is `noted` in the ledger, the coordinator is not woken, and the commit is pushed as after any pass

#### Scenario: Passed with an untriaged finding
- **WHEN** a keeper review passes and a verifier reported one blocking finding without evidence
- **THEN** the finding is `untriaged` in the ledger and the coordinator gets the cycle's message telling it to triage

#### Scenario: Passed with a skipped verifier
- **WHEN** a keeper review at a level running Correctness and Regressions passes with Regressions skipped
- **THEN** the coordinator gets the cycle's message naming Regressions as skipped and why, and the commit is pushed as after any pass
