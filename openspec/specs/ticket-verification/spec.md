# ticket-verification Specification

## Purpose
Verify a ticket with every verifier of the level the coordinator names at one pinned commit, send failures back to the implementer and report the outcome to the coordinator once.

## Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers, a list of review levels (see Review levels), a round cap and a cycle cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, and an optional instruction. Its model SHALL come from its row in the Who runs what table (see model-routing). A saved `size` or `provider` SHALL be ignored. The round cap SHALL default to 3, the cycle cap SHALL default to 2, and each SHALL accept values from 1 to 5. With no saved setting, the list SHALL be a tester (focus "Tests"), a Claude reviewer (focus "Correctness", instruction "Correctness against the ticket's acceptance criteria.") and a Codex reviewer (focus "Regressions", instruction "Regressions and test coverage."), and the levels SHALL be one level named "Full", a full review that runs all three. A saved setting SHALL keep its verifiers and caps; a saved setting without a cycle cap SHALL use the default. The setting SHALL be edited only on the Models page's Reviews card.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` naming the level "Full" starts a tester "Tests", a reviewer "Correctness" and a reviewer "Regressions", each reviewer's instruction naming its lens, with a round cap of 3 and a cycle cap of 2

#### Scenario: Saved setting is not migrated
- **WHEN** the human saved a list with reviewers "Claude" and "Codex" and a round cap of 2 before review levels existed
- **THEN** `verify_ticket` naming the level "Full" keeps starting those reviewers with a round cap of 2, and the cycle cap is 2

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

### Requirement: Verifiers run concurrently
The host SHALL start all of a round's verifier turns together rather than one after another: testers and reviewers in a running cycle SHALL share the ticket worktree as readers, and the round SHALL be admitted only when every one of its verifiers can start, never partially. A round SHALL NOT wait for turn capacity, and a round that cannot start yet SHALL NOT hold back any other session's turn. Each round's record SHALL store, per verifier, its `session_id` and the `message_id` of the message that started it; the verifier's run is the `provider_runs` row for that session whose `messages` list contains that `message_id`, and its interval is that row's `started_at` to `finished_at` (milliseconds since the Unix epoch).

#### Scenario: Two testers share the worktree
- **WHEN** a round contains a Claude tester and a Codex tester on the same ticket
- **THEN** both turns run at the same time instead of the second waiting for the first

#### Scenario: Overlap is provable from stored timestamps
- **WHEN** an SH-1171-sized ticket finishes round 1 with three verifiers
- **THEN** for the three `provider_runs` rows identified by the round's `session_id` and `message_id` pairs, the latest `started_at` is earlier than the earliest `finished_at`

#### Scenario: A waiting round holds no one else
- **WHEN** a round waits because the ticket's implementer is still in a turn in the worktree
- **THEN** workers on other tickets with due input start their turns

### Requirement: Verifiers are read-only and pinned
A verifier SHALL NOT change the ticket worktree. When a verifier reports `passed` or `failed`, the host SHALL compare the worktree's HEAD with the round's pinned commit and check that the worktree has no uncommitted or untracked files. If either check fails, the host SHALL record that verifier's result as failed with the reason "worktree changed during verification", whatever it reported.

#### Scenario: A verifier commits
- **WHEN** a verifier commits in the ticket worktree and then reports `passed`
- **THEN** its result is recorded as failed with the reason "worktree changed during verification"

#### Scenario: Clean pass
- **WHEN** a verifier reports `passed` and the worktree is clean at the pinned commit
- **THEN** its result is recorded as passed for that commit

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
This requirement covers cycles started with `verify_ticket`; how a review the host starts for the PR keeper ends is defined under "A keeper review that passes stays" and "A keeper review that does not pass is withdrawn". Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when the latest result of every verifier of the cycle's level that ran passed, a skipped verifier not counting, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number, the cycle's level and whether it was a re-check, for a full review which of the full reviews the cycle cap allows it was, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided, and SHALL include the report of any verifier that reported `blocked` and each skipped verifier with its reason. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries and accepting; after a block, sending fixes and verifying again with a full review only while the cycle cap allows another one, and never with a re-check, accepting with a waiver only when no verifier reported `blocked`, closing the ticket, or asking the human. Once no full review remains, the waiver it offers for accepting SHALL also name, by short SHA, any commit no review checked, and it SHALL say that `publish_ticket` takes a waiver only for such commits, and it SHALL offer asking the human only for a product, security or data-loss decision. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every verifier of the level it names.

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
Each ticket SHALL keep a ledger of every recorded finding, across all its cycles. Each entry SHALL have an id unique in the ticket (`F1`, `F2`, …), the finding's severity, location, summary, trigger and evidence, its source (verifier focus, cycle and round), a status, and a reason once the coordinator decides it. The status SHALL be one of `open`, `fixed`, `untriaged`, `fix_now`, `follow_up` or `wont_fix`. Only a completed check SHALL add or change entries: a `passed` or `failed` report whose worktree check passed. A `blocked` report, or a result the host recorded as failed because the worktree changed, SHALL leave the ledger unchanged, so an `open` entry stays `open` until a completed check clears it. An evidenced blocking finding SHALL enter as `open`, and every other finding as `untriaged`. After a completed check, each `open` entry of the verifier's focus that the report did not name again SHALL become `fixed`; an entry it names again SHALL stay `open`, with its round and evidence updated. A `wont_fix` or `follow_up` entry named again SHALL stay unchanged. When a cycle ends, an `open` entry whose focus had no verifier in that cycle SHALL become `untriaged`. Entries SHALL never be deleted, except that withdrawing a keeper review SHALL remove the entries that review recorded, and no others.

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
- **THEN** the ledger holds F1 to F4, each with whatever decision it has at that moment, and F5 and F6 are kept only on the keeper's record of that review

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
`verify_ticket` SHALL refuse a level whose kind is full review, starting nothing, when the ticket has already started as many full reviews as the cycle cap allows. The refusal SHALL treat the cap as the coordinator's point to judge whether more review still pays for itself: it SHALL tell the coordinator to read any commits no review checked itself, then accept or publish the ticket with a waiver naming each by its short SHA (see ticket-workspaces and ticket-publishing), file what remains as follow-ups, or close the ticket, and to ask the human only when the waiver would settle a product, security or data-loss decision. It SHALL NOT tell the coordinator that the human can raise the cap or offer raising it as an option. Every started full review SHALL count, including one an implementer's report ended early and a cycle recorded without a level. A re-check SHALL NOT count against the cycle cap and SHALL NOT be refused by it. Cycle numbers SHALL stay one sequence across levels.

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
Starting a new cycle SHALL keep every earlier cycle of the ticket, with its rounds, results and outcome, instead of replacing it. A keeper review that is withdrawn SHALL NOT remain among the ticket's cycles, and its cycle number SHALL NOT be given to a later cycle. The latest cycle SHALL remain the one that verdicts, readiness and acceptance refer to.

#### Scenario: Second cycle
- **WHEN** cycle 1 passed and the coordinator starts cycle 2
- **THEN** the ticket's record holds cycle 2 as its latest cycle and cycle 1 with its rounds and outcome unchanged

#### Scenario: The number of a withdrawn review is not reused
- **WHEN** cycle 1 passed, the keeper's review ran as cycle 2 and was withdrawn, and the coordinator then starts a review
- **THEN** that review is cycle 3, the ticket's cycles are 1 and 3, and each of its verifiers receives its message

### Requirement: The ledger is visible to the coordinator and the human
`workspace_context` SHALL include each open ticket's ledger, earlier cycles and waiver reason. It SHALL show an accepted or closed ticket as one short entry holding only its id, title, state, repository, branch and, if it has one, its pull request's number, state and URL; the full record SHALL stay available through the optional `ticket_id` argument, which SHALL return that one ticket of the caller's project in full, whatever its state, and SHALL refuse a ticket of another project. The Tickets panel SHALL show, on each ticket's card, every cycle's verification lines, any waiver reason, and a "Decision ledger" section listing each entry's id, summary, location, source, status and reason.

#### Scenario: Coordinator reads the ledger
- **WHEN** the coordinator calls `workspace_context` after a cycle with three findings
- **THEN** that ticket's entry lists the three ledger entries with their ids, statuses and sources

#### Scenario: A finished ticket is one line
- **WHEN** the coordinator calls `workspace_context` and a ticket is closed with a merged pull request
- **THEN** that ticket's entry holds only its id, title, state, repository, branch and the pull request's number, state and URL

#### Scenario: A finished ticket by id
- **WHEN** the coordinator calls `workspace_context` with the `ticket_id` of a closed ticket
- **THEN** the result is that ticket's full record, with its brief, cycles, ledger and pull request

#### Scenario: A ticket of another project
- **WHEN** the coordinator calls `workspace_context` with the `ticket_id` of another project's ticket
- **THEN** the call fails and returns nothing of that ticket

#### Scenario: Human reads the ledger
- **WHEN** the human opens the Tickets panel for a ticket with two cycles and five ledger entries
- **THEN** the ticket's card shows both cycles' lines and all five entries with their status and reason

### Requirement: Review levels
The verification setting SHALL hold an ordered list of review levels. Each level SHALL have a name of 1 to 60 bytes that is unique in the list ignoring case; a kind, either full review or re-check; the focuses of one or more configured verifiers, none of them twice; and a non-empty one-line note of at most 200 characters saying when to use it. The list SHALL hold at least one level, and at least one level whose kind is full review. A verifier MAY be in no level, and then no review starts it. A setting that breaks any of these SHALL be refused on save, and the previous setting SHALL stay in effect.

A setting saved without levels SHALL be read as its saved verifiers and caps, unchanged and in the same order, with one level named "Full", a full review that runs every saved verifier, with the note "for every review.". Reading it SHALL NOT fail and SHALL NOT rewrite the settings file. No other level SHALL be added to a saved setting.

The suggested setup of a machine with no saved model selection SHALL offer two levels: "Full", a full review with every suggested verifier and the note "for a ticket's first review, and after any change to what the code does."; and "Light", a re-check with the first suggested reviewer and the note "after a rebase, a resolved merge conflict or a small fix, on a ticket whose last review passed.".

#### Scenario: A setting saved before levels
- **WHEN** the settings file holds the verifiers Tests, Claude, Codex and Style with a round cap of 2 and a cycle cap of 3, and no levels
- **THEN** the setting reads as those four verifiers in that order with their instructions and providers, a round cap of 2 and a cycle cap of 3, and one level "Full", a full review that runs all four
- **AND** the settings file is not rewritten until the human next changes the setting

#### Scenario: No Light on update
- **WHEN** a setting saved before levels is read
- **THEN** it has no level other than "Full"

#### Scenario: Suggested levels with both providers
- **WHEN** a machine with both providers signed in has no saved model selection
- **THEN** the suggested setup's levels are "Full", a full review that runs Tests, Correctness and Regressions, and "Light", a re-check that runs Correctness

#### Scenario: Suggested levels with one provider
- **WHEN** only Codex is signed in on a machine with no saved model selection
- **THEN** "Full" runs the tester and the one suggested reviewer, and "Light" runs that reviewer

#### Scenario: No full review left
- **WHEN** the human saves a setting in which every level is a re-check
- **THEN** the setting is refused and the previous setting stays in effect

#### Scenario: A level with no verifier
- **WHEN** a save would leave the level "Light" with no verifier
- **THEN** the setting is refused and the previous setting stays in effect

#### Scenario: A level names a missing verifier
- **WHEN** a save removes the verifier Style while the level "Full" still names it
- **THEN** the setting is refused and the previous setting stays in effect

#### Scenario: Two levels with one name
- **WHEN** the human saves levels named "Light" and "light"
- **THEN** the setting is refused and the previous setting stays in effect

#### Scenario: A level without a note
- **WHEN** the human saves a level whose note is empty or has two lines
- **THEN** the setting is refused and the previous setting stays in effect

#### Scenario: A verifier in no level
- **WHEN** the human saves a setting in which the verifier Style is in no level
- **THEN** the setting is saved, and no review starts Style

### Requirement: A re-check rests on a passed review
`verify_ticket` SHALL refuse a level whose kind is re-check, starting nothing, unless the ticket's latest verification cycle passed. The refusal SHALL say whether the ticket has not passed a review yet or its latest review ended blocked, and SHALL name the levels whose kind is full review. A level whose kind is full review SHALL need no earlier cycle. A cycle recorded without a level SHALL count as a full review. The rule SHALL use the kind each cycle recorded when it started. `accept_ticket` and `publish_ticket` SHALL treat a passed re-check as they treat any passed cycle.

#### Scenario: A re-check as the first review
- **WHEN** a ticket has no verification cycle and `verify_ticket` names the re-check level "Light"
- **THEN** the call is refused, saying Light is a re-check and the ticket has not passed a review yet, and naming the full-review levels
- **AND** no verifier is started

#### Scenario: A re-check after a blocked review
- **WHEN** a ticket's only cycle, a full review, ended blocked and `verify_ticket` names "Light"
- **THEN** the call is refused, saying the ticket's last review ended blocked, and no verifier is started

#### Scenario: A re-check after a passed full review
- **WHEN** a ticket's latest cycle, a full review, passed, its implementer commits a resolved conflict and reports `ready_for_testing`, and `verify_ticket` names "Light"
- **THEN** the cycle starts with Light's verifiers

#### Scenario: A re-check after a passed re-check
- **WHEN** a ticket's latest cycle, a re-check, passed and `verify_ticket` names "Light" again after a new commit
- **THEN** the cycle starts

#### Scenario: After a blocked re-check
- **WHEN** cycle 1, a full review, passed and cycle 2, a re-check, ended blocked
- **THEN** `verify_ticket` naming "Light" is refused, and naming a full review starts cycle 3 while the cycle cap allows it

#### Scenario: A pass from before levels
- **WHEN** a ticket's latest cycle passed and was recorded without a level
- **THEN** `verify_ticket` naming "Light" starts the cycle

#### Scenario: A migrated ticket that was never verified
- **WHEN** a ticket migrated in state `passed` has no verification record and `verify_ticket` names "Light"
- **THEN** the call is refused, and naming a full review starts its first cycle

#### Scenario: Accepting after a re-check
- **WHEN** a ticket's latest cycle, a re-check, passed, no finding is untriaged and HEAD is the verified commit
- **THEN** `accept_ticket` accepts the ticket without a waiver

#### Scenario: A level re-marked after it ran
- **WHEN** cycle 2 ran "Light" as a re-check and ended blocked, and the human then marks "Light" as a full review
- **THEN** cycle 2's record still says re-check, and `verify_ticket` naming "Light" now starts a full review, which the cycle cap counts

### Requirement: A level cannot leave out a verifier with an open finding
`verify_ticket` SHALL refuse a level, starting nothing, when the ticket's ledger holds an `open` entry whose focus is a configured verifier that the level does not run. The refusal SHALL name that verifier and the entries' ids, and SHALL say to use a level that runs the verifier or to decide the entries with `triage_findings`. An `open` entry whose focus is no longer a configured verifier SHALL NOT refuse a level; it becomes `untriaged` when the cycle ends, as the decision ledger defines.

#### Scenario: Open finding outside the level
- **WHEN** a full review ended blocked with F5 `open` from the Regressions reviewer, and `verify_ticket` names "Docs", a full review that runs only Style
- **THEN** the call is refused, naming Regressions and F5, and no verifier is started

#### Scenario: A level that runs the verifier
- **WHEN** F5 is `open` from the Regressions reviewer and `verify_ticket` names "Full", which runs Regressions
- **THEN** the cycle starts, and the Regressions reviewer's message lists F5 as its open finding

#### Scenario: The finding is overruled first
- **WHEN** the coordinator marks the open entry F5 `wont_fix` with a reason and then names "Docs"
- **THEN** the cycle starts

#### Scenario: The verifier was removed from the settings
- **WHEN** F5 is `open` from a "Regressions" reviewer that the human has since removed from the settings, and `verify_ticket` names "Docs"
- **THEN** the cycle starts, and F5 becomes `untriaged` when it ends

### Requirement: Each cycle records its level
Each verification cycle SHALL record the name and the kind of the level it ran, as they were when it started. Renaming the level, changing its kind or removing it later SHALL NOT change the record. `workspace_context` SHALL include the record with each of a ticket's cycles. The Tickets panel SHALL name the level on each cycle's line and SHALL say when the cycle was a re-check. A cycle recorded without a level SHALL be shown as before.

#### Scenario: The ticket's card names each level
- **WHEN** the human opens the Tickets panel for a ticket whose cycle 1 ran "Full" in two rounds and whose cycle 2 ran the re-check "Light" in one
- **THEN** the card shows "Verification cycle 2 · Light re-check: passed, 1 of 3 rounds" and "Verification cycle 1 · Full: passed, 2 of 3 rounds"

#### Scenario: A level renamed later
- **WHEN** cycle 1 ran "Full" and the human then renames that level "Thorough"
- **THEN** the card's line for cycle 1 still says "Full"

#### Scenario: A cycle from before levels
- **WHEN** a ticket's cycle 1 was recorded without a level
- **THEN** the card shows "Verification cycle 1: passed, 1 of 3 rounds"

#### Scenario: The coordinator reads the level
- **WHEN** the coordinator calls `workspace_context` after cycle 2 ran the re-check "Light"
- **THEN** that ticket's latest cycle names the level "Light" and the kind re-check, and its earlier cycle names "Full" and full review

### Requirement: The coordinator is told how to choose a level
The bundled project coordinator instructions and the `verify_ticket` tool's description SHALL tell the coordinator to name a level from `model_selection.review_levels`, chosen by each level's note on when to use it; that the workspace starts every verifier of that level and all of them must pass; that a re-check starts only on a ticket whose last review passed; that no level may leave out a verifier that still has an open finding; and that only full reviews count against the cycle cap. They SHALL NOT say that `verify_ticket` starts every configured verifier. The host SHALL NOT judge whether the level the coordinator names suits the ticket.

#### Scenario: Bundled instructions
- **WHEN** the host builds the project coordinator's instructions
- **THEN** they name `model_selection.review_levels` and the level's note, state the re-check rule, the open-finding rule and that re-checks are not counted, and do not say that every configured verifier is started

#### Scenario: Tool description
- **WHEN** an agent lists its tools
- **THEN** `verify_ticket` requires `ticket_id`, `level` and `verifiers`, and its description states the same rules

#### Scenario: The choice is the coordinator's
- **WHEN** "Docs" is a full review whose note says it is for tickets that change only documentation, and the coordinator names "Docs" for a ticket that changes code
- **THEN** the cycle starts

### Requirement: The coordinator's context stays bounded as a project ages
`workspace_context` SHALL show an archived agent in `team` as a short entry holding only its id, parent, name, role, status and `archived`, and SHALL leave an archived child's reports out of `child_reports`. Open tickets and live agents SHALL keep their full entries.

#### Scenario: An aged project stays small
- **WHEN** a project has twenty closed tickets with long briefs and ledgers, 120 archived agents and one open ticket, and the coordinator calls `workspace_context`
- **THEN** the reply is far under 256 KB and the open ticket is still in full

#### Scenario: Archived agents are short
- **WHEN** an archived agent has reported to the coordinator and the coordinator calls `workspace_context`
- **THEN** `team` lists it by id, parent, name, role, status and `archived` only, and `child_reports` has nothing of it

### Requirement: The host may start a review for the PR keeper
The host SHALL be able to start one verification cycle on a ticket for the PR keeper, without a `verify_ticket` call and without the ticket being `ready_for_testing`, only for a commit the keeper has moved onto the ticket's branch. The cycle SHALL run the level the keeper's settings name, whose kind SHALL be re-check, and every rule for starting a level SHALL apply unchanged: the ticket's latest cycle passed, and the level leaves out no configured verifier that has an open finding. It SHALL NOT start while the ledger holds any `open` entry, so that the review can only add entries and never change one. It SHALL be one round. Each verifier SHALL resolve its row as in any cycle, and when every verifier of the level would skip, the review cannot start. When any of this cannot be met, no cycle SHALL start and nothing about the ticket SHALL change. The cycle SHALL be recorded as the keeper's, SHALL take the next cycle number, and SHALL NOT count against the cycle cap. When it starts the cycle, the host SHALL record on the ticket, for every keeper review whether or not a model made the commit, the cycle's number, the commit the branch had before the keeper moved it and the ticket's state at that moment; a withdrawal uses that record and nothing else. The Tickets panel and `workspace_context` SHALL mark it as the PR keeper's. Verifiers SHALL get the same message and SHALL be bound by the same read-only and pinned-commit rules as in any cycle.

#### Scenario: No model was ever chosen
- **WHEN** the keeper's level runs Style, and Style has never run on this ticket
- **THEN** the cycle starts with Style on what its row resolves to

#### Scenario: The chosen model is no longer allowed
- **WHEN** the model Correctness last ran on this ticket is no longer in its row
- **THEN** the review runs on a fresh Correctness session on what its row resolves to now

#### Scenario: A keeper review starts
- **WHEN** the keeper has moved a fix onto the branch of a ticket whose cycle 1, a "Full" review, passed, and the keeper's level is "Light", a re-check that runs Correctness, whose row resolves to Claude · opus · high
- **THEN** cycle 2 starts with the Correctness reviewer on Claude · opus · high, pinned to the keeper's commit, and the card shows "Verification cycle 2 · Light re-check by the PR keeper"

#### Scenario: The latest review did not pass
- **WHEN** the ticket's latest cycle ended blocked
- **THEN** no keeper review starts

#### Scenario: A verifier with an open finding is left out
- **WHEN** the ledger holds an open entry from Regressions and the keeper's level runs only Correctness
- **THEN** no keeper review starts

#### Scenario: An open entry in the ledger
- **WHEN** the ledger holds an `open` entry and the keeper has a commit to review
- **THEN** no keeper review starts

#### Scenario: A review without a try
- **WHEN** the keeper reviews a rebase whose patch changed, which no model made
- **THEN** the ticket records the cycle's number, the commit before and the state before, exactly as for a try's review

#### Scenario: Not counted against the cap
- **WHEN** a ticket has used both full reviews of a cycle cap of 2 and the second one passed
- **THEN** a keeper review may still start

#### Scenario: Every verifier of the keeper's level would skip
- **WHEN** the keeper's level runs only Correctness, whose row says skip, and Claude is spent
- **THEN** no keeper review starts and the keeper's commit is not pushed

### Requirement: A keeper review that passes stays
When every verifier of a keeper review passes in its one round, the cycle SHALL end `passed` and SHALL remain the ticket's latest cycle, treated by `accept_ticket`, `publish_ticket` and later re-checks as any passed re-check is. The ticket's state SHALL be `passed`. The host SHALL wake the coordinator with the cycle's message only when the review left an entry to triage or skipped a verifier; otherwise it SHALL send no cycle message.

#### Scenario: Passed with nothing to triage
- **WHEN** a keeper review passes with no finding
- **THEN** it is the ticket's latest cycle, the ticket is `passed`, and the coordinator is not woken

#### Scenario: Passed with a non-blocking finding
- **WHEN** a keeper review passes and a verifier reported one non-blocking finding
- **THEN** the finding is `untriaged` in the ledger and the coordinator gets the cycle's message telling it to triage

#### Scenario: Passed with a skipped verifier
- **WHEN** a keeper review at a level running Correctness and Regressions passes with Regressions skipped
- **THEN** the coordinator gets the cycle's message naming Regressions as skipped and why, and the commit is pushed as after any pass

### Requirement: A keeper review that does not pass is withdrawn
A keeper review SHALL end after its one round. When any verifier's result is failed or blocked, the host SHALL withdraw the review instead of ending it blocked: it SHALL NOT send findings to the implementer, SHALL NOT start another round, and SHALL NOT send the coordinator a cycle message.

The host SHALL withdraw only the cycle the keeper recorded as its own when it started it, for a try or for a rebase whose patch changed, and only while that cycle is the ticket's latest; it SHALL never withdraw a cycle started with `verify_ticket`, an earlier cycle, or a keeper review that passed.

Withdrawing SHALL, in one store change: make the cycle before it the ticket's latest cycle again, with its level, rounds, results and outcome unchanged; remove from the ledger exactly the entries that cycle recorded, and no others; and put the ticket's state back to what the record says it was. Every other ledger entry SHALL stay as it is at that moment, with any decision the coordinator recorded while the review ran. The withdrawn cycle, its verdicts and its findings, with any decision recorded on them, SHALL be kept on the keeper's record of that review and SHALL be shown there, not among the ticket's cycles. The cycle's activity events SHALL remain, and a `keeper_review_withdrawn` event SHALL be logged. Verifier sessions SHALL be kept.

The host SHALL then put the ticket's branch back to the commit it had before the keeper moved it, under the rule for moving a ticket's branch (see pr-keeper): only while the worktree is on the ticket's branch at the keeper's commit, with no uncommitted or untracked files, and no agent of the ticket is in a turn. While an agent is in a turn it SHALL wait. When the worktree is not clean or its HEAD is no longer the keeper's commit, the host SHALL NOT change the worktree or the branch; the records SHALL stay withdrawn, and the host SHALL record the keeper's commit as a head that is not reviewed, as it does for an adopted head (see pr-keeper), so that `accept_ticket`, `publish_ticket` and the keeper refuse until a later cycle ends, whatever the commit's patch id. The coordinator SHALL be told by the keeper only after the branch is back or has been found not restorable, and in the second case the message SHALL name the commit the worktree is at and the verified commit.

#### Scenario: Withdrawn
- **WHEN** cycle 1, a "Full" review, passed at `3daf610` with ledger entries F1 to F4, and the keeper's review, cycle 2 at `77b0c13`, records a blocking finding F5
- **THEN** the ticket's latest cycle is cycle 1, passed at `3daf610`; the ledger holds F1 to F4 as before; the ticket is `passed`; and cycle 2 with F5 is on the keeper's record of that review

#### Scenario: The coordinator triaged during the review
- **WHEN** the ledger holds F1 `untriaged` when a keeper review starts, the coordinator marks F1 `wont_fix` with a reason while the review runs, the review records F5, and it is then withdrawn
- **THEN** F1 is still `wont_fix` with that reason, F5 is not in the ledger, and F5 is on the keeper's record of that review

#### Scenario: A decision on the review's own finding
- **WHEN** the coordinator marks the keeper review's own entry F5 `follow_up` while the review runs, and the review is then withdrawn
- **THEN** F5 leaves the ledger and is kept, with that decision and its reason, on the keeper's record of that review

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
- **THEN** nothing is reset, the implementer's commit is kept, the keeper's commit is recorded as not reviewed so `publish_ticket` refuses, and the coordinator's message names both commits

#### Scenario: A verifier cannot check
- **WHEN** the keeper review's only verifier reports `blocked`
- **THEN** the review is withdrawn in the same way

### Requirement: One call starts every verifier of a level
`verify_ticket` SHALL take a `ticket_id`, the name of a review level as `level`, and an optional `small` that applies to the whole level. The level's name SHALL be matched ignoring surrounding spaces and ASCII case. There SHALL be no default level. It SHALL refuse with nothing started when `level` is missing or names no configured level, naming the configured levels; when a level names a verifier that is not configured; when it is sent `verifiers`, `profile` or `reason`; when a verifier's row has no choice that can run on this machine and does not say skip; or when every verifier of the level would skip. Each verifier of the level SHALL resolve its row (see model-routing). An existing verifier session SHALL be reused only when its pinned profile equals the resolved one; otherwise the host SHALL archive it and start a fresh verifier, never changing a pinned model. On success it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per verifier of the level that does not skip and none for any other verifier, pin each new verifier with Wiffletree's reason, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier that must wait for a reset SHALL be pinned and held as model-routing says, and SHALL NOT stop the others. A verifier with the same role and focus SHALL be the same session in every round of a cycle and keep its model. A cycle's verifiers SHALL be fixed when it starts: a later round SHALL start only verifiers of that cycle whose result failed, and a change to the levels while the cycle runs SHALL NOT affect it. A later cycle MAY name a different level. `verify_ticket` SHALL also refuse when the ticket's state is not `ready_for_testing`, except a finished ticket that was never verified (no verification record, state `passed` or `failed`), when its worktree holds uncommitted or untracked files, or when a cycle is already running, naming the reason. It SHALL NOT refuse because of how many verifiers are configured.

#### Scenario: Three verifiers from one call
- **WHEN** the level "Full" runs three verifiers and the project coordinator calls `verify_ticket` once, naming "Full", on a ticket that is `ready_for_testing`
- **THEN** three verifier sessions each receive a message naming the same commit, each pinned to what its row resolves to, with the reason
- **AND** the ticket's state is `verifying`

#### Scenario: Unready ticket
- **WHEN** `verify_ticket` is called on a ticket whose implementer has not reported `ready_for_testing`
- **THEN** the host refuses and starts no verifier

#### Scenario: The old verifiers argument
- **WHEN** `verify_ticket` is sent a `verifiers` list
- **THEN** the call is refused, saying Wiffletree picks each verifier's model, and no verifier is started

#### Scenario: Small work for a level
- **WHEN** `verify_ticket` names "Full" with `small: true` and only the Tester row has a small line
- **THEN** the tester resolves its small line and the reviewers their main lines

#### Scenario: A row changed between cycles
- **WHEN** a verifier ran cycle 1 on a Claude model, its row now resolves to a Codex model, and the next cycle starts
- **THEN** cycle 2 runs on a fresh Codex verifier session, and the Claude session is archived with its model unchanged

#### Scenario: No level named
- **WHEN** the levels are "Full", "Light" and "Docs" and `verify_ticket` is called without `level`
- **THEN** the call is refused with "Name a review level: Full, Light or Docs." and no verifier is started

#### Scenario: Unknown level
- **WHEN** `verify_ticket` names the level "Quick" and no level has that name
- **THEN** the call is refused, naming the configured levels, and no verifier is started

#### Scenario: Level named in another case
- **WHEN** `verify_ticket` names the level " light " and a level is named "Light"
- **THEN** the cycle starts at the level "Light"

#### Scenario: Only the level's verifiers start
- **WHEN** Tests, Correctness, Regressions and Style are configured, the level "Light" runs only Correctness, and the cycle before it passed
- **THEN** `verify_ticket` naming "Light" starts one verifier session, and Tests, Regressions and Style receive no message

#### Scenario: A level names a verifier that is not configured
- **WHEN** a hand-edited settings file has the level "Light" naming the focus "Style" and no verifier has that focus
- **THEN** `verify_ticket` naming "Light" is refused, saying why, and no verifier is started

#### Scenario: A later cycle at another level
- **WHEN** cycle 1 ran at "Full" and passed, the implementer commits a rebase and reports `ready_for_testing`, and `verify_ticket` names "Light"
- **THEN** cycle 2 starts with only Light's verifiers, and cycle 1's record still names "Full"

#### Scenario: Levels edited while a cycle runs
- **WHEN** round 1 of a cycle at "Full" failed for the Correctness reviewer, the human then unticks Correctness in "Full", and the implementer reports `ready_for_testing`
- **THEN** round 2 starts with the Correctness reviewer

### Requirement: A review goes ahead without a skipped verifier
When a cycle is created, a verifier of its level whose row resolves to skip (see model-routing) SHALL start no session and SHALL be recorded on the cycle as skipped, with its role, focus and a one-line reason such as "Claude's weekly limit was at 96%; its row says skip". A skipped verifier SHALL take no part in any round of that cycle, SHALL never block it, count as pending or hold a verification slot. When every verifier of the level would skip, no cycle SHALL start: `verify_ticket` SHALL refuse with each verifier's reason, and a keeper review SHALL NOT start. The cycle's outcome message SHALL name each skipped verifier and why. A cycle that passed with skipped verifiers SHALL be treated by `accept_ticket` and `publish_ticket` as any passed cycle. The Tickets inspector SHALL show a Skipped pill with the reason for each skipped verifier, and `workspace_context` SHALL list them on the cycle.

#### Scenario: One verifier skipped
- **WHEN** the level "Full" runs Tests, Claude and Codex, Claude's row says skip and Claude's weekly window is at 96%
- **THEN** Tests and Codex start, Claude starts no session and is recorded as skipped with "Claude's weekly limit was at 96%; its row says skip"
- **AND** when Tests and Codex pass, the cycle passes, the coordinator's message names Claude as skipped and why, and the ticket can be accepted and published as after a full pass

#### Scenario: Every verifier would skip
- **WHEN** every verifier of the level says skip and each one's first choice is spent
- **THEN** `verify_ticket` is refused with each verifier's reason, no session starts and the ticket's state is unchanged
