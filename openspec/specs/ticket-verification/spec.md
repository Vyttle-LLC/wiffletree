# ticket-verification Specification

## Purpose
Verify a ticket with every configured verifier at one pinned commit, send failures back to the implementer and report the outcome to the coordinator once.

## Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers and a round cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, an optional instruction, and optionally a provider that limits the coordinator's model choice for that verifier. A saved `size` SHALL be ignored. The round cap SHALL default to 2 and accept values from 1 to 5. With no saved setting, the list SHALL be a tester (focus "Tests"), a Claude reviewer (focus "Claude") and a Codex reviewer (focus "Codex"), which fits the default project turn limit of 4. The setting SHALL be edited only on the Models page's Review card.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` starts a tester, a Claude reviewer and a Codex reviewer, and the round cap is 2

#### Scenario: Invalid setting
- **WHEN** the human saves two verifiers with the same focus, or a round cap of 0
- **THEN** the setting is rejected and the previous setting stays in effect

### Requirement: One call starts every verifier
`verify_ticket` SHALL take a `ticket_id` and a `verifiers` list with one `{focus, profile, reason}` entry per configured verifier. It SHALL refuse with nothing started when an entry is missing, duplicated or unknown; when a reason is not one line; when a profile's provider differs from the verifier's configured provider, or is outside its role's providers (a reviewer may use any enabled provider), so a tester or reviewer with a configured provider must satisfy both; when a profile is not allowed on this machine; or when an existing verifier session it would reuse would be held. An existing verifier session SHALL be reused only when its pinned profile equals the submitted one; otherwise the host SHALL archive it and start a fresh verifier on the submitted profile and reason, never changing a pinned model. Provider and model refusals SHALL record a `model_rejected` event, and the host SHALL never substitute a model. On success it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per configured verifier, pin each new verifier to its choice with the coordinator and reason recorded like an assignment, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier with the same role and focus SHALL be the same session in every round of a cycle and keep its model. `verify_ticket` SHALL also refuse when the ticket's state is not `ready_for_testing`, except a finished ticket that was never verified (no verification record, state `passed` or `failed`), when its worktree holds uncommitted or untracked files, when a cycle is already running, or when the number of verifiers exceeds the smaller of the project's turn limit less one and the host-wide worker turn limit, naming the reason.

#### Scenario: Three verifiers from one call
- **WHEN** three verifiers are configured and the project coordinator calls `verify_ticket` once with an allowed profile and reason for each on a ticket that is `ready_for_testing`
- **THEN** three verifier sessions each receive a message naming the same commit, each pinned to its chosen profile with its reason
- **AND** the ticket's state is `verifying`

#### Scenario: Unready ticket
- **WHEN** `verify_ticket` is called on a ticket whose implementer has not reported `ready_for_testing`
- **THEN** the host refuses and starts no verifier

#### Scenario: Wrong provider for a verifier
- **WHEN** a Codex tester verifier is given a Claude profile
- **THEN** the call fails with "Claude is not configured for Tester · Codex. Tester · Codex uses: Codex.", no verifier session is created and a `model_rejected` event is recorded

#### Scenario: Turn limit too small
- **WHEN** three verifiers are configured and the project's turn limit is 3
- **THEN** `verify_ticket` refuses, naming the verifier count and the turn limit

#### Scenario: More verifiers than the host can run
- **WHEN** the project's turn limit is 20 and more verifiers are configured than the host-wide worker turn limit
- **THEN** `verify_ticket` refuses, naming the verifier count and the host-wide limit

#### Scenario: Verifier provider changed between cycles
- **WHEN** a verifier ran cycle 1 on a Claude model, its provider is changed to Codex, and the next `verify_ticket` gives it a Codex profile
- **THEN** cycle 2 runs on a fresh Codex verifier session with that profile and reason, and the Claude session is archived with its model unchanged

### Requirement: Ticket state during a cycle
While a verification cycle is running, the ticket's state SHALL change only when a round starts, to `verifying` (for round 1 and every later round), and when a round ends, to `passed`, `failed` or `blocked` as defined by the cycle outcome rules. A verifier's verdict or the implementer's `ready_for_testing` SHALL NOT set the ticket's state during a cycle. Any other implementer report except `progress`, such as `blocked`, SHALL end the cycle immediately with outcome `blocked`, set the ticket to `blocked`, and wake the project coordinator with that report. Because `accept_ticket` requires `passed`, it SHALL refuse while any round is in progress.

#### Scenario: First verdict does not pass the ticket
- **WHEN** in a round of three verifiers the first verifier reports `passed` while the other two are still running
- **THEN** the ticket's state stays `verifying`

#### Scenario: Accept refused mid-round
- **WHEN** the project coordinator calls `accept_ticket` after one of three verifiers has reported `passed` and the others are still running
- **THEN** the host refuses because the ticket is `verifying`, and the ticket, its agents and its worktree are unchanged

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
The host SHALL start all of a round's verifier turns together rather than one after another: testers and reviewers in a running cycle SHALL share the ticket worktree as readers, and the round SHALL be admitted only when every one of its verifiers can start, never partially. While a round waits for capacity, the host SHALL start no new worker turn in any project, so other work cannot starve it; coordinator turns are unaffected. Each round's record SHALL store, per verifier, its `session_id` and the `message_id` of the message that started it; the verifier's run is the `provider_runs` row for that session whose `messages` list contains that `message_id`, and its interval is that row's `started_at` to `finished_at` (milliseconds since the Unix epoch).

#### Scenario: Two testers share the worktree
- **WHEN** a round contains a Claude tester and a Codex tester on the same ticket
- **THEN** both turns run at the same time instead of the second waiting for the first

#### Scenario: Overlap is provable from stored timestamps
- **WHEN** an SH-1171-sized ticket finishes round 1 with three verifiers
- **THEN** no verifier's turn started while another verifier of the round was waiting for capacity
- **AND** for the three `provider_runs` rows identified by the round's `session_id` and `message_id` pairs, the latest `started_at` is earlier than the earliest `finished_at`

### Requirement: Verifiers are read-only and pinned
A verifier SHALL NOT change the ticket worktree. When a verifier reports `passed` or `failed`, the host SHALL compare the worktree's HEAD with the round's pinned commit and check that the worktree has no uncommitted or untracked files. If either check fails, the host SHALL record that verifier's result as failed with the reason "worktree changed during verification", whatever it reported.

#### Scenario: A verifier commits
- **WHEN** a verifier commits in the ticket worktree and then reports `passed`
- **THEN** its result is recorded as failed with the reason "worktree changed during verification"

#### Scenario: Clean pass
- **WHEN** a verifier reports `passed` and the worktree is clean at the pinned commit
- **THEN** its result is recorded as passed for that commit

### Requirement: Failures go back to the implementer
A round SHALL end when every verifier in it has reported. If any verifier failed and the round is below the cap, the host SHALL send the ticket's implementer session one message listing every failure report of that round, set the ticket's state to `failed`, and leave the project coordinator asleep. When that implementer next reports `ready_for_testing`, the host SHALL refuse the report while the worktree holds uncommitted or untracked files; otherwise it SHALL start the next round with only the verifiers that failed, pinned to the new HEAD. Verifiers that passed SHALL keep their earlier result.

#### Scenario: One verifier fails
- **WHEN** in round 1 the Codex tester fails and the other two verifiers pass
- **THEN** the same implementer session that reported ready receives one message with the Codex tester's failure
- **AND** the project coordinator has no turn started by those three reports

#### Scenario: Only failed verifiers re-run
- **WHEN** the implementer commits a fix and reports `ready_for_testing` after a round 1 failure by the Codex tester
- **THEN** round 2 starts with only the Codex tester, pinned to the implementer's new commit
- **AND** the Claude tester and style reviewer receive no new message

#### Scenario: Ready with unsaved work
- **WHEN** the implementer reports `ready_for_testing` during a cycle while its worktree has uncommitted changes
- **THEN** the report fails with an instruction to commit or discard them, and no round starts

### Requirement: Cycle outcomes reach the coordinator once
Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's 30-minute check-in SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when every configured verifier's latest result passed, or with state `blocked` when a round at the cap still has a failure, a failed round has no unarchived implementer to send its failures to, or any verifier reported `blocked`. The `blocked` message SHALL list every unresolved failure, the rounds used and the cap. Reaching the cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every configured verifier.

#### Scenario: All pass
- **WHEN** every verifier passes in round 1
- **THEN** the ticket's state is `passed` and the project coordinator wakes once with a summary naming each verifier and the verified commit

#### Scenario: Check-in during a round
- **WHEN** a verifier's turn in a running round reaches 30 minutes
- **THEN** the project coordinator wakes with its check-in, and the ticket's state and verification record are unchanged

#### Scenario: Cap reached
- **WHEN** with a cap of 2 the Codex tester fails in rounds 1 and 2
- **THEN** the ticket's state is `blocked`, the implementer receives no third request, and the project coordinator wakes once with both rounds' failures
- **AND** the human's inbox gains no item from the host

#### Scenario: Configurable cap
- **WHEN** the round cap is set to 3 and a verifier fails in rounds 1 and 2
- **THEN** round 3 starts with that verifier

#### Scenario: Verifier cannot verify
- **WHEN** a verifier reports `blocked`
- **THEN** after its round ends the ticket is `blocked` and the project coordinator wakes with that report, without another round
