## ADDED Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers and a round cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, an optional instruction, and optionally the `provider` and `size` assignment arguments; no other model-selection argument is accepted. The round cap SHALL default to 2 and accept values from 1 to 5. With no saved setting, the list SHALL be one tester using the tester role's default profile.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` starts one tester and the round cap is 2

#### Scenario: Invalid setting
- **WHEN** the human saves two verifiers with the same focus, or a round cap of 0
- **THEN** the setting is rejected and the previous setting stays in effect

### Requirement: One call starts every verifier
`verify_ticket` with a `ticket_id` SHALL start a verification cycle: it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per configured verifier on the ticket, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier with the same role and focus on the ticket SHALL be the same session in every round. `verify_ticket` SHALL refuse when the ticket's state is not `ready_for_testing`, when its worktree holds uncommitted or untracked files, when a cycle is already running, or when the number of verifiers exceeds the worker turns the project allows at once (its turn limit less the one turn reserved for coordinators), naming the reason.

#### Scenario: Three verifiers from one call
- **WHEN** the configured verifiers are a Claude tester, a Codex tester and a style reviewer, and the project coordinator calls `verify_ticket` once on a ticket that is `ready_for_testing`
- **THEN** three verifier sessions on that ticket each receive a message naming the same commit
- **AND** the ticket's state is `verifying` and its verification record lists round 1 with that commit and the three sessions

#### Scenario: Unready ticket
- **WHEN** `verify_ticket` is called on a ticket whose implementer has not reported `ready_for_testing`
- **THEN** the host refuses and starts no verifier

#### Scenario: Turn limit too small
- **WHEN** three verifiers are configured and the project's turn limit is 3
- **THEN** `verify_ticket` refuses, naming the verifier count and the turn limit

### Requirement: Ticket state during a cycle
While a verification cycle is running, the ticket's state SHALL change only when a round starts, to `verifying` (for round 1 and every later round), and when a round ends, to `passed`, `failed` or `blocked` as defined by the cycle outcome rules. An individual verifier's or the implementer's report SHALL NOT set the ticket's state during a cycle. Because `accept_ticket` requires `passed`, it SHALL refuse while any round is in progress.

#### Scenario: First verdict does not pass the ticket
- **WHEN** in a round of three verifiers the first verifier reports `passed` while the other two are still running
- **THEN** the ticket's state stays `verifying`

#### Scenario: Accept refused mid-round
- **WHEN** the project coordinator calls `accept_ticket` after one of three verifiers has reported `passed` and the others are still running
- **THEN** the host refuses because the ticket is `verifying`, and the ticket, its agents and its worktree are unchanged

#### Scenario: Next round starts verifying again
- **WHEN** the implementer reports `ready_for_testing` after a failed round 1
- **THEN** the ticket's state becomes `verifying` when round 2 starts, never `ready_for_testing`

### Requirement: Verifiers run concurrently
The host SHALL start all of a round's verifier turns together rather than one after another: testers and reviewers in a running cycle SHALL share the ticket worktree as readers, and the round SHALL be admitted only when every one of its verifiers can start, never partially. Each round's record SHALL store, per verifier, its `session_id` and the `message_id` of the message that started it; the verifier's run is the `provider_runs` row for that session whose `messages` list contains that `message_id`, and its interval is that row's `started_at` to `finished_at` (milliseconds since the Unix epoch).

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
Reports from verifiers and the implementer during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when every configured verifier's latest result passed, or with state `blocked` when a round at the cap still has a failure or any verifier reported `blocked`. The `blocked` message SHALL list every unresolved failure, the rounds used and the cap. Reaching the cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every configured verifier.

#### Scenario: All pass
- **WHEN** every verifier passes in round 1
- **THEN** the ticket's state is `passed` and the project coordinator wakes once with a summary naming each verifier and the verified commit

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
